//! 条件树：比较叶子 + AND / OR / NOT / 分组。
//!
//! 值一律通过 `?` 占位符传递；列名在构造期按 `E::COLUMNS` 校验，
//! 校验结果内嵌到节点里，由 `NOrmQueryBuilder` 在终结方法中统一报错（延后语义）。

use std::marker::PhantomData;

use crate::schema::{validate_column, NOrmSchema};
use crate::value::{NOrmToValue, NOrmValue};

/// 比较运算符。枚举即 SQL 关键字白名单，杜绝拼接注入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NOrmOperator {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Like,
    NotLike,
    In,
    NotIn,
    IsNull,
    IsNotNull,
}

impl NOrmOperator {
    fn sql_token(self) -> &'static str {
        match self {
            NOrmOperator::Eq => "=",
            NOrmOperator::Ne => "!=",
            NOrmOperator::Gt => ">",
            NOrmOperator::Ge => ">=",
            NOrmOperator::Lt => "<",
            NOrmOperator::Le => "<=",
            NOrmOperator::Like => "LIKE",
            NOrmOperator::NotLike => "NOT LIKE",
            NOrmOperator::In => "IN",
            NOrmOperator::NotIn => "NOT IN",
            NOrmOperator::IsNull => "IS NULL",
            NOrmOperator::IsNotNull => "IS NOT NULL",
        }
    }
}

/// 一棵可独立构造、可组合的条件树。
///
/// `E` 用 `NOrmSchema` 约束，因此跨实体（跨表）的条件无法混用。
/// 类型本身不含生命周期：所有值在构造期转成 owning 的 `NOrmValue`。
pub struct NOrmCondition<E: NOrmSchema> {
    root: Node<E>,
}

enum Node<E: NOrmSchema> {
    Cmp(Comparison<E>),
    And(Vec<Node<E>>),
    Or(Vec<Node<E>>),
    Not(Box<Node<E>>),
    /// 显式分组：渲染时无条件加括号。
    Group(Box<Node<E>>),
}

struct Comparison<E: NOrmSchema> {
    /// 构造期校验结果：Ok(白名单里的 DB 列名) / Err(错误消息)。
    /// 存 String 而非 io::Error（后者不实现 Clone），统一以 InvalidInput 上报。
    column: Result<&'static str, String>,
    op: NOrmOperator,
    /// 普通比较 1 个；IN / NOT IN 多个；IS NULL / IS NOT NULL 零个。
    values: Vec<NOrmValue>,
    /// E 只参与校验，字段里没有用到，需要显式占位。
    phantom: PhantomData<E>,
}

impl<E: NOrmSchema> NOrmCondition<E> {
    fn leaf(column: &str, op: NOrmOperator, values: Vec<NOrmValue>) -> Self {
        let column = validate_column::<E>(column)
            .map(|c| c.name)
            .map_err(|e| e.to_string());
        Self {
            root: Node::Cmp(Comparison {
                column,
                op,
                values,
                phantom: PhantomData,
            }),
        }
    }

    /// 构造一个带错误消息的叶子，用于 API 误用的延后报错。
    fn with_error(message: String) -> Self {
        Self {
            root: Node::Cmp(Comparison {
                column: Err(message),
                op: NOrmOperator::Eq,
                values: Vec::new(),
                phantom: PhantomData,
            }),
        }
    }

    /// 通用比较。`In` / `NotIn` / `IsNull` / `IsNotNull` 不接受单值，
    /// 误用时生成延后错误，提示改用专用构造器。
    pub fn cmp(column: &str, op: NOrmOperator, value: impl NOrmToValue) -> Self {
        match op {
            NOrmOperator::In | NOrmOperator::NotIn | NOrmOperator::IsNull | NOrmOperator::IsNotNull => {
                Self::with_error(format!(
                    "运算符 {:?} 不接受单值，请用 in_list / not_in_list / is_null / is_not_null 构造器",
                    op
                ))
            }
            _ => Self::leaf(column, op, vec![value.to_value()]),
        }
    }

    pub fn eq(column: &str, value: impl NOrmToValue) -> Self {
        Self::cmp(column, NOrmOperator::Eq, value)
    }

