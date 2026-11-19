//! 链式查询构建器。
//!
//! 值一律通过 `?` 占位符 + params 传递；列名必须在 `E::COLUMNS` 白名单内，
//! 否则在终结方法中返回 `InvalidInput`。这是防止 SQL 注入的核心约束。
//!
//! # 条件连接语义
//!
//! 链式条件按调用顺序**从左到右**结合：每个方法用自身携带的连接词
//! （AND / OR）把新条件接到已累积的条件上，构建器不按 SQL 优先级重排。
//! 渲染器用最小括号忠实还原这一左结合顺序。
//!
//! 嵌套、取反与比较运算通过 [`NOrmCondition`] 树注入：
//! `where_condition` / `or_where_condition` / `not_where_condition`。

use std::collections::{HashMap, HashSet};
use std::io::{Error, ErrorKind};
use std::marker::PhantomData;

use crate::condition::{NOrmCondition, NOrmOperator};
use crate::driver::NOrmDriver;
use crate::entity::{NOrmEntity, NOrmPreload};
use crate::join::{NOrmFromJoined, NOrmJoin, NOrmJoinKind, NOrmOnTerm, NOrmSubquery};
use crate::row::{NOrmOwnedRow, NOrmPrefixedRow, NOrmRow};
use crate::schema::{validate_column, NOrmRelationKind, NOrmSchema};
use crate::value::{NOrmToValue, NOrmValue};

/// 单次批量关联查询的主键上限。SQLite 默认占位符上限 999，留余量取 500。
pub const NORM_PRELOAD_CHUNK_SIZE: usize = 500;

/// 预加载填充闭包类型。约束在 `preload()` 处单态化，all() 的 bound 不变。
type PreloadFn<E, D> = Box<dyn Fn(&mut [E], &mut D) -> Result<(), Error>>;

/// 分组键。`NOrmValue` 因 F64(f64) 只有 PartialEq，这里做可哈希包装。
#[derive(Hash, Eq, PartialEq)]
enum RelKey {
    Null,
    Bool(bool),
    I64(i64),
    F64(u64),
    Text(String),
    Blob(Vec<u8>),
    DateTime(String),
}

fn rel_key(value: &NOrmValue) -> RelKey {
    match value {
        NOrmValue::Null => RelKey::Null,
        NOrmValue::Bool(b) => RelKey::Bool(*b),
        NOrmValue::I64(n) => RelKey::I64(*n),
        NOrmValue::F64(n) => RelKey::F64(n.to_bits()),
        NOrmValue::Text(s) => RelKey::Text(s.clone()),
        NOrmValue::Blob(b) => RelKey::Blob(b.clone()),
        NOrmValue::NOrmDateTime(dt) => RelKey::DateTime(dt.to_string()),
    }
}

/// 链式查询条件。方法返回 `&mut Self`，对齐 `Aes::set_key` 的风格。
pub struct NOrmQueryBuilder<'a, E: NOrmEntity, D: NOrmDriver> {
    driver: &'a mut D,
    where_: Option<NOrmCondition<E>>,
    /// 已声明的 JOIN，按声明顺序分配别名 t1 / t2 / ...
    joins: Vec<NOrmJoin>,
    /// JOIN 声明期的校验错误，延后到终结方法暴露。
    join_error: Option<String>,
    /// 已声明的预加载：(关系名, 填充闭包)。闭包在 preload() 处单态化。
    preloads: Vec<(&'static str, PreloadFn<E, D>)>,
    /// 未知关系名的校验错误，延后到终结方法暴露。
    preload_error: Option<String>,
    order: Option<(String, bool)>,
    /// order_by 的列名校验错误，延后到终结方法暴露。
    order_error: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    phantom: PhantomData<E>,
}

/// 链式条件的连接词。
enum Connector {
    And,
    Or,
}

/// ON 列来自调用方提供的 schema 名，必须只含标识符字符，防止注入。
fn is_safe_identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !name.bytes().next().unwrap().is_ascii_digit()
}

impl<'a, E: NOrmEntity, D: NOrmDriver> NOrmQueryBuilder<'a, E, D> {
    pub(crate) fn new(driver: &'a mut D) -> Self {
        Self {
            driver,
            where_: None,
            joins: Vec::new(),
            join_error: None,
            preloads: Vec::new(),
            preload_error: None,
            order: None,
            order_error: None,
            limit: None,
            offset: None,
            phantom: PhantomData,
        }
    }

    /// 把一项条件按连接词接到累积条件上（左折叠）。
    fn push_term(&mut self, term: NOrmCondition<E>, connector: Connector) -> &mut Self {
        self.where_ = Some(match self.where_.take() {
            None => term,
            Some(acc) => match connector {
                Connector::And => acc.and(term),
                Connector::Or => acc.or(term),
            },
        });
        self
    }

