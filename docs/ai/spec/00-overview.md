# 00 — 项目规格总览

- 文档基线：2026-11-18（新增 MySQL 驱动 crate 后）
- 状态：现行实现的事实描述，不是设计方案
- 事实来源：`Cargo.toml`、`src/lib.rs`、各 `src/**/mod.rs`

## 1. 标识与产物

| 项 | 值 |
| --- | --- |
| crate 名 / 版本 | `nothings` / `0.0.9` |
| edition / MSRV | 2024 / Rust 1.89+（`aes 0.9` 的 `rust-version` 决定） |
| `crate-type` | `["lib"]` |
| License | MIT |
| repository / homepage | `https://github.com/aid297/aid-rs.git` / `https://github.com/aid297/aid` |
| 过程宏子 crate | `nothings-macros` 0.0.3，`proc-macro = true`，路径依赖；提供 `Nothings` / `NOrmEntity` 派生宏 |
| ORM 核心 crate | `nothings-orm` 0.0.1，独立 crate，零 DB 依赖（`crate-type = ["lib"]`） |
| SQLite 驱动 crate | `nothings-orm-sqlite` 0.0.1，rusqlite 0.32 bundled（`crate-type = ["lib"]`） |
| MySQL 驱动 crate | `nothings-orm-mysql` 0.0.1，mysql 28.0 `minimal-rust`（纯 Rust，无 C/TLS 依赖） |
| 构建入口 | 根目录 `Cargo.toml`；无 workspace、无 Makefile、无 CI、无 examples。契约登记 4 个 crate（磁盘 5 个 Cargo.toml，宏 crate 不进契约），各自构建需 `--manifest-path` |

## 2. 单一消费面

| 消费面 | 入口 | 使用者 | 契约文档 |
| --- | --- | --- | --- |
| `nothings` Rust API | `nothings::*` | 下游 Rust crate | `spec/10-rust-api.md` + `open-api/public-api.json` |
| `nothings-orm` Rust API | `nothings_orm::*` | 需要 ORM 的下游 crate | `nothings-orm/README.md` + `spec/50-orm-design.md` |
| `nothings-orm-sqlite` Rust API | `nothings_orm_sqlite::NOrmSqliteDriver` | 使用 SQLite 的下游 crate | `nothings-orm-sqlite/README.md` |
| `nothings-orm-mysql` Rust API | `nothings_orm_mysql::NOrmMysqlDriver` | 使用 MySQL 的下游 crate | `nothings-orm-mysql/README.md` + `spec/51-mysql-driver-design.md` |

契约登记的 4 个 crate 独立发布、独立版本；根 crate 不依赖 ORM，下游按需引入。发布顺序：`nothings-macros` → `nothings-orm` → `nothings-orm-sqlite` → `nothings-orm-mysql` → 根 `nothings`。

## 3. 模块支持

所有模块无条件参与编译，无平台差异（zstd 为 C 绑定，需原生工具链）。

| 模块 / 能力 | 支持 | 说明 |
| --- | --- | --- |
| `any_slices` / `any_maps` | ✅ | 纯 Rust |
| `digests`（bcrypt / md5 / sha） | ✅ | 纯 Rust |
| `compressions::{lz4, zlib}` + `with_lz4*` / `with_zlib*` | ✅ | 纯 Rust |
| `compressions::zstd4` + `with_zstd*` | ✅ | zstd 是 C 绑定 |
| `secrets`（含 `Aes`、`with_aes_*`） | ✅ | RustCrypto 纯 Rust |
| `filesystem`（`File` / `Dir`） | ✅ | 依赖原生文件系统 |
| `http_responses` / `texts::timer` / `validations` / `singletons` | ✅ | 纯 Rust |
| `coroutines` | ✅ | 空占位，`app.rs` 为 0 字节 |

## 4. 依赖

依赖：`rand 0.10.2`、`flate2 1`、`bcrypt 0.17`、`md-5 0.10`、`sha2 0.10`、`nothings-macros 0.0.3`（path）、`serde 1.0.229`（derive）、`lz4_flex 0.14`（frame）、`zstd 0.13`、`aes 0.9`、`cbc 0.2`（alloc）、`ctr 0.10`（alloc）、`ecb 0.2`（alloc）、`aes-gcm 0.11`（`default-features = false`，features `aes` + `alloc`）。无 target 条件依赖。

