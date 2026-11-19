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

/// 关联种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NOrmRelationKind {
    /// 一对多：本实体主键 ↔ 目标表外键，填充 `Vec<T>`。
    HasMany,
    /// 多对一：本实体外键 ↔ 目标表主键，填充 `Option<T>`。
    BelongsTo,
}

/// 一条关联的静态元信息，由 `#[derive(NOrmEntity)]` 生成。
///
/// 只含 `'static` 纯数据（无函数指针），因此可作为 `NOrmSchema::RELATIONS`
/// 关联常量。目标实体类型在宏展开期捕获，这里只存它的表元信息。
#[derive(Clone, Copy, Debug)]
pub struct NOrmRelation {
    /// `preload(name)` 使用的关系名。
    pub name: &'static str,
    pub kind: NOrmRelationKind,
    /// 外键 DB 列名：HasMany 时在目标表上，BelongsTo 时在本表上。
    pub foreign_key: &'static str,
    /// 目标表名 `<T as NOrmSchema>::TABLE`。
    pub target_table: &'static str,
    /// 目标全列 `<T as NOrmSchema>::COLUMNS`，用于 SELECT 与列名白名单。
    pub target_columns: &'static [NOrmColumn],
    /// 目标实体的软删除列（`<T as NOrmSchema>::SOFT_DELETE_COL`）。
    /// 预加载的关联查询据此带上 `IS NULL` 过滤，避免带出已删除的关联行。
    pub soft_delete_col: Option<&'static str>,
}

/// 实体到表的静态映射。
pub trait NOrmSchema {
    const TABLE: &'static str;
    const COLUMNS: &'static [NOrmColumn];
    /// 关联元信息。带默认空值，未声明关联的实体零改动。
    const RELATIONS: &'static [NOrmRelation] = &[];
    /// 软删除列（`#[entity(soft_delete)]`）。None = 未启用。
    const SOFT_DELETE_COL: Option<&'static str> = None;
    /// 插入时注入当前 UTC 时间的列（`#[entity(created_at)]`）。
    const CREATED_AT_COL: Option<&'static str> = None;
    /// 插入/更新时注入当前 UTC 时间的列（`#[entity(updated_at)]`）。
    const UPDATED_AT_COL: Option<&'static str> = None;

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
