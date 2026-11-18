//! MySQL 驱动：mysql(minimal-rust) 实现 nothings_orm::Driver。
//!
//! 每个 crate 都是 `crate-type = ["lib"]`，MySQL 客户端不会进入其它产物。

use std::io::{Error, ErrorKind};

use mysql::consts::{ColumnFlags, ColumnType};
use mysql::prelude::Queryable;
use mysql::{
    Column, Conn, DriverError, Error as MysqlError, Opts, OptsBuilder, Params, ServerError,
    Value as MysqlValue,
};
use nothings_orm::driver::Driver;
use nothings_orm::row::OwnedRow;
use nothings_orm::value::{DateTime, UtcOffset, Value};

/// MySQL 驱动，持有一条同步连接；并发场景每线程一个实例。
pub struct MysqlDriver {
    conn: Conn,
    orm_tz: UtcOffset,
    db_tz: UtcOffset,
}

impl MysqlDriver {
    /// 用 `mysql://user:pass@host:port/db` 建立连接，ORM 与 DB 时区均为 UTC（不做转换）。
    pub fn connect(url: &str) -> Result<Self, Error> {
        Conn::new(url)
            .map(|conn| Self {
                conn,
                orm_tz: UtcOffset::UTC,
                db_tz: UtcOffset::UTC,
            })
            .map_err(to_io)
    }

    /// 用双时区建立连接：`orm_tz` 是业务墙钟时区，`db_tz` 是库中时间表达的时区。
    ///
    /// 写入时 `ORM → DB`，读取时 `DB → ORM`，查询条件共用写入路径。
    /// 会话时区通过 `OptsBuilder::init` 下发 `SET time_zone`，连接重置时自动重放。
    /// DSN 不支持 `?timezone=` 参数（会报 `UrlError::UnknownParameter`）。
    pub fn connect_with_tz(
        url: &str,
        orm_tz: UtcOffset,
        db_tz: UtcOffset,
    ) -> Result<Self, Error> {
        let opts = Opts::from_url(url).map_err(|e| {
            Error::new(ErrorKind::InvalidInput, format!("MySQL 连接 URL 非法: {e}"))
        })?;
        let init_sql = format!("SET time_zone = '{}'", db_tz);
        let opts = OptsBuilder::from_opts(opts).init(vec![init_sql]);
        let conn = Conn::new(opts).map_err(to_io)?;
        let mut driver = Self {
            conn,
            orm_tz,
            db_tz,
        };
        // 防御：命名时区未加载 / 权限不足会静默错位，这里显式校验。
        let actual = driver
            .conn
            .query_first::<String, _>("SELECT @@session.time_zone")
            .map_err(to_io)?
            .unwrap_or_default();
        if actual != db_tz.to_string().as_str() {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("会话时区设置失败: 期望 {db_tz}，实际 {actual}"),
            ));
        }
        Ok(driver)
    }

    /// 执行单条原始 DDL，仅供建表/删表/测试；不参数化，禁止接用户输入。
    ///
    /// MySQL 默认禁用多语句，这里不叫 `execute_batch`。
    pub fn execute_ddl(&mut self, sql: &str) -> Result<(), Error> {
        self.conn.query_drop(sql).map_err(to_io)
    }
}

/// 把 mysql 的错误映射为 std::io::Error。
/// orphan rule 不允许 impl From，所以用固有函数。
fn to_io(err: MysqlError) -> Error {
    match &err {
        MysqlError::MySqlError(e) => {
            let kind = match e.code {
                code if code == ServerError::ER_DUP_ENTRY as u16 => ErrorKind::AlreadyExists,
                code if code == ServerError::ER_BAD_DB_ERROR as u16
                    || code == ServerError::ER_NO_SUCH_TABLE as u16 =>
                {
                    ErrorKind::NotFound
                }
                code if code == ServerError::ER_BAD_FIELD_ERROR as u16 => ErrorKind::InvalidInput,
                code if code == ServerError::ER_PARSE_ERROR as u16 => ErrorKind::InvalidData,
                code if code == ServerError::ER_ACCESS_DENIED_ERROR as u16 => {
                    ErrorKind::PermissionDenied
                }
                _ => ErrorKind::Other,
            };
            Error::new(
                kind,
                format!("MySQL 错误 {} ({}): {}", e.code, e.state, e.message),
            )
        }
        MysqlError::IoError(io) => Error::new(io.kind(), format!("MySQL I/O 错误: {io}")),
        MysqlError::UrlError(_) => {
            Error::new(ErrorKind::InvalidInput, format!("MySQL 连接 URL 非法: {err}"))
        }
        MysqlError::DriverError(
            DriverError::MismatchedStmtParams(..)
            | DriverError::MixedParams
            | DriverError::Params(_),
        ) => Error::new(ErrorKind::InvalidInput, format!("MySQL 参数错误: {err}")),
        MysqlError::CodecError(_)
        | MysqlError::FromValueError(_)
        | MysqlError::FromRowError(_) => {
            Error::new(ErrorKind::InvalidData, format!("MySQL 数据解码失败: {err}"))
        }
        _ => Error::other(format!("MySQL 错误: {err}")),
    }
}

