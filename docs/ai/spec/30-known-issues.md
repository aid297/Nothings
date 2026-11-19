# 30 — 已知问题、禁用清单与安全边界

状态：2026-11-18 基线（新增 `MYSQL1`-`MYSQL6`），全部条目**未修复**。修掉任何一条时，必须同步更新本文件、根 `README.md` 的已知问题表、对应模块 README，以及 `open-api/public-api.json` 中的 `status` 标注（改 `tools/gen_api_index.py` 的 `ANNOTATIONS` 后重生成）。

编号 B19-B22、C2-C4、C11-C12 随 wasm 支持移除而作废，保留编号空洞避免其它文档引用失效。

**编号说明**：ORM1-ORM7 是本文的编号体系；`tools/gen_api_index.py` 的 `ANNOTATIONS` 里另有一套 ORM1-ORM5，两者语义已错位、互不对齐，本次不重排旧编号。MySQL 驱动新增条目统一使用独立的 `MYSQL*` 前缀，不占用任何 `ORM*` 编号。

## 1. A 类：功能失效（禁止依赖）

| # | 位置 | 现象 | 根因 | 替代方案 |
| --- | --- | --- | --- | --- |
| A1 | `AnySlice::remove_by_indexes` (`src/any_slices/app.rs:347`) | 完全不删除元素 | `let _ = indexes.iter().map(...)`，惰性迭代器未消费 | `to_vec()` + `enumerate().filter()` + `set_data()` |
| A2 | `AnyMap::filter` / `remove_by_keys` / `remove_by_values` / `remove_empty` | 全部不生效 | 依赖 A1 | `to_entries()` → `filter` → `AnyMap::with_iter()` |
| A3 | `AnySlice::shuffle` | 返回的副本仍是原顺序 | 打乱的是临时克隆 | `shuffle_self(&mut self)` |
| A4 | `AnyMap::every` | 返回 `&Self` 而不是 `bool` | 两条分支都返回 `&Self` | `to_entries().iter().all(...)` |
| A5 | `Dir::join` | 返回原目录 | `PathBuf::join` 返回值被 `let _ =` 丢弃 | 自行 `PathBuf::join` 后再 `Dir::new` |
| A6 | `impl_singleton!` | 外部 crate **整个宏编译失败**（`E0603: Singleton is private`） | 宏展开引用 `pub(crate) trait Singleton` | 手写 `OnceLock<Mutex<T>>`，见 `src/singletons/README.md` |
| A7 | `Lz4::compress_with_level` / `with_lz4_level` | `level` 不影响输出（1/8/16 逐字节相同） | `lz4_flex` 只有快速压缩模式 | 要压缩率用 Zstd / Zlib |

A6 的修复只需把 `src/singletons/app.rs:1` 的 `pub(crate)` 改成 `pub`，但改之前不要生成依赖该宏的代码。A1 的修复会同时让 A2 的四个方法恢复可用——修完要一起复测。

## 2. B 类：接口陷阱（能跑但语义容易误判）

