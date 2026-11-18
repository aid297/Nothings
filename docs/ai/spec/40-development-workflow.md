# 40 — 开发工作流与变更验收

给 AI 编码助手的操作清单。核心原则：**源码是唯一事实来源，文档同步是变更的一部分，不是收尾工作**。

## 0. 通则

- 动手前：读 `../../AGENTS.md` → `00-overview.md` → 相关模块 README → `30-known-issues.md`。
- 不要跑 `cargo fmt`（仓库整体非 rustfmt 格式）。
- 不要新增公开错误类型，沿用 `std::io::Error` / `Result<_, String>` / `err` 字段三套既有风格。
- 新增代码要求 `cargo clippy --all-targets` 0 告警；存量告警不处理。
- 每个变更都要能回答：改了哪些 public item？需要同步哪些文档？

## 1. 场景 A：新增模块

1. 建目录 `src/<module>/`，按 `mod.rs` + `app.rs` + `test.rs` + `README.md` 四件套布局。
2. 在 `src/lib.rs` 加 `pub mod <module>;`。
3. 在 `Cargo.toml` 的 `[dependencies]` 加依赖。
4. 模块 README 写清 API 一览、约束、已知问题；根 `README.md` 模块目录表登记一行。
5. 若模块提供「可当参数传递」的算法抽象，对齐 `compressions::Compressor` / `secrets::Symmetric`：父模块定义 trait + `with_xxx()` 工厂函数返回 `Box<dyn Trait>`，实现放子模块 `app.rs`。
6. 测试覆盖：round-trip、非法输入、边界值；公开方法尽量带可运行的 rustdoc 示例。
7. 文档同步：`00-overview.md` 的模块树/依赖表、`10-rust-api.md` 增小节、重生成 `public-api.json`。

## 2. 场景 B：修改公开 API

1. 先判断是否破坏性：改签名、改返回类型、改参数类型（如 `&usize` → `usize`）都是破坏性变更，当前版本线是 `0.0.x`，需升版本并在根 README 安装说明里同步。
2. 改代码 + 对应 `test.rs`；若 rustdoc 示例受影响，同步更新（doctest 会被 `cargo test` 执行）。
3. 同步四处：模块 README、根 README、`10-rust-api.md`（含易错签名速查表）、`30-known-issues.md`（若行为变化消除了某条陷阱）。
4. 重生成契约：`python3 docs/ai/tools/gen_api_index.py`。
5. 跑 `cargo test`、`cargo clippy --all-targets`、`cargo doc --no-deps`。

## 3. 场景 C：修复已知问题

1. 优先修根因（例：`AnySlice::remove_by_indexes` 一处修复可连带恢复 `AnyMap` 的四个方法），别给每个症状打补丁。
2. 加回归测试固化修复后的行为，别只改实现。
3. 更新 `tools/gen_api_index.py` 的 `ANNOTATIONS`：把 `status` 从 `broken` 改为 `ok` 或删掉标注。
4. 从 `30-known-issues.md`、根 README 已知问题表、对应模块 README 移除条目；`10-rust-api.md` 里的 `broken` 描述同步改。
5. 重生成 `public-api.json`，并在 `00-overview.md` 版本表登记。

## 5. 场景 E：新增独立 crate

以本次新增 `nothings-orm` / `nothings-orm-sqlite` 为例。文档层原来只覆盖「新增模块」，独立 crate 是新增场景。

1. 建 `<crate>/Cargo.toml`（edition 2024、license MIT、`crate-type = ["lib"]`）+ `src/lib.rs` + `README.md` + `tests/`。
2. 若依赖另一个本地 crate，用 `{ version = "x.y.z", path = "..." }` 双声明；发布时 Cargo 剥离 path 按 version 从 crates.io 解析。
3. **改生成器**：`docs/ai/tools/gen_api_index.py` 的 `SOURCE_ROOTS` 加新源根与模块前缀，`MANIFESTS` 加新 manifest，否则契约静默漏收且 `--check` 仍返回 0。
4. 同步 `AGENTS.md` §1/§4/§8、`llms.txt` 独立 crate 段、`docs/ai/README.md` 文件职责表、`spec/00` 标识/消费面/依赖/模块树/版本表、`spec/10` 新小节、`spec/30` 新边界条目（若适用）。
5. 较大设计变更先落 `spec/NN-xxx-design.md`（含空的「实现记录」节）作为设计基线。
6. 验收：三个 crate 各自 `cargo test --manifest-path ...`；`cargo tree | grep` 确认根 crate 未被新依赖污染；`gen_api_index.py --check` 返回 0。
7. 发布顺序按依赖拓扑：宏 crate → 核心 crate → 驱动 crate，逐个 `cargo publish --dry-run --manifest-path ...`。

## 6. 场景 F：发布前检查

| # | 检查项 |
| --- | --- |
| 1 | 各 `Cargo.toml` 版本号、根 `README.md` 安装示例、`00-overview.md` 版本表与变更登记一致（`public-api.json` 的 `crates[]` 由脚本读 manifest 自动生成） |
| 2 | `cargo test`（含 doctest） |
| 3 | `cargo clippy --all-targets`（新增代码 0 告警）、`cargo doc --no-deps` |
| 4 | `python3 docs/ai/tools/gen_api_index.py --check` 返回 0 |
| 5 | `cargo publish --dry-run`；按依赖拓扑顺序：`nothings-macros` → `nothings-orm` → `nothings-orm-sqlite` → 根 `nothings`，path + version 双依赖要求前置 crate 已上传 |
| 6 | 新增 crate 时：每个 crate 各自 `cargo test --manifest-path ...`，且 `cargo tree` 确认根 crate 未被 DB 依赖污染 |

## 7. 文档同步矩阵

| 变更类型 | 根 README | 模块 README | spec/00 | spec/10 | spec/30 | public-api.json |
| --- | --- | --- | --- | --- | --- | --- |
| 新增模块 | ✅ | ✅ 新建 | ✅ | ✅ | — | ✅ 重生成 |
| 改 Rust 公开签名 | ✅ | ✅ | — | ✅ | 视情况 | ✅ 重生成 |
| 改依赖 | ✅ | 视情况 | ✅ | ✅ | — | ✅ 重生成 |
| 修已知缺陷 | ✅ | ✅ | ✅ 版本表 | ✅ | ✅ | ✅ 重生成 |
| 版本升级 | ✅ | — | ✅ | — | — | ✅ 重生成 |
| 新增独立 crate | ✅ | ✅ 新建 | ✅ 标识/消费面/依赖/模块树/版本表 | ✅ 新小节 | 视情况 | ✅ 改生成器后重生成 |
| 新增 Cargo 依赖 | ✅ | 视情况 | ✅ | ✅ | — | ✅ 重生成 |

## 8. 决策记录模板

较大的设计变更沿用 `docs/superpowers/specs/` 已有 spec 的结构，新文件放 `docs/ai/spec/` 并加编号：

```markdown
# <主题> 设计
- 日期 / 状态 / 范围
## 背景与目标
## 模块结构与依赖
## 接口（签名 + 参数规格 + 错误处理）
## 明确不做（YAGNI）
## 测试计划
## 验收标准
## 实现记录（设计与落地的差异，落地后才暴露的事实）
```

「实现记录」这一节是硬要求：设计与代码出现偏差时必须写清原因（参考 `2026-09-11-secrets-symmetric-aes-design.md` 里 setter 返回 `&mut Self`、MSRV 抬升到 1.89 等记录）。
