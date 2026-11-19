//! `#[derive(NOrmEntity)]` 的展开逻辑。
//!
//! 属性语法：
//! ```text
//! #[derive(NOrmEntity)]
//! #[entity(table = "users")]
//! struct User {
//!     #[entity(primary_key, column = "id")]
//!     id: i64,
//!     #[entity(column = "user_name")]
//!     name: String,
//!     #[entity(nullable)]
//!     email: Option<String>,
//! }
//! ```
//!
//! 不复用 `parse_validator_attrs`（validator 专用手写扫描），改用 syn 的
//! `Punctuated<Meta, Token![,]>`，天然正确处理字符串中的逗号。

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{
    parse_quote, Data, DeriveInput, Expr, ExprLit, Fields, Lit, Meta, Token, Type,
};

#[derive(Clone, Copy)]
enum RelKind {
    HasMany,
    BelongsTo,
}

struct RelationInfo {
    name: String,
    kind: RelKind,
    target: Type,
    foreign_key: String,
}

struct FieldInfo {
    ident: syn::Ident,
    ty: Type,
    column: String,
    primary_key: bool,
    nullable: bool,
    is_option: bool,
    relation: Option<RelationInfo>,
    soft_delete: bool,
    created_at: bool,
    updated_at: bool,
}

/// 从 Meta::NameValue 里取字符串字面量。
fn lit_str(value: &Expr, what: &str) -> syn::Result<String> {
    if let Expr::Lit(ExprLit {
        lit: Lit::Str(s), ..
    }) = value
    {
        Ok(s.value())
    } else {
        Err(syn::Error::new_spanned(value, format!("NOrmEntity: {what} 必须是字符串字面量")))
    }
}