    pub fn ne(column: &str, value: impl NOrmToValue) -> Self {
        Self::cmp(column, NOrmOperator::Ne, value)
    }

    pub fn gt(column: &str, value: impl NOrmToValue) -> Self {
        Self::cmp(column, NOrmOperator::Gt, value)
    }

    pub fn ge(column: &str, value: impl NOrmToValue) -> Self {
        Self::cmp(column, NOrmOperator::Ge, value)
    }

    pub fn lt(column: &str, value: impl NOrmToValue) -> Self {
        Self::cmp(column, NOrmOperator::Lt, value)
    }

    pub fn le(column: &str, value: impl NOrmToValue) -> Self {
        Self::cmp(column, NOrmOperator::Le, value)
    }

    /// LIKE 的通配符由调用方在 pattern 里提供，本方法不自动添加 `%`。
    pub fn like(column: &str, pattern: impl NOrmToValue) -> Self {
        Self::cmp(column, NOrmOperator::Like, pattern)
    }

    pub fn not_like(column: &str, pattern: impl NOrmToValue) -> Self {
        Self::cmp(column, NOrmOperator::NotLike, pattern)
    }

    /// IN 列表。空列表在渲染期折叠为 `0 = 1`，不生成非法 SQL。
    pub fn in_list<V: NOrmToValue>(column: &str, values: impl IntoIterator<Item = V>) -> Self {
        Self::leaf(
            column,
            NOrmOperator::In,
            values.into_iter().map(|v| v.to_value()).collect(),
        )
    }

    /// NOT IN 列表。空列表渲染为 `1 = 1`。
    pub fn not_in_list<V: NOrmToValue>(column: &str, values: impl IntoIterator<Item = V>) -> Self {
        Self::leaf(
            column,
            NOrmOperator::NotIn,
            values.into_iter().map(|v| v.to_value()).collect(),
        )
    }

    pub fn is_null(column: &str) -> Self {
        Self::leaf(column, NOrmOperator::IsNull, Vec::new())
    }

    pub fn is_not_null(column: &str) -> Self {
        Self::leaf(column, NOrmOperator::IsNotNull, Vec::new())
    }

    /// 消费自身，与另一棵树做 AND 组合。
    pub fn and(self, other: NOrmCondition<E>) -> Self {
        Self {
            root: Node::And(vec![self.root, other.root]),
        }
    }

    /// 消费自身，与另一棵树做 OR 组合。
    pub fn or(self, other: NOrmCondition<E>) -> Self {
        Self {
            root: Node::Or(vec![self.root, other.root]),
        }
    }

    /// 显式分组。渲染为 `(<child>)`。
    pub fn group(self) -> Self {
        Self {
            root: Node::Group(Box::new(self.root)),
        }
    }

    /// 取最左（最先声明）的校验错误，供终结方法统一上报。
    pub(crate) fn first_error(&self) -> Option<&str> {
        fn walk<E: NOrmSchema>(node: &Node<E>) -> Option<&str> {
            match node {
                Node::Cmp(c) => c.column.as_ref().err().map(|s| s.as_str()),
                Node::And(kids) | Node::Or(kids) => kids.iter().find_map(walk),
                Node::Not(child) | Node::Group(child) => walk(child),
            }
        }
        walk(&self.root)
    }

    /// 把条件渲染进 `sql`，同时按 `?` 出现顺序收集 `params`。
    pub(crate) fn render_into(&self, sql: &mut String, params: &mut Vec<NOrmValue>) {
        self.root.render_prec(sql, params, 0);
    }
}

/// `!condition` 取反，渲染为 `NOT (<child>)`。
impl<E: NOrmSchema> std::ops::Not for NOrmCondition<E> {
    type Output = Self;

    fn not(self) -> Self {
        Self {
            root: Node::Not(Box::new(self.root)),
        }
    }
}

