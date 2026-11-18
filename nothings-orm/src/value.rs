//! 行值类型与双向转换。
//!
//! `Value` 覆盖 SQLite 的存储类；`ToValue` / `FromValue` 负责 Rust 类型与
//! 数据库值的双向转换，`HasSqlType` 提供静态 schema 所需的 SQL 类型标签。

use std::fmt;
use std::io::{Error, ErrorKind};

/// 固定 UTC 偏移，形如 `+08:00` / `-05:00` / `+05:45`。
///
/// 用分钟存储，不含 DST 规则——固定偏移没有夏令时跳变，避免本地时间消歧问题。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UtcOffset {
    total_minutes: i32,
}

impl UtcOffset {
    pub const UTC: Self = Self { total_minutes: 0 };

    /// 解析 `+08:00` / `-05:30` / `+05:45` 形式的偏移。
    pub fn parse(text: &str) -> Result<Self, Error> {
        let bytes = text.as_bytes();
        if bytes.len() != 6 || (bytes[0] != b'+' && bytes[0] != b'-') || bytes[3] != b':' {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!("非法 UTC 偏移格式: {text}（应为 +08:00 形式）"),
            ));
        }
        let hours = text[1..3].parse::<i32>().map_err(|_| {
            Error::new(ErrorKind::InvalidInput, format!("非法 UTC 偏移小时: {text}"))
        })?;
        let minutes = text[4..6].parse::<i32>().map_err(|_| {
            Error::new(ErrorKind::InvalidInput, format!("非法 UTC 偏移分钟: {text}"))
        })?;
        if hours > 23 || minutes > 59 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!("UTC 偏移超出范围: {text}"),
            ));
        }
        let total = hours * 60 + minutes;
        Ok(Self {
            total_minutes: if bytes[0] == b'-' { -total } else { total },
        })
    }

    /// 以分钟为单位的偏移，东为正。
    pub fn total_minutes(&self) -> i32 {
        self.total_minutes
    }

    /// 与另一个偏移的差（self - other），单位分钟。
    pub fn diff_minutes(&self, other: &Self) -> i32 {
        self.total_minutes - other.total_minutes
    }
}

impl fmt::Display for UtcOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.total_minutes < 0 { '-' } else { '+' };
        let abs = self.total_minutes.unsigned_abs();
        write!(f, "{sign}{:02}:{:02}", abs / 60, abs % 60)
    }
}

/// 墙钟日期时间，语义由使用方约定（在 MySQL 驱动里表示「按 ORM 时区解释」）。
///
/// 不携带时区：时区是连接级配置，不是值的一部分。无外部依赖。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateTime {
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    micros: u32,
}

