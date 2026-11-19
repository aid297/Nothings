# nothings-orm — ORM 核心

对标 Go 的 `gorm.io/gorm`：只定义抽象，不绑定具体数据库。零 DB 依赖，可在任何 native crate 中使用。

- 版本：0.0.1
- edition：2024
- MSRV：1.89+
- License：MIT

## 设计

完整设计见 [`../docs/ai/spec/50-orm-design.md`](../docs/ai/spec/50-orm-design.md)。

```
NOrmValue / NOrmSqlType        数据库值的运行时表示与静态 SQL 类型标签
NOrmRow / NOrmOwnedRow         结果集行的抽象与 owned 实现
NOrmSchema / NOrmColumn        静态表结构（由 #[derive(NOrmEntity)] 生成）
NOrmEntity                 行 → 结构体、按列名取值（由宏生成）
NOrmDriver                 数据库后端抽象（execute/query/事务）
NOrmRepository             CRUD + 事务的泛型入口
NOrmQueryBuilder           链式 where/order/limit/offset，列名白名单校验
NOrmCondition / NOrmOperator   条件树：AND/OR/NOT/分组 + 比较运算，值全部 ? 参数化
NOrmJoinKind / NOrmSubquery   SQL JOIN 与子查询声明
NOrmRelation / NOrmRelationKind / NOrmPreload  关联预加载（含点分递归）的元数据、填充与遍历 trait
```

## 快速开始

`NOrmDriver` 由具体驱动实现（见 `nothings-orm-sqlite`）。核心 crate 只提供 trait 与派生宏。

```rust
use nothings_orm::{NOrmEntity, NOrmSchema};

#[derive(NOrmEntity)]
#[entity(table = "users")]
struct User {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(column = "user_name")]
    name: String,
    #[entity(nullable)]
    email: Option<String>,
}

assert_eq!(User::TABLE, "users");
assert_eq!(User::COLUMNS.len(), 3);
```

## 条件构建

链式方法按调用顺序**从左到右**结合（左折叠）：每个方法用自身携带的连接词把新条件接到已累积条件上，构建器不按 SQL 优先级重排；渲染器用最小括号忠实还原该顺序。

```rust
use nothings_orm::condition::{NOrmCondition, NOrmOperator};

// (id = ? OR user_name = ?) AND id > ?
repo.query::<User>()
    .r#where("id", 1i64)
    .or_where("user_name", "alice")
    .where_op("id", NOrmOperator::Gt, 10i64)
    .all()?;

// (id > ? OR user_name = ?) AND NOT (email IS NULL)
let cond = NOrmCondition::<User>::gt("id", 10i64)
    .or(NOrmCondition::eq("user_name", "alice"))
    .and(!NOrmCondition::is_null("email"));
repo.query::<User>().where_condition(cond).all()?;
```

链式入口：`r#where` / `and_where` / `or_where` / `not_where`（等值），`where_op` / `or_where_op`（比较运算），`where_condition` / `or_where_condition` / `not_where_condition`（整树注入）。

`NOrmCondition` 构造器：`eq` / `ne` / `gt` / `ge` / `lt` / `le` / `like` / `not_like` / `cmp`（单值），`in_list` / `not_in_list`（多值），`is_null` / `is_not_null`（无值）；组合器 `and` / `or` / `group`，取反用 `!` 运算符（实现 `std::ops::Not`）。

边界：首项 `or_where` 退化为裸首项；`IN` 空列表折叠为 `0 = 1` / `1 = 1`；LIKE 通配符由调用方提供；独立构造 `NOrmCondition` 时需写 `NOrmCondition::<User>::...`（参数里不含实体类型）。

## 属性语法

