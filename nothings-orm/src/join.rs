//! JOIN 与子查询的声明式构件，以及 JOIN 结果的多实体映射。
//!
//! 别名协议：主表固定 `t0`，第 n 个 JOIN 表为 `tn`。
//! 带 JOIN 的查询把列渲染为 `t0.id AS t0_id`，再由
//! [`NOrmPrefixedRow`] 剥离前缀交给各实体的 `from_row`。

use std::io::{Error, ErrorKind};

use crate::entity::NOrmEntity;
use crate::row::{NOrmPrefixedRow, NOrmRow};
use crate::value::NOrmValue;

/// SQL JOIN 类型。枚举即 SQL 关键字白名单。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NOrmJoinKind {
    Inner,
    Left,
    Right,
    Cross,
}

impl NOrmJoinKind {
    pub(crate) fn sql_token(self) -> &'static str {
        match self {
            NOrmJoinKind::Inner => "INNER JOIN",
            NOrmJoinKind::Left => "LEFT JOIN",
            NOrmJoinKind::Right => "RIGHT JOIN",
            NOrmJoinKind::Cross => "CROSS JOIN",
        }
    }
}

/// 一个 ON 等值项：`left_alias.left_col = right_alias.right_col`。
#[derive(Clone, Debug)]
pub(crate) struct NOrmOnTerm {
    pub left_alias: String,
    pub left_col: String,
    pub right_alias: String,
    pub right_col: String,
}

/// 一条已声明的 JOIN。由 `NOrmQueryBuilder::join` 构造，不对外暴露字段。
#[derive(Clone, Debug)]
pub(crate) struct NOrmJoin {
    pub kind: NOrmJoinKind,
    pub table: &'static str,
    pub alias: String,
    /// JOIN 表的列名（来自 T::COLUMNS），用于生成带别名的 SELECT 列。
    pub columns: Vec<&'static str>,
    pub on: Vec<NOrmOnTerm>,
}

impl NOrmJoin {
    /// 渲染 `JOIN table AS alias ON ...`。ON 项按声明顺序 AND 连接。
    pub(crate) fn render(&self, sql: &mut String) {
        sql.push(' ');
        sql.push_str(self.kind.sql_token());
        sql.push(' ');
        sql.push_str(self.table);
        sql.push_str(" AS ");
        sql.push_str(&self.alias);
        if !self.on.is_empty() {
            sql.push_str(" ON ");
            for (i, term) in self.on.iter().enumerate() {
                if i > 0 {
                    sql.push_str(" AND ");
                }
                sql.push_str(&term.left_alias);
                sql.push('.');
                sql.push_str(&term.left_col);
                sql.push_str(" = ");
                sql.push_str(&term.right_alias);
                sql.push('.');
                sql.push_str(&term.right_col);
            }
        }
    }
}

/// 一条参数化的子查询，作为 `IN` / `NOT IN` / `EXISTS` 的操作数。
///
/// SQL 文本由调用方提供（固定字面量，禁止拼接用户输入），值通过 `params`
/// 走 `?` 占位符绑定，与外层查询的参数按顺序合并。
#[derive(Clone, Debug)]
pub struct NOrmSubquery {
    sql: String,
    params: Vec<NOrmValue>,
}

impl NOrmSubquery {
    /// 无参数子查询。
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            params: Vec::new(),
        }
    }

    /// 带参数的子查询。`params` 的顺序必须与 SQL 中 `?` 的出现顺序一致。
    pub fn with_params(sql: impl Into<String>, params: Vec<NOrmValue>) -> Self {
        Self {
            sql: sql.into(),
            params,
        }
    }

    pub(crate) fn render_into(&self, sql: &mut String, params: &mut Vec<NOrmValue>) {
        sql.push_str(&self.sql);
        params.extend(self.params.iter().cloned());
    }
}

/// 把 JOIN 宽行拆成多个实体元组。
///
/// 为 1~4 元元组实现。每个元素按位置取对应前缀（`t0_` / `t1_` / ...），
/// 用前缀投影复用派生宏生成的 `from_row`。
pub trait NOrmFromJoined: Sized {
    fn from_joined_row<R: NOrmRow>(row: &R, prefixes: &[&str]) -> Result<Self, Error>;
}

macro_rules! impl_from_joined {
    ($(($($T:ident : $idx:tt),+)),+) => {
        $(
            impl<$($T: NOrmEntity),+> NOrmFromJoined for ($($T,)+) {
                fn from_joined_row<R: NOrmRow>(row: &R, prefixes: &[&str]) -> Result<Self, Error> {
                    if prefixes.len() < [$(stringify!($idx)),+].len() {
                        return Err(Error::new(
                            ErrorKind::InvalidInput,
                            "JOIN 结果映射的前缀数量不足",
                        ));
                    }
                    Ok(($(
                        $T::from_row(&NOrmPrefixedRow::new(row, prefixes[$idx]))?,
                    )+))
                }
            }
        )+
    };
}

impl_from_joined!(
    (A: 0),
    (A: 0, B: 1),
    (A: 0, B: 1, C: 2),
    (A: 0, B: 1, C: 2, D: 3)
);

#[cfg(test)]
mod test {
    use super::*;

    fn prefix_of(index: usize) -> String {
        format!("t{index}_")
    }

    #[test]
    fn join_sql_renders_aliases_and_on() {
        let join = NOrmJoin {
            kind: NOrmJoinKind::Inner,
            table: "posts",
            alias: "t1".into(),
            columns: vec!["id", "user_id", "title"],
            on: vec![NOrmOnTerm {
                left_alias: "t0".into(),
                left_col: "id".into(),
                right_alias: "t1".into(),
                right_col: "user_id".into(),
            }],
        };
        let mut sql = String::from("SELECT 1 FROM users AS t0");
        join.render(&mut sql);
        assert_eq!(
            sql,
            "SELECT 1 FROM users AS t0 INNER JOIN posts AS t1 ON t0.id = t1.user_id"
        );
    }

    #[test]
    fn join_kind_tokens() {
        assert_eq!(NOrmJoinKind::Inner.sql_token(), "INNER JOIN");
        assert_eq!(NOrmJoinKind::Left.sql_token(), "LEFT JOIN");
        assert_eq!(NOrmJoinKind::Right.sql_token(), "RIGHT JOIN");
        assert_eq!(NOrmJoinKind::Cross.sql_token(), "CROSS JOIN");
    }

    #[test]
    fn subquery_collects_params_in_order() {
        let sub = NOrmSubquery::with_params(
            "SELECT user_id FROM posts WHERE score > ? AND active = ?",
            vec![NOrmValue::I64(10), NOrmValue::Bool(true)],
        );
        let mut sql = String::new();
        let mut params = Vec::new();
        sub.render_into(&mut sql, &mut params);
        assert_eq!(sql, "SELECT user_id FROM posts WHERE score > ? AND active = ?");
        assert_eq!(params, vec![NOrmValue::I64(10), NOrmValue::Bool(true)]);
    }

    #[test]
    fn prefix_helpers() {
        assert_eq!(prefix_of(0), "t0_");
        assert_eq!(prefix_of(3), "t3_");
    }
}