随机数走平台默认熵源。`aes-gcm` 关闭默认 `getrandom` feature，只启用 `aes` + `alloc`。

`nothings-macros` 自身依赖 `quote 1`、`syn 2`（full + parsing）、`proc-macro2 1`。

`nothings-orm` 只依赖 `nothings-macros 0.0.3`（path），零 DB 依赖。`nothings-orm-sqlite` 依赖 `nothings-orm 0.0.1`（path）+ `rusqlite 0.32`（`default-features = false`，features `bundled`；bundled 需要构建机有 C 编译器）。`nothings-orm-mysql` 依赖 `nothings-orm 0.0.1`（path）+ `mysql 28.0`（`default-features = false`，features `minimal-rust`；实测 `mysql 28.0.4` + `mysql_common 0.37.3`，无 native-tls / openssl / zlib-sys / tokio）。

## 5. 模块树

```
src/lib.rs                        # extern crate self as nothings; + 11 个 pub mod
├── any_slices/       app.rs test.rs README.md     # AnySlice<T>
├── any_maps/         app.rs test.rs README.md     # AnyMap<K,V>
├── compressions/     mod.rs(trait+工厂) README.md
│   ├── lz4/          app.rs test.rs README.md
│   ├── zlib/         app.rs test.rs README.md
│   └── zstd4/        app.rs test.rs README.md
├── coroutines/       mod.rs app.rs(空)            # 预留，无内容
├── digests/          bcrypt.rs md5.rs sha.rs test.rs README.md
├── filesystem/       file.rs dir.rs *_test.rs README.md
├── http_responses/   mod.rs README.md            # HttpResponse<T>
├── secrets/          mod.rs(trait+工厂) README.md
│   └── symmetric/aes/ app.rs test.rs README.md
├── singletons/       app.rs test.rs README.md    # impl_singleton! 在 crate 根导出
├── texts/            timer.rs(公开) volumer.rs prelude.rs(均私有) README.md
├── validations/      validation.rs check.rs checker.rs field.rs error.rs
│                     validator_struct_parser.rs test.rs README.md
```

独立 crate（与 `src/` 平级，各自有 Cargo.toml 与 README）：

```
nothings-orm/         Cargo.toml README.md tests/entity.rs
└── src/              lib.rs value.rs row.rs schema.rs entity.rs driver.rs repository.rs query.rs condition.rs
nothings-orm-sqlite/  Cargo.toml README.md tests/{crud,file_db}.rs
└── src/              lib.rs      # NOrmSqliteDriver + rusqlite(bundled)
nothings-orm-mysql/   Cargo.toml README.md tests/{crud,connection,types}.rs tests/common/mod.rs
└── src/              lib.rs      # NOrmMysqlDriver + mysql(minimal-rust)
nothings-macros/      Cargo.toml src/{lib,entity}.rs
```

`src/lib.rs` 中 `texts::prelude`、`texts::volumer` 是私有 `mod`；`digests::test` 是 `#[cfg(test)] pub mod`。

## 6. 全局约定

| 约定 | 内容 |
| --- | --- |
| 模块布局 | `mod.rs` 声明 + `app.rs`/功能文件实现 + `test.rs` 单测 + `README.md` 文档 |
| 错误类型 | 压缩/加密用 `std::io::Error`；文件系统多数方法用 `Result<_, String>`；`mv`/`create`/`up`/`join` 写公开 `err: Option<String>` 字段且不返回错误 |
| 链式调用 | 变更类方法返回 `&mut Self`；查询类返回克隆值或 `bool` |
| 构造 | 构造函数不落错，参数校验推迟到 `encrypt` / `compress` 等实际调用 |
| trait 命名 | 不加 `Trait` 后缀（`Compressor` / `Symmetric`）；`TimeTrait` 等为历史遗留 |
| 派生宏 | `#[derive(Nothings)]` 由 `validations::mod` 重新导出；`#[derive(NOrmEntity)]` 由 `nothings_orm::lib` 重新导出 |
| 格式 | 不跑 `cargo fmt`（仓库整体非 rustfmt 格式，会产生大量无关 diff） |
| 测试 | 单测放同目录 `*_test.rs` / `test.rs`；文档示例写在 rustdoc |

## 7. 验收基线（2026-11-18 新增 MySQL 驱动后实测）

