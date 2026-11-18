# 50 — ORM 模块设计（GORM 式核心 + 独立驱动）

- 日期：2026-11-18
- 状态：已批准，实现中
- 范围：新增 3 个 crate 的 Phase 1 ORM 能力（Entity 派生宏、静态 schema、行映射、CRUD、QueryBuilder、事务）
- 批准计划：`/Users/yujizhou/.workbuddy/plans/swift-beacon-curie-hcGyIdO2.md`

## 背景与目标

仓库 `nothings` 是纯 Rust 工具库，零 DB 驱动、零 SQL、零 async、零 `Deserialize`。用户熟悉 Go 的 GORM（核心 `gorm.io/gorm` + 独立驱动 `gorm.io/driver/mysql`），要求实现类似能力。

Rust 生态主流（sqlx / Diesel / SeaORM）用一个 crate + Cargo feature 编译期选后端；严格对标 GORM 的范式是 rbatis 核心 + 独立 `rbdc-*` 驱动 crate。本设计选后者：根 crate 保持零 DB 依赖，独立 crate「不用就不依赖」，避免 Cargo feature unification 把 bundled C 代码拖进任何下游构建；MySQL 等后续驱动也就能自带版本与 MSRV，不牵动根 crate。

目标：Phase 1 跑通最小闭环；Phase 2（关联 / Migration / 钩子）只规划不实现。

## 模块结构与依赖

```
nothings/                       # 根 crate：只改 Cargo.toml:22 的宏版本号，其余零改动
├── nothings-macros/            # 宏 crate：0.0.2 → 0.0.3，新增 src/entity.rs
├── nothings-orm/               # 核心 crate，零 DB 依赖，全平台可编译
│   └── src/{lib,value,row,schema,entity,driver,repository,query}.rs
└── nothings-orm-sqlite/        # 驱动 crate，仅 native
    └── src/lib.rs              # SqliteDriver + rusqlite(bundled)
```

- 所有 crate 都是 `crate-type = ["lib"]`。根 crate 在 2026-11-18 的外部改动中已移除 `cdylib` 与 wasm 支持，成为纯 native 库；SQLite 驱动仍放独立 crate，保持根 crate 零 DB 依赖。
- 无 workspace、无 `[features]`，根 `cargo build` 看不到新 crate。
- `rusqlite` 选 bundled（无需下游装系统 SQLite），落地时实测 MSRV ≤ 1.89，不满足则降版本。
- 根 `Cargo.toml:30,37-39` 的 getrandom / aes-gcm 平衡零改动。

## 接口

### 宏属性

```rust
#[derive(Entity)]
#[entity(table = "users")]
struct User {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(column = "user_name")]
    name: String,
    #[entity(nullable)]            // Option<T> 自动推断
    email: Option<String>,
}
```

解析用 `attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)`，匹配 `Meta::NameValue`（`Lit::Str`）与 `Meta::Path` 标志。不复用 `parse_validator_attrs`（validator 专用手写字符扫描）。

生成 `impl ::nothings_orm::schema::Schema`（`TABLE` / `COLUMNS`）与 `impl ::nothings_orm::entity::Entity`（`from_row` / `pk_value`），SQL 类型发射为 `<FieldType as HasSqlType>::SQL_TYPE`，宏侧不维护映射表。

### 核心 trait（签名级）

```
Value { Null, Bool, I64, F64, Text, Blob }
ToValue::to_value() -> Value
FromValue::from_value(&Value) -> Result<Self, io::Error>
SqlType { Integer, Real, Text, Blob }
HasSqlType::SQL_TYPE: SqlType

Row { column_count, column_name, value }
OwnedRow { Vec<String> + Vec<Value> }        // 即时物化，规避 stmt 借用

Column { field, name, sql_type, nullable, primary_key }
Schema { const TABLE, const COLUMNS, fn pk() }
Entity: Schema { fn from_row<R: Row>(&R) -> Result<Self, io::Error>; fn pk_value() -> Value }

Driver { execute, query, last_insert_id, begin, commit, rollback }
Repository<'a, D: Driver> { new, insert, get, find_one, find_all, update, delete, query, transaction }
QueryBuilder<'a, E, D> { r#where, order_by, limit, offset, all, one, count }  // 链式 &mut Self
```

### 错误处理

只用 `std::io::Error`，按 `ErrorKind` 编码类别，message 携带结构化细节：

| 场景 | ErrorKind |
| --- | --- |
| DB 文件不存在 / `get` 未找到 | `NotFound`（`find_one` 返回 `Ok(None)`） |
| UNIQUE / PK 冲突 | `AlreadyExists` |
| 行值→Rust 类型失败 / SQL 语法错误 | `InvalidData` |
| 未知列名 / 非法标识符 / 嵌套事务 / 非法参数 | `InvalidInput` |
| 其它连接失败 | `Other` |

orphan rule：不能 `impl From<rusqlite::Error> for io::Error`，驱动 crate 用固有函数 `to_io`。

### 安全边界

