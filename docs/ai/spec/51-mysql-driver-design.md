# 51 — MySQL 驱动设计

- 日期：2026-11-18
- 状态：已实现，验收中
- 范围：新增独立 crate `nothings-orm-mysql` 0.0.1，实现 `nothings_orm::Driver`
- 批准计划：`/Users/yujizhou/.workbuddy/plans/electric-thunder-turing-KS8oKYI5.md`

## 背景与目标

`nothings-orm` / `nothings-orm-sqlite` 已跑通 Phase 1 最小闭环。本次补齐 MySQL 驱动，对标 `gorm.io/driver/mysql`。

四项已确认决策：

1. 用同步 `mysql` crate，不引 tokio / `mysql_async` / `sqlx`。
2. **零改动 `nothings-orm`**：MySQL 原生支持 `?` 占位符、`LAST_INSERT_ID()`、`START TRANSACTION`，核心 SQL 模板全部可用。代价是裸标识符拼接带来的保留字问题，作为已知边界。
3. `Value` 六种变体不变，MySQL 特有类型降级映射。
4. 集成测试强制连真实 MySQL，缺 DSN 直接失败，不静默跳过。

硬边界：只新增一个代码目录 + 本设计文档；`nothings-orm` / `nothings-macros` / `nothings-orm-sqlite` / 根 crate 零改动。

## 模块结构与依赖

```
nothings-orm-mysql/
├── Cargo.toml        edition 2024 / crate-type = ["lib"] / MIT
├── README.md
├── src/lib.rs        MysqlDriver + to_io / to_sql / bound / from_value / is_binary
└── tests/
    ├── common/mod.rs  强制 DSN + 独立 database 夹具
    ├── crud.rs        CRUD / QueryBuilder / 事务 / 注入防护 / __count / AUTO_INCREMENT
    ├── connection.rs  连接失败 / execute_ddl / 多语句实测
    └── types.rs       类型降级 / UInt 溢出 / UTF-8 与 BLOB
```

- `nothings-orm = { version = "0.0.1", path = "../nothings-orm" }`（path + version 双声明）。
- `mysql = { version = "28.0", default-features = false, features = ["minimal-rust"] }`：关掉 `minimal`（`flate2/zlib` C 依赖）与全部 TLS feature，走纯 Rust `miniz_oxide`。实测解析为 `mysql 28.0.4` + `mysql_common 0.37.3`，依赖树只有一份 `mysql_common`。
- 不显式依赖 `mysql_common`：统一用 `mysql::consts::{ColumnType, ColumnFlags}` 与 `mysql::{Value, Column, Params, Conn, Error, MySqlError, ServerError}` 重导出，避免跨版本类型分叉。
- 无 workspace、无 `[features]`、无 `[dev-dependencies]`、无 CI / Makefile / examples。

## 接口

### 公开 API

```rust
pub struct MysqlDriver { conn: mysql::Conn }
impl MysqlDriver {
    pub fn connect(url: &str) -> Result<Self, std::io::Error>;
    pub fn execute_ddl(&mut self, sql: &str) -> Result<(), std::io::Error>;
}
impl nothings_orm::driver::Driver for MysqlDriver { /* 6 个方法 */ }
```

`execute_ddl` 刻意不叫 `execute_batch`：只接固定字面量 DDL，不参数化、绝不接用户输入。

### 六个 trait 方法

| 方法 | 实现 |
| --- | --- |
| `execute` | `exec_drop(sql, bound(params))` → `conn.affected_rows()`。`QueryResult` 持有 `&mut Conn`，必须先丢弃才能读连接状态 |
| `query` | `exec_iter`（二进制协议 / prepared statement，带 `?` 参数）→ `Column::name_str()` 原样取列名 → 全量物化 `OwnedRow` |
| `last_insert_id` | `conn.last_insert_id()`（缓存的 OK packet），`u64 → i64` 溢出报 `InvalidData` |
| `begin` / `commit` / `rollback` | `query_drop("START TRANSACTION" / "COMMIT" / "ROLLBACK")`。`Conn::transaction()` 借用 `&mut Conn` 且 `Drop` 隐式 rollback，存进 `self` 构成自引用；不改 `autocommit` |

两个不可破坏的核心契约：`query()` 必须原样保留 `COUNT(*) AS __count` 列名/别名，`COUNT(*)` 必须映射到 `Value::I64`；`execute()` 返回前必须让连接缓存好 OK packet，因为 `Repository::insert` 紧接着调用 `last_insert_id()`。

### 类型映射

