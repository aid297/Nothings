//! 静态 schema 元信息。由 `#[derive(Entity)]` 生成实现。

use crate::value::SqlType;

/// 一个列的静态描述。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Column {
    pub field: &'static str,
    pub name: &'static str,
    pub sql_type: SqlType,
    pub nullable: bool,
    pub primary_key: bool,
}

/// 实体到表的静态映射。
pub trait Schema {
    const TABLE: &'static str;
    const COLUMNS: &'static [Column];

    fn pk() -> &'static Column;
}

/// 校验列名是否在 schema 白名单中。
///
/// 这是 QueryBuilder 的 SQL 注入防线：表名与列名不能参数化，
/// 必须来自编译期的 `Schema::COLUMNS`。
pub fn validate_column<S: Schema>(name: &str) -> Result<&'static Column, std::io::Error> {
    S::COLUMNS
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("表 {} 中不存在列 {}", S::TABLE, name),
            )
        })
}
