//! SQLite 驱动：rusqlite(bundled) 实现 nothings_orm::NOrmDriver。
//!
//! 每个 crate 都是 `crate-type = ["lib"]`，SQLite 的 C 代码不会进入其它产物。

use std::io::{Error, ErrorKind};

use nothings_orm::driver::NOrmDriver;
use nothings_orm::row::NOrmOwnedRow;
use nothings_orm::value::NOrmValue;
use rusqlite::params_from_iter;
use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{Connection, ErrorCode};

/// SQLite 驱动，持有一条连接。
pub struct NOrmSqliteDriver {
    conn: Connection,
}

impl NOrmSqliteDriver {
    /// 打开（或创建）数据库文件。
    pub fn open(path: &str) -> Result<Self, Error> {
        Connection::open(path).map(|conn| Self { conn }).map_err(to_io)
    }

    /// 打开内存数据库，常用于测试。
    pub fn in_memory() -> Result<Self, Error> {
        Connection::open_in_memory().map(|conn| Self { conn }).map_err(to_io)
    }

    /// 直接执行 DDL，供测试建表使用。
    pub fn execute_batch(&mut self, sql: &str) -> Result<(), Error> {
        self.conn.execute_batch(sql).map_err(to_io)
    }
}

/// 把 rusqlite 的错误映射为 std::io::Error。
/// orphan rule 不允许 impl From，所以用固有函数。
fn to_io(err: rusqlite::Error) -> Error {
    match &err {
        rusqlite::Error::SqliteFailure(code, msg) => {
            let kind = match code.code {
                ErrorCode::ConstraintViolation => ErrorKind::AlreadyExists,
                ErrorCode::CannotOpen | ErrorCode::NotFound => ErrorKind::NotFound,
                ErrorCode::ApiMisuse | ErrorCode::ParameterOutOfRange => ErrorKind::InvalidInput,
                ErrorCode::NotADatabase | ErrorCode::TypeMismatch | ErrorCode::DatabaseCorrupt => {
                    ErrorKind::InvalidData
                }
                _ => ErrorKind::Other,
            };
            let text = msg
                .as_deref()
                .map(|m| format!("SQLite 错误 {:?}: {m}", code.code))
                .unwrap_or_else(|| format!("SQLite 错误 {:?}", code.code));
            Error::new(kind, text)
        }
        rusqlite::Error::QueryReturnedNoRows => Error::new(ErrorKind::NotFound, "查询未返回行"),
        rusqlite::Error::InvalidColumnName(name) => {
            Error::new(ErrorKind::InvalidInput, format!("未知列: {name}"))
        }
        rusqlite::Error::InvalidColumnIndex(i) => {
            Error::new(ErrorKind::InvalidInput, format!("列索引越界: {i}"))
        }
        other => Error::other(other.to_string()),
    }
}

fn from_sql(value: ValueRef<'_>) -> Result<NOrmValue, Error> {
    match value {
        ValueRef::Null => Ok(NOrmValue::Null),
        ValueRef::Integer(n) => Ok(NOrmValue::I64(n)),
        ValueRef::Real(n) => Ok(NOrmValue::F64(n)),
        ValueRef::Text(bytes) => std::str::from_utf8(bytes)
            .map(|s| NOrmValue::Text(s.to_string()))
            .map_err(|e| Error::new(ErrorKind::InvalidData, format!("非法 UTF-8 文本: {e}"))),
        ValueRef::Blob(bytes) => Ok(NOrmValue::Blob(bytes.to_vec())),
    }
}

fn to_sql(value: &NOrmValue) -> SqlValue {
    match value {
        NOrmValue::Null => SqlValue::Null,
        NOrmValue::Bool(v) => SqlValue::Integer(i64::from(*v)),
        NOrmValue::I64(v) => SqlValue::Integer(*v),
        NOrmValue::F64(v) => SqlValue::Real(*v),
        NOrmValue::Text(v) => SqlValue::Text(v.clone()),
        NOrmValue::Blob(v) => SqlValue::Blob(v.clone()),
        // SQLite 无列类型、无会话时区：按 ORM 墙钟原样写 TEXT。
        NOrmValue::NOrmDateTime(v) => SqlValue::Text(v.to_string()),
    }
}

fn bound(values: &[NOrmValue]) -> Vec<SqlValue> {
    values.iter().map(to_sql).collect()
}

impl NOrmDriver for NOrmSqliteDriver {
    fn execute(&mut self, sql: &str, values: &[NOrmValue]) -> Result<usize, Error> {
        self.conn
            .execute(sql, params_from_iter(bound(values)))
            .map_err(to_io)
    }

    fn query(&mut self, sql: &str, values: &[NOrmValue]) -> Result<Vec<NOrmOwnedRow>, Error> {
        let mut stmt = self.conn.prepare(sql).map_err(to_io)?;
        let names = stmt
            .column_names()
            .into_iter()
            .map(|name| name.to_string())
            .collect::<Vec<_>>();
        let mut rows = stmt
            .query(params_from_iter(bound(values)))
            .map_err(to_io)?;
        let mut result = Vec::new();
        while let Some(row) = rows.next().map_err(to_io)? {
            let mut values = Vec::with_capacity(names.len());
            for i in 0..names.len() {
                values.push(from_sql(row.get_ref(i).map_err(to_io)?)?);
            }
            result.push(NOrmOwnedRow::new(names.clone(), values));
        }
        Ok(result)
    }

    fn last_insert_id(&self) -> Result<i64, Error> {
        Ok(self.conn.last_insert_rowid())
    }

    fn begin(&mut self) -> Result<(), Error> {
        self.conn.execute_batch("BEGIN").map_err(to_io)
    }

    fn commit(&mut self) -> Result<(), Error> {
        self.conn.execute_batch("COMMIT").map_err(to_io)
    }

    fn rollback(&mut self) -> Result<(), Error> {
        self.conn.execute_batch("ROLLBACK").map_err(to_io)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_to_sql() {
        assert_eq!(to_sql(&NOrmValue::I64(1)), SqlValue::Integer(1));
        assert_eq!(to_sql(&NOrmValue::Bool(true)), SqlValue::Integer(1));
        assert_eq!(to_sql(&NOrmValue::Null), SqlValue::Null);
    }

    #[test]
    fn test_in_memory() {
        let mut driver = NOrmSqliteDriver::in_memory().unwrap();
        driver.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)").unwrap();
        assert_eq!(driver.execute("INSERT INTO t (name) VALUES (?)", &[NOrmValue::Text("x".into())]).unwrap(), 1);
        let rows = driver.query("SELECT id, name FROM t", &[]).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].values()[1], NOrmValue::Text("x".into()));
    }
}