- 值一律 `?` + `params: &[Value]` 参数化；列名/表名不可参数化，必须在 `E::COLUMNS` 白名单内，否则 `InvalidInput`。
- 事务单层，嵌套 `InvalidInput`；`Err` 必 rollback；无 savepoint。

## 明确不做（YAGNI）

async / tokio；workspace；Cargo features；新公开错误类型；thiserror / anyhow；`serde::Deserialize` 派生；Postgres 驱动；关联 / Migration / 钩子实现；trybuild；连接池、缓存、懒加载、复合主键、UUID 主键、SQL 日志、metrics；改 C-ABI（仓库已移除 wasm 导出层）；`cargo fmt`；新增 CI / Dockerfile / examples / Makefile。

MySQL 驱动已于 2026-11-18 作为独立 crate `nothings-orm-mysql` 落地，核心 / 宏 / SQLite 驱动零改动，设计见 `spec/51-mysql-driver-design.md`；本 spec 的 Phase 1 范围与验收基线不变。

## 测试计划

| 层次 | 位置 | 做法 |
| --- | --- | --- |
| 核心纯逻辑 | `nothings-orm/src/*` 内 `#[cfg(test)]` | 假 Driver 记录 SQL / 返回 canned `OwnedRow`，零 DB |
| 宏展开成功 | `nothings-orm/tests/entity.rs` | 断言 `Schema::TABLE/COLUMNS`、`from_row`、`pk_value` |
| SQLite 内存 | `nothings-orm-sqlite/tests/crud.rs` | CRUD / QueryBuilder / 事务 / 约束冲突 / 类型不匹配 / 0 行更新 |
| 真实文件 | `nothings-orm-sqlite/tests/file_db.rs` | `tempfile` 临时库，关闭重开验证持久性 |
| 宏展开失败 | 不引入 trybuild | 固定 `compile_error!` 文案 + README 反例 |

## 验收标准

```bash
cargo test && cargo clippy --all-targets && cargo doc --no-deps    # 根基线不变：299 单测 + 21 doctest（1 ignored）
cargo test --manifest-path nothings-orm/Cargo.toml
cargo test --manifest-path nothings-orm-sqlite/Cargo.toml
cargo test --manifest-path nothings-macros/Cargo.toml
cargo tree | grep -iE 'rusqlite|sqlite|nothings[-_]orm'            # 期望为空
python3 docs/ai/tools/gen_api_index.py && python3 docs/ai/tools/gen_api_index.py --check
```

文档同步清单见 `40-development-workflow.md` 场景 F（实现时新增）。`gen_api_index.py` 需 6 处改造：多源根、模块名前缀、native-only 判定（现在所有 crate 都 native，判定逻辑可保留备用）、ANNOTATIONS、`const/async fn` 正则、`strip_literals`、宏生成盲区声明、`source_roots` 输出。

## 实现记录

- **Entity trait 增加了 `column_value(&self, name)`**：设计阶段只列了 `from_row` / `pk_value`。落地时 Repository 的 `insert` / `update` 需要按列名取实体值，若做成旁支 `ColumnValues` trait 会多一层间接且要宏生成两个 impl，直接扩展 `Entity` 更干净。这是设计外新增的公开方法。
- **`ToValue` 也实现给了 `Value` 与 `&str`**：`Repository::get/find_one` 直接接收 `Value` 主键，需 `Value: ToValue`；`r#where` 传字符串字面量是高频用法，需 `&str: ToValue`。两者都是落地时暴露的需求。
- **列名校验的错误延后**：`r#where` / `order_by` 返回 `&mut Self`（链式风格），无法即时返回 `Result`。实现把非法列名记为哨兵字符串，`all()` / `one()` / `count()` 时统一转 `InvalidInput`。调用方要注意错误不在构造点抛出（已登记为 `spec/30` 的 ORM1）。
- **rusqlite 错误码变体与预期不同**：`ErrorCode` 没有 `CantOpen` / `InvalidColumnName` 等变体，实际是 `CannotOpen` / `ApiMisuse` 等；按源码定义的枚举重写了映射表。
- **orphan rule 避开方式**：不能给外部类型 `Value` 实现外部 trait `rusqlite::ToSql`。驱动 crate 改为把 `Value` 映射成本地函数 `to_sql` 返回的 `rusqlite::types::Value`，再交给 `params_from_iter`。
- **raw identifier 解析**：`gen_api_index.py` 初版把 `r#where` 解析成 `r`，已修 fn 正则支持 `r#` 前缀；这是生成器第一次遇到 raw identifier。
- **基线变化**：实现期间工作区的 wasm 支持被外部改动移除（删 `wasm_exports`、`cdylib`、wasm 依赖），根基线 304 → 299 单测、契约 389 → 376 items。本 spec 与计划文件已按新基线改写，wasm 双目标验收项作废。
- **验证结果**：根 299 单测 + 21 doctest（1 ignored）通过；nothings-orm 8 测、nothings-orm-sqlite 10 测通过；契约 456 items，生成器 `--check` 返回 0；根 `cargo tree` 无 sqlite/orm 依赖。
