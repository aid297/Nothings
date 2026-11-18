# docs/ai — 面向 AI 的文档导航

这一目录是 `nothings` 的 **AI 开发文档层**。目标：让 AI 编码助手不用逐个读源码就能拿到准确的签名和失效标记。已有的人类文档（`README.md`、`src/**/README.md`、`docs/superpowers/specs/`）**保持原样，不迁移、不重写**。

## 读的顺序

| 场景 | 先读 | 再读 |
| --- | --- | --- |
| 任何代码改动 | `../../AGENTS.md` | `spec/00-overview.md` |
| 调用某个 Rust API | `open-api/public-api.json` | 对应 `src/**/README.md` + `spec/30-known-issues.md` |
| 新增模块 / 改公开 API | `spec/40-development-workflow.md` | `spec/10-rust-api.md` |
| 判断某个方法能不能用 | `spec/30-known-issues.md` | 源码 `src/**/app.rs` |
| 用 ORM 做数据库操作 | `spec/50-orm-design.md` | `nothings-orm/README.md` + 对应驱动 README（`nothings-orm-sqlite/README.md` / `nothings-orm-mysql/README.md`） |
| 用 MySQL 驱动 | `spec/51-mysql-driver-design.md` | `nothings-orm-mysql/README.md` |

仓库根的 `llms.txt` 是链接入口，供支持 llmstxt 的工具直接抓取。

## 文件职责

| 文件 | 性质 | 谁维护 |
| --- | --- | --- |
| `spec/00-overview.md` | 手写规格：元信息、依赖、模块树、全局约定 | 人工 |
| `spec/10-rust-api.md` | 手写规格：Rust 公开 API 的语义契约与易错签名 | 人工 |
| `spec/30-known-issues.md` | 手写规格：禁用清单与安全边界 | 人工 |
| `spec/40-development-workflow.md` | 手写规格：变更流程与验收标准 | 人工 |
| `spec/50-orm-design.md` | 手写规格：ORM 设计与 Phase 2 路线图 | 人工 |
| `spec/51-mysql-driver-design.md` | 手写规格：MySQL 驱动设计、映射表与实现记录 | 人工 |
| `../../nothings-orm/README.md` | ORM 核心 crate 用法与属性语法 | 人工 |
| `../../nothings-orm-sqlite/README.md` | SQLite 驱动用法与错误映射 | 人工 |
| `../../nothings-orm-mysql/README.md` | MySQL 驱动用法、错误映射与已知边界 | 人工 |
| `open-api/public-api.json` | **生成物**：4 个 crate 的公开 API 清单 | `tools/gen_api_index.py` |
| `open-api/README.md` | 字段定义与校验方法 | 人工 |
| `tools/gen_api_index.py` | 生成脚本，从 4 个源根提取 public item | 人工改脚本 |

## 事实来源优先级

1. **源码**（`src/**/*.rs`、`nothings-orm/src/**`、`nothings-orm-sqlite/src/**`、`nothings-orm-mysql/src/**`、各 `Cargo.toml`）是唯一事实来源。
2. `open-api/public-api.json` 由脚本从源码提取，带文件路径与行号，可用来快速定位；**不要手工编辑**，改完源码重跑脚本。
3. `spec/*.md` 与 `src/**/README.md` 是人工解释层，负责语义、陷阱、替代方案。
4. 三者冲突时，以源码为准，并立即修正文档。

## 维护约定

- 改任何 `pub` item 或 cfg 条件：按 `../../AGENTS.md` 第 6 节同步文档，并重新生成 JSON。
- 修掉一个已知缺陷：从 `spec/30-known-issues.md` 移除条目，同步根 `README.md` 的已知问题表、对应模块 README，并更新脚本里的状态标注。
- 版本号变更：`spec/00-overview.md` 的版本表、`public-api.json` 的 `crates[]`（脚本读各 `Cargo.toml`，自动生效）必须一起更新。
- 新增独立 crate：把新源根加进 `tools/gen_api_index.py` 的 `SOURCE_ROOTS`，把 manifest 加进 `MANIFESTS`，否则契约会静默漏收。
- 派生宏生成的方法不进 JSON（生成器只扫源码文本，不展开宏）。`#[derive(Entity)]` 生成的 `Schema` / `Entity` 方法要在 crate README 里显式列出。
- 不要在这里复制教程式示例。示例留在各模块 README，避免两份文档漂移。

## 漂移自检

```bash
python3 docs/ai/tools/gen_api_index.py --check   # JSON 与源码不一致时返回非 0
cargo test                                       # 根 crate：299 单测 + 21 doctest（1 ignored）
cargo test --manifest-path nothings-orm/Cargo.toml
cargo test --manifest-path nothings-orm-sqlite/Cargo.toml
cargo test --manifest-path nothings-orm-mysql/Cargo.toml   # 需 NOTHINGS_MYSQL_DSN，强制真库
```

`gen_api_index.py` 只依赖 Python 标准库。
