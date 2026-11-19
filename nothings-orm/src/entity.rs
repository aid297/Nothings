//! 实体 trait：行 → 结构体的反序列化入口，以及按列名取值。

use std::io::{Error, ErrorKind};

use crate::driver::NOrmDriver;
use crate::row::{NOrmOwnedRow, NOrmRow};
use crate::schema::NOrmSchema;
use crate::value::NOrmValue;

/// 可映射到数据库表的实体。
///
/// `NOrmSchema`、`from_row`、`pk_value`、`column_value` 全部由派生宏生成，
/// 调用方无需手写行映射代码。
pub trait NOrmEntity: NOrmSchema + Sized {
    fn from_row<R: NOrmRow>(row: &R) -> Result<Self, Error>;

    fn pk_value(&self) -> NOrmValue;

    /// 按 schema 列名取出当前值；未知列名返回 InvalidInput。
    fn column_value(&self, name: &str) -> Result<NOrmValue, Error>;
}

/// 关联装配：由 `#[derive(NOrmEntity)]` 为所有派生实体生成（无关联时仅返回未知名错误）。
///
/// 独立 trait 而非 `NOrmEntity` 的 supertrait：手写 `NOrmEntity` 不调用 `preload()`
/// 就完全无感。`rows` 是通用层已查到的目标行，本方法只负责解码并填入具体字段。
pub trait NOrmPreload: NOrmEntity {
    /// 按关系名把目标行解码为目标实体，填回 self 的关联字段。
    fn fill_relation(&mut self, name: &str, rows: &[NOrmOwnedRow]) -> Result<(), Error>;

    /// 单层批量预加载入口。内部执行「批量查关联表 → 按主键去重 → 分组填回」，
    /// 供派生宏生成代码与 Repository 在同一条路径上调用。
    fn preload_one<D: NOrmDriver>(
        name: &str,
        entities: &mut [Self],
        driver: &mut D,
    ) -> Result<(), Error> {
        crate::query::apply_preload::<Self, D>(name, entities, driver)
    }

    /// 点分路径递归预加载入口。`#[derive(NOrmEntity)]` 会为派生实体生成
    /// 支持多层的实现；手写 impl 使用下面的默认实现，仅支持单层路径。
    ///
    /// 默认实现：`path` 为空直接返回；单层委托 [`preload_one`](Self::preload_one)；
    /// 多层返回 `InvalidInput`（提示该实体未生成递归支持）。
    fn preload_path<D: NOrmDriver>(
        entities: &mut [Self],
        path: &[&str],
        driver: &mut D,
    ) -> Result<(), Error> {
        match path {
            [] => Ok(()),
            [name] => Self::preload_one(name, entities, driver),
            _ => Err(Error::new(
                ErrorKind::InvalidInput,
                "该实体未生成递归预加载支持，无法处理多层路径",
            )),
        }
    }
}