| # | 位置 | 陷阱 |
| --- | --- | --- |
| B1 | `AnySlice::get_indexes_by_values` | 内部 `unwrap`，任一值不存在即 panic（`AnyMap` 的同名方法用 `filter_map`，无此问题） |
| B2 | `AnySlice::set_value` / `chunk(0)` | 越界 / `size == 0` 会 panic |
| B3 | `AnyMap::push_datum` | 不去重，重复 key 会出现多次 |
| B4 | `AnyMap::in_keys` / `in_values` / `not_in_*` | 语义是「**所有**元素都满足」，不是「任一」 |
| B5 | `AnyMap::get_value_by_key` / `AnySlice::first/last` | 返回克隆值，不是引用；大对象有拷贝开销 |
| B6 | `HttpResponse` | 只派生 `Serialize`，无 `Debug` / `Clone` / `PartialEq` / `Deserialize`；`errors` 字段被 `#[serde(skip)]` 且 `std::fmt::Error` 无信息；状态码无法自定义（`empty()` 私有）；`updated` 参数是 `Option<String>` 与其它构造器不一致 |
| B7 | `HttpResponse::deleted` | code 204 在 HTTP 标准里禁止响应体，但这里仍可 `.content(...)`；202 语义也非标准 Accepted |
| B8 | `File::mv` | 失败写 `err`，成功后**不清除**旧 `err`；调用前先 `f.err = None`。`Dir::mv` 无此问题 |
| B9 | `File::read` | 依赖构造时 `exist` 快照，文件后来才创建会误报「文件不存在」 |
| B10 | `File::write` / `Dir::zip` / `Dir::unzip` | 全量覆盖，没有追加模式 |
| B11 | `Dir::new` | 递归扫描整棵子树，大目录开销巨大且无惰性接口；浅层信息用 `std::fs::read_dir` |
| B12 | `Dir::up` | 对相对路径失效（`parent()` 为空串会原样返回自身并写 `err`）；先 `canonicalize` |
| B13 | `Dir::set_mode` | 非 Unix 总返回 `Err`；且不校验目标是否为目录，会把普通文件权限改掉 |
| B14 | `File` / `Dir` | 没有 `Debug` / `Clone`，字段全 `pub`，调试只能逐字段输出；`mv`/`create` 返回 `&Self` 会延长可变借用 |
| B15 | `Dir::zip` 产物 | **不是标准 ZIP**（自定义 TLV + 压缩，magic 为压缩算法头而非 `PK\x03\x04`）；只能用 `Dir::unzip` 配同一算法还原；空目录丢失；小数据会放大体积 |
| B16 | `texts::timer` | `prelude` / `volumer` 是私有 mod；调用 `Time::with_text` 必须同时导入 `TimeTrait` |
| B17 | `validations` 分隔符 / ex 注册表 | 是**进程级全局**配置，一处修改影响所有 `Check` 实例 |
| B18 | `singletons::with` | `.lock().unwrap()`，闭包 panic 导致锁中毒后所有调用永久 panic |
| B23 | `AnyMap` / `AnySlice` 的 `*Trait` | `AnySliceTrait` / `AnyMapTrait` 只有声明没有实现，不要 `use`。`AnyMapTrait` 的 intended signature 为 `every -> bool` / `clean(&mut self) -> &mut Self`，与实体 `AnyMap::every`（A4，返回 `&Self`）不一致属已知状态 |
| B24 | 集合运算 | `union` / `intersection` / `difference` 是 O(n·m) 线性查找 |
| ORM1 | `NOrmQueryBuilder` 的 where 系列 / `NOrmCondition` 叶子 / `order_by` | 列名不在 `E::COLUMNS` 白名单时，错误延后到 `all()` / `one()` / `count()` 才以 `InvalidInput` 抛出；构造查询时不报错。错误文本不含内部哨兵前缀 |
| ORM2 | `NOrmRepository::transaction` | 只支持单层，嵌套返回 `InvalidInput`；`Err` 路径自动 rollback 且忽略 rollback 自身错误；无 savepoint |
| ORM3 | `NOrmEntity` 名字冲突 | crate 根 `nothings_orm::NOrmEntity` 是派生宏，`nothings_orm::entity::NOrmEntity` 是 trait。调用 `from_row` / `pk_value` / `column_value` 需 `use nothings_orm::entity::NOrmEntity as EntityTrait` |
| ORM4 | `NOrmSqliteDriver::execute_batch` | 直接执行原始 SQL，不参数化；仅供 DDL/测试，禁止传用户输入 |
| ORM5 | 派生宏生成 API 不进 JSON | `public-api.json` 只扫源码文本，不展开过程宏；`NOrmSchema` / `NOrmEntity` 的生成方法需查 crate README |
| MYSQL1 | `nothings-orm-mysql` 保留字列名 | core 裸拼接标识符、不加反引号，`E::TABLE` / `E::COLUMNS` 不得使用 MySQL 保留字（`order` / `key` / `group` / `desc` / `rank` …），否则 1064；DDL 列名与 `#[entity(column = ...)]` 必须字面一致（宏按列名大小写敏感精确匹配） |
| MYSQL2 | `NOrmQueryBuilder::offset()` 单独调用 | 会生成 `SELECT ... OFFSET n`，MySQL 报 1064（SQLite 允许）。必须搭配 `limit()`；`one()` 自带 `limit(1)`，`get` / `find_one` 不受影响 |
| MYSQL3 | `NOrmMysqlDriver::query` 的列名/别名 | 必须原样保留 `COUNT(*) AS __count`，且 `COUNT(*)` 映射为 `NOrmValue::I64`；core 的 `count()` 精确匹配列名 `__count`，`to_i64()` 不接受其它变体 |
| MYSQL4 | MySQL 类型降级 | `NOrmValue` 只有六种变体：DATETIME / DATE / TIME / DECIMAL / JSON → `Text`（无类型保真）；`Bool` 写 TINYINT 0/1、读回 `I64`；`BIGINT UNSIGNED > i64::MAX` → `InvalidData`；Text/Blob 靠列元数据判定，不靠 UTF-8 试探 |