| core `Value` → `mysql::Value` | | `mysql::Value` → core `Value` | |
| --- | --- | --- | --- |
| `Null` | `NULL` | `NULL` | `Null` |
| `Bool` | `Int(1/0)` | `Int(n)` | `I64(n)` |
| `I64` | `Int(n)` | `UInt(n ≤ i64::MAX)` | `I64` |
| `F64` | `Double(n)` | `UInt` 溢出 | `InvalidData` |
| `Text` | `Bytes(utf8)` | `Float` / `Double` | `F64` |
| `Blob` | `Bytes(b)` | `Bytes` + 二进制列 | `Blob` |
| | | `Bytes` + 文本列 | `Text`；非法 UTF-8 → `InvalidData` |
| | | `Date` / `Time` | `Text`（`YYYY-MM-DD HH:MM:SS[.ffffff]` / `[-]HHH:MM:SS[.ffffff]`） |

DATETIME / DATE / DECIMAL / JSON 一律降级为 `Text`，无类型保真。

### Text / Blob 判定

按 `ColumnType` 优先级，绝不靠 UTF-8 试探（BLOB 内容可能恰好是合法 UTF-8，JSON/DECIMAL 自带 binary charset）：

1. `JSON` / `DECIMAL` / `NEWDECIMAL` / `DATE` / `DATETIME*` / `TIMESTAMP*` / `TIME*` / `YEAR` / `ENUM` / `SET` → 强制 `Text`。
2. `BLOB` 家族 / `BIT` / `GEOMETRY` / `VECTOR` → `Blob`。
3. 字符串类型 → `ColumnFlags::BINARY_FLAG || character_set() == 63` 决定 `Blob` / `Text`。

### 错误映射

`mysql::Error` → `std::io::Error`，orphan rule 下用私有固有函数 `to_io`，不 `impl From`、不新增公开错误类型。

| 来源 | ErrorKind |
| --- | --- |
| 1062 `ER_DUP_ENTRY` | `AlreadyExists` |
| 1049 / 1146 库表不存在 | `NotFound` |
| 1054 未知列 | `InvalidInput` |
| 1064 语法错误 | `InvalidData` |
| 1045 认证失败 | `PermissionDenied` |
| URL 非法、占位符/参数不匹配 | `InvalidInput` |
| 非法 UTF-8、UInt 溢出、编解码失败 | `InvalidData` |
| `IoError` | 保留原 `io.kind()` |
| 其它服务端码 / 连接失败 | `Other` |

## 明确不做（YAGNI）

- 引入 Dialect 抽象 / 标识符反引号引用（保留字作为已知边界）。**2026-11-18 追加**：`Value` 枚举已因双时区需求扩展 `DateTime` 变体，原「零改动 core」边界被推翻，见「实现记录」。
- 扩展 `Value` 枚举的 Decimal / Json / U64 变体；扩展派生宏属性。
- async / tokio / 连接池 / TLS / Migration / 关联 / 钩子 / 批处理 / upsert / 复合主键 / 新公开错误类型。
- 命名时区（依赖 MySQL 时区表 + 自动 DST 规则）；固定偏移无 DST 是明确取舍。
- 测试静默跳过 / `#[ignore]` / CI / Dockerfile。

## 测试计划

强制 DSN `NOTHINGS_MYSQL_DSN`（不带 database，账号需 `CREATE/DROP DATABASE` 权限）。每个测试建独立 database（`pid + 原子序号`），`Drop` 自动清理；`Schema::TABLE` 是编译期常量，不能动态改表名。

覆盖：CRUD roundtrip、QueryBuilder（含非法列名注入防护）、约束冲突、事务提交/回滚、嵌套事务、`__count` 别名、AUTO_INCREMENT 递增、连接失败、DDL 辅助、类型降级、UInt 溢出、中文 `Text` 与非 UTF-8 `BLOB`。事务内只做 DML（DDL 隐式提交）。

## 验收标准

- `cargo test --manifest-path nothings-orm-mysql/Cargo.toml`：18 测全通过（4 单元 + 14 集成），实测 MySQL 8.0.46。
- `cargo clippy --all-targets`：新增代码 0 告警。
- `cargo tree`：根 crate 无 DB 依赖；新 crate 无 native-tls / openssl / zlib-sys / tokio，只有一份 `mysql_common`。
- `python3 docs/ai/tools/gen_api_index.py --check` 返回 0。
- MSRV：`rustc 1.96.0` 编译通过。`cargo +1.89.0 check` **未实测成功**——1.89.0 toolchain 下载/安装受网络限制中断（缺 manifest），不能声称通过。`mysql 28.0.4` / `mysql_common 0.37.3` 均未声明 `rust-version`，关键依赖 `lru 0.18.5` 声明 1.85.0，静态推断不高于仓库声明的 1.89+。验证命令：`rustup toolchain install 1.89.0 --profile minimal && cargo +1.89.0 check --manifest-path nothings-orm-mysql/Cargo.toml`；若失败按 `mysql 26.0.1` → `25.0.1` 回退（保持 `default-features = false`）。

