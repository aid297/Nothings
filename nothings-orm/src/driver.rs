//! 数据库驱动抽象。对标 GORM 的 Dialector / rbatis 的 rbdc::NOrmDriver。

use std::io::{Error, ErrorKind};

use crate::row::NOrmOwnedRow;
use crate::value::NOrmValue;

/// 执行 SQL 并物化结果的后端接口。
///
/// 所有 SQL 都必须使用 `?` 占位符 + `params` 传值，禁止拼接用户输入。
/// `query` 返回的每一行都是 owned 的，不借用语句或连接。
pub trait NOrmDriver {
    /// 执行非查询语句，返回受影响的行数。
    fn execute(&mut self, sql: &str, params: &[NOrmValue]) -> Result<usize, Error>;

    /// 执行查询语句，把结果全部物化为 owned 行。
    fn query(&mut self, sql: &str, params: &[NOrmValue]) -> Result<Vec<NOrmOwnedRow>, Error>;

    /// 最近一次插入产生的主键。
    fn last_insert_id(&self) -> Result<i64, Error>;

    fn begin(&mut self) -> Result<(), Error>;
    fn commit(&mut self) -> Result<(), Error>;
    fn rollback(&mut self) -> Result<(), Error>;

    /// 生成 upsert（冲突时更新）SQL 的方言尾句，例如
    /// ` ON CONFLICT(id) DO UPDATE SET name = excluded.name`（SQLite）或
    /// ` ON DUPLICATE KEY UPDATE name = VALUES(name)`（MySQL）。
    ///
    /// 参数：
    /// - `table`：目标表名（来自 `E::TABLE`，已是编译期白名单常量）。
    /// - `pk`：主键列名。
    /// - `update_cols`：冲突时需要更新的列名列表，已排除主键与不应覆盖的
    ///   托管列（软删列、created_at），全部来自 `E::COLUMNS` 白名单。
    ///
    /// 返回的尾句**必须以一个空格开头**，由 Repository 直接拼在
    /// `INSERT INTO ... VALUES (...)` 之后。所有列名/表名均来自 schema
    /// 白名单，不接受用户输入；值仍由调用方走 `?` 参数化。
    ///
    /// 默认返回 `InvalidInput`，表示驱动不支持 upsert——手写驱动无需实现
    /// 即可保持编译通过，调用 `upsert_batch` 时会得到明确错误。
    fn upsert_sql(
        &self,
        table: &str,
        pk: &str,
        update_cols: &[&str],
    ) -> Result<String, Error> {
        let _ = (table, pk, update_cols);
        Err(Error::new(
            ErrorKind::InvalidInput,
            "该驱动不支持 upsert（未实现 NOrmDriver::upsert_sql）",
        ))
    }
}