    /// 追加一个等值条件（AND）。列名经过 schema 白名单校验。
    pub fn r#where(&mut self, column: &str, value: impl NOrmToValue) -> &mut Self {
        self.push_term(NOrmCondition::<E>::eq(column, value), Connector::And)
    }

    /// 追加一个等值条件（AND）。语义与 `r#where` 完全相同。
    pub fn and_where(&mut self, column: &str, value: impl NOrmToValue) -> &mut Self {
        self.push_term(NOrmCondition::<E>::eq(column, value), Connector::And)
    }

    /// 追加一个 OR 等值条件。若为首项，退化为裸首项（不生成孤立 OR）。
    pub fn or_where(&mut self, column: &str, value: impl NOrmToValue) -> &mut Self {
        self.push_term(NOrmCondition::<E>::eq(column, value), Connector::Or)
    }

    /// 追加 `AND NOT (column = value)`。
    pub fn not_where(&mut self, column: &str, value: impl NOrmToValue) -> &mut Self {
        self.push_term(!NOrmCondition::<E>::eq(column, value), Connector::And)
    }

    /// 追加一个 AND 比较条件。`In` / `NotIn` / `IsNull` / `IsNotNull`
    /// 不接受单值，误用会在终结方法报 `InvalidInput`。
    pub fn where_op(&mut self, column: &str, op: NOrmOperator, value: impl NOrmToValue) -> &mut Self {
        self.push_term(NOrmCondition::<E>::cmp(column, op, value), Connector::And)
    }

    /// 追加一个 OR 比较条件。约束同 `where_op`。
    pub fn or_where_op(&mut self, column: &str, op: NOrmOperator, value: impl NOrmToValue) -> &mut Self {
        self.push_term(NOrmCondition::<E>::cmp(column, op, value), Connector::Or)
    }

    /// 注入一整棵条件树（AND）。树内部优先级由渲染器的括号忠实表达。
    pub fn where_condition(&mut self, condition: NOrmCondition<E>) -> &mut Self {
        self.push_term(condition, Connector::And)
    }

    /// 注入一整棵条件树（OR）。
    pub fn or_where_condition(&mut self, condition: NOrmCondition<E>) -> &mut Self {
        self.push_term(condition, Connector::Or)
    }

    /// 注入一棵取反的条件树（AND NOT）。
    pub fn not_where_condition(&mut self, condition: NOrmCondition<E>) -> &mut Self {
        self.push_term(!condition, Connector::And)
    }

    /// 追加一条 JOIN。
    ///
    /// `on` 是一组 `(主表/前序表列, 本次 JOIN 表列)` 等值项，列名分别按
    /// `E::COLUMNS` 与 `T::COLUMNS` 白名单校验。第 n 次调用的表别名为 `tn`。
    /// CROSS JOIN 不需要 ON，传空切片即可。
    ///
    /// 校验错误不立即返回，延后到终结方法报 `InvalidInput`。
    pub fn join<T: NOrmEntity>(
        &mut self,
        kind: NOrmJoinKind,
        on: &[(&str, &str)],
    ) -> &mut Self {
        if self.join_error.is_some() {
            return self;
        }
        let alias = format!("t{}", self.joins.len() + 1);
        let mut terms = Vec::with_capacity(on.len());
        for (left, right) in on {
            // ON 列来自编译期 schema 调用方，不做值绑定；必须是安全标识符以防注入。
            // 能在主表 schema 找到时取规范列名（防止大小写/别名偏差）。
            if !is_safe_identifier(left) || !is_safe_identifier(right) {
                self.join_error = Some(format!("JOIN ON 列名 {} / {} 不是合法标识符", left, right));
                return self;
            }
            let right_col = match validate_column::<T>(right) {
                Ok(c) => c.name,
                Err(err) => {
                    self.join_error = Some(err.to_string());
                    return self;
                }
            };
            let left_col = validate_column::<E>(left).map(|c| c.name).unwrap_or(left);
            terms.push(NOrmOnTerm {
                left_alias: "t0".to_string(),
                left_col: left_col.to_string(),
                right_alias: alias.clone(),
                right_col: right_col.to_string(),
            });
        }
        self.joins.push(NOrmJoin {
            kind,
            table: T::TABLE,
            alias,
            columns: T::COLUMNS.iter().map(|c| c.name).collect(),
            on: terms,
        });
        self
    }

    /// 带表别名的等值条件，如 `where_qualified::<Post>("t1", "status", 1)`。
    /// 列名按 `T::COLUMNS` 校验。
    pub fn where_qualified<T: NOrmSchema>(
        &mut self,
        alias: &str,
        column: &str,
        value: impl NOrmToValue,
    ) -> &mut Self {
        self.push_term(
            NOrmCondition::<E>::qualified::<T>(alias, column, NOrmOperator::Eq, vec![value.to_value()]),
            Connector::And,
        )
    }

    /// `col IN (subquery)`。
    pub fn where_in_subquery(&mut self, column: &str, subquery: NOrmSubquery) -> &mut Self {
        self.push_term(NOrmCondition::<E>::in_subquery(column, subquery), Connector::And)
    }

    /// `col NOT IN (subquery)`。
    pub fn where_not_in_subquery(&mut self, column: &str, subquery: NOrmSubquery) -> &mut Self {
        self.push_term(NOrmCondition::<E>::not_in_subquery(column, subquery), Connector::And)
    }

    /// `EXISTS (subquery)`。
    pub fn where_exists(&mut self, subquery: NOrmSubquery) -> &mut Self {
        self.push_term(NOrmCondition::<E>::exists(subquery), Connector::And)
    }

    /// `NOT EXISTS (subquery)`。
    pub fn where_not_exists(&mut self, subquery: NOrmSubquery) -> &mut Self {
        self.push_term(NOrmCondition::<E>::not_exists(subquery), Connector::And)
    }

    pub fn order_by(&mut self, column: &str, asc: bool) -> &mut Self {
        match validate_column::<E>(column) {
            Ok(_) => self.order = Some((column.to_string(), asc)),
            Err(err) => self.order_error = Some(err.to_string()),
        }
        self
    }

    pub fn limit(&mut self, n: usize) -> &mut Self {
        self.limit = Some(n);
        self
    }

    pub fn offset(&mut self, n: usize) -> &mut Self {
        self.offset = Some(n);
        self
    }

    /// 声明预加载关联。支持点分路径（`books.comments`），每层批量查询、非 N+1。
    ///
    /// 关系名来自 `#[entity(has_many = "...")]` / `#[entity(belongs_to = "...")]`。
    /// 首段在构造期按 `E::RELATIONS` 校验，后续段在执行期逐层校验；错误均延后到
    /// 终结方法报 `InvalidInput`。重叠路径自动保留最长（避免重复查询前缀层）。
    ///
    /// 需要实体派生 `NOrmPreload`（`#[derive(NOrmEntity)]` 自动生成）。
    pub fn preload(&mut self, name: &'static str) -> &mut Self
    where
        E: NOrmPreload,
    {
        if self.preload_error.is_some() {
            return self;
        }
        let segments: Vec<&str> = name.split('.').collect();
        if segments.iter().any(|s| s.is_empty()) {
            self.preload_error = Some(format!("预加载路径 {} 含空段", name));
            return self;
        }
        if !E::RELATIONS.iter().any(|r| r.name == segments[0]) {
            self.preload_error = Some(format!("表 {} 不存在关联 {}", E::TABLE, name));
            return self;
        }

        // 段级前缀合并：已声明路径若与本次重叠，只保留最长的那条。
        self.preloads.retain(|(existing, _)| {
            let old: Vec<&str> = existing.split('.').collect();
            let prefix_len = old.len().min(segments.len());
            let is_prefix = old[..prefix_len] == segments[..prefix_len];
            // 新路径覆盖旧路径（更长或相等）时移除旧的；旧路径更长则本次忽略
            !(is_prefix && old.len() <= segments.len())
        });
        if self.preloads.iter().any(|(n, _)| *n == name) {
            return self;
        }

        self.preloads.push((
            name,
            Box::new(move |entities: &mut [E], driver: &mut D| {
                E::preload_path(entities, &segments, driver)
            }),
        ));
        self
    }

    fn check_valid(&self) -> Result<(), Error> {
        if let Some(message) = &self.preload_error {
            return Err(Error::new(ErrorKind::InvalidInput, message.clone()));
        }
        if let Some(message) = &self.join_error {
            return Err(Error::new(ErrorKind::InvalidInput, message.clone()));
        }
        if let Some(message) = self.where_.as_ref().and_then(|c| c.first_error()) {
            return Err(Error::new(ErrorKind::InvalidInput, message.to_string()));
        }
        if let Some(message) = &self.order_error {
            return Err(Error::new(ErrorKind::InvalidInput, message.clone()));
        }
        Ok(())
    }

    /// JOIN 模式下主表别名（存在 JOIN 时为 t0，否则无别名）。
    fn default_alias(&self) -> Option<&str> {
        if self.joins.is_empty() { None } else { Some("t0") }
    }

    /// 唯一的 WHERE 渲染入口：返回 (不含 " WHERE " 的片段, 参数)。
    fn render_where(&self) -> (Option<String>, Vec<NOrmValue>) {
        match &self.where_ {
            None => (None, Vec::new()),
            Some(condition) => {
                let mut sql = String::new();
                let mut params = Vec::new();
                condition.render_into(&mut sql, &mut params, self.default_alias());
                (Some(sql), params)
            }
        }
    }

    fn build_select(&self) -> (String, Vec<NOrmValue>) {
        let mut sql = if self.joins.is_empty() {
            // 单表查询：保持原有 SQL 字符串不变。
            let columns = E::COLUMNS
                .iter()
                .map(|c| c.name)
                .collect::<Vec<_>>()
                .join(", ");
            format!("SELECT {} FROM {}", columns, E::TABLE)
        } else {
            // JOIN 查询：每列带 tN_ 前缀的别名，主表为 t0。
            let mut cols: Vec<String> = Vec::new();
            for c in E::COLUMNS {
                cols.push(format!("t0.{} AS t0_{}", c.name, c.name));
            }
            for join in &self.joins {
                for c in &join.columns {
                    cols.push(format!("{}.{} AS {}_{}", join.alias, c, join.alias, c));
                }
            }
            format!("SELECT {} FROM {} AS t0", cols.join(", "), E::TABLE)
        };
        for join in &self.joins {
            join.render(&mut sql);
        }
        let (where_sql, params) = self.render_where();
        if let Some(where_sql) = where_sql {
            sql.push_str(" WHERE ");
            sql.push_str(&where_sql);
        }
        if let Some((column, asc)) = &self.order {
            sql.push_str(" ORDER BY ");
            sql.push_str(column);
            sql.push_str(if *asc { " ASC" } else { " DESC" });
        }
        if let Some(n) = self.limit {
            sql.push_str(&format!(" LIMIT {}", n));
        }
        if let Some(n) = self.offset {
            sql.push_str(&format!(" OFFSET {}", n));
        }
        (sql, params)
    }

    pub fn all(&mut self) -> Result<Vec<E>, Error> {
        self.check_valid()?;
        let (sql, params) = self.build_select();
        let rows = self.driver.query(&sql, &params)?;
        let mut entities: Vec<E> = if self.joins.is_empty() {
            rows.iter().map(|row| E::from_row(row)).collect::<Result<_, _>>()?
        } else {
            // JOIN 结果列带 t0_ 前缀，投影后复用派生宏的 from_row 抽主表实体。
            rows.iter()
                .map(|row| E::from_row(&NOrmPrefixedRow::new(row, "t0_")))
                .collect::<Result<_, _>>()?
        };

        // 快路径：无预加载声明或主表结果为空 → 不产生任何额外 SQL。
        if self.preloads.is_empty() || entities.is_empty() {
            return Ok(entities);
        }
        let preloads = &self.preloads;
        let driver = &mut *self.driver;
        for (_, fill) in preloads.iter() {
            fill(&mut entities, driver)?;
        }
        Ok(entities)
    }

    /// 执行查询并把宽行拆成多实体元组，如 `all_with::<(User, Post)>()`。
    ///
    /// 元组元素个数需与 JOIN 数量 + 1（主表）一致，否则报 `InvalidInput`。
    pub fn all_with<T: NOrmFromJoined>(&mut self) -> Result<Vec<T>, Error> {
        self.check_valid()?;
        if self.joins.is_empty() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "all_with 需要至少一条 JOIN；单表请用 all()",
            ));
        }
        let (sql, params) = self.build_select();
        let rows = self.driver.query(&sql, &params)?;
        let mut prefixes: Vec<String> = vec!["t0_".to_string()];
        for join in &self.joins {
            prefixes.push(format!("{}_", join.alias));
        }
        let prefix_refs: Vec<&str> = prefixes.iter().map(|s| s.as_str()).collect();
        rows.iter()
            .map(|row| T::from_joined_row(row, &prefix_refs))
            .collect()
    }

    pub fn one(&mut self) -> Result<Option<E>, Error> {
        self.limit(1);
        let mut rows = self.all()?;
        if rows.is_empty() {
            Ok(None)
        } else {
            Ok(Some(rows.remove(0)))
        }
    }

    pub fn count(&mut self) -> Result<i64, Error> {
        self.check_valid()?;
        let mut sql = if self.joins.is_empty() {
            format!("SELECT COUNT(*) AS __count FROM {}", E::TABLE)
        } else {
            format!("SELECT COUNT(*) AS __count FROM {} AS t0", E::TABLE)
        };
        for join in &self.joins {
            join.render(&mut sql);
        }
        let (where_sql, params) = self.render_where();
        if let Some(where_sql) = where_sql {
            sql.push_str(" WHERE ");
            sql.push_str(&where_sql);
        }
        let rows = self.driver.query(&sql, &params)?;
        let row = rows.first().ok_or_else(|| {
            Error::new(ErrorKind::InvalidData, "COUNT 查询没有返回行")
        })?;
        for i in 0..NOrmRow::column_count(row) {
            if NOrmRow::column_name(row, i) == "__count" {
                return NOrmRow::value(row, i)?.to_i64();
            }
        }
        Err(Error::new(ErrorKind::InvalidData, "COUNT 查询缺少 __count 列"))
    }
}