fn to_sql(value: &Value, orm_tz: &UtcOffset, db_tz: &UtcOffset) -> MysqlValue {
    match value {
        Value::Null => MysqlValue::NULL,
        Value::Bool(v) => MysqlValue::Int(i64::from(*v)),
        Value::I64(v) => MysqlValue::Int(*v),
        Value::F64(v) => MysqlValue::Double(*v),
        Value::Text(v) => MysqlValue::Bytes(v.as_bytes().to_vec()),
        Value::Blob(v) => MysqlValue::Bytes(v.clone()),
        // 写侧类型标记明确，无需列名或列类型上下文。
        Value::DateTime(dt) => {
            let db_wall = dt.shifted_minutes(db_tz.diff_minutes(orm_tz));
            MysqlValue::Date(
                db_wall.year() as u16,
                db_wall.month(),
                db_wall.day(),
                db_wall.hour(),
                db_wall.minute(),
                db_wall.second(),
                db_wall.micros(),
            )
        }
    }
}

fn bound(values: &[Value], orm_tz: &UtcOffset, db_tz: &UtcOffset) -> Params {
    values
        .iter()
        .map(|v| to_sql(v, orm_tz, db_tz))
        .collect::<Vec<_>>()
        .into()
}

/// Text / Blob 的判定依据只能是列元数据，不能靠 UTF-8 试探：
/// BLOB 内容可能恰好是合法 UTF-8，而 JSON/DECIMAL 自带 binary charset。
fn is_binary(col: &Column) -> bool {
    match col.column_type() {
        // 降级为 Text 的类型：即便带 binary charset 也不当二进制。
        ColumnType::MYSQL_TYPE_JSON
        | ColumnType::MYSQL_TYPE_DECIMAL
        | ColumnType::MYSQL_TYPE_NEWDECIMAL
        | ColumnType::MYSQL_TYPE_DATE
        | ColumnType::MYSQL_TYPE_DATETIME
        | ColumnType::MYSQL_TYPE_DATETIME2
        | ColumnType::MYSQL_TYPE_TIMESTAMP
        | ColumnType::MYSQL_TYPE_TIMESTAMP2
        | ColumnType::MYSQL_TYPE_TIME
        | ColumnType::MYSQL_TYPE_TIME2
        | ColumnType::MYSQL_TYPE_YEAR
        | ColumnType::MYSQL_TYPE_ENUM
        | ColumnType::MYSQL_TYPE_SET => false,
        // 明确的二进制类型。
        ColumnType::MYSQL_TYPE_BLOB
        | ColumnType::MYSQL_TYPE_TINY_BLOB
        | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
        | ColumnType::MYSQL_TYPE_LONG_BLOB
        | ColumnType::MYSQL_TYPE_BIT
        | ColumnType::MYSQL_TYPE_GEOMETRY
        | ColumnType::MYSQL_TYPE_VECTOR => true,
        // 字符串与其它类型靠 flags / charset 判定。
        _ => col.flags().contains(ColumnFlags::BINARY_FLAG) || col.character_set() == 63,
    }
}

fn date_to_text(y: u16, mo: u8, d: u8, h: u8, mi: u8, s: u8, us: u32) -> String {
    let base = format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}");
    if us == 0 {
        base
    } else {
        format!("{base}.{us:06}")
    }
}

