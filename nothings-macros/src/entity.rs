//! `#[derive(Entity)]` 的展开逻辑。
//!
//! 属性语法：
//! ```text
//! #[derive(Entity)]
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

struct FieldInfo {
    ident: syn::Ident,
    ty: Type,
    column: String,
    primary_key: bool,
    nullable: bool,
    is_option: bool,
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
                        "Entity: table 必须是字符串字面量",
                    ));
                }
            }
        }
    }
    let table = table.ok_or_else(|| {
        syn::Error::new_spanned(name, "Entity: 缺少 #[entity(table = \"...\")] 属性")
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
                        .ok_or_else(|| syn::Error::new_spanned(field, "Entity: 字段必须有名字"))?;
                    let field_name = ident.to_string();
                    let mut column = field_name.clone();
                    let mut primary_key = false;
                    let mut nullable = false;

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
                                Meta::NameValue(nv) if nv.path.is_ident("column") => {
                                    if let Expr::Lit(ExprLit {
                                        lit: Lit::Str(s), ..
                                    }) = nv.value
                                    {
                                        column = s.value();
                                    } else {
                                        return Err(syn::Error::new_spanned(
                                            nv.value,
                                            "Entity: column 必须是字符串字面量",
                                        ));
                                    }
                                }
                                other => {
                                    return Err(syn::Error::new_spanned(
                                        other,
                                        "Entity: 不支持的字段属性",
                                    ));
                                }
                            }
                        }
                    }

                    let is_option = is_option(&field.ty);
                    if is_option {
                        nullable = true;
                    }
                    Ok(FieldInfo {
                        ident,
                        ty: field.ty.clone(),
                        column,
                        primary_key,
                        nullable,
                        is_option,
                    })
                })
                .collect::<syn::Result<Vec<_>>>()?,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "Entity 只支持命名字段的结构体",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "Entity 只能用于结构体",
            ));
        }
    };

    if fields.is_empty() {
        return Err(syn::Error::new_spanned(
            &input,
            "Entity: 结构体至少需要一个字段",
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
            "Entity: 必须且只能有一个字段标记 primary_key",
        ));
    }
    let pk_index = pk_indexes[0];

    // 为泛型结构体补 trait bound；非泛型时 where 子句保持原样。
    let mut generics = input.generics.clone();
    for field in &fields {
        let ty = &field.ty;
        generics.make_where_clause().predicates.push(parse_quote!(
            #ty: ::nothings_orm::value::HasSqlType
                + ::nothings_orm::value::FromValue
                + ::nothings_orm::value::ToValue
        ));
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Schema::COLUMNS
    let column_literals = fields.iter().map(|f| {
        let field_name = f.ident.to_string();
        let column = &f.column;
        let ty = &f.ty;
        let nullable = f.nullable;
        let primary_key = f.primary_key;
        quote! {
            ::nothings_orm::schema::Column {
                field: #field_name,
                name: #column,
                sql_type: <#ty as ::nothings_orm::value::HasSqlType>::SQL_TYPE,
                nullable: #nullable,
                primary_key: #primary_key,
            }
        }
    });

    // from_row：按列名在 Row 中查找索引，再走 FromValue 解码。
    let lookups = fields.iter().enumerate().map(|(i, f)| {
        let idx_var = syn::Ident::new(&format!("__orm_idx_{}", i), f.ident.span());
        let val_var = syn::Ident::new(&format!("__orm_val_{}", i), f.ident.span());
        let column = &f.column;
        let _ = f.is_option; // Option<T> 的 Null 处理交给 FromValue 的 blanket impl
        quote! {
            let mut #idx_var: ::std::option::Option<::std::primitive::usize> = ::std::option::Option::None;
            for __orm_i in 0..::nothings_orm::row::Row::column_count(row) {
                if ::nothings_orm::row::Row::column_name(row, __orm_i) == #column {
                    #idx_var = ::std::option::Option::Some(__orm_i);
                    break;
                }
            }
            let #idx_var = #idx_var.ok_or_else(|| {
                ::std::io::Error::new(
                    ::std::io::ErrorKind::InvalidData,
                    ::std::format!("Entity {}: 结果集缺少列 {}", #table, #column),
                )
            })?;
            let #val_var = ::nothings_orm::row::Row::value(row, #idx_var)?;
        }
    });

    let field_inits = fields.iter().enumerate().map(|(i, f)| {
        let ident = &f.ident;
        let val_var = syn::Ident::new(&format!("__orm_val_{}", i), f.ident.span());
        let ty = &f.ty;
        quote! {
            #ident: <#ty as ::nothings_orm::value::FromValue>::from_value(&#val_var)?
        }
    });

    let pk_ident = &fields[pk_index].ident;
    let column_value_arms = fields.iter().map(|f| {
        let column = &f.column;
        let ident = &f.ident;
        quote! {
            #column => ::std::result::Result::Ok(
                ::nothings_orm::value::ToValue::to_value(&self.#ident)
            ),
        }
    });
    let table_for_error = &table;

    Ok(quote! {
        impl #impl_generics ::nothings_orm::schema::Schema for #name #ty_generics #where_clause {
            const TABLE: &'static str = #table;
            const COLUMNS: &'static [::nothings_orm::schema::Column] = &[
                #(#column_literals),*
            ];
            fn pk() -> &'static ::nothings_orm::schema::Column {
                &Self::COLUMNS[#pk_index]
            }
        }

        impl #impl_generics ::nothings_orm::entity::Entity for #name #ty_generics #where_clause {
            fn from_row<R: ::nothings_orm::row::Row>(
                row: &R,
            ) -> ::std::result::Result<Self, ::std::io::Error> {
                #(#lookups)*
                ::std::result::Result::Ok(Self { #(#field_inits),* })
            }

            fn pk_value(&self) -> ::nothings_orm::value::Value {
                ::nothings_orm::value::ToValue::to_value(&self.#pk_ident)
            }

            fn column_value(
                &self,
                __orm_name: &str,
            ) -> ::std::result::Result<::nothings_orm::value::Value, ::std::io::Error> {
                match __orm_name {
                    #(#column_value_arms)*
                    _ => ::std::result::Result::Err(::std::io::Error::new(
                        ::std::io::ErrorKind::InvalidInput,
                        ::std::format!("Entity {}: 未知列 {}", #table_for_error, __orm_name),
                    )),
                }
            }
        }
    })
}