/// 执行一个关系的两阶段预加载：批量查目标表 → 按主键去重 → 分组 → 填回实体。
pub(crate) fn apply_preload<E: NOrmPreload, D: NOrmDriver>(
    name: &str,
    entities: &mut [E],
    driver: &mut D,
) -> Result<(), Error> {
    let rel = E::RELATIONS
        .iter()
        .find(|r| r.name == name)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, format!("未知关联 {name}")))?;

    // 目标主键列名运行时扫描（pk() 非 const fn，不能存进 RELATIONS 常量）。
    let target_pk = rel
        .target_columns
        .iter()
        .find(|c| c.primary_key)
        .map(|c| c.name)
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "目标实体缺少主键列"))?;

    // ① 收集关联键。
    let mut keys: Vec<NOrmValue> = match rel.kind {
        NOrmRelationKind::HasMany => entities.iter().map(|e| e.pk_value()).collect(),
        NOrmRelationKind::BelongsTo => {
            let mut v = Vec::with_capacity(entities.len());
            for e in entities.iter() {
                let fk = e.column_value(rel.foreign_key)?;
                if !matches!(fk, NOrmValue::Null) {
                    v.push(fk);
                }
            }
            v
        }
    };

    // ② 空键集：不发任何关联 SQL。
    if keys.is_empty() {
        return Ok(());
    }

    // ③ 保序去重 + 分块。
    let mut seen = HashSet::new();
    keys.retain(|k| seen.insert(rel_key(k)));
    let chunks: Vec<&[NOrmValue]> = keys.chunks(NORM_PRELOAD_CHUNK_SIZE).collect();

    // ④ IN 列取白名单里的规范列名，杜绝属性字符串里的注入。
    let in_col: &'static str = match rel.kind {
        NOrmRelationKind::HasMany => rel
            .target_columns
            .iter()
            .find(|c| c.name == rel.foreign_key)
            .map(|c| c.name)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::InvalidInput,
                    format!("目标表 {} 无外键列 {}", rel.target_table, rel.foreign_key),
                )
            })?,
        NOrmRelationKind::BelongsTo => target_pk,
    };
    let select_cols = rel
        .target_columns
        .iter()
        .map(|c| c.name)
        .collect::<Vec<_>>()
        .join(", ");

    // ⑤ 分批查询并合并行。
    let mut target_rows: Vec<NOrmOwnedRow> = Vec::new();
    for chunk in chunks {
        let placeholders = vec!["?"; chunk.len()].join(", ");
        let sql = format!(
            "SELECT {} FROM {} WHERE {} IN ({})",
            select_cols, rel.target_table, in_col, placeholders
        );
        debug_assert_eq!(sql.matches('?').count(), chunk.len());
        target_rows.extend(driver.query(&sql, chunk)?);
    }

    // ⑤.5 按目标实体主键去重（保留首次出现）：多批查询或目标表冗余行
    // 都可能带回同一主键的多行，去重后再分组避免同一实体被重复解码。
    if !target_rows.is_empty() {
        let pk_idx = find_column_index(&target_rows, target_pk)?;
        let mut seen_rows = HashSet::new();
        target_rows.retain(|row| match row.value(pk_idx) {
            Ok(v) => seen_rows.insert(rel_key(&v)),
            // 解码异常的行保留，交给后续 from_row 按统一路径报错
            Err(_) => true,
        });
    }

    // ⑥ 分组并填回。
    match rel.kind {
        NOrmRelationKind::HasMany => {
            let mut grouped: HashMap<RelKey, Vec<NOrmOwnedRow>> = HashMap::new();
            let fk_idx = find_column_index(&target_rows, rel.foreign_key)?;
            for row in target_rows {
                grouped
                    .entry(rel_key(&row.value(fk_idx)?))
                    .or_default()
                    .push(row);
            }
            for e in entities.iter_mut() {
                let rows_for_e = grouped
                    .get(&rel_key(&e.pk_value()))
                    .cloned()
                    .unwrap_or_default();
                e.fill_relation(name, &rows_for_e)?;
            }
        }
        NOrmRelationKind::BelongsTo => {
            let mut grouped: HashMap<RelKey, NOrmOwnedRow> = HashMap::new();
            let pk_idx = find_column_index(&target_rows, target_pk)?;
            for row in target_rows {
                grouped.insert(rel_key(&row.value(pk_idx)?), row);
            }
            for e in entities.iter_mut() {
                let fk = e.column_value(rel.foreign_key)?;
                if matches!(fk, NOrmValue::Null) {
                    e.fill_relation(name, &[])?;
                    continue;
                }
                let rows_for_e: Vec<NOrmOwnedRow> = grouped
                    .get(&rel_key(&fk))
                    .cloned()
                    .into_iter()
                    .collect();
                e.fill_relation(name, &rows_for_e)?;
            }
        }
    }
    Ok(())
}