impl DateTime {
    /// 构造并校验日期时间，非法值返回 `InvalidInput`。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        year: i32,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
        micros: u32,
    ) -> Result<Self, Error> {
        if !(1..=12).contains(&month)
            || day == 0
            || day > days_in_month(year, month)
            || hour > 23
            || minute > 59
            || second > 59
            || micros > 999_999
        {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!("非法日期时间: {year}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{micros:06}"),
            ));
        }
        Ok(Self { year, month, day, hour, minute, second, micros })
    }

    pub fn year(&self) -> i32 { self.year }
    pub fn month(&self) -> u8 { self.month }
    pub fn day(&self) -> u8 { self.day }
    pub fn hour(&self) -> u8 { self.hour }
    pub fn minute(&self) -> u8 { self.minute }
    pub fn second(&self) -> u8 { self.second }
    pub fn micros(&self) -> u32 { self.micros }

    /// 按给定偏移解释这个墙钟，返回 Unix 秒（UTC）。
    pub fn to_unix_secs(&self, offset: &UtcOffset) -> i64 {
        let days = days_from_civil(self.year, self.month, self.day);
        let secs = days * 86_400
            + i64::from(self.hour) * 3600
            + i64::from(self.minute) * 60
            + i64::from(self.second);
        secs - i64::from(offset.total_minutes()) * 60
    }

    /// 从 Unix 秒构造指定偏移下的墙钟，微秒为 0。
    pub fn from_unix_secs(secs: i64, offset: &UtcOffset) -> Self {
        let shifted = secs + i64::from(offset.total_minutes()) * 60;
        let days = shifted.div_euclid(86_400);
        let rem = shifted.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: (rem / 3600) as u8,
            minute: ((rem % 3600) / 60) as u8,
            second: (rem % 60) as u8,
            micros: 0,
        }
    }

    /// 按分钟差平移墙钟（用于 ORM 时区与 DB 时区之间换算）。
    pub fn shifted_minutes(&self, delta_minutes: i32) -> Self {
        let total_secs = self.to_unix_secs(&UtcOffset::UTC) + i64::from(delta_minutes) * 60;
        let mut out = Self::from_unix_secs(total_secs, &UtcOffset::UTC);
        out.micros = self.micros;
        out
    }

    /// 解析 `YYYY-MM-DD HH:MM:SS[.ffffff]`。
    pub fn parse(text: &str) -> Result<Self, Error> {
        let text = text.trim();
        if text.len() < 19 {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("非法日期时间字符串: {text}"),
            ));
        }
        let year = text[0..4].parse::<i32>().map_err(|_| invalid_text(text))?;
        let month = text[5..7].parse::<u8>().map_err(|_| invalid_text(text))?;
        let day = text[8..10].parse::<u8>().map_err(|_| invalid_text(text))?;
        let hour = text[11..13].parse::<u8>().map_err(|_| invalid_text(text))?;
        let minute = text[14..16].parse::<u8>().map_err(|_| invalid_text(text))?;
        let second = text[17..19].parse::<u8>().map_err(|_| invalid_text(text))?;
        let micros = if text.len() > 19 {
            let frac = &text[20..];
            if text.as_bytes()[19] != b'.' || frac.is_empty() || frac.len() > 6 {
                return Err(invalid_text(text));
            }
            let padded = format!("{frac:0<6}");
            padded.parse::<u32>().map_err(|_| invalid_text(text))?
        } else {
            0
        };
        Self::new(year, month, day, hour, minute, second, micros)
            .map_err(|_| invalid_text(text))
    }
}

fn invalid_text(text: &str) -> Error {
    Error::new(ErrorKind::InvalidData, format!("非法日期时间字符串: {text}"))
}

fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if is_leap(year) { 29 } else { 28 },
        _ => 0,
    }
}

/// 公历日期 → 距 1970-01-01 的天数（Howard Hinnant civil_from_days 的逆运算）。
fn days_from_civil(y: i32, m: u8, d: u8) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = i64::from(m) + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 距 1970-01-01 的天数 → 公历日期。
fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    ((y + i64::from(m <= 2)) as i32, m as u8, d as u8)
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.micros == 0 {
            write!(f, "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                self.year, self.month, self.day, self.hour, self.minute, self.second)
        } else {
            write!(f, "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}",
                self.year, self.month, self.day, self.hour, self.minute, self.second, self.micros)
        }
    }
}

/// 数据库列的运行时值。
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    I64(i64),
    F64(f64),
    Text(String),
    Blob(Vec<u8>),
    /// 墙钟时间，语义由驱动的连接时区配置决定并自动换算。
    DateTime(DateTime),
}

impl ToValue for Value {
    fn to_value(&self) -> Value {
        self.clone()
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Bool(v) => write!(f, "{v}"),
            Value::I64(v) => write!(f, "{v}"),
            Value::F64(v) => write!(f, "{v}"),
            Value::Text(v) => write!(f, "{v}"),
            Value::Blob(v) => write!(f, "Blob({} bytes)", v.len()),
            Value::DateTime(v) => write!(f, "{v}"),
        }
    }
}

/// SQL 类型的静态标签，由 `HasSqlType` 关联常量提供。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqlType {
    Integer,
    Real,
    Text,
    Blob,
}

/// Rust 值 → 数据库值。
pub trait ToValue {
    fn to_value(&self) -> Value;
}

/// 数据库值 → Rust 值。
pub trait FromValue: Sized {
    fn from_value(value: &Value) -> Result<Self, Error>;
}

/// 字段类型的静态 SQL 类型标签。
pub trait HasSqlType {
    const SQL_TYPE: SqlType;
}

fn invalid(value: &Value, target: &str) -> Error {
    Error::new(
        ErrorKind::InvalidData,
        format!("无法将 {value} 转换为 {target}"),
    )
}

