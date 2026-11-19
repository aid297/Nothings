//! 静态 schema 元信息。由 `#[derive(NOrmEntity)]` 生成实现。

use crate::value::NOrmSqlType;

/// 一个列的静态描述。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NOrmColumn {
    pub field: &'static str,
    pub name: &'static str,
    pub sql_type: NOrmSqlType,
    pub nullable: bool,
    pub primary_key: bool,
}

/// 实体到表的静态映射。
pub trait NOrmSchema {
    const TABLE: &'static str;
    const COLUMNS: &'static [NOrmColumn];

    fn pk() -> &'static NOrmColumn;
}

/// 校验列名是否在 schema 白名单中。
///
/// 这是 NOrmQueryBuilder 的 SQL 注入防线：表名与列名不能参数化，
/// 必须来自编译期的 `NOrmSchema::COLUMNS`。
pub fn validate_column<S: NOrmSchema>(name: &str) -> Result<&'static NOrmColumn, std::io::Error> {
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