/// 在结果集首行里找列名对应的索引；空行集返回 0（循环体不会执行）。
fn find_column_index(rows: &[NOrmOwnedRow], name: &str) -> Result<usize, Error> {
    if rows.is_empty() {
        return Ok(0);
    }
    for i in 0..NOrmRow::column_count(&rows[0]) {
        if NOrmRow::column_name(&rows[0], i) == name {
            return Ok(i);
        }
    }
    Err(Error::new(
        ErrorKind::InvalidData,
        format!("关联结果集缺少列 {name}"),
    ))
}

/// 把 NOrmValue 转成 i64 的辅助方法（避免依赖具体驱动的类型）。
trait AsI64 {
    fn to_i64(&self) -> Result<i64, Error>;
}

impl AsI64 for NOrmValue {
    fn to_i64(&self) -> Result<i64, Error> {
        match self {
            NOrmValue::I64(n) => Ok(*n),
            NOrmValue::F64(n) => Ok(*n as i64),
            NOrmValue::Null => Ok(0),
            other => Err(Error::new(
                ErrorKind::InvalidData,
                format!("COUNT 返回了非数值: {other}"),
            )),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::row::NOrmOwnedRow;
    use crate::NOrmEntity;

    #[allow(dead_code)]
    #[derive(Debug, NOrmEntity)]
    #[entity(table = "t")]
    struct T {
        #[entity(primary_key, column = "a")]
        a: i64,
        #[entity(column = "b")]
        b: i64,
        #[entity(column = "c")]
        c: String,
        #[entity(nullable, column = "email")]
        email: Option<String>,
    }

    #[allow(dead_code)]
    #[derive(Debug, NOrmEntity)]
    #[entity(table = "u")]
    struct U {
        #[entity(primary_key, column = "id")]
        id: i64,
        #[entity(column = "t_a")]
        t_a: i64,
        #[entity(column = "label")]
        label: String,
    }

    /// 记录 SQL 与参数的假驱动，用于精确断言渲染结果。
    #[derive(Default)]
    struct RecordingDriver {
        calls: Vec<(String, Vec<NOrmValue>)>,
    }

    impl NOrmDriver for RecordingDriver {
        fn execute(&mut self, sql: &str, params: &[NOrmValue]) -> Result<usize, Error> {
            self.calls.push((sql.to_string(), params.to_vec()));
            Ok(0)
        }

        fn query(&mut self, sql: &str, params: &[NOrmValue]) -> Result<Vec<NOrmOwnedRow>, Error> {
            self.calls.push((sql.to_string(), params.to_vec()));
            if sql.contains("COUNT(*)") {
                Ok(vec![NOrmOwnedRow::new(
                    vec!["__count".into()],
                    vec![NOrmValue::I64(0)],
                )])
            } else {
                Ok(Vec::new())
            }
        }

        fn last_insert_id(&self) -> Result<i64, Error> {
            Ok(0)
        }

        fn begin(&mut self) -> Result<(), Error> {
            Ok(())
        }

        fn commit(&mut self) -> Result<(), Error> {
            Ok(())
        }

        fn rollback(&mut self) -> Result<(), Error> {
            Ok(())
        }
    }

    /// 取最后一次 query 的 SQL，并断言核心不变量：`?` 数 == params 数。
    fn last(drv: &RecordingDriver) -> (String, Vec<NOrmValue>) {
        let (sql, params) = drv.calls.last().expect("没有记录到 query");
        assert_eq!(
            sql.matches('?').count(),
            params.len(),
            "占位符数量与参数数量不一致: {sql}"
        );
        (sql.clone(), params.clone())
    }

    fn select_where(drv: &RecordingDriver) -> String {
        let (sql, _) = last(drv);
        let where_start = sql.find(" WHERE ").expect("SQL 中没有 WHERE") + 7;
        sql[where_start..].to_string()
    }

    #[test]
    fn single_eq_backcompat() {
        let mut drv = RecordingDriver::default();
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .r#where("a", 1i64)
            .all()
            .unwrap();
        assert_eq!(select_where(&drv), "a = ?");
    }

    #[test]
    fn two_eq_and_backcompat() {
        let mut drv = RecordingDriver::default();
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .r#where("a", 1i64)
            .r#where("b", 2i64)
            .all()
            .unwrap();
        assert_eq!(select_where(&drv), "a = ? AND b = ?");
        assert_eq!(last(&drv).1, vec![NOrmValue::I64(1), NOrmValue::I64(2)]);
    }

    #[test]
    fn leading_or_is_bare() {
        let mut drv = RecordingDriver::default();
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .or_where("a", 1i64)
            .all()
            .unwrap();
        assert_eq!(select_where(&drv), "a = ?");
    }

    #[test]
    fn left_assoc_and_or_and() {
        let mut drv = RecordingDriver::default();
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .r#where("a", 1i64)
            .or_where("b", 2i64)
            .r#where("a", 3i64)
            .all()
            .unwrap();
        assert_eq!(select_where(&drv), "(a = ? OR b = ?) AND a = ?");
        assert_eq!(
            last(&drv).1,
            vec![NOrmValue::I64(1), NOrmValue::I64(2), NOrmValue::I64(3)]
        );
    }

    #[test]
    fn not_where() {
        let mut drv = RecordingDriver::default();
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .r#where("a", 1i64)
            .not_where("b", 2i64)
            .all()
            .unwrap();
        assert_eq!(select_where(&drv), "a = ? AND NOT (b = ?)");
    }

    #[test]
    fn nested_not_example() {
        let mut drv = RecordingDriver::default();
        let cond = NOrmCondition::<T>::eq("a", 1i64)
            .or(NOrmCondition::gt("b", 2i64))
            .and(!NOrmCondition::like("c", "%tmp%"));
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .where_condition(cond)
            .all()
            .unwrap();
        assert_eq!(select_where(&drv), "(a = ? OR b > ?) AND NOT (c LIKE ?)");
        assert_eq!(
            last(&drv).1,
            vec![NOrmValue::I64(1), NOrmValue::I64(2), NOrmValue::Text("%tmp%".into())]
        );
    }

    #[test]
    fn comparison_ops_via_chaining() {
        let mut drv = RecordingDriver::default();
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .where_op("a", NOrmOperator::Ge, 18i64)
            .or_where_op("a", NOrmOperator::Lt, 5i64)
            .all()
            .unwrap();
        assert_eq!(select_where(&drv), "a >= ? OR a < ?");
    }

    #[test]
    fn in_and_not_in_via_condition() {
        let mut drv = RecordingDriver::default();
        let cond = NOrmCondition::<T>::in_list("a", [1i64, 2])
            .and(NOrmCondition::<T>::not_in_list("a", [3i64, 4]));
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .where_condition(cond)
            .all()
            .unwrap();
        assert_eq!(select_where(&drv), "a IN (?, ?) AND a NOT IN (?, ?)");
        assert_eq!(last(&drv).1.len(), 4);
    }

    #[test]
    fn empty_in_folds_to_constant() {
        let mut drv = RecordingDriver::default();
        let empty: [i64; 0] = [];
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .where_condition(NOrmCondition::<T>::in_list("a", empty))
            .all()
            .unwrap();
        assert_eq!(select_where(&drv), "0 = 1");
        assert!(last(&drv).1.is_empty());
    }

    #[test]
    fn null_checks_take_no_param() {
        let mut drv = RecordingDriver::default();
        let cond = NOrmCondition::<T>::is_null("email").and(NOrmCondition::<T>::is_not_null("email"));
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .where_condition(cond)
            .all()
            .unwrap();
        assert_eq!(select_where(&drv), "email IS NULL AND email IS NOT NULL");
        assert!(last(&drv).1.is_empty());
    }

    #[test]
    fn count_renders_same_where() {
        let mut drv = RecordingDriver::default();
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .r#where("a", 1i64)
            .or_where("b", 2i64)
            .all()
            .unwrap();
        let (select_sql, select_params) = last(&drv);
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .r#where("a", 1i64)
            .or_where("b", 2i64)
            .count()
            .unwrap();
        let (count_sql, count_params) = last(&drv);

        assert_eq!(
            count_sql,
            "SELECT COUNT(*) AS __count FROM t WHERE a = ? OR b = ?"
        );
        assert_eq!(select_params, count_params);
        assert!(count_sql.matches(" WHERE ").count() == 1);
        assert!(!count_sql.contains("ORDER BY"));
        assert!(select_sql.contains("SELECT a, b, c, email FROM t"));
    }

    #[test]
    fn order_limit_offset_no_regression() {
        let mut drv = RecordingDriver::default();
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .r#where("a", 1i64)
            .order_by("b", false)
            .limit(10)
            .offset(5)
            .all()
            .unwrap();
        assert_eq!(
            last(&drv).0,
            "SELECT a, b, c, email FROM t WHERE a = ? ORDER BY b DESC LIMIT 10 OFFSET 5"
        );
    }

    #[test]
    fn invalid_column_delayed_and_no_sql() {
        let mut drv = RecordingDriver::default();
        let err = NOrmQueryBuilder::<T, _>::new(&mut drv)
            .r#where("bad_column", 1i64)
            .all()
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(drv.calls.is_empty(), "非法列不应生成任何 SQL");
    }

    #[test]
    fn invalid_column_inside_condition_tree() {
        let mut drv = RecordingDriver::default();
        let cond = NOrmCondition::<T>::eq("a", 1i64).or(NOrmCondition::eq("bad", 2i64));
        let err = NOrmQueryBuilder::<T, _>::new(&mut drv)
            .where_condition(cond)
            .all()
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(drv.calls.is_empty());
    }

    #[test]
    fn invalid_order_column_delayed() {
        let mut drv = RecordingDriver::default();
        let err = NOrmQueryBuilder::<T, _>::new(&mut drv)
            .order_by("bad_column", true)
            .all()
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(drv.calls.is_empty());
    }

    #[test]
    fn first_error_is_leftmost() {
        let mut drv = RecordingDriver::default();
        let err = NOrmQueryBuilder::<T, _>::new(&mut drv)
            .r#where("bad_a", 1i64)
            .r#where("bad_b", 2i64)
            .all()
            .unwrap_err();
        assert!(err.to_string().contains("bad_a"), "得到 {}", err);
        assert!(!err.to_string().contains("__invalid_column__"));
    }

    #[test]
    fn nested_group_parens() {
        let mut drv = RecordingDriver::default();
        let cond = NOrmCondition::<T>::eq("a", 1i64).group().or(NOrmCondition::eq("b", 2i64));
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .where_condition(cond)
            .all()
            .unwrap();
        assert_eq!(select_where(&drv), "(a = ?) OR b = ?");
    }

    #[test]
    fn or_where_condition_mixed_with_chain() {
        let mut drv = RecordingDriver::default();
        let cond = NOrmCondition::<T>::eq("a", 1i64).or(NOrmCondition::eq("b", 2i64));
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .r#where("a", 9i64)
            .or_where_condition(cond)
            .r#where("b", 8i64)
            .all()
            .unwrap();
        assert_eq!(select_where(&drv), "(a = ? OR a = ? OR b = ?) AND b = ?");
    }

    #[test]
    fn not_where_condition() {
        let mut drv = RecordingDriver::default();
        let cond = NOrmCondition::<T>::eq("a", 1i64).or(NOrmCondition::eq("b", 2i64));
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .not_where_condition(cond)
            .all()
            .unwrap();
        assert_eq!(select_where(&drv), "NOT (a = ? OR b = ?)");
    }

    // ---------- JOIN / 子查询 ----------

    #[test]
    fn join_select_sql_and_aliased_where() {
        let mut drv = RecordingDriver::default();
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .join::<U>(NOrmJoinKind::Inner, &[("a", "t_a")])
            .r#where("a", 1i64)
            .all()
            .unwrap();
        let (sql, params) = last(&drv);
        assert_eq!(
            sql,
            "SELECT t0.a AS t0_a, t0.b AS t0_b, t0.c AS t0_c, t0.email AS t0_email, \
             t1.id AS t1_id, t1.t_a AS t1_t_a, t1.label AS t1_label \
             FROM t AS t0 INNER JOIN u AS t1 ON t0.a = t1.t_a WHERE t0.a = ?"
        );
        assert_eq!(params, vec![NOrmValue::I64(1)]);
    }

    #[test]
    fn join_left_and_count() {
        let mut drv = RecordingDriver::default();
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .join::<U>(NOrmJoinKind::Left, &[("a", "t_a")])
            .count()
            .unwrap();
        let (sql, _) = last(&drv);
        assert_eq!(
            sql,
            "SELECT COUNT(*) AS __count FROM t AS t0 LEFT JOIN u AS t1 ON t0.a = t1.t_a"
        );
    }

    #[test]
    fn join_all_with_tuple() {
        // 假驱动返回一行带 t0_/t1_ 前缀列的宽行。
        struct JoinDriver;
        impl NOrmDriver for JoinDriver {
            fn execute(&mut self, _: &str, _: &[NOrmValue]) -> Result<usize, Error> { Ok(0) }
            fn query(&mut self, _: &str, _: &[NOrmValue]) -> Result<Vec<NOrmOwnedRow>, Error> {
                Ok(vec![NOrmOwnedRow::new(
                    vec![
                        "t0_a".into(), "t0_b".into(), "t0_c".into(), "t0_email".into(),
                        "t1_id".into(), "t1_t_a".into(), "t1_label".into(),
                    ],
                    vec![
                        NOrmValue::I64(1), NOrmValue::I64(2), NOrmValue::Text("x".into()), NOrmValue::Null,
                        NOrmValue::I64(9), NOrmValue::I64(1), NOrmValue::Text("L".into()),
                    ],
                )])
            }
            fn last_insert_id(&self) -> Result<i64, Error> { Ok(0) }
            fn begin(&mut self) -> Result<(), Error> { Ok(()) }
            fn commit(&mut self) -> Result<(), Error> { Ok(()) }
            fn rollback(&mut self) -> Result<(), Error> { Ok(()) }
        }
        let mut drv = JoinDriver;
        let rows = NOrmQueryBuilder::<T, _>::new(&mut drv)
            .join::<U>(NOrmJoinKind::Inner, &[("a", "t_a")])
            .all_with::<(T, U)>()
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0.a, 1);
        assert_eq!(rows[0].1.id, 9);
        assert_eq!(rows[0].1.label, "L");
    }

    #[test]
    fn qualified_where_on_joined_table() {
        let mut drv = RecordingDriver::default();
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .join::<U>(NOrmJoinKind::Inner, &[("a", "t_a")])
            .where_qualified::<U>("t1", "label", "L")
            .all()
            .unwrap();
        let (sql, params) = last(&drv);
        assert!(sql.ends_with("WHERE t1.label = ?"), "得到 {sql}");
        assert_eq!(params, vec![NOrmValue::Text("L".into())]);
    }

    #[test]
    fn subquery_in_and_exists() {
        let mut drv = RecordingDriver::default();
        let sub = NOrmSubquery::with_params(
            "SELECT t_a FROM u WHERE label = ?",
            vec![NOrmValue::Text("L".into())],
        );
        NOrmQueryBuilder::<T, _>::new(&mut drv)
            .where_in_subquery("a", sub)
            .where_exists(NOrmSubquery::new("SELECT 1 FROM u"))
            .all()
            .unwrap();
        let (sql, params) = last(&drv);
        assert_eq!(
            sql,
            "SELECT a, b, c, email FROM t WHERE a IN (SELECT t_a FROM u WHERE label = ?) \
             AND EXISTS (SELECT 1 FROM u)"
        );
        assert_eq!(params, vec![NOrmValue::Text("L".into())]);
    }

    #[test]
    fn join_invalid_on_column_delayed() {
        let mut drv = RecordingDriver::default();
        let err = NOrmQueryBuilder::<T, _>::new(&mut drv)
            .join::<U>(NOrmJoinKind::Inner, &[("a", "bad-col")])
            .all()
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(drv.calls.is_empty());
    }

    #[test]
    fn all_with_requires_join() {
        let mut drv = RecordingDriver::default();
        let err = NOrmQueryBuilder::<T, _>::new(&mut drv)
            .all_with::<(T, U)>()
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }

    // ---------- Preload ----------

    #[allow(dead_code)]
    #[derive(Debug, NOrmEntity)]
    #[entity(table = "authors")]
    struct Author {
        #[entity(primary_key, column = "id")]
        id: i64,
        #[entity(column = "name")]
        name: String,
        #[entity(has_many = "books", target = "Book", foreign_key = "author_id")]
        books: Vec<Book>,
    }

    #[allow(dead_code)]
    #[derive(Debug, NOrmEntity)]
    #[entity(table = "books")]
    struct Book {
        #[entity(primary_key, column = "id")]
        id: i64,
        #[entity(nullable, column = "author_id")]
        author_id: Option<i64>,
        #[entity(column = "title")]
        title: String,
        #[entity(belongs_to = "author", target = "Author", foreign_key = "author_id")]
        author: Option<Author>,
        #[entity(has_many = "comments", target = "Comment", foreign_key = "book_id")]
        comments: Vec<Comment>,
    }

    #[allow(dead_code)]
    #[derive(Debug, NOrmEntity)]
    #[entity(table = "comments")]
    struct Comment {
        #[entity(primary_key, column = "id")]
        id: i64,
        #[entity(column = "book_id")]
        book_id: i64,
        #[entity(column = "body")]
        body: String,
    }

    /// 按 SQL 特征返回 canned 行，并记录每次调用的 (sql, params)。
    #[derive(Default)]
    struct PreloadDriver {
        calls: Vec<(String, Vec<NOrmValue>)>,
        parents: Vec<NOrmOwnedRow>,
        children: Vec<NOrmOwnedRow>,
    }

    impl PreloadDriver {
        fn with_parents(parents: Vec<NOrmOwnedRow>) -> Self {
            Self {
                parents,
                ..Default::default()
            }
        }
    }

    impl NOrmDriver for PreloadDriver {
        fn execute(&mut self, _: &str, _: &[NOrmValue]) -> Result<usize, Error> { Ok(0) }
        fn query(&mut self, sql: &str, params: &[NOrmValue]) -> Result<Vec<NOrmOwnedRow>, Error> {
            self.calls.push((sql.to_string(), params.to_vec()));
            if !sql.contains(" IN (")
                && (sql.contains("FROM authors") || sql.contains("FROM books"))
            {
                Ok(self.parents.clone())
            } else if sql.contains("FROM authors") {
                // belongs_to 的目标查询
                Ok(self.children.clone())
            } else if sql.contains("FROM books") {
                // has_many 的目标查询：按 IN 参数造行
                let mut rows = Vec::new();
                for v in params {
                    if let NOrmValue::I64(id) = v {
                        rows.push(NOrmOwnedRow::new(
                            vec!["id".into(), "author_id".into(), "title".into()],
                            vec![
                                NOrmValue::I64(*id * 10),
                                NOrmValue::I64(*id),
                                NOrmValue::Text(format!("book-{id}")),
                            ],
                        ));
                    }
                }
                Ok(rows)
            } else if sql.contains("FROM comments") {
                // 第三层：按 IN 的 book_id 造 comment 行
                let mut rows = Vec::new();
                for v in params {
                    if let NOrmValue::I64(book_id) = v {
                        rows.push(NOrmOwnedRow::new(
                            vec!["id".into(), "book_id".into(), "body".into()],
                            vec![
                                NOrmValue::I64(*book_id * 100),
                                NOrmValue::I64(*book_id),
                                NOrmValue::Text(format!("comment-{book_id}")),
                            ],
                        ));
                    }
                }
                Ok(rows)
            } else {
                Ok(self.children.clone())
            }
        }
        fn last_insert_id(&self) -> Result<i64, Error> { Ok(0) }
        fn begin(&mut self) -> Result<(), Error> { Ok(()) }
        fn commit(&mut self) -> Result<(), Error> { Ok(()) }
        fn rollback(&mut self) -> Result<(), Error> { Ok(()) }
    }

    fn author_row(id: i64) -> NOrmOwnedRow {
        NOrmOwnedRow::new(
            vec!["id".into(), "name".into()],
            vec![NOrmValue::I64(id), NOrmValue::Text(format!("A{id}"))],
        )
    }

    fn nth(drv: &PreloadDriver, i: usize) -> (String, Vec<NOrmValue>) {
        let (sql, params) = &drv.calls[i];
        assert_eq!(sql.matches('?').count(), params.len(), "第 {i} 次 SQL 占位符不匹配");
        (sql.clone(), params.clone())
    }

    #[test]
    fn preload_has_many_sql_and_fill() {
        let mut drv = PreloadDriver::with_parents(vec![author_row(1), author_row(2)]);
        let rows = NOrmQueryBuilder::<Author, _>::new(&mut drv)
            .preload("books")
            .all()
            .unwrap();
        assert_eq!(drv.calls.len(), 2);
        assert_eq!(nth(&drv, 0).0, "SELECT id, name FROM authors");
        let (sql, params) = nth(&drv, 1);
        assert_eq!(sql, "SELECT id, author_id, title FROM books WHERE author_id IN (?, ?)");
        assert_eq!(params, vec![NOrmValue::I64(1), NOrmValue::I64(2)]);
        assert_eq!(rows[0].books.len(), 1);
        assert_eq!(rows[0].books[0].title, "book-1");
        assert_eq!(rows[1].books[0].author_id, Some(2));
    }

    #[test]
    fn preload_belongs_to_sql_and_fill() {
        let mut drv = PreloadDriver::with_parents(vec![NOrmOwnedRow::new(
            vec!["id".into(), "author_id".into(), "title".into()],
            vec![NOrmValue::I64(10), NOrmValue::I64(1), NOrmValue::Text("one".into())],
        )]);
        drv.children = vec![author_row(1)];
        let rows = NOrmQueryBuilder::<Book, _>::new(&mut drv)
            .preload("author")
            .all()
            .unwrap();
        assert_eq!(nth(&drv, 1).0, "SELECT id, name FROM authors WHERE id IN (?)");
        assert_eq!(rows[0].author.as_ref().unwrap().name, "A1");
    }

    #[test]
    fn preload_chunk_two_batches() {
        let mut drv = PreloadDriver::with_parents((1..=501).map(author_row).collect());
        let rows = NOrmQueryBuilder::<Author, _>::new(&mut drv)
            .preload("books")
            .all()
            .unwrap();
        assert_eq!(drv.calls.len(), 3);
        assert_eq!(nth(&drv, 1).1.len(), 500);
        assert_eq!(nth(&drv, 2).1.len(), 1);
        assert_eq!(rows.len(), 501);
        assert_eq!(rows[500].books[0].author_id, Some(501));
    }

    #[test]
    fn preload_empty_parents_skips_relation_query() {
        let mut drv = PreloadDriver::default();
        let rows = NOrmQueryBuilder::<Author, _>::new(&mut drv)
            .preload("books")
            .all()
            .unwrap();
        assert!(rows.is_empty());
        assert_eq!(drv.calls.len(), 1);
    }

    #[test]
    fn preload_unknown_name_delayed_error() {
        let mut drv = PreloadDriver::default();
        let err = NOrmQueryBuilder::<Author, _>::new(&mut drv)
            .preload("comments")
            .all()
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(drv.calls.is_empty());
    }

    #[test]
    fn preload_no_declaration_backcompat() {
        let mut drv = PreloadDriver::with_parents(vec![author_row(1)]);
        let rows = NOrmQueryBuilder::<Author, _>::new(&mut drv).all().unwrap();
        assert_eq!(drv.calls.len(), 1);
        assert!(rows[0].books.is_empty());
    }

    #[test]
    fn preload_dedup_keys() {
        // 两个实体主键相同 → IN 参数去重后只有 1 个
        let mut drv = PreloadDriver::with_parents(vec![author_row(7), author_row(7)]);
        NOrmQueryBuilder::<Author, _>::new(&mut drv)
            .preload("books")
            .all()
            .unwrap();
        assert_eq!(nth(&drv, 1).1, vec![NOrmValue::I64(7)]);
    }

    #[test]
    fn preload_null_fk_stays_none() {
        let mut drv = PreloadDriver::with_parents(vec![NOrmOwnedRow::new(
            vec!["id".into(), "author_id".into(), "title".into()],
            vec![NOrmValue::I64(10), NOrmValue::Null, NOrmValue::Text("orphan".into())],
        )]);
        let rows = NOrmQueryBuilder::<Book, _>::new(&mut drv)
            .preload("author")
            .all()
            .unwrap();
        assert!(rows[0].author.is_none());
        // 空键集 → 无关联查询
        assert_eq!(drv.calls.len(), 1);
    }

    // ---------- 递归 / 懒加载 / 去重 ----------

    #[test]
    fn preload_two_levels_sql_and_fill() {
        let mut drv = PreloadDriver::with_parents(vec![author_row(1), author_row(2)]);
        // children 作为第二层 belongs_to（authors）的查询结果
        drv.children = vec![author_row(1)];
        let rows = NOrmQueryBuilder::<Author, _>::new(&mut drv)
            .preload("books.author")
            .all()
            .unwrap();
        // 主表 + books + authors = 3 次 SQL（批量，非 N+1）
        assert_eq!(drv.calls.len(), 3);
        assert!(nth(&drv, 1).0.contains("FROM books WHERE author_id IN (?, ?)"));
        assert!(nth(&drv, 2).0.contains("FROM authors WHERE id IN ("));
        assert_eq!(rows[0].books[0].author.as_ref().unwrap().name, "A1");
    }

    #[test]
    fn preload_three_levels_sql_and_fill() {
        let mut drv = PreloadDriver::with_parents(vec![author_row(1)]);
        let rows = NOrmQueryBuilder::<Author, _>::new(&mut drv)
            .preload("books.comments")
            .all()
            .unwrap();
        assert_eq!(drv.calls.len(), 3);
        assert!(nth(&drv, 2).0.contains("FROM comments WHERE book_id IN (?)"));
        assert_eq!(rows[0].books[0].comments[0].body, "comment-10");
    }

    #[test]
    fn preload_bidirectional_cycle_is_bounded() {
        let mut drv = PreloadDriver::with_parents(vec![author_row(1)]);
        drv.children = vec![author_row(1)];
        let rows = NOrmQueryBuilder::<Author, _>::new(&mut drv)
            .preload("books.author.books")
            .all()
            .unwrap();
        // 4 层 = 4 次 SQL，path 有限故自然终止
        assert_eq!(drv.calls.len(), 4);
        assert_eq!(rows[0].books[0].author.as_ref().unwrap().books.len(), 1);
    }

    #[test]
    fn preload_prefix_merge_keeps_longest() {
        let mut drv = PreloadDriver::with_parents(vec![author_row(1)]);
        let rows = NOrmQueryBuilder::<Author, _>::new(&mut drv)
            .preload("books")
            .preload("books.comments")
            .all()
            .unwrap();
        // 只查一次 books（前缀合并），共 3 次 SQL
        assert_eq!(drv.calls.len(), 3);
        assert_eq!(rows[0].books[0].comments.len(), 1);
    }

    #[test]
    fn preload_later_segment_error_is_delayed() {
        let mut drv = PreloadDriver::with_parents(vec![author_row(1)]);
        // Book 存在 author 与 comments，但不存在的段在执行期报错
        let err = NOrmQueryBuilder::<Author, _>::new(&mut drv)
            .preload("books.nonexistent")
            .all()
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(drv.calls.len() >= 2); // 主表与 books 已查，后续段才报错
    }

    #[test]
    fn preload_empty_segment_delayed_error() {
        let mut drv = PreloadDriver::with_parents(vec![author_row(1)]);
        let err = NOrmQueryBuilder::<Author, _>::new(&mut drv)
            .preload("books..author")
            .all()
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(drv.calls.is_empty());
    }

    #[test]
    fn preload_dedup_target_rows_by_pk() {
        // canned 返回同一 book 主键两行（title 不同），去重后只保留首次
        let drv = PreloadDriver::with_parents(vec![author_row(1)]);
        struct DupDriver {
            inner: PreloadDriver,
        }
        impl NOrmDriver for DupDriver {
            fn execute(&mut self, _: &str, _: &[NOrmValue]) -> Result<usize, Error> { Ok(0) }
            fn query(&mut self, sql: &str, params: &[NOrmValue]) -> Result<Vec<NOrmOwnedRow>, Error> {
                if sql.contains("FROM books") {
                    self.inner.calls.push((sql.to_string(), params.to_vec()));
                    let mut rows = Vec::new();
                    for v in params {
                        if let NOrmValue::I64(id) = v {
                            for (bid, title) in [(id * 10, "first"), (id * 10, "second")] {
                                rows.push(NOrmOwnedRow::new(
                                    vec!["id".into(), "author_id".into(), "title".into()],
                                    vec![NOrmValue::I64(bid), NOrmValue::I64(*id), NOrmValue::Text(title.into())],
                                ));
                            }
                        }
                    }
                    Ok(rows)
                } else {
                    self.inner.query(sql, params)
                }
            }
            fn last_insert_id(&self) -> Result<i64, Error> { Ok(0) }
            fn begin(&mut self) -> Result<(), Error> { Ok(()) }
            fn commit(&mut self) -> Result<(), Error> { Ok(()) }
            fn rollback(&mut self) -> Result<(), Error> { Ok(()) }
        }
        let mut dup = DupDriver { inner: drv };
        let rows = NOrmQueryBuilder::<Author, _>::new(&mut dup)
            .preload("books")
            .all()
            .unwrap();
        assert_eq!(rows[0].books.len(), 1);
        assert_eq!(rows[0].books[0].title, "first");
    }

    #[test]
    fn load_relation_single_entity_and_path() {
        let mut drv = PreloadDriver::with_parents(vec![author_row(1)]);
        let mut author = <Author as NOrmEntity>::from_row(&author_row(1)).unwrap();
        {
            let mut repo = crate::repository::NOrmRepository::new(&mut drv);
            repo.load_relation(&mut author, "books").unwrap();
            let _ = repo;
        }
        assert_eq!(author.books.len(), 1);
        // 实体已在外部取出，懒加载只发一条关联查询
        assert_eq!(drv.calls.len(), 1);

        let mut drv = PreloadDriver::with_parents(vec![author_row(1)]);
        let mut author = <Author as NOrmEntity>::from_row(&author_row(1)).unwrap();
        {
            let mut repo = crate::repository::NOrmRepository::new(&mut drv);
            repo.load_relation(&mut author, "books.comments").unwrap();
        }
        assert_eq!(author.books[0].comments.len(), 1);
    }

    #[test]
    fn load_relation_unknown_error() {
        let mut drv = PreloadDriver::default();
        let mut author = <Author as NOrmEntity>::from_row(&author_row(1)).unwrap();
        {
            let mut repo = crate::repository::NOrmRepository::new(&mut drv);
            let err = repo.load_relation(&mut author, "nope").unwrap_err();
            assert_eq!(err.kind(), ErrorKind::InvalidInput);
        }
        assert!(drv.calls.is_empty());
    }
}
