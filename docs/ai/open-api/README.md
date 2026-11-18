# open-api — 机器可读的公开 API 契约

`nothings` 是库 crate，**没有 HTTP 端点**，因此这里不是 OpenAPI/Swagger 规范，而是 Rust 公开 API 契约。目标是让 AI 助手能直接查签名和失效标记，不用逐个读源码。

| 文件 | 消费面 | 维护方式 |
| --- | --- | --- |
| `public-api.json` | 4 个 crate 的公开 API | **生成物**，由 `../tools/gen_api_index.py` 从四个源根提取，禁止手改 |

## public-api.json 结构

| 顶层字段 | 含义 |
| --- | --- |
| `spec` / `generated_by` / `generator_version` | 契约格式版本与生成器 |
| `source_roots` | 四个扫描根：`src` / `nothings-orm/src` / `nothings-orm-sqlite/src` / `nothings-orm-mysql/src` |
| `crates` | 四个 crate 的 name / manifest 路径 / version，由各 `Cargo.toml` 读取 |
| `crate` | 兼容旧消费者：主 crate 的 manifest 摘要 |
| `generator_version` | 3（新增 MySQL 驱动源根） |
| `platforms` | 编译目标，当前仅 `native` |
| `status_legend` | `status` 取值说明 |
| `issue_index` | 已知问题清单路径 |
| `modules` | 所有 `mod` 声明：`path` / `visibility` / `cfg` / `test_only` / `declaration` / `declared_in` / `file` |
| `items` | 公开 item 清单（见下表） |
| `exports` | C-ABI 导出清单；wasm 支持移除后为空数组 |
| `counts` | 汇总计数，用于快速校验是否漂移 |

`items[]` 字段：

| 字段 | 含义 |
| --- | --- |
| `path` | 完整路径，格式为 `<crate>::<模块>::<Owner>::<方法>`；crate 前缀为 `nothings` / `nothings_orm` / `nothings_orm_sqlite` / `nothings_orm_mysql` |
| `module` | 所在模块路径 |
| `kind` | `fn` / `struct` / `enum` / `trait` / `type` / `const` / `field` / `use` / `macro` |
| `name` / `owner` / `visibility` | 名称、所属类型、可见性（`pub` 或 `inherited (trait method)`） |
| `signature` | 规范化后的 Rust 签名（含 `where` 子句） |
| `cfg` / `platform` | cfg 条件；`platform` 当前恒为 `all` |
| `status` / `note` | 行为状态与说明 |
| `file` / `line` | 源码位置，1-based 行号 |
| `attributes` / `extern` / `export` | 附加属性与 ABI 标记；当前仓库无导出函数 |

`status` 取值：

| 值 | 含义 |
| --- | --- |
| `ok` | 可用，无特别限制 |
| `broken` | **实现失效，禁止依赖**（如 `AnySlice::remove_by_indexes`） |
| `limited` | 可用，但有已知陷阱或安全边界，读 `note` 与 `spec/30-known-issues.md` |
| `unimplemented` | trait 只有声明没有实现（`AnySliceTrait` / `AnyMapTrait`），禁止 `use` |


## 常用查询（jq）

```bash
# 查某个方法的签名与状态
jq '.items[] | select(.path | contains("AnyMap::filter"))' docs/ai/open-api/public-api.json

# 所有禁止依赖的 item
jq -r '.items[] | select(.status=="broken" or .status=="unimplemented") | "\(.status)\t\(.path)"' docs/ai/open-api/public-api.json

# 只看 ORM crate 的 item
jq -r '.items[] | select(.path | startswith("nothings_orm")) | "\(.path)\t\(.signature)"' docs/ai/open-api/public-api.json

# 按 crate 统计 item 数
jq -r '.items | group_by(.path | split("::")[0])[] | "\(.[0].path | split("::")[0])\t\(length)"' docs/ai/open-api/public-api.json
```

## 生成与校验

```bash
python3 docs/ai/tools/gen_api_index.py            # 重生成 public-api.json
python3 docs/ai/tools/gen_api_index.py --check    # 与源码不一致时返回非 0
cargo test                                       # 根 crate：299 单测 + 21 doctest（1 ignored）
cargo test --manifest-path nothings-orm/Cargo.toml
cargo test --manifest-path nothings-orm-sqlite/Cargo.toml
cargo test --manifest-path nothings-orm-mysql/Cargo.toml   # 需 NOTHINGS_MYSQL_DSN
```

生成器只依赖 Python 标准库，不写时间戳，因此重复生成结果稳定，`--check` 可直接放进提交前检查。

**已知盲区**：生成器只扫源码文本，不展开过程宏。`#[derive(Entity)]` 生成的 `Schema` / `Entity` impl 不会进 JSON；`#[derive(Nothings)]` 同理。跨 crate 的 `pub use` 只记一条 `use` item，不展开目标类型。这些 API 要在 crate README 里显式列出。

新增独立 crate 时必须先把源根加进生成器的 `SOURCE_ROOTS` 与 `MANIFESTS`，否则 JSON 静默漏收且 `--check` 仍返回 0。改公开 API 后的同步流程见 `../spec/40-development-workflow.md`。