impl<E: NOrmSchema> Node<E> {
    /// 优先级：OR=1, AND=2, NOT/Group=3, 叶子=4。
    /// 子节点优先级严格小于父上下文优先级时才加括号；同优先级不加。
    fn render_prec(&self, sql: &mut String, params: &mut Vec<NOrmValue>, parent_prec: u8) {
        match self {
            Node::Cmp(c) => c.render(sql, params),
            Node::Group(child) => {
                sql.push('(');
                child.render_prec(sql, params, 0);
                sql.push(')');
            }
            Node::Not(child) => {
                sql.push_str("NOT (");
                child.render_prec(sql, params, 0);
                sql.push(')');
            }
            Node::And(kids) => {
                let need_parens = parent_prec > 2;
                if need_parens {
                    sql.push('(');
                }
                for (i, kid) in kids.iter().enumerate() {
                    if i > 0 {
                        sql.push_str(" AND ");
                    }
                    kid.render_prec(sql, params, 2);
                }
                if need_parens {
                    sql.push(')');
                }
            }
            Node::Or(kids) => {
                let need_parens = parent_prec > 1;
                if need_parens {
                    sql.push('(');
                }
                for (i, kid) in kids.iter().enumerate() {
                    if i > 0 {
                        sql.push_str(" OR ");
                    }
                    kid.render_prec(sql, params, 1);
                }
                if need_parens {
                    sql.push(')');
                }
            }
        }
    }
}