fn time_to_text(neg: bool, days: u32, h: u8, mi: u8, s: u8, us: u32) -> String {
    let hours = u64::from(days) * 24 + u64::from(h);
    let sign = if neg { "-" } else { "" };
    let base = format!("{sign}{hours:02}:{mi:02}:{s:02}");
    if us == 0 {
        base
    } else {
        format!("{base}.{us:06}")
    }
}

fn from_value(
    value: Option<&MysqlValue>,
    col: &Column,
    orm_tz: &UtcOffset,
    db_tz: &UtcOffset,
) -> Result<Value, Error> {
    let value = value.ok_or_else(|| {
        Error::new(ErrorKind::InvalidInput, "结果集缺少列值")
    })?;
    match value {
        MysqlValue::NULL => Ok(Value::Null),
        MysqlValue::Int(n) => Ok(Value::I64(*n)),
        MysqlValue::UInt(n) => i64::try_from(*n).map(Value::I64).map_err(|_| {
            Error::new(
                ErrorKind::InvalidData,
                format!("无符号整数 {n} 超出 i64 范围"),
            )
        }),
        MysqlValue::Float(f) => Ok(Value::F64(f64::from(*f))),
        MysqlValue::Double(f) => Ok(Value::F64(*f)),
        MysqlValue::Bytes(bytes) => {
            if is_binary(col) {
                Ok(Value::Blob(bytes.clone()))
            } else {
                String::from_utf8(bytes.clone()).map(Value::Text).map_err(|e| {
                    Error::new(ErrorKind::InvalidData, format!("非法 UTF-8 文本: {e}"))
                })
            }
        }
        MysqlValue::Date(y, mo, d, h, mi, s, us) => {
            match col.column_type() {
                // 完整墙钟：统一返回 DateTime，DB → ORM 换算。
                ColumnType::MYSQL_TYPE_TIMESTAMP
                | ColumnType::MYSQL_TYPE_TIMESTAMP2
                | ColumnType::MYSQL_TYPE_DATETIME
                | ColumnType::MYSQL_TYPE_DATETIME2 => {
                    let db_wall = DateTime::new(
                        i32::from(*y), *mo, *d, *h, *mi, *s, *us,
                    )?;
                    Ok(Value::DateTime(db_wall.shifted_minutes(orm_tz.diff_minutes(db_tz))))
                }
                // DATE / TIME / YEAR 无完整墙钟，维持文本。
                _ => Ok(Value::Text(date_to_text(*y, *mo, *d, *h, *mi, *s, *us))),
            }
        }
        MysqlValue::Time(neg, days, h, mi, s, us) => Ok(Value::Text(time_to_text(
            *neg, *days, *h, *mi, *s, *us,
        ))),
    }
}

impl Driver for MysqlDriver {
    fn execute(&mut self, sql: &str, values: &[Value]) -> Result<usize, Error> {
        // exec_drop 会丢弃 QueryResult 并触发其 Drop 消费结果集，
        // 之后才能读 conn.affected_rows()（QueryResult 持有 &mut Conn）。
        self.conn
            .exec_drop(sql, bound(values, &self.orm_tz, &self.db_tz))
            .map_err(to_io)?;
        usize::try_from(self.conn.affected_rows()).map_err(|_| {
            Error::new(ErrorKind::InvalidData, "受影响行数超出 usize 范围")
        })
    }

    fn query(&mut self, sql: &str, values: &[Value]) -> Result<Vec<OwnedRow>, Error> {
        // 带 ? 参数必须走二进制协议（prepared statement）；query_iter 不接受 params。
        let params = bound(values, &self.orm_tz, &self.db_tz);
        let result_set = self.conn.exec_iter(sql, params).map_err(to_io)?;
        let columns = result_set.columns().as_ref().to_vec();
        // 列名/别名原样保留：__count 与 #[entity(column=...)] 都是大小写敏感精确匹配。
        let names = columns
            .iter()
            .map(|c| c.name_str().into_owned())
            .collect::<Vec<_>>();
        let mut rows = Vec::new();
        for row in result_set {
            let fields = row.map_err(to_io)?.unwrap();
            let mut row_values = Vec::with_capacity(names.len());
            for (i, col) in columns.iter().enumerate() {
                row_values.push(from_value(fields.get(i), col, &self.orm_tz, &self.db_tz)?);
            }
            rows.push(OwnedRow::new(names.clone(), row_values));
        }
        Ok(rows)
    }

