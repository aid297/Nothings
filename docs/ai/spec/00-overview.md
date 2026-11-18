# 00 — 项目规格总览

- 文档基线：2026-11-18（移除 wasm 后）
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
| 过程宏子 crate | `nothings-macros` 0.0.3，`proc-macro = true`，路径依赖；提供 `Nothings` / `Entity` 派生宏 |
| ORM 核心 crate | `nothings-orm` 0.0.1，独立 crate，零 DB 依赖（`crate-type = ["lib"]`） |
| SQLite 驱动 crate | `nothings-orm-sqlite` 0.0.1，rusqlite 0.32 bundled（`crate-type = ["lib"]`） |
| 构建入口 | 根目录 `Cargo.toml`；无 workspace、无 Makefile、无 CI、无 examples。三个 crate 各自构建，需 `--manifest-path` |

## 2. 单一消费面

| 消费面 | 入口 | 使用者 | 契约文档 |
| --- | --- | --- | --- |
| `nothings` Rust API | `nothings::*` | 下游 Rust crate | `spec/10-rust-api.md` + `open-api/public-api.json` |
| `nothings-orm` Rust API | `nothings_orm::*` | 需要 ORM 的下游 crate | `nothings-orm/README.md` + `spec/50-orm-design.md` |
| `nothings-orm-sqlite` Rust API | `nothings_orm_sqlite::SqliteDriver` | 使用 SQLite 的下游 crate | `nothings-orm-sqlite/README.md` |

三个 crate 独立发布、独立版本；根 crate 不依赖 ORM，下游按需引入。发布顺序：`nothings-macros` → `nothings-orm` → `nothings-orm-sqlite`。

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

`nothings-orm` 只依赖 `nothings-macros 0.0.3`（path），零 DB 依赖。`nothings-orm-sqlite` 依赖 `nothings-orm 0.0.1`（path）+ `rusqlite 0.32`（`default-features = false`，features `bundled`；bundled 需要构建机有 C 编译器）。

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
└── src/              lib.rs value.rs row.rs schema.rs entity.rs driver.rs repository.rs query.rs
nothings-orm-sqlite/  Cargo.toml README.md tests/{crud,file_db}.rs
└── src/              lib.rs      # SqliteDriver + rusqlite(bundled)
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
| 派生宏 | `#[derive(Nothings)]` 由 `validations::mod` 重新导出；`#[derive(Entity)]` 由 `nothings_orm::lib` 重新导出 |
| 格式 | 不跑 `cargo fmt`（仓库整体非 rustfmt 格式，会产生大量无关 diff） |
| 测试 | 单测放同目录 `*_test.rs` / `test.rs`；文档示例写在 rustdoc |

## 7. 验收基线（2026-11-18 移除 wasm 后实测）

| 检查 | 结果 |
| --- | --- |
| 工具链 | `rustc 1.96.0` / `cargo 1.96.0`；目标平台 `aarch64-apple-darwin` |
| `cargo test` | 299 个单元测试通过；文档测试 21 passed / 1 ignored |
| 三个 crate 各自 `cargo test --manifest-path ...` | 根 299+21；nothings-orm 8；nothings-orm-sqlite 10（全部通过） |
| `cargo doc --no-deps` | 无 secrets 相关告警（历史记录） |
| `cargo clippy --all-targets` | 0 error（存量 `mut_from_ref` deny 已修复）；其它存量告警不处理，只要求新增代码 0 告警 |
| `python3 docs/ai/tools/gen_api_index.py --check` | 契约与源码一致：456 个公开 item（根 376 + orm 70 + sqlite 10）、0 个 C-ABI 导出 |

## 8. 版本与变更登记

| 日期 | 变更 | 影响 |
| --- | --- | --- |
| 2026-09-11 | 新增 `secrets` / `symmetric::aes`（设计 spec 见 `docs/superpowers/specs/2026-09-11-secrets-symmetric-aes-design.md`） | MSRV 1.85 → 1.89；新增 4 个 `with_aes_*` 工厂函数 |
| 2026-09-11 | 主文档与模块文档更新，补 wasm 支持与已知问题表 | 文档层 |
| 2026-11-17 | 新增 AI 开发文档层（`AGENTS.md`、`llms.txt`、`docs/ai/`） | 纯文档，无 API 变更 |
| 2026-11-18 | 彻底移除 wasm：删 `cdylib`、`wasm_exports`（13 个 C-ABI 导出）、wasm target 依赖，回归纯 Rust lib | 破坏性：移除 C-ABI 消费面；0.0.x 线不升版本 |
| 2026-11-18 | 修正 `AnyMapTrait` 的 `every` / `clean` 声明签名，消除 2 个 `mut_from_ref` deny | trait 仍 unimplemented，无运行时行为变化；契约签名 2 处更新，item 数不变；0.0.x 线不升版本 |
| 2026-11-18 | 新增 3 个独立 crate：`nothings-macros` 0.0.3（加 `Entity` 派生宏）、`nothings-orm` 0.0.1、`nothings-orm-sqlite` 0.0.1 | 根 crate 零 DB 依赖，仅宏依赖升版本；契约 376 → 456 item；不建 workspace、不加 features |

## 9. 明确不提供

- HTTP 服务 / 路由 / OpenAPI 风格的 HTTP 端点（这是库 crate）。
- Cargo feature 开关（当前没有 `[features]` 段）。
- AES-GCM 的 AAD、base64/hex 编解码、文件加解密、密钥内存擦除（`zeroize` 未启用）。
- 标准 ZIP 归档（`Dir::zip` 是自定义 TLV + 压缩）。
- SM4 / 非对称加密：`secrets::symmetric` 是预留的分类命名空间，尚未实现。
- ORM 的 async、连接池、关联加载、Migration、软删除/时间戳钩子、MySQL/Postgres 驱动（见 `spec/50` Phase 2 路线图）。
