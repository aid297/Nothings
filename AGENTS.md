# AGENTS.md — nothings 的 AI 开发约定

> 本文件给 AI 编码助手看。先读完再动代码。人类可读的完整说明在 `README.md`，机器可读的 API 契约在 `docs/ai/open-api/`。

## 1. 这是什么

`nothings` 是一个 **Rust 库 crate**（`crate-type = ["lib"]`），不是服务、不是 CLI、没有 HTTP 端点、没有 examples、没有 CI 配置、没有 Makefile。消费面只有一个：**Rust 公开 API**（`nothings::*`），供下游 crate `use`。

| 项 | 值 |
| --- | --- |
| 版本 / edition | 0.0.9 / 2024 |
| MSRV | Rust 1.89+（`aes 0.9` 决定，高于 edition 2024 本身的 1.85） |
| License | MIT |
| 主 crate | 根目录 `Cargo.toml` |
| 过程宏子 crate | `nothings-macros/`（`proc-macro = true`，提供 `#[derive(Nothings)]` 与 `#[derive(NOrmEntity)]`） |
| ORM 核心 crate | `nothings-orm/`（零 DB 依赖，对标 gorm.io/gorm） |
| SQLite 驱动 crate | `nothings-orm-sqlite/`（rusqlite bundled，对标 gorm.io/driver/*） |
| MySQL 驱动 crate | `nothings-orm-mysql/`（同步 `mysql` crate，`minimal-rust` 纯 Rust；`connect_with_tz` 提供双时区自动转换） |

## 2. 动手前必须知道的三件事

1. **方法名不等于行为**。本库存在一批「签名存在、实现失效」的公开方法。生成代码前查 `docs/ai/spec/30-known-issues.md`。最高频的坑：
   - `AnySlice::remove_by_indexes` 是空操作，连带 `AnyMap::filter` / `remove_by_keys` / `remove_by_values` / `remove_empty` 全部失效。
   - `AnySlice::shuffle` 返回未打乱的副本；用 `shuffle_self`。
   - `AnyMap::every` 返回 `&Self` 而不是 `bool`，不能当谓词。
   - `Dir::join` 是空操作；`Dir::zip` 产出的不是 ZIP 文件。
   - `impl_singleton!` 宏在外部 crate **整体编译失败**（`Singleton` trait 是 `pub(crate)`）。
2. **平台无差异**。所有模块无条件参与编译，`filesystem` 与 `compressions::zstd4`（含 `with_zstd*`）均已移除 `cfg` 门控。改 `src/lib.rs` 或 `Cargo.toml` 的依赖前先看 `docs/ai/spec/00-overview.md`。
3. **错误风格有三套且混用**：`Result<_, std::io::Error>`（压缩/加密）、`Result<_, String>`（文件系统多数方法）、公开 `err: Option<String>` 字段（文件系统 `mv` / `create` / `up` / `join`）。第三种不返回错误，必须手动检查 `err`。

## 3. 命令

```bash
cargo test                                      # 单元测试 + 文档测试，改代码后必跑
cargo test --manifest-path nothings-orm/Cargo.toml
cargo test --manifest-path nothings-orm-sqlite/Cargo.toml
cargo test --manifest-path nothings-orm-mysql/Cargo.toml   # 需 NOTHINGS_MYSQL_DSN，强制真库
cargo test --manifest-path nothings-macros/Cargo.toml
cargo doc --no-deps                             # 公开 API 文档
cargo clippy --all-targets                      # 存量告警较多，只要求「新增代码 0 告警」
python3 docs/ai/tools/gen_api_index.py           # 重新生成公开 API 契约 JSON（改过 public item 就跑）
python3 docs/ai/tools/gen_api_index.py --check   # 漂移自检，返回 0 才算一致
```

不要跑 `cargo fmt`：仓库整体不是 rustfmt 格式，全量格式化会产生巨大无关 diff。新代码跟随所在文件的现有风格。

## 4. 模块地图

每个二级模块通常是 `mod.rs`（声明 + `#[cfg(test)] mod test;`）、`app.rs`（实现）、`test.rs`（测试）、`README.md`（完整 API 文档）四件套。

| 模块 | 导入路径 | 状态 | 文档 |
| --- | --- | --- | --- |
| AnySlice | `any_slices::app::AnySlice` | 有失效方法 | `src/any_slices/README.md` |
| AnyMap | `any_maps::app::AnyMap` | 有失效方法 | `src/any_maps/README.md` |
| Compressions | `compressions::{Compressor, with_lz4, ...}` | 可用 | `src/compressions/README.md` |
| Lz4 / Zlib / Zstd | `compressions::{lz4,zlib,zstd4}::app::*` | 可用 | 各目录 `README.md` |
| Digests | `digests::{bcrypt,md5,sha}::*` | 可用 | `src/digests/README.md` |
| Secrets | `secrets::{Symmetric, with_aes_*}` | 可用，纯 Rust | `src/secrets/README.md` |
| Aes | `secrets::symmetric::aes::app::Aes` | 可用 | `src/secrets/symmetric/aes/README.md` |
| Filesystem | `filesystem::{file::File, dir::Dir}` | 坑多 | `src/filesystem/README.md` |
| HttpResponse | `http_responses::HttpResponse<T>` | 仅 `Serialize` | `src/http_responses/README.md` |
| Texts / Timer | `texts::timer::{Time, TimeTrait}` | 可用；`prelude` / `volumer` 私有 | `src/texts/README.md` |
| Validations | `validations::{Nothings, check::Check, ...}` | 可用 | `src/validations/README.md` |
| Singletons | `nothings::impl_singleton!` | **外部 crate 不可用** | `src/singletons/README.md` |
| Coroutines | `coroutines` | 空占位（`app.rs` 为 0 字节） | 无 |

另有三个独立 crate（无 workspace，需 `--manifest-path` 单独构建）：

| crate | 入口 | 说明 | 文档 |
| --- | --- | --- | --- |
| nothings-orm | `nothings_orm::{NOrmEntity, NOrmSchema, NOrmDriver, NOrmRepository}` | ORM 核心，零 DB 依赖 | `nothings-orm/README.md` |
| nothings-orm-sqlite | `nothings_orm_sqlite::NOrmSqliteDriver` | SQLite 驱动（rusqlite bundled） | `nothings-orm-sqlite/README.md` |
| nothings-orm-mysql | `nothings_orm_mysql::NOrmMysqlDriver` | MySQL 驱动（同步 mysql crate；测试强制真库） | `nothings-orm-mysql/README.md` |

ORM 设计：`docs/ai/spec/50-orm-design.md`；MySQL 驱动设计：`docs/ai/spec/51-mysql-driver-design.md`。派生宏生成的 API 不进 JSON 契约（生成器不展开过程宏），查签名看 crate README。

## 5. 代码约定（不要自创风格）

- **Trait 命名不加 `Trait` 后缀**：`Compressor` / `Symmetric` 是正例；`TimeTrait` / `AnySliceTrait` / `AnyMapTrait` 是历史遗留，别照抄。`AnySliceTrait` / `AnyMapTrait` 只有声明没有实现，**不要 `use`**。
- **链式 setter 返回 `&mut Self`**（`Aes::set_key` / `set_iv` 是正例）。既有 `Bcrypt::set_origin` / `Sha256Hash::set_origin` 返回 `&Self`，导致无法连续链式调用，不要复制这个缺陷。
- **构造函数不落错**：参数校验推迟到实际调用（如 `Aes::gcm` 接受任意长度 key，长度错在 `encrypt` 时返回 `ErrorKind::InvalidInput`）。
- **错误类型沿用 `std::io::Error`**，不新增公开错误类型；`InvalidInput` = 输入参数非法，`InvalidData` = 解密/解压失败。
- **过程宏改动在 `nothings-macros/`**，主 crate 通过 `pub use nothings_macros::Nothings;` 重新导出；改宏必须同时验 `validations` 的测试。`#[derive(NOrmEntity)]` 同理，验 `nothings-orm` 的测试。
- **ORM 的 SQL 注入防线**：值走 `?` 参数化；列名/表名必须在 `E::COLUMNS` 白名单内。不要在 ORM 代码里拼接用户输入到 SQL。
- **新模块必须配 `README.md`**，并在根 `README.md` 的模块目录表登记。

## 6. 修改公开 API 时的文档同步（硬性）

改任何 `pub` item 或 cfg 条件，都要同步：

1. 对应模块的 `README.md` + 根 `README.md`（模块表 / 快速开始 / 已知问题）。
2. `docs/ai/spec/10-rust-api.md`、`docs/ai/spec/30-known-issues.md` 中受影响的条目。
3. 跑 `python3 docs/ai/tools/gen_api_index.py` 重生成 `docs/ai/open-api/public-api.json`（禁止手改生成物）。
4. 在 `docs/ai/spec/00-overview.md` 的版本表登记变更。
5. 新增独立 crate 时额外更新：`llms.txt`、`docs/ai/README.md` 文件职责表、`spec/40` 同步矩阵加「新增 crate」行，并确认 `gen_api_index.py` 的 `SOURCE_ROOTS` 与 `MANIFESTS` 已包含新 crate。

## 7. 建议工作流

1. 读 `docs/ai/spec/00-overview.md` 确认依赖约束。
2. 查 `docs/ai/open-api/public-api.json`（签名、源码位置、状态标记）。
3. 读对应模块 `README.md` 拿语义与示例；**不要凭方法名猜语义**。
4. 改动后跑 `cargo test` 与 `cargo clippy --all-targets`，新增代码 0 告警。
5. 按第 6 节同步文档。

## 8. 文档索引

| 文件 | 用途 |
| --- | --- |
| `llms.txt` | AI 入口索引（链接清单） |
| `docs/ai/README.md` | 面向 AI 的文档导航与维护约定 |
| `docs/ai/spec/00-overview.md` | 项目规格：版本、依赖、模块树、全局约定、验收基线 |
| `docs/ai/spec/10-rust-api.md` | Rust 公开 API 契约与签名速查 |
| `docs/ai/spec/30-known-issues.md` | 失效 API 与安全边界（禁用清单） |
| `docs/ai/spec/40-development-workflow.md` | 新增模块 / 改 API / 发布的操作流程 |
| `docs/ai/spec/50-orm-design.md` | ORM 设计（GORM 式核心 + 独立驱动）、Phase 2 路线图 |
| `docs/ai/spec/51-mysql-driver-design.md` | MySQL 驱动设计（依赖、映射、已知边界、实现记录） |
| `docs/ai/open-api/public-api.json` | 机器可读的公开 API 清单（脚本生成，含 4 个 crate） |
| `docs/ai/open-api/README.md` | 契约字段说明与校验方法 |
| `docs/superpowers/specs/` | 已有的历史设计 spec（secrets/AES 设计，含实现记录） |