    fn last_insert_id(&self) -> Result<i64, Error> {
        i64::try_from(self.conn.last_insert_id()).map_err(|_| {
            Error::new(ErrorKind::InvalidData, "自增主键超出 i64 范围")
        })
    }

    // Conn::transaction() 借用 &mut Conn 且 Drop 隐式 rollback，无法存进 self，
    // 因此让服务端 session 管理事务状态；autocommit 保持默认 1。
    fn begin(&mut self) -> Result<(), Error> {
        self.conn.query_drop("START TRANSACTION").map_err(to_io)
    }

    fn commit(&mut self) -> Result<(), Error> {
        self.conn.query_drop("COMMIT").map_err(to_io)
    }

    fn rollback(&mut self) -> Result<(), Error> {
        self.conn.query_drop("ROLLBACK").map_err(to_io)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_to_sql() {
        let utc = UtcOffset::UTC;
        assert_eq!(to_sql(&Value::Null, &utc, &utc), MysqlValue::NULL);
        assert_eq!(to_sql(&Value::Bool(true), &utc, &utc), MysqlValue::Int(1));
        assert_eq!(to_sql(&Value::Bool(false), &utc, &utc), MysqlValue::Int(0));
        assert_eq!(to_sql(&Value::I64(1), &utc, &utc), MysqlValue::Int(1));
        assert_eq!(to_sql(&Value::F64(1.5), &utc, &utc), MysqlValue::Double(1.5));
        assert_eq!(
            to_sql(&Value::Text("x".into()), &utc, &utc),
            MysqlValue::Bytes(b"x".to_vec())
        );
        assert_eq!(
            to_sql(&Value::Blob(vec![1, 2]), &utc, &utc),
            MysqlValue::Bytes(vec![1, 2])
        );
    }

    #[test]
    fn test_to_sql_datetime_shift() {
        let utc = UtcOffset::UTC;
        let sh = UtcOffset::parse("+08:00").unwrap();
        let dt = DateTime::new(2026, 1, 2, 9, 0, 0, 0).unwrap();
        // ORM=UTC, DB=+08:00 → 写库墙钟 17:00
        assert_eq!(
            to_sql(&Value::DateTime(dt.clone()), &utc, &sh),
            MysqlValue::Date(2026, 1, 2, 17, 0, 0, 0)
        );
        // ORM=+08:00, DB=UTC → 写库墙钟 01:00
        assert_eq!(
            to_sql(&Value::DateTime(dt), &sh, &utc),
            MysqlValue::Date(2026, 1, 2, 1, 0, 0, 0)
        );
    }

    #[test]
    fn test_bound() {
        let utc = UtcOffset::UTC;
        assert_eq!(bound(&[], &utc, &utc), Params::Empty);
        assert_eq!(
            bound(&[Value::I64(7), Value::Null], &utc, &utc),
            Params::Positional(vec![MysqlValue::Int(7), MysqlValue::NULL])
        );
    }

    #[test]
    fn test_date_time_text() {
        assert_eq!(date_to_text(2026, 1, 2, 3, 4, 5, 0), "2026-01-02 03:04:05");
        assert_eq!(
            date_to_text(2026, 1, 2, 3, 4, 5, 12),
            "2026-01-02 03:04:05.000012"
        );
        assert_eq!(time_to_text(true, 1, 2, 3, 4, 0), "-26:03:04");
    }

    #[test]
    fn test_to_io() {
        let dup = MysqlError::MySqlError(mysql::MySqlError {
            state: "23000".into(),
            message: "Duplicate entry".into(),
            code: ServerError::ER_DUP_ENTRY as u16,
        });
        assert_eq!(to_io(dup).kind(), ErrorKind::AlreadyExists);

        let syntax = MysqlError::MySqlError(mysql::MySqlError {
            state: "42000".into(),
            message: "syntax error".into(),
            code: ServerError::ER_PARSE_ERROR as u16,
        });
        assert_eq!(to_io(syntax).kind(), ErrorKind::InvalidData);

        let url = MysqlError::UrlError(mysql::UrlError::BadUrl);
        assert_eq!(to_io(url).kind(), ErrorKind::InvalidInput);
    }
}