macro_rules! impl_integer {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl ToValue for $ty {
                fn to_value(&self) -> Value {
                    Value::I64(*self as i64)
                }
            }
            impl FromValue for $ty {
                fn from_value(value: &Value) -> Result<Self, Error> {
                    match value {
                        Value::I64(n) => <$ty>::try_from(*n)
                            .map_err(|_| invalid(value, stringify!($ty))),
                        Value::F64(n) => Ok(*n as $ty),
                        Value::Text(s) => s
                            .parse::<$ty>()
                            .map_err(|_| invalid(value, stringify!($ty))),
                        _ => Err(invalid(value, stringify!($ty))),
                    }
                }
            }
            impl HasSqlType for $ty {
                const SQL_TYPE: SqlType = SqlType::Integer;
            }
        )+
    };
}

impl_integer!(i8, i16, i32, i64, u8, u16, u32);

macro_rules! impl_float {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl ToValue for $ty {
                fn to_value(&self) -> Value {
                    Value::F64(*self as f64)
                }
            }
            impl FromValue for $ty {
                fn from_value(value: &Value) -> Result<Self, Error> {
                    match value {
                        Value::F64(n) => Ok(*n as $ty),
                        Value::I64(n) => Ok(*n as $ty),
                        Value::Text(s) => s
                            .parse::<$ty>()
                            .map_err(|_| invalid(value, stringify!($ty))),
                        _ => Err(invalid(value, stringify!($ty))),
                    }
                }
            }
            impl HasSqlType for $ty {
                const SQL_TYPE: SqlType = SqlType::Real;
            }
        )+
    };
}

impl_float!(f32, f64);

impl ToValue for bool {
    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }
}
impl FromValue for bool {
    fn from_value(value: &Value) -> Result<Self, Error> {
        match value {
            Value::Bool(v) => Ok(*v),
            Value::I64(v) => Ok(*v != 0),
            _ => Err(invalid(value, "bool")),
        }
    }
}
impl HasSqlType for bool {
    const SQL_TYPE: SqlType = SqlType::Integer;
}

impl ToValue for &str {
    fn to_value(&self) -> Value {
        Value::Text((*self).to_string())
    }
}

impl ToValue for String {
    fn to_value(&self) -> Value {
        Value::Text(self.clone())
    }
}
impl FromValue for String {
    fn from_value(value: &Value) -> Result<Self, Error> {
        match value {
            Value::Text(s) => Ok(s.clone()),
            _ => Err(invalid(value, "String")),
        }
    }
}
impl HasSqlType for String {
    const SQL_TYPE: SqlType = SqlType::Text;
}

impl ToValue for Vec<u8> {
    fn to_value(&self) -> Value {
        Value::Blob(self.clone())
    }
}
impl FromValue for Vec<u8> {
    fn from_value(value: &Value) -> Result<Self, Error> {
        match value {
            Value::Blob(v) => Ok(v.clone()),
            _ => Err(invalid(value, "Vec<u8>")),
        }
    }
}
impl HasSqlType for Vec<u8> {
    const SQL_TYPE: SqlType = SqlType::Blob;
}

impl<T: ToValue> ToValue for Option<T> {
    fn to_value(&self) -> Value {
        match self {
            Some(v) => v.to_value(),
            None => Value::Null,
        }
    }
}
impl<T: FromValue> FromValue for Option<T> {
    fn from_value(value: &Value) -> Result<Self, Error> {
        match value {
            Value::Null => Ok(None),
            other => Ok(Some(T::from_value(other)?)),
        }
    }
}
impl<T: HasSqlType> HasSqlType for Option<T> {
    const SQL_TYPE: SqlType = T::SQL_TYPE;
}

impl ToValue for DateTime {
    fn to_value(&self) -> Value {
        Value::DateTime(self.clone())
    }
}

impl FromValue for DateTime {
    fn from_value(value: &Value) -> Result<Self, Error> {
        match value {
            Value::DateTime(dt) => Ok(dt.clone()),
            // SQLite 与历史数据把时间存成文本，按标准格式兜底解析。
            Value::Text(s) => DateTime::parse(s),
            other => Err(invalid(other, "DateTime")),
        }
    }
}

impl HasSqlType for DateTime {
    const SQL_TYPE: SqlType = SqlType::Text;
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_integer_roundtrip() {
        assert_eq!(42i32.to_value(), Value::I64(42));
        assert_eq!(i32::from_value(&Value::I64(42)).unwrap(), 42);
        assert!(i32::from_value(&Value::I64(i64::MAX)).is_err());
    }

