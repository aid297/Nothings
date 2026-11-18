//! 实体 trait：行 → 结构体的反序列化入口，以及按列名取值。

use std::io::Error;

use crate::row::Row;
use crate::schema::Schema;
use crate::value::Value;

/// 可映射到数据库表的实体。
///
/// `Schema`、`from_row`、`pk_value`、`column_value` 全部由派生宏生成，
/// 调用方无需手写行映射代码。
pub trait Entity: Schema + Sized {
    fn from_row<R: Row>(row: &R) -> Result<Self, Error>;

    fn pk_value(&self) -> Value;

    /// 按 schema 列名取出当前值；未知列名返回 InvalidInput。
    fn column_value(&self, name: &str) -> Result<Value, Error>;
}