fn is_option(ty: &Type) -> bool {
    if let Type::Path(path) = ty {
        path.path
            .segments
            .last()
            .map(|seg| seg.ident == "Option")
            .unwrap_or(false)
    } else {
        false
    }
}

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;

    // 结构体级属性：table
    let mut table: Option<String> = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("entity") {
            continue;
        }
        let metas = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
        for meta in metas {
            if let Meta::NameValue(nv) = meta && nv.path.is_ident("table") {
                if let Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) = nv.value
                {
                    table = Some(s.value());
                } else {
                    return Err(syn::Error::new_spanned(
                        nv.value,
                        "NOrmEntity: table 必须是字符串字面量",
                    ));
                }
            }
        }
    }
    let table = table.ok_or_else(|| {
        syn::Error::new_spanned(name, "NOrmEntity: 缺少 #[entity(table = \"...\")] 属性")
    })?;

    // 字段级属性
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => named
                .named
                .iter()
                .map(|field| -> syn::Result<FieldInfo> {
                    let ident = field
                        .ident
                        .clone()
                        .ok_or_else(|| syn::Error::new_spanned(field, "NOrmEntity: 字段必须有名字"))?;
                    let field_name = ident.to_string();
                    let mut column = field_name.clone();
                    let mut primary_key = false;
                    let mut nullable = false;
                    let mut column_explicit = false;

                    let mut rel_kind: Option<RelKind> = None;
                    let mut rel_name: Option<String> = None;
                    let mut rel_target: Option<Type> = None;
                    let mut rel_fk: Option<String> = None;
                    let mut soft_delete = false;
                    let mut created_at = false;
                    let mut updated_at = false;

                    for attr in &field.attrs {
                        if !attr.path().is_ident("entity") {
                            continue;
                        }
                        let metas =
                            attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
                        for meta in metas {
                            match meta {
                                Meta::Path(path) if path.is_ident("primary_key") => {
                                    primary_key = true;
                                }
                                Meta::Path(path) if path.is_ident("nullable") => {
                                    nullable = true;
                                }
                                Meta::Path(path) if path.is_ident("soft_delete") => {
                                    soft_delete = true;
                                }
                                Meta::Path(path) if path.is_ident("created_at") => {
                                    created_at = true;
                                }
                                Meta::Path(path) if path.is_ident("updated_at") => {
                                    updated_at = true;
                                }
                                Meta::NameValue(nv) if nv.path.is_ident("column") => {
                                    column = lit_str(&nv.value, "column")?;
                                    column_explicit = true;
                                }
                                Meta::NameValue(nv)
                                    if nv.path.is_ident("has_many")
                                        || nv.path.is_ident("belongs_to") =>
                                {
                                    rel_kind = Some(if nv.path.is_ident("has_many") {
                                        RelKind::HasMany
                                    } else {
                                        RelKind::BelongsTo
                                    });
                                    rel_name = Some(lit_str(&nv.value, "关联名")?);
                                }
                                Meta::NameValue(nv) if nv.path.is_ident("target") => {
                                    let s = lit_str(&nv.value, "target")?;
                                    rel_target = Some(
                                        syn::parse_str::<Type>(&s).map_err(|e| {
                                            syn::Error::new_spanned(&nv.value, e.to_string())
                                        })?,
                                    );
                                }
                                Meta::NameValue(nv) if nv.path.is_ident("foreign_key") => {
                                    rel_fk = Some(lit_str(&nv.value, "foreign_key")?);
                                }
                                other => {
                                    return Err(syn::Error::new_spanned(
                                        other,
                                        "NOrmEntity: 不支持的字段属性",
                                    ));
                                }
                            }
                        }
                    }

                    let relation = if let Some(kind) = rel_kind {
                        let (Some(name), Some(target), Some(foreign_key)) =
                            (rel_name, rel_target, rel_fk)
                        else {
                            return Err(syn::Error::new_spanned(
                                field,
                                "NOrmEntity: has_many/belongs_to 必须同时提供 target 与 foreign_key",
                            ));
                        };
                        if primary_key || nullable || column_explicit {
                            return Err(syn::Error::new_spanned(
                                field,
                                "NOrmEntity: 关联字段不能再标注 primary_key / nullable / column",
                            ));
                        }
                        Some(RelationInfo {
                            name,
                            kind,
                            target,
                            foreign_key,
                        })
                    } else {
                        if rel_target.is_some() || rel_fk.is_some() {
                            return Err(syn::Error::new_spanned(
                                field,
                                "NOrmEntity: target/foreign_key 需配合 has_many/belongs_to 使用",
                            ));
                        }
                        None
                    };

                    let is_option = is_option(&field.ty);
                    if is_option && relation.is_none() {
                        nullable = true;
                    }

                    // 角色标志校验：互斥、类型必须是 NOrmDateTime / Option<NOrmDateTime>
                    let roles = [soft_delete, created_at, updated_at]
                        .iter()
                        .filter(|b| **b)
                        .count();
                    if roles > 1 {
                        return Err(syn::Error::new_spanned(
                            field,
                            "NOrmEntity: soft_delete / created_at / updated_at 不能标在同一字段",
                        ));
                    }
                    if roles > 0 {
                        if relation.is_some() {
                            return Err(syn::Error::new_spanned(
                                field,
                                "NOrmEntity: 关联字段不能标 soft_delete / created_at / updated_at",
                            ));
                        }
                        if primary_key {
                            return Err(syn::Error::new_spanned(
                                field,
                                "NOrmEntity: 时间/软删除角色字段不能是 primary_key",
                            ));
                        }
                        let inner: &Type = if is_option {
                            let last = match &field.ty {
                                Type::Path(p) => p.path.segments.last(),
                                _ => None,
                            };
                            let first_arg = last.and_then(|s| match &s.arguments {
                                syn::PathArguments::AngleBracketed(a) => a.args.first(),
                                _ => None,
                            });
                            match first_arg {
                                Some(syn::GenericArgument::Type(t)) => t,
                                _ => &field.ty,
                            }
                        } else {
                            &field.ty
                        };
                        let is_datetime = matches!(inner, Type::Path(p)
                            if p.path.segments.last().map(|s| s.ident == "NOrmDateTime").unwrap_or(false));
                        if !is_datetime {
                            return Err(syn::Error::new_spanned(
                                field,
                                "NOrmEntity: soft_delete/created_at/updated_at 字段类型必须是 NOrmDateTime 或 Option<NOrmDateTime>",
                            ));
                        }
                        if soft_delete && !is_option {
                            return Err(syn::Error::new_spanned(
                                field,
                                "NOrmEntity: soft_delete 字段必须是 Option<NOrmDateTime>",
                            ));
                        }
                    }

                    Ok(FieldInfo {
                        ident,
                        ty: field.ty.clone(),
                        column,
                        primary_key,
                        nullable,
                        is_option,
                        relation,
                        soft_delete,
                        created_at,
                        updated_at,
                    })
                })
                .collect::<syn::Result<Vec<_>>>()?,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "NOrmEntity 只支持命名字段的结构体",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "NOrmEntity 只能用于结构体",
            ));
        }
    };

    if fields.is_empty() {
        return Err(syn::Error::new_spanned(
            &input,
            "NOrmEntity: 结构体至少需要一个字段",
        ));
    }
    let pk_indexes: Vec<usize> = fields
        .iter()
        .enumerate()
        .filter_map(|(i, f)| if f.primary_key { Some(i) } else { None })
        .collect();
    if pk_indexes.len() != 1 {
        return Err(syn::Error::new_spanned(
            &input,
            "NOrmEntity: 必须且只能有一个字段标记 primary_key",
        ));
    }
    let pk_index = pk_indexes[0];

    // 为泛型结构体补 trait bound；非泛型时 where 子句保持原样。
    let mut generics = input.generics.clone();
    for field in fields.iter().filter(|f| f.relation.is_none()) {
        let ty = &field.ty;
        generics.make_where_clause().predicates.push(parse_quote!(
            #ty: ::nothings_orm::value::NOrmHasSqlType
                + ::nothings_orm::value::NOrmFromValue
                + ::nothings_orm::value::NOrmToValue
        ));
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // NOrmSchema::COLUMNS（关联字段不占列）
    let column_literals = fields.iter().filter(|f| f.relation.is_none()).map(|f| {
        let field_name = f.ident.to_string();
        let column = &f.column;
        let ty = &f.ty;
        let nullable = f.nullable;
        let primary_key = f.primary_key;
        quote! {
            ::nothings_orm::schema::NOrmColumn {
                field: #field_name,
                name: #column,
                sql_type: <#ty as ::nothings_orm::value::NOrmHasSqlType>::SQL_TYPE,
                nullable: #nullable,
                primary_key: #primary_key,
            }
        }
    });

    // from_row：按列名在 NOrmRow 中查找索引，再走 NOrmFromValue 解码。
    // 关联字段不查列；只对标量字段 enumerate，避免索引错位。
    let scalar_fields: Vec<&FieldInfo> = fields.iter().filter(|f| f.relation.is_none()).collect();
    let lookups = scalar_fields.iter().enumerate().map(|(i, f)| {
        let idx_var = syn::Ident::new(&format!("__orm_idx_{}", i), f.ident.span());
        let val_var = syn::Ident::new(&format!("__orm_val_{}", i), f.ident.span());
        let column = &f.column;
        let _ = f.is_option; // Option<T> 的 Null 处理交给 NOrmFromValue 的 blanket impl
        quote! {
            let mut #idx_var: ::std::option::Option<::std::primitive::usize> = ::std::option::Option::None;
            for __orm_i in 0..::nothings_orm::row::NOrmRow::column_count(row) {
                if ::nothings_orm::row::NOrmRow::column_name(row, __orm_i) == #column {
                    #idx_var = ::std::option::Option::Some(__orm_i);
                    break;
                }
            }
            let #idx_var = #idx_var.ok_or_else(|| {
                ::std::io::Error::new(
                    ::std::io::ErrorKind::InvalidData,
                    ::std::format!("NOrmEntity {}: 结果集缺少列 {}", #table, #column),
                )
            })?;
            let #val_var = ::nothings_orm::row::NOrmRow::value(row, #idx_var)?;
        }
    });

    let scalar_indices: std::collections::HashMap<String, usize> = scalar_fields
        .iter()
        .enumerate()
        .map(|(i, f)| (f.ident.to_string(), i))
        .collect();
    let field_inits = fields.iter().map(|f| {
        let ident = &f.ident;
        if let Some(rel) = &f.relation {
            // 关联字段由通用层在 all() 后填充，这里给空初值。
            let init = match rel.kind {
                RelKind::HasMany => quote!(::std::vec::Vec::new()),
                RelKind::BelongsTo => quote!(::std::option::Option::None),
            };
            quote!(#ident: #init)
        } else {
            let i = scalar_indices[&ident.to_string()];
            let val_var = syn::Ident::new(&format!("__orm_val_{}", i), f.ident.span());
            let ty = &f.ty;
            quote! {
                #ident: <#ty as ::nothings_orm::value::NOrmFromValue>::from_value(&#val_var)?
            }
        }
    });

    let pk_ident = &fields[pk_index].ident;
    let column_value_arms = fields.iter().filter(|f| f.relation.is_none()).map(|f| {
        let column = &f.column;
        let ident = &f.ident;
        quote! {
            #column => ::std::result::Result::Ok(
                ::nothings_orm::value::NOrmToValue::to_value(&self.#ident)
            ),
        }
    });
    let table_for_error = &table;

    // NOrmSchema::RELATIONS：每项展开目标实体的表元信息。
    let relation_literals = fields
        .iter()
        .filter_map(|f| f.relation.as_ref().map(|r| (&f.ident, r)))
        .map(|(_, rel)| {
            let rel_name = &rel.name;
            let fk = &rel.foreign_key;
            let target = &rel.target;
            let kind = match rel.kind {
                RelKind::HasMany => quote!(HasMany),
                RelKind::BelongsTo => quote!(BelongsTo),
            };
            quote! {
                ::nothings_orm::schema::NOrmRelation {
                    name: #rel_name,
                    kind: ::nothings_orm::schema::NOrmRelationKind::#kind,
                    foreign_key: #fk,
                    target_table: <#target as ::nothings_orm::schema::NOrmSchema>::TABLE,
                    target_columns: <#target as ::nothings_orm::schema::NOrmSchema>::COLUMNS,
                    soft_delete_col: <#target as ::nothings_orm::schema::NOrmSchema>::SOFT_DELETE_COL,
                }
            }
        });

    // 角色列常量：find 第一个标注该角色的字段，取其数据库列名。
    let role_const = |pick: fn(&FieldInfo) -> bool| -> proc_macro2::TokenStream {
        match fields.iter().find(|f| pick(f)) {
            Some(f) => {
                let col = &f.column;
                quote!(::std::option::Option::Some(#col))
            }
            None => quote!(::std::option::Option::None),
        }
    };
    let soft_const = role_const(|f| f.soft_delete);
    let created_const = role_const(|f| f.created_at);
    let updated_const = role_const(|f| f.updated_at);

    // NOrmPreload::fill_relation：按关系名 match，分支里用宏展开期捕获的具体类型解码。
    let fill_arms = fields
        .iter()
        .filter_map(|f| f.relation.as_ref().map(|r| (&f.ident, r)))
        .map(|(ident, rel)| {
            let rel_name = &rel.name;
            let target = &rel.target;
            match rel.kind {
                RelKind::HasMany => quote! {
                    #rel_name => {
                        let mut __preloaded = ::std::vec::Vec::with_capacity(rows.len());
                        for __row in rows {
                            __preloaded.push(
                                <#target as ::nothings_orm::entity::NOrmEntity>::from_row(__row)?
                            );
                        }
                        self.#ident = __preloaded;
                        ::std::result::Result::Ok(())
                    }
                },
                RelKind::BelongsTo => quote! {
                    #rel_name => {
                        self.#ident = match rows.first() {
                            ::std::option::Option::Some(__row) =>
                                ::std::option::Option::Some(
                                    <#target as ::nothings_orm::entity::NOrmEntity>::from_row(__row)?
                                ),
                            ::std::option::Option::None => ::std::option::Option::None,
                        };
                        ::std::result::Result::Ok(())
                    }
                },
            }
        });

    // NOrmPreload::preload_path：点分路径逐层递归。目标类型 #target 在宏展开期捕获，
    // 每一层先把同层子实体收集成连续切片批量递归，再按原长度回填，保持批量语义而非 N+1。
    let path_arms = fields
        .iter()
        .filter_map(|f| f.relation.as_ref().map(|r| (&f.ident, r)))
        .map(|(ident, rel)| {
            let rel_name = &rel.name;
            let target = &rel.target;
            let collect_backfill = match rel.kind {
                RelKind::HasMany => quote! {
                    let mut __all: ::std::vec::Vec<#target> = ::std::vec::Vec::new();
                    let mut __lens: ::std::vec::Vec<::std::primitive::usize> =
                        ::std::vec::Vec::with_capacity(entities.len());
                    for __e in entities.iter_mut() {
                        __lens.push(__e.#ident.len());
                        __all.append(&mut __e.#ident);
                    }
                    <#target as ::nothings_orm::entity::NOrmPreload>::preload_path(
                        &mut __all, &__path[1..], &mut *driver)?;
                    let mut __it = __all.into_iter();
                    for (__e, __n) in entities.iter_mut().zip(__lens) {
                        __e.#ident = __it.by_ref().take(__n).collect();
                    }
                },
                RelKind::BelongsTo => quote! {
                    let mut __all: ::std::vec::Vec<#target> = ::std::vec::Vec::new();
                    let mut __present: ::std::vec::Vec<::std::primitive::bool> =
                        ::std::vec::Vec::with_capacity(entities.len());
                    for __e in entities.iter_mut() {
                        match ::std::option::Option::take(&mut __e.#ident) {
                            ::std::option::Option::Some(__child) => {
                                __present.push(true);
                                __all.push(__child);
                            }
                            ::std::option::Option::None => __present.push(false),
                        }
                    }
                    <#target as ::nothings_orm::entity::NOrmPreload>::preload_path(
                        &mut __all, &__path[1..], &mut *driver)?;
                    let mut __it = __all.into_iter();
                    for (__e, __was) in entities.iter_mut().zip(__present) {
                        __e.#ident = if __was {
                            __it.next()
                        } else {
                            ::std::option::Option::None
                        };
                    }
                },
            };
            quote! {
                #rel_name => {
                    <Self as ::nothings_orm::entity::NOrmPreload>::preload_one(
                        __head, entities, &mut *driver)?;
                    if __path.len() == 1 {
                        return ::std::result::Result::Ok(());
                    }
                    #collect_backfill
                    ::std::result::Result::Ok(())
                }
            }
        });

    Ok(quote! {
        impl #impl_generics ::nothings_orm::schema::NOrmSchema for #name #ty_generics #where_clause {
            const TABLE: &'static str = #table;
            const COLUMNS: &'static [::nothings_orm::schema::NOrmColumn] = &[
                #(#column_literals),*
            ];
            const RELATIONS: &'static [::nothings_orm::schema::NOrmRelation] = &[
                #(#relation_literals),*
            ];
            const SOFT_DELETE_COL: ::std::option::Option<&'static str> = #soft_const;
            const CREATED_AT_COL: ::std::option::Option<&'static str> = #created_const;
            const UPDATED_AT_COL: ::std::option::Option<&'static str> = #updated_const;
            fn pk() -> &'static ::nothings_orm::schema::NOrmColumn {
                &Self::COLUMNS[#pk_index]
            }
        }

        impl #impl_generics ::nothings_orm::entity::NOrmEntity for #name #ty_generics #where_clause {
            fn from_row<R: ::nothings_orm::row::NOrmRow>(
                row: &R,
            ) -> ::std::result::Result<Self, ::std::io::Error> {
                #(#lookups)*
                ::std::result::Result::Ok(Self { #(#field_inits),* })
            }

            fn pk_value(&self) -> ::nothings_orm::value::NOrmValue {
                ::nothings_orm::value::NOrmToValue::to_value(&self.#pk_ident)
            }

            fn column_value(
                &self,
                __orm_name: &str,
            ) -> ::std::result::Result<::nothings_orm::value::NOrmValue, ::std::io::Error> {
                match __orm_name {
                    #(#column_value_arms)*
                    _ => ::std::result::Result::Err(::std::io::Error::new(
                        ::std::io::ErrorKind::InvalidInput,
                        ::std::format!("NOrmEntity {}: 未知列 {}", #table_for_error, __orm_name),
                    )),
                }
            }
        }

        impl #impl_generics ::nothings_orm::entity::NOrmPreload for #name #ty_generics #where_clause {
            fn fill_relation(
                &mut self,
                __name: &str,
                rows: &[::nothings_orm::row::NOrmOwnedRow],
            ) -> ::std::result::Result<(), ::std::io::Error> {
                match __name {
                    #(#fill_arms)*
                    _ => ::std::result::Result::Err(::std::io::Error::new(
                        ::std::io::ErrorKind::InvalidInput,
                        ::std::format!("NOrmEntity {}: 未知关联 {}", #table_for_error, __name),
                    )),
                }
            }

            fn preload_path<D: ::nothings_orm::driver::NOrmDriver>(
                entities: &mut [Self],
                __path: &[&str],
                driver: &mut D,
            ) -> ::std::result::Result<(), ::std::io::Error> {
                let ::std::option::Option::Some(&__head) = __path.first() else {
                    return ::std::result::Result::Ok(());
                };
                match __head {
                    #(#path_arms)*
                    _ => ::std::result::Result::Err(::std::io::Error::new(
                        ::std::io::ErrorKind::InvalidInput,
                        ::std::format!("NOrmEntity {}: 未知关联 {}", #table_for_error, __head),
                    )),
                }
            }
        }
    })
}
