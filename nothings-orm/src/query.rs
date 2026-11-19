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

use std::io::{Error, ErrorKind};
use std::marker::PhantomData;

use crate::condition::{NOrmCondition, NOrmOperator};
use crate::driver::NOrmDriver;
use crate::entity::NOrmEntity;
use crate::schema::validate_column;
use crate::value::{NOrmToValue, NOrmValue};
use crate::row::NOrmRow;

/// 链式查询条件。方法返回 `&mut Self`，对齐 `Aes::set_key` 的风格。
pub struct NOrmQueryBuilder<'a, E: NOrmEntity, D: NOrmDriver> {
    driver: &'a mut D,
    where_: Option<NOrmCondition<E>>,
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

impl<'a, E: NOrmEntity, D: NOrmDriver> NOrmQueryBuilder<'a, E, D> {
    pub(crate) fn new(driver: &'a mut D) -> Self {
        Self {
            driver,
            where_: None,
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

    fn check_valid(&self) -> Result<(), Error> {
        if let Some(message) = self.where_.as_ref().and_then(|c| c.first_error()) {
            return Err(Error::new(ErrorKind::InvalidInput, message.to_string()));
        }
        if let Some(message) = &self.order_error {
            return Err(Error::new(ErrorKind::InvalidInput, message.clone()));
        }
        Ok(())
    }

    /// 唯一的 WHERE 渲染入口：返回 (不含 " WHERE " 的片段, 参数)。
    fn render_where(&self) -> (Option<String>, Vec<NOrmValue>) {
        match &self.where_ {
            None => (None, Vec::new()),
            Some(condition) => {
                let mut sql = String::new();
                let mut params = Vec::new();
                condition.render_into(&mut sql, &mut params);
                (Some(sql), params)
            }
        }
    }

    fn build_select(&self) -> (String, Vec<NOrmValue>) {
        let columns = E::COLUMNS
            .iter()
            .map(|c| c.name)
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql = format!("SELECT {} FROM {}", columns, E::TABLE);
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
        rows.iter().map(|row| E::from_row(row)).collect()
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
        let mut sql = format!("SELECT COUNT(*) AS __count FROM {}", E::TABLE);
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
}