    #[test]
    fn test_option_null() {
        let none: Option<i32> = None;
        assert_eq!(none.to_value(), Value::Null);
        assert_eq!(Option::<i32>::from_value(&Value::Null).unwrap(), None);
        assert_eq!(
            Option::<i32>::from_value(&Value::I64(7)).unwrap(),
            Some(7)
        );
    }

    #[test]
    fn test_sql_type() {
        assert_eq!(<String as HasSqlType>::SQL_TYPE, SqlType::Text);
        assert_eq!(<Option<i64> as HasSqlType>::SQL_TYPE, SqlType::Integer);
    }

    #[test]
    fn test_utc_offset_parse() {
        assert_eq!(UtcOffset::parse("+08:00").unwrap().total_minutes(), 480);
        assert_eq!(UtcOffset::parse("-05:00").unwrap().total_minutes(), -300);
        assert_eq!(UtcOffset::parse("+05:45").unwrap().total_minutes(), 345);
        assert_eq!(UtcOffset::UTC.to_string(), "+00:00");
        assert_eq!(UtcOffset::parse("-03:30").unwrap().to_string(), "-03:30");
        assert!(UtcOffset::parse("8:00").is_err());
        assert!(UtcOffset::parse("+24:00").is_err());
        assert!(UtcOffset::parse("+08:60").is_err());
    }

    #[test]
    fn test_datetime_parse_and_display() {
        let dt = DateTime::parse("2026-01-02 03:04:05").unwrap();
        assert_eq!(dt.to_string(), "2026-01-02 03:04:05");
        let dt2 = DateTime::parse("2026-01-02 03:04:05.123456").unwrap();
        assert_eq!(dt2.to_string(), "2026-01-02 03:04:05.123456");
        assert_eq!(dt2.micros(), 123456);
        assert!(DateTime::parse("2026-13-02 03:04:05").is_err());
        assert!(DateTime::new(2025, 2, 29, 0, 0, 0, 0).is_err());
        assert!(DateTime::new(2024, 2, 29, 0, 0, 0, 0).is_ok());
    }

    #[test]
    fn test_datetime_unix_roundtrip() {
        let tz8 = UtcOffset::parse("+08:00").unwrap();
        let dt = DateTime::new(2026, 1, 2, 11, 4, 5, 0).unwrap();
        let secs = dt.to_unix_secs(&tz8);
        let back = DateTime::from_unix_secs(secs, &tz8);
        assert_eq!(dt, back);
        // 11:04 +08:00 = 03:04 UTC
        let utc = DateTime::from_unix_secs(secs, &UtcOffset::UTC);
        assert_eq!(utc.to_string(), "2026-01-02 03:04:05");
    }

    #[test]
    fn test_datetime_shift_cross_boundaries() {
        let dt = DateTime::new(2024, 2, 29, 23, 30, 0, 999).unwrap();
        let shifted = dt.shifted_minutes(60);
        assert_eq!(shifted.to_string(), "2024-03-01 00:30:00.000999");
        let back = shifted.shifted_minutes(-60);
        assert_eq!(back, dt);

        let year_end = DateTime::new(2026, 12, 31, 23, 0, 0, 0).unwrap();
        assert_eq!(
            year_end.shifted_minutes(120).to_string(),
            "2027-01-01 01:00:00"
        );

        let quarter = UtcOffset::parse("+05:45").unwrap();
        let dt2 = DateTime::new(2026, 6, 1, 0, 0, 0, 0).unwrap();
        let secs = dt2.to_unix_secs(&quarter);
        assert_eq!(DateTime::from_unix_secs(secs, &quarter), dt2);
    }

    #[test]
    fn test_datetime_value_conversion() {
        let dt = DateTime::new(2026, 1, 2, 3, 4, 5, 6).unwrap();
        assert_eq!(dt.to_value(), Value::DateTime(dt.clone()));
        assert_eq!(DateTime::from_value(&Value::DateTime(dt.clone())).unwrap(), dt);
        let as_text = DateTime::from_value(&Value::Text(dt.to_string())).unwrap();
        assert_eq!(as_text, dt);
    }
}
