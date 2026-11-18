//! 链式查询构建器。
//!
//! 值一律通过 `?` 占位符 + params 传递；列名必须在 `E::COLUMNS` 白名单内，
//! 否则返回 `InvalidInput`。这是防止 SQL 注入的核心约束。

use std::io::{Error, ErrorKind};
use std::marker::PhantomData;

use crate::driver::Driver;
use crate::entity::Entity;
use crate::schema::validate_column;
use crate::value::{ToValue, Value};
use crate::row::Row;

/// 链式查询条件。方法返回 `&mut Self`，对齐 `Aes::set_key` 的风格。
pub struct QueryBuilder<'a, E: Entity, D: Driver> {
    driver: &'a mut D,
    conditions: Vec<String>,
    params: Vec<Value>,
    order: Option<(String, bool)>,
    limit: Option<usize>,
    offset: Option<usize>,
    phantom: PhantomData<E>,
}

impl<'a, E: Entity, D: Driver> QueryBuilder<'a, E, D> {
    pub(crate) fn new(driver: &'a mut D) -> Self {
        Self {
            driver,
            conditions: Vec::new(),
            params: Vec::new(),
            order: None,
            limit: None,
            offset: None,
            phantom: PhantomData,
        }
    }

    /// 追加一个等值条件。列名经过 schema 白名单校验。
    pub fn r#where(&mut self, column: &str, value: impl ToValue) -> &mut Self {
        match validate_column::<E>(column) {
            Ok(_) => {
                self.conditions.push(format!("{} = ?", column));
                self.params.push(value.to_value());
            }
            Err(err) => {
                // 列名非法时不生成任何 SQL；错误延后到 all()/one()/count() 暴露。
                self.conditions.push(format!("__invalid_column__:{}", err));
            }
        }
        self
    }

    pub fn order_by(&mut self, column: &str, asc: bool) -> &mut Self {
        match validate_column::<E>(column) {
            Ok(_) => self.order = Some((column.to_string(), asc)),
            Err(err) => self.conditions.push(format!("__invalid_column__:{}", err)),
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
        if let Some(bad) = self.conditions.iter().find(|c| c.starts_with("__invalid_column__:")) {
            return Err(Error::new(ErrorKind::InvalidInput, bad.clone()));
        }
        Ok(())
    }

    fn build_select(&self) -> (String, Vec<Value>) {
        let columns = E::COLUMNS
            .iter()
            .map(|c| c.name)
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql = format!("SELECT {} FROM {}", columns, E::TABLE);
        if !self.conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.conditions.join(" AND "));
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
        (sql, self.params.clone())
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
        if !self.conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.conditions.join(" AND "));
        }
        let rows = self.driver.query(&sql, &self.params)?;
        let row = rows.first().ok_or_else(|| {
            Error::new(ErrorKind::InvalidData, "COUNT 查询没有返回行")
        })?;
        for i in 0..Row::column_count(row) {
            if Row::column_name(row, i) == "__count" {
                return Row::value(row, i)?.to_i64();
            }
        }
        Err(Error::new(ErrorKind::InvalidData, "COUNT 查询缺少 __count 列"))
    }
}

/// 把 Value 转成 i64 的辅助方法（避免依赖具体驱动的类型）。
trait AsI64 {
    fn to_i64(&self) -> Result<i64, Error>;
}

impl AsI64 for Value {
    fn to_i64(&self) -> Result<i64, Error> {
        match self {
            Value::I64(n) => Ok(*n),
            Value::F64(n) => Ok(*n as i64),
            Value::Null => Ok(0),
            other => Err(Error::new(
                ErrorKind::InvalidData,
                format!("COUNT 返回了非数值: {other}"),
            )),
        }
    }
}