impl<E: NOrmSchema> Comparison<E> {
    fn render(&self, sql: &mut String, params: &mut Vec<NOrmValue>) {
        // check_valid 保证运行到这里时 column 必为 Ok；unwrap_or 仅作防御兜底。
        let column = self.column.as_deref().unwrap_or("<invalid>");
        match self.op {
            NOrmOperator::IsNull | NOrmOperator::IsNotNull => {
                sql.push_str(column);
                sql.push(' ');
                sql.push_str(self.op.sql_token());
            }
            NOrmOperator::In | NOrmOperator::NotIn => {
                if self.values.is_empty() {
                    // IN () 在多数数据库是语法错误，折叠成恒假/恒真常量。
                    sql.push_str(if self.op == NOrmOperator::In { "0 = 1" } else { "1 = 1" });
                } else {
                    sql.push_str(column);
                    sql.push(' ');
                    sql.push_str(self.op.sql_token());
                    sql.push_str(" (");
                    for (i, _) in self.values.iter().enumerate() {
                        if i > 0 {
                            sql.push_str(", ");
                        }
                        sql.push('?');
                    }
                    sql.push(')');
                    params.extend(self.values.iter().cloned());
                }
            }
            other => {
                sql.push_str(column);
                sql.push(' ');
                sql.push_str(other.sql_token());
                sql.push_str(" ?");
                if let Some(value) = self.values.first() {
                    params.push(value.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::schema::NOrmColumn;
    use crate::value::NOrmSqlType;

    struct T;

    impl NOrmSchema for T {
        const TABLE: &'static str = "t";
        const COLUMNS: &'static [NOrmColumn] = &[
            NOrmColumn {
                field: "a",
                name: "a",
                sql_type: NOrmSqlType::Integer,
                nullable: false,
                primary_key: true,
            },
            NOrmColumn {
                field: "b",
                name: "b",
                sql_type: NOrmSqlType::Integer,
                nullable: false,
                primary_key: false,
            },
            NOrmColumn {
                field: "c",
                name: "c",
                sql_type: NOrmSqlType::Text,
                nullable: false,
                primary_key: false,
            },
            NOrmColumn {
                field: "email",
                name: "email",
                sql_type: NOrmSqlType::Text,
                nullable: true,
                primary_key: false,
            },
        ];

        fn pk() -> &'static NOrmColumn {
            &Self::COLUMNS[0]
        }
    }

    /// 渲染并断言核心不变量：SQL 中 `?` 的数量 == params 数量。
    fn render<E: NOrmSchema>(condition: &NOrmCondition<E>) -> (String, Vec<NOrmValue>) {
        let mut sql = String::new();
        let mut params = Vec::new();
        condition.render_into(&mut sql, &mut params);
        assert_eq!(
            sql.matches('?').count(),
            params.len(),
            "占位符数量与参数数量不一致: {sql}"
        );
        (sql, params)
    }

    #[test]
    fn comparison_operators() {
        let cases: Vec<(NOrmCondition<T>, &str)> = vec![
            (NOrmCondition::eq("a", 1i64), "a = ?"),
            (NOrmCondition::ne("a", 1i64), "a != ?"),
            (NOrmCondition::gt("a", 1i64), "a > ?"),
            (NOrmCondition::ge("a", 1i64), "a >= ?"),
            (NOrmCondition::lt("a", 1i64), "a < ?"),
            (NOrmCondition::le("a", 1i64), "a <= ?"),
            (NOrmCondition::like("c", "%x%"), "c LIKE ?"),
            (NOrmCondition::not_like("c", "%x%"), "c NOT LIKE ?"),
        ];
        for (condition, expected) in cases {
            let (sql, params) = render(&condition);
            assert_eq!(sql, expected);
            assert_eq!(params.len(), 1);
        }
    }

    #[test]
    fn in_and_not_in() {
        let (sql, params) = render(&NOrmCondition::<T>::in_list("a", [1i64, 2, 3]));
        assert_eq!(sql, "a IN (?, ?, ?)");
        assert_eq!(params, vec![NOrmValue::I64(1), NOrmValue::I64(2), NOrmValue::I64(3)]);

        let (sql, params) = render(&NOrmCondition::<T>::not_in_list("c", ["x", "y"]));
        assert_eq!(sql, "c NOT IN (?, ?)");
        assert_eq!(params, vec![NOrmValue::Text("x".into()), NOrmValue::Text("y".into())]);
    }

    #[test]
    fn empty_in_folds_to_constant() {
        let empty: [i64; 0] = [];
        let (sql, params) = render(&NOrmCondition::<T>::in_list("a", empty));
        assert_eq!(sql, "0 = 1");
        assert!(params.is_empty());

        let empty: [i64; 0] = [];
        let (sql, params) = render(&NOrmCondition::<T>::not_in_list("a", empty));
        assert_eq!(sql, "1 = 1");
        assert!(params.is_empty());
    }

    #[test]
    fn null_checks_take_no_param() {
        let (sql, params) = render(&NOrmCondition::<T>::is_null("email"));
        assert_eq!(sql, "email IS NULL");
        assert!(params.is_empty());

        let (sql, params) = render(&NOrmCondition::<T>::is_not_null("email"));
        assert_eq!(sql, "email IS NOT NULL");
        assert!(params.is_empty());
    }

    #[test]
    fn and_or_not_group_render() {
        let cond = NOrmCondition::<T>::eq("a", 1i64)
            .or(NOrmCondition::gt("b", 2i64))
            .and(!NOrmCondition::like("c", "%tmp%"));
        let (sql, params) = render(&cond);
        assert_eq!(sql, "(a = ? OR b > ?) AND NOT (c LIKE ?)");
        assert_eq!(
            params,
            vec![NOrmValue::I64(1), NOrmValue::I64(2), NOrmValue::Text("%tmp%".into())]
        );

        let grouped = NOrmCondition::<T>::eq("a", 1i64).group().or(NOrmCondition::eq("b", 2i64));
        let (sql, _) = render(&grouped);
        assert_eq!(sql, "(a = ?) OR b = ?");
    }

    #[test]
    fn first_error_is_leftmost() {
        let cond = NOrmCondition::<T>::eq("bad_a", 1i64)
            .or(NOrmCondition::eq("bad_b", 2i64))
            .and(NOrmCondition::eq("a", 3i64));
        let err = cond.first_error().unwrap();
        assert!(err.contains("bad_a"), "期望最左错误 bad_a，得到 {err}");
    }

    #[test]
    fn valid_condition_has_no_error() {
        let cond = NOrmCondition::<T>::in_list("a", [1i64, 2])
            .and(NOrmCondition::<T>::is_null("email"))
            .and(NOrmCondition::eq("c", "x"));
        assert!(cond.first_error().is_none());
    }

    #[test]
    fn single_value_op_rejects_in_and_null() {
        for op in [NOrmOperator::In, NOrmOperator::NotIn, NOrmOperator::IsNull, NOrmOperator::IsNotNull] {
            let cond = NOrmCondition::<T>::cmp("a", op, 1i64);
            let err = cond.first_error().unwrap();
            assert!(err.contains("不接受单值"), "得到 {err}");
        }
    }
}
