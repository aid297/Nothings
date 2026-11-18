# nothings-orm — ORM 核心

对标 Go 的 `gorm.io/gorm`：只定义抽象，不绑定具体数据库。零 DB 依赖，可在任何 native crate 中使用。

- 版本：0.0.1
- edition：2024
- MSRV：1.89+
- License：MIT

## 设计

完整设计见 [`../docs/ai/spec/50-orm-design.md`](../docs/ai/spec/50-orm-design.md)。

```
Value / SqlType        数据库值的运行时表示与静态 SQL 类型标签
Row / OwnedRow         结果集行的抽象与 owned 实现
Schema / Column        静态表结构（由 #[derive(Entity)] 生成）
Entity                 行 → 结构体、按列名取值（由宏生成）
Driver                 数据库后端抽象（execute/query/事务）
Repository             CRUD + 事务的泛型入口
QueryBuilder           链式 where/order/limit/offset，列名白名单校验
```

## 快速开始

`Driver` 由具体驱动实现（见 `nothings-orm-sqlite`）。核心 crate 只提供 trait 与派生宏。

```rust
use nothings_orm::{Entity, Schema};

#[derive(Entity)]
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

## 属性语法

| 位置 | 属性 | 说明 |
| --- | --- | --- |
| 结构体 | `#[entity(table = "users")]` | 必填，表名 |
| 字段 | `#[entity(primary_key)]` | 必填（且只能有一个），主键 |
| 字段 | `#[entity(column = "user_name")]` | 可选，默认用 Rust 字段名 |
| 字段 | `#[entity(nullable)]` | 可选，`Option<T>` 自动推断 |

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
- 事务只支持单层，无 savepoint；`Err` 路径自动 rollback。

## 生成器盲区

`docs/ai/open-api/public-api.json` 由 `docs/ai/tools/gen_api_index.py` 从源码文本提取，**不展开过程宏**。`#[derive(Entity)]` 生成的方法（`Schema::TABLE/COLUMNS/pk`、`Entity::from_row/pk_value/column_value`）不会出现在 JSON 中。查这些 API 请直接看本页与 `spec/50`。

## 不做

async、连接池、缓存、关联加载、Migration、软删除/时间戳钩子、MySQL/Postgres 驱动。Phase 2 路线见 `spec/50`。
