# 50 — ORM 模块设计（GORM 式核心 + 独立驱动）

- 日期：2026-11-18
- 状态：已批准，实现中
- 范围：新增 3 个 crate 的 Phase 1 ORM 能力（NOrmEntity 派生宏、静态 schema、行映射、CRUD、NOrmQueryBuilder、事务）
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
    └── src/lib.rs              # NOrmSqliteDriver + rusqlite(bundled)
```

- 所有 crate 都是 `crate-type = ["lib"]`。根 crate 在 2026-11-18 的外部改动中已移除 `cdylib` 与 wasm 支持，成为纯 native 库；SQLite 驱动仍放独立 crate，保持根 crate 零 DB 依赖。
- 无 workspace、无 `[features]`，根 `cargo build` 看不到新 crate。
- `rusqlite` 选 bundled（无需下游装系统 SQLite），落地时实测 MSRV ≤ 1.89，不满足则降版本。
- 根 `Cargo.toml:30,37-39` 的 getrandom / aes-gcm 平衡零改动。

## 接口

### 宏属性

```rust
#[derive(NOrmEntity)]
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

生成 `impl ::nothings_orm::schema::NOrmSchema`（`TABLE` / `COLUMNS`）与 `impl ::nothings_orm::entity::NOrmEntity`（`from_row` / `pk_value`），SQL 类型发射为 `<FieldType as NOrmHasSqlType>::SQL_TYPE`，宏侧不维护映射表。

### 核心 trait（签名级）

