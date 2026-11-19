//! 数据库驱动抽象。对标 GORM 的 Dialector / rbatis 的 rbdc::NOrmDriver。

use std::io::Error;

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
}