## 3. C 类：安全边界（写代码前必须评估）

| # | 风险 | 说明与要求 |
| --- | --- | --- |
| C1 | **zip-slip** | `Dir::unzip` 用 `output.join(relative_path)` 且不校验路径逃逸。构造 `../../etc/passwd` 可写到任意位置。**不要对不可信归档调用** |
| C5 | **GCM nonce 复用** | 同一密钥下复用 nonce 会泄露认证子密钥、允许伪造密文。每条消息用 `Aes::generate_nonce()`，nonce 随密文保存 |
| C6 | **CBC / CTR / ECB 无完整性校验** | 篡改不可检出；CBC 有 padding oracle 风险；ECB 泄露数据模式。无互通需求一律 GCM |
| C7 | **密钥不擦除** | `Aes` 的 key/iv 存在普通 `Vec<u8>`，`Drop` 不清零，`zeroize` 未启用。有内存取证要求需自行评估 |
| C8 | **口令直接 SHA256 当密钥** | SHA256 太快，不抗 GPU 暴力破解；面向用户口令要先经 bcrypt / Argon2 拉伸 |
| C9 | **MD5 用于安全场景** | 存在碰撞攻击，只用于非安全的数据完整性校验 |
| C10 | **bcrypt cost 热路径** | cost 每 +1 耗时翻倍；默认 12，高并发场景要限流 |
| ORM6 | **SQL 注入** | 所有值必须走 `?` 参数化；列名/表名不可参数化，必须在 `E::COLUMNS` 白名单内。`execute_batch` / `execute_ddl` 绝不接用户输入 |
| ORM7 | **SQLite 并发** | 一个 `NOrmSqliteDriver` 持一条连接，不是 `Sync`；并发场景每线程一个实例。连接池在 YAGNI 清单 |
| MYSQL5 | **`NOrmMysqlDriver::execute_ddl` 原始 SQL** | 不参数化、不校验语句条数。实测 `mysql 28.0.4` 的 `query_drop` 允许分号拼接多语句，绝不接用户输入，只用于固定字面量 DDL；DDL 会隐式提交，事务内只做 DML |
| MYSQL6 | **MySQL 连接与测试** | 一个 `NOrmMysqlDriver` 持一条连接，无连接池、非 `Sync`；`query` 全量物化，大结果集内存线性增长。集成测试强制 `NOTHINGS_MYSQL_DSN`，缺变量直接失败，账号需 `CREATE/DROP DATABASE` 权限 |
| MYSQL7 | **时区自动转换的边界** | 自动转换只对 `NOrmValue::NOrmDateTime` 生效，`NOrmValue::Text` / `I64` 一律不转（避免误伤 DECIMAL/JSON/普通字符串）。`connect_with_tz` 用 `OptsBuilder::init` 下发 `SET time_zone`，但会话设置对 DATETIME 内部无效——由驱动层墙钟换算补偿，外部直写 DATETIME 的数据不保证正确。**所有 TIMESTAMP/DATETIME 列读回一律 `NOrmValue::NOrmDateTime`，`String` 字段会读到 `InvalidData`，须改用 `NOrmDateTime` 字段**。固定偏移无 DST，不支持命名时区自动夏令时；SQLite 驱动不支持自动转换（按墙钟原样写 TEXT） |

## 4. 使用规则

1. AI 生成代码时，A 类方法视为**不存在**，不要用「看起来应该可以」的方式绕过。
2. B 类可以调用，但必须在代码/注释里体现已知语义（例如手动检查 `err`、导入 `TimeTrait`）。
3. C 类不是 bug，是设计边界；涉及加密、不可信输入时必须显式处理并在 PR/变更说明里写明。
4. 修缺陷时优先修根因（A1 一行修复能连带解决 A2），不要给每个失效方法各打一个补丁。