```
NOrmValue { Null, Bool, I64, F64, Text, Blob }
NOrmToValue::to_value() -> NOrmValue
NOrmFromValue::from_value(&NOrmValue) -> Result<Self, io::Error>
NOrmSqlType { Integer, Real, Text, Blob }
NOrmHasSqlType::SQL_TYPE: NOrmSqlType

NOrmRow { column_count, column_name, value }
NOrmOwnedRow { Vec<String> + Vec<NOrmValue> }        // 即时物化，规避 stmt 借用

NOrmColumn { field, name, sql_type, nullable, primary_key }
NOrmSchema { const TABLE, const COLUMNS, fn pk() }
NOrmEntity: NOrmSchema { fn from_row<R: NOrmRow>(&R) -> Result<Self, io::Error>; fn pk_value() -> NOrmValue }

NOrmDriver { execute, query, last_insert_id, begin, commit, rollback }
NOrmRepository<'a, D: NOrmDriver> { new, insert, get, find_one, find_all, update, delete, query, transaction }
NOrmQueryBuilder<'a, E, D> { r#where, and_where, or_where, not_where, where_op, or_where_op,
                         where_condition, or_where_condition, not_where_condition,
                         order_by, limit, offset, all, one, count }          // 链式 &mut Self
NOrmCondition<E: NOrmSchema> { eq/ne/gt/ge/lt/le/like/not_like/cmp, in_list/not_in_list,
                       is_null/is_not_null, and/or/group, ! (Not trait) }    // 独立条件树
NOrmOperator { Eq, Ne, Gt, Ge, Lt, Le, Like, NotLike, In, NotIn, IsNull, IsNotNull }
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

- 值一律 `?` + `params: &[NOrmValue]` 参数化；列名/表名不可参数化，必须在 `E::COLUMNS` 白名单内，否则 `InvalidInput`。
- 比较运算符来自 `NOrmOperator` 枚举白名单，SQL 关键字不来自用户输入；`IS NULL` 不接参数；`IN` 空列表折叠为 `0 = 1` / `1 = 1`；递归渲染保证 `?` 数量与参数顺序严格一致。
- 事务单层，嵌套 `InvalidInput`；`Err` 必 rollback；无 savepoint。

## 明确不做（YAGNI）

async / tokio；workspace；Cargo features；新公开错误类型；thiserror / anyhow；`serde::Deserialize` 派生；Postgres 驱动；关联 / Migration / 钩子实现；trybuild；连接池、缓存、懒加载、复合主键、UUID 主键、SQL 日志、metrics；改 C-ABI（仓库已移除 wasm 导出层）；`cargo fmt`；新增 CI / Dockerfile / examples / Makefile。

MySQL 驱动已于 2026-11-18 作为独立 crate `nothings-orm-mysql` 落地，核心 / 宏 / SQLite 驱动零改动，设计见 `spec/51-mysql-driver-design.md`；本 spec 的 Phase 1 范围与验收基线不变。

## 测试计划

| 层次 | 位置 | 做法 |
| --- | --- | --- |
| 核心纯逻辑 | `nothings-orm/src/*` 内 `#[cfg(test)]` | 假 NOrmDriver 记录 SQL / 返回 canned `NOrmOwnedRow`，零 DB |
| 宏展开成功 | `nothings-orm/tests/entity.rs` | 断言 `NOrmSchema::TABLE/COLUMNS`、`from_row`、`pk_value` |
| SQLite 内存 | `nothings-orm-sqlite/tests/crud.rs` | CRUD / NOrmQueryBuilder / 事务 / 约束冲突 / 类型不匹配 / 0 行更新 |
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

- **NOrmEntity trait 增加了 `column_value(&self, name)`**：设计阶段只列了 `from_row` / `pk_value`。落地时 NOrmRepository 的 `insert` / `update` 需要按列名取实体值，若做成旁支 `ColumnValues` trait 会多一层间接且要宏生成两个 impl，直接扩展 `NOrmEntity` 更干净。这是设计外新增的公开方法。
- **`NOrmToValue` 也实现给了 `NOrmValue` 与 `&str`**：`NOrmRepository::get/find_one` 直接接收 `NOrmValue` 主键，需 `NOrmValue: NOrmToValue`；`r#where` 传字符串字面量是高频用法，需 `&str: NOrmToValue`。两者都是落地时暴露的需求。
- **列名校验的错误延后**：`r#where` / `order_by` 返回 `&mut Self`（链式风格），无法即时返回 `Result`。实现把非法列名记为哨兵字符串，`all()` / `one()` / `count()` 时统一转 `InvalidInput`。调用方要注意错误不在构造点抛出（已登记为 `spec/30` 的 ORM1）。
- **rusqlite 错误码变体与预期不同**：`ErrorCode` 没有 `CantOpen` / `InvalidColumnName` 等变体，实际是 `CannotOpen` / `ApiMisuse` 等；按源码定义的枚举重写了映射表。
- **orphan rule 避开方式**：不能给外部类型 `NOrmValue` 实现外部 trait `rusqlite::ToSql`。驱动 crate 改为把 `NOrmValue` 映射成本地函数 `to_sql` 返回的 `rusqlite::types::NOrmValue`，再交给 `params_from_iter`。
- **raw identifier 解析**：`gen_api_index.py` 初版把 `r#where` 解析成 `r`，已修 fn 正则支持 `r#` 前缀；这是生成器第一次遇到 raw identifier。
- **基线变化**：实现期间工作区的 wasm 支持被外部改动移除（删 `wasm_exports`、`cdylib`、wasm 依赖），根基线 304 → 299 单测、契约 389 → 376 items。本 spec 与计划文件已按新基线改写，wasm 双目标验收项作废。
- **验证结果**：根 299 单测 + 21 doctest（1 ignored）通过；nothings-orm 8 测、nothings-orm-sqlite 10 测通过；契约 456 items，生成器 `--check` 返回 0；根 `cargo tree` 无 sqlite/orm 依赖。

### 2026-11-18 WHERE 条件增强（AST 化）

- `conditions: Vec<String> + params: Vec<NOrmValue>` 替换为 `where_: Option<NOrmCondition<E>>`；参数由渲染期按 `?` 遍历顺序收集，从结构上保证占位符与参数一一对应。
- 新增 `condition` 模块：`NOrmOperator` 枚举（12 个运算符）、`NOrmCondition<E>` 条件树（比较叶子 + AND/OR/NOT/Group），构造期校验列名并把结果内嵌到节点。旧的哨兵字符串被节点内 `Result` 取代，延后报错语义（ORM1）不变，但错误文本不再带 `__invalid_column__:` 内部前缀。
- 链式入口追加 `and_where` / `or_where` / `not_where` / `where_op` / `or_where_op` / `where_condition` / `or_where_condition` / `not_where_condition`。连接语义为**左折叠**：调用顺序决定结合顺序，渲染期用最小括号（`OR=1 < AND=2`）忠实还原；`where_condition` 不强制包 Group，避免多余括号。
- `build_select` 与 `count` 两处重复的 `join(" AND ")` 合并为单一 `render_where()`，保证 `all()` 与 `count()` 的条件完全一致。
- 驱动 crate 零改动——`NOrmDriver::query(&str, &[NOrmValue])` 只绑定参数；新增核心单测用假 NOrmDriver 精确断言 SQL，SQLite/MySQL 各加条件集成测试。
