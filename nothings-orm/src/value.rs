//! 行值类型与双向转换。
//!
//! `Value` 覆盖 SQLite 的存储类；`ToValue` / `FromValue` 负责 Rust 类型与
//! 数据库值的双向转换，`HasSqlType` 提供静态 schema 所需的 SQL 类型标签。

use std::fmt;
use std::io::{Error, ErrorKind};

/// 数据库列的运行时值。
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    I64(i64),
    F64(f64),
    Text(String),
    Blob(Vec<u8>),
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
}