| 检查 | 结果 |
| --- | --- |
| 工具链 | `rustc 1.96.0` / `cargo 1.96.0`；目标平台 `aarch64-apple-darwin` |
| `cargo test` | 299 个单元测试通过；文档测试 21 passed / 1 ignored |
| 四个 crate 各自 `cargo test --manifest-path ...` | 根 299+21；nothings-orm 40（36 单测含 16 项 WHERE 渲染断言 + 4 派生宏集成）；nothings-orm-sqlite 22（含 10 个新增条件集成）；nothings-orm-mysql 基线 29，另新增 6 个条件集成测试（已编译验证，本次环境无 `NOTHINGS_MYSQL_DSN` 未实跑） |
| `cargo doc --no-deps` | 无 secrets 相关告警（历史记录） |
| `cargo clippy --all-targets` | 0 error（存量 `mut_from_ref` deny 已修复）；其它存量告警不处理，只要求新增代码 0 告警 |
| `python3 docs/ai/tools/gen_api_index.py --check` | 契约与源码一致：**559** 个公开 item（根 378 + orm 161 + sqlite 10 + mysql 10）、0 个 C-ABI 导出；`generator_version = 3`、4 个源根 / 4 个 crate |

## 8. 版本与变更登记

| 日期 | 变更 | 影响 |
| --- | --- | --- |
| 2026-09-11 | 新增 `secrets` / `symmetric::aes`（设计 spec 见 `docs/superpowers/specs/2026-09-11-secrets-symmetric-aes-design.md`） | MSRV 1.85 → 1.89；新增 4 个 `with_aes_*` 工厂函数 |
| 2026-09-11 | 主文档与模块文档更新，补 wasm 支持与已知问题表 | 文档层 |
| 2026-11-17 | 新增 AI 开发文档层（`AGENTS.md`、`llms.txt`、`docs/ai/`） | 纯文档，无 API 变更 |
| 2026-11-18 | 彻底移除 wasm：删 `cdylib`、`wasm_exports`（13 个 C-ABI 导出）、wasm target 依赖，回归纯 Rust lib | 破坏性：移除 C-ABI 消费面；0.0.x 线不升版本 |
| 2026-11-18 | 修正 `AnyMapTrait` 的 `every` / `clean` 声明签名，消除 2 个 `mut_from_ref` deny | trait 仍 unimplemented，无运行时行为变化；契约签名 2 处更新，item 数不变；0.0.x 线不升版本 |
| 2026-11-18 | 新增 3 个独立 crate：`nothings-macros` 0.0.3（加 `NOrmEntity` 派生宏）、`nothings-orm` 0.0.1、`nothings-orm-sqlite` 0.0.1 | 根 crate 零 DB 依赖，仅宏依赖升版本；契约 376 → 456 item；不建 workspace、不加 features |
| 2026-11-18 | 新增独立 crate `nothings-orm-mysql` 0.0.1（同步 `mysql` crate，纯 Rust），新增设计 spec `docs/ai/spec/51-mysql-driver-design.md` | ORM core / 宏 / SQLite 驱动 / 根 crate 零改动；契约 456 → 465 item、`generator_version` 2 → 3、源根 3 → 4；集成测试强制真实 MySQL |
| 2026-11-18 | 双时区自动转换：`nothings-orm` 新增 `NOrmUtcOffset` / `NOrmDateTime` / `NOrmValue::NOrmDateTime`，`NOrmMysqlDriver::connect_with_tz` | **破坏性**：`NOrmValue` 加变体；TIMESTAMP/DATETIME 读回一律 `NOrmValue::NOrmDateTime`，`String` 字段须迁移。契约 465 → 488 item；新增 MYSQL7 边界；依赖仍零新增（不引 chrono/time） |
| 2026-11-18 | WHERE 条件增强：`nothings-orm` 新增 `condition` 模块（`NOrmCondition<E>` / `NOrmOperator`），`NOrmQueryBuilder` 追加 8 个链式条件方法；WHERE 渲染 AST 化并合并 `all()` / `count()` 路径 | 非破坏性：现有 7 个方法签名与 AND-only SQL 不变；错误文本去掉内部哨兵前缀；契约新增公开 item，需重生成 `public-api.json`；驱动 crate 零改动 |
| 2026-11-18 | ORM 全部公开类型加 `NOrm` 前缀（19 个：`NOrmEntity` / `NOrmSchema` / `NOrmDriver` / `NOrmRepository` / `NOrmQueryBuilder` / `NOrmCondition` / `NOrmOperator` / `NOrmValue` / `NOrmToValue` / `NOrmFromValue` / `NOrmHasSqlType` / `NOrmSqlType` / `NOrmColumn` / `NOrmRow` / `NOrmOwnedRow` / `NOrmDateTime` / `NOrmUtcOffset` / `NOrmSqliteDriver` / `NOrmMysqlDriver`） | **破坏性**：旧名无别名、全部移除；派生宏改 `#[derive(NOrmEntity)]`，但 `#[entity(...)]` 属性名与 trait 成员名（`SQL_TYPE` / `TABLE` / `from_row` 等）保持不变；宏生成路径、驱动第三方类型（`mysql::Column` / `rusqlite::Value`）不受影响；契约 item 数不变（515），仅路径改名 |
| 2026-11-19 | ORM SQL 层 JOIN / 子查询：`join.rs` 新增 `NOrmJoinKind` / `NOrmSubquery` / `NOrmFromJoined`；`NOrmQueryBuilder` 加 join / where_qualified / where_*_subquery / all_with；`NOrmPrefixedRow` 实现零宏改动的宽行映射 | 非破坏性（纯追加）：单表 SQL 字符串不变；契约 515 → **539 items**；JOIN 表别名 t1..tn、主表 t0；关联对象聚合仍在 YAGNI；两驱动零改动 |
| 2026-11-19 | ORM Preload 单层关联预加载：`schema.rs` 加 `NOrmRelation` / `NOrmRelationKind` / `RELATIONS` 默认常量，`entity.rs` 加 `NOrmPreload` trait；派生宏支持 `has_many` / `belongs_to` 字段属性并生成 `fill_relation`；`NOrmQueryBuilder::preload` 两阶段批量查询 + 500 分块 | 非破坏性（纯追加）：无关联实体零改动，`all()` bound 不变；契约 539 → **550 items**；递归/多层预加载仍在 YAGNI；驱动零改动 |
| 2026-11-19 | 修复 spec/30 的 A1-A7 七个失效 API：容器批量删除/shuffle/every、`Dir::join`、`impl_singleton!` 外部可见性、Lz4 分级压缩 | A1→A6 纯修复（两处签名对齐 trait：`AnyMap::every -> bool`、`remove_empty -> &mut Self`）；A7 新增 `lz4 = "1"`（bundled liblz4 HC，C 工具链必需，解压端 lz4_flex 不变），MSRV 1.89 实测通过；新增跨 crate 集成测试 `tests/singletons.rs`；契约 550 → **552 items**（`Singleton` trait + `with` 随公开化新增） |
| 2026-11-19 | ORM 关联能力 Phase 4：`preload` 支持点分路径递归（`books.comments`，逐层批量非 N+1）、结果按目标主键去重、前缀路径自动合并；`NOrmPreload` 新增 provided `preload_one` / `preload_path`（宏生成多层实现，手写 impl 默认单层）；`NOrmRepository::load_relation` 懒加载 | 非破坏性（provided 默认方法 + 纯追加）；契约 552 → **555 items**；新增 PL4/PL5 边界；两驱动零改动 |
| 2026-11-19 | ORM Phase 5：软删除 / 时间戳钩子 / 自动时间。字段标志 `soft_delete`/`created_at`/`updated_at`；`NOrmSchema` 三个带默认值的角色列常量 + `NOrmRelation::soft_delete_col`；`unscoped()` 与 `hard_delete()`；`NOrmDateTime::now_utc()` 零依赖 UTC 墙钟 | 非破坏性（默认值常量 + 纯追加）：未启用软删的实体 SQL 逐字节不变；契约 555 → **559 items**；新增 SD1-SD2/TM1-TM2 边界；时间戳 MySQL 连接须 `orm_tz=UTC` |

## 9. 明确不提供

- HTTP 服务 / 路由 / OpenAPI 风格的 HTTP 端点（这是库 crate）。
- Cargo feature 开关（当前没有 `[features]` 段）。
- AES-GCM 的 AAD、base64/hex 编解码、文件加解密、密钥内存擦除（`zeroize` 未启用）。
- 标准 ZIP 归档（`Dir::zip` 是自定义 TLV + 压缩）。
- SM4 / 非对称加密：`secrets::symmetric` 是预留的分类命名空间，尚未实现。
- ORM 的 async、连接池、Migration、Postgres 驱动（MySQL 驱动已由 `nothings-orm-mysql` 提供，见 `spec/51`；Phase 2 路线见 `spec/50`）。