| 位置 | 属性 | 说明 |
| --- | --- | --- |
| 结构体 | `#[entity(table = "users")]` | 必填，表名 |
| 字段 | `#[entity(primary_key)]` | 必填（且只能有一个），主键 |
| 字段 | `#[entity(column = "user_name")]` | 可选，默认用 Rust 字段名 |
| 字段 | `#[entity(nullable)]` | 可选，`Option<T>` 自动推断 |
| 字段 | `#[entity(has_many = "books", target = "Book", foreign_key = "author_id")]` | 关联：一对多，填 `Vec<T>`；外键在目标表 |
| 字段 | `#[entity(belongs_to = "author", target = "Author", foreign_key = "author_id")]` | 关联：多对一，填 `Option<T>`；外键在本表 |
| 字段 | `#[entity(soft_delete)]` | 软删除列，须 `Option<NOrmDateTime>` |
| 字段 | `#[entity(created_at)]` / `#[entity(updated_at)]` | 时间戳列，注入 `NOrmDateTime::now_utc()` |

关联三者必须同时出现，字段不得再标 `primary_key` / `nullable` / `column`。查询时用 `preload("关系名")` 填充，见 cookbook 6.2。

## 错误处理

所有公开 API 返回 `Result<_, std::io::Error>`，调用方按 `ErrorKind` 分支：

| ErrorKind | 场景 |
| --- | --- |
| `NotFound` | `get` 未找到主键 |
| `AlreadyExists` | UNIQUE / PK 冲突 |
| `InvalidData` | 行值类型转换失败、缺失列 |
| `InvalidInput` | 列名不在白名单、嵌套事务、未知列 |

`find_one` 未找到返回 `Ok(None)`，不算错误。

## 安全边界

- 所有值走 `?` 占位符 + params 参数化，禁止 SQL 拼接。
- 列名/表名不能参数化，必须在 `E::COLUMNS` 白名单内，否则 `InvalidInput`。
- 比较运算符来自 `NOrmOperator` 枚举白名单；`IS NULL` 不接参数，`IN` 空列表折叠为常量。
- 事务只支持单层，无 savepoint；`Err` 路径自动 rollback。

## 日期时间与时区

`value` 模块提供两个零依赖类型（不引 chrono / time）：

| 类型 | 说明 |
| --- | --- |
| `NOrmUtcOffset` | 固定 UTC 偏移，`NOrmUtcOffset::parse("+08:00")` / `NOrmUtcOffset::UTC`；无 DST |
| `NOrmDateTime` | 墙钟日期时间，`NOrmDateTime::new(...)` / `NOrmDateTime::parse(...)` / `from_unix_secs(secs, offset)` |

`NOrmValue` 新增 `NOrmDateTime(NOrmDateTime)` 变体，作为「这是需要时区转换的时间」的类型标记。`NOrmDateTime` 实现 `NOrmToValue` / `NOrmFromValue` / `NOrmHasSqlType`（`SQL_TYPE = Text`），可直接作实体字段：

```rust
use nothings_orm::value::NOrmDateTime;
// struct Event { #[entity(primary_key)] id: i64, at: NOrmDateTime }
```

转换由驱动的连接时区配置负责（`nothings-orm-mysql` 的 `connect_with_tz`）；`i64` Unix 时间戳不参与转换。派生宏生成的方法不进 JSON，以上 API 须看本节或 `docs/orm-cookbook.md`。

## 生成器盲区

`docs/ai/open-api/public-api.json` 由 `docs/ai/tools/gen_api_index.py` 从源码文本提取，**不展开过程宏**。`#[derive(NOrmEntity)]` 生成的方法（`NOrmSchema::TABLE/COLUMNS/RELATIONS/pk`、`NOrmEntity::from_row/pk_value/column_value`、`NOrmPreload::fill_relation/preload_one/preload_path`、`NOrmSchema` 的角色列常量）不会出现在 JSON 中。查这些 API 请直接看本页与 `spec/50`。

## 不做

async、连接池、缓存、Migration、Postgres 驱动（MySQL 驱动已由 `nothings-orm-mysql` 提供，设计见 `docs/ai/spec/51-mysql-driver-design.md`）。**SQL JOIN 与子查询已支持**（`join` / `all_with` / `where_in_subquery`，见 cookbook 6.1）；**关联预加载已支持**：`preload("books.comments")` 点分递归、结果按主键去重、`repo.load_relation(&mut entity, name)` 懒加载（见 cookbook 6.2）。Phase 2 路线见 `spec/50`。