### 落地偏差（实测）

- **多语句假设被推翻**：设计阶段假定「MySQL 默认禁用多语句」，实测 `mysql 28.0.4` 的 `query_drop` 允许分号拼接多语句（客户端开启 `CLIENT_MULTI_STATEMENTS`），并成功创建两张表。`execute_ddl` 因此不主动拒绝多语句，安全边界改为「只能用于固定字面量 DDL、绝不接用户输入」，并新增 `test_multi_statement_behavior` 固化该行为。
- `mysql` 与 `mysql_common` 均未声明 `rust-version`；`lru 0.18.5` 声明 1.85.0。MSRV 只能靠实测，不能靠声明断定。

## 实现记录

### 2026-11-18 追加：双时区自动转换（原 YAGNI 被需求推翻）

用户新需求：`MysqlDriver` 连接时增加 DB 时区与 ORM 时区两个参数，写入 ORM→DB、读取 DB→ORM 自动转换，查询条件走同一路径；`i64` 时间戳不参与。

**决策变更**：原「零改动 core / 不扩展 Value 枚举」被直接推翻，理由写死在此：

- 写侧 `Driver::execute(sql, &[Value])` 只有 SQL 字符串 + 无类型 `Value`，没有列名/列类型；`Value::Text` 同时承载 DATETIME / TIMESTAMP / DECIMAL / JSON / 普通字符串。不在 `Value` 层加类型标记，驱动无法知道哪个参数是时间——靠内容猜测被驱动既有原则明确禁止。
- 因此 core 新增零依赖 `UtcOffset`（固定偏移，分钟）与 `DateTime`（墙钟）类型，`Value` 新增 `DateTime(DateTime)` 变体。`SqlType` 不加变体（`DateTime` 的 `SQL_TYPE = Text`），降低破坏面。
- 派生宏**无需改动**：`column_value` / `from_row` / `COLUMNS` 全走 `ToValue/FromValue/HasSqlType` 泛型路径，不硬编码 `Value` 变体。

**语义**：

- `connect(url)` 保留，内部 UTC/UTC 恒等转换，行为向后兼容；`connect_with_tz(url, orm_tz, db_tz)` 新增。
- 会话时区通过 `OptsBuilder::init(vec!["SET time_zone = '+08:00'"])` 下发（DSN 不接受 `?timezone=`，会 `UrlError::UnknownParameter`），`init` 在连接与 `reset()` 自动重放；连接后 `SELECT @@session.time_zone` 校验，不符返回 `InvalidData`。
- 写入：`Value::DateTime(dt)` → `dt.shifted_minutes(db_tz - orm_tz)` → `MysqlValue::Date`。TIMESTAMP 列由会话时区再转 UTC 存储，是正确衔接非双重转换。
- 读取：`ColumnType` 区分 TIMESTAMP*/DATETIME* → 转回 ORM 墙钟返回 `Value::DateTime`；DATE/TIME/YEAR 维持 `Text`。
- **语义统一（拍板）**：所有 TIMESTAMP/DATETIME 列读回一律 `Value::DateTime`，不分入口。用 `String` 字段接时间列的既有实体会读到 `InvalidData`，必须改用 `DateTime` 字段——有意破坏性变更，见 `spec/30` MYSQL7。
- SQLite 不支持自动转换：`to_sql` 把 `DateTime` 按墙钟原样写 TEXT，读回由 `FromValue for DateTime` 的 Text 解析兜底。文档明确「时区自动转换为 MySQL 专属能力」。

**实测**：10 个时区集成测试（真 MySQL 8.0.46）覆盖 ORM=UTC/DB=+8 写入读回、裸 SQL 查库墙钟差 8 小时、TIMESTAMP 与 DATETIME 一致、查询条件共用转换、负偏移与 +05:45/-03:30、i64 不受影响、微秒（需 `DATETIME(6)`）、会话时区校验、旧入口兼容。全套 29 测通过，clippy 0 告警。契约 465 → 488 items。

**MSRV**：不引入 chrono/time，仍 1.89+；1.89 toolchain 实测仍未成功，保持原有标注。
