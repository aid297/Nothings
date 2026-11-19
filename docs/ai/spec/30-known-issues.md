# 30 — 已知问题、禁用清单与安全边界

状态：2026-11-19 基线。A 类 A1-A7 **已全部修复**（2026-11-19），编号保留空洞避免其它文档引用失效；其余条目未修复。修掉任何一条时，必须同步更新本文件、根 `README.md` 的已知问题表、对应模块 README，以及 `open-api/public-api.json` 中的 `status` 标注（改 `tools/gen_api_index.py` 的 `ANNOTATIONS` 后重生成）。

编号 A1-A7、B19-B22、C2-C4、C11-C12 随 wasm 支持移除或修复而作废，保留编号空洞避免其它文档引用失效。

**编号说明**：ORM1-ORM7 是本文的编号体系；`tools/gen_api_index.py` 的 `ANNOTATIONS` 里另有一套 ORM1-ORM5，两者语义已错位、互不对齐，本次不重排旧编号。MySQL 驱动新增条目统一使用独立的 `MYSQL*` 前缀，不占用任何 `ORM*` 编号。

## 1. A 类：功能失效（2026-11-19 已全部修复）

| # | 原位置 | 原现象 | 修复方式 |
| --- | --- | --- | --- |
| A1 | `AnySlice::remove_by_indexes` | 惰性迭代器未消费，完全不删除 | 改为 `mem::take` + `into_iter().enumerate().filter()`，保签名、不加 `Clone` 约束 |
| A2 | `AnyMap::filter` / `remove_by_keys` / `remove_by_values` / `remove_empty` | 依赖 A1，全部不生效 | A1 修复后自动恢复；`remove_empty` 返回类型统一为 `&mut Self` |
| A3 | `AnySlice::shuffle` | 打乱临时克隆，返回原顺序 | 打乱待返回的 `Vec` 后再构造 `AnySlice` |
| A4 | `AnyMap::every` | 返回 `&Self`，不能作谓词 | 改为 `-> bool`，短路语义不变，与 `AnyMapTrait` 声明对齐 |
| A5 | `Dir::join` | `PathBuf::join` 返回值被丢弃 | `let mut path` + `path = path.join(segment)` |
| A6 | `impl_singleton!` | 外部 crate 编译失败 E0603 | `Singleton` trait 改 `pub`；新增 `tests/singletons.rs` 跨 crate 守护 |
| A7 | `Lz4::compress_with_level` / `with_lz4_level` | `level` 不影响输出 | 换 `lz4` crate（bundled liblz4 HC），`level 1-16` 生效，>16 钳 16；输出仍是标准 frame，`lz4_flex` 解码端不变 |

七个失效项均有「修复前失败、修复后通过」的回归测试（A6 为独立集成测试）。A7 新增 C 依赖 `lz4 = "1"`（`lz4-sys` 内置 liblz4 C 源码，通过 `cc` 编译，不依赖系统库），MSRV 1.89 实测通过。

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
| ORM8 | **JOIN 的 COUNT 一对多膨胀** | JOIN 下 `count()` 是 `COUNT(*)`，一对多会按行数计数而非主表实体数；需要去重请自己写裸 SQL |
| ORM9 | **JOIN 左列只做标识符校验** | 多 JOIN 链中 `join()` 的 `on` 左列无法携带实体类型信息，只校验安全标识符（不查白名单），写错列名会在执行时报 SQL 错误而非构造期 `InvalidInput`；右列仍走 `T::COLUMNS` 白名单 |
| ORM10 | **`all_with` 元组长度不匹配** | 元组元素个数须等于 JOIN 数 + 1，否则投影时返回 `InvalidInput`；`all_with` 在无 JOIN 时也报 `InvalidInput` |
| PL1 | **Preload 路径校验时机** | `preload(name)` 支持点分路径（`books.comments`），逐层批量查询；首段构造期按 `E::RELATIONS` 校验（拼错延后报 `InvalidInput` 且零 SQL），后续段在执行期由递归方法逐层校验（此时前序层查询已发生）；重叠路径自动保留最长，避免重复查询前缀层；空段（`books..author`）报 `InvalidInput` |
| PL2 | **Preload 外键与关联字段形态** | belongs_to 的 `foreign_key` 必须是本表真实标量 DB 列名（关联字段不进 `column_value`）；双向关联必须用 `Vec<T>` / `Option<T>` 打断尺寸递归，直接互含会编译失败（E0072）；has_many/belongs_to 必须与 target/foreign_key 同时出现，关联字段不得再标 primary_key/nullable/column |
| PL3 | **Preload 批量与 NULL 语义** | 批量 IN 每批最多 `NORM_PRELOAD_CHUNK_SIZE`（500）个键，自动分批；belongs_to 外键为 NULL 时关联字段保持 `None` 且不进 IN 列表；结果按目标主键去重（保留首次出现），跨父实体仍各自独立解码实例；手写 `NOrmEntity` 需自行实现 `NOrmPreload` 才能调用 `preload()` |
| PL4 | **递归 Preload 的实体状态** | `NOrmPreload::preload_path` 由 `#[derive(NOrmEntity)]` 生成以支持多层；手写 impl 用 provided 默认实现，仅支持单层（多层报 `InvalidInput`）。递归中每层由 `from_row` 重新解码为独立实例，关联字段为空初值、仅由更深路径填充，不形成对象图环；错误路径下实体可能半填充（关联字段已被 take 出），状态未定义 |
| PL5 | **懒加载 N+1** | `NOrmRepository::load_relation(&mut entity, name)` 单实体原地填充，支持点分路径；每次调用只查一个实体的关联，逐个调用产生 N+1，批量场景请用 `query().preload(..)` |
| SD1 | **软删除只影响读过滤** | `unscoped()` 仅跳过 SELECT/COUNT 的 `deleted_at IS NULL`；`delete()`/`hard_delete()`/`update()` 是 Repository 方法不受其影响。JOIN 主表过滤为 `t0.deleted_at IS NULL`，Preload 关联查询各自独立过滤（不继承 unscoped）。用户条件为 OR 时整体加括号保证过滤不被抢占 |
| SD2 | **软删除的写语义** | 启用 soft_delete 时 `delete(pk)` 改 `UPDATE ... SET deleted_at=now() WHERE pk=? AND deleted_at IS NULL`；`hard_delete(pk)` 才是物理删除。`update()` 对已删行返回 0 受影响行。`insert()` 强制写 deleted_at=NULL，不依赖 DDL 默认值 |
| TM1 | **自动时间 UTC 与时区** | `NOrmDateTime::now_utc()` 取 UTC 墙钟、微秒为 0，系统时间异常回退 epoch。MySQL 使用时间戳钩子时**必须** `connect_with_tz(orm_tz=UTC, ..)`，否则注入值被误当其它时区墙钟产生偏移；SQLite 原样写 TEXT 无此问题 |
| TM2 | **时间戳总覆盖、不写回** | `created_at`/`updated_at` 列由 repository 注入 now_utc，**覆盖实体自带值**（无法用它回填历史时间，需裸 SQL）；`&E` 签名不变、实体字段不写回，需重新查询见新值。`update()` 把 created_at 从 SET 排除（含该标志的实体 UPDATE 列数变化），`insert()` 时 created_at == updated_at（同一 now） |
| BT1 | **批量插入排除主键、非原子** | `insert_batch` 的 INSERT 排除主键列（依赖 DB 自增），实体自带主键被忽略；与 `insert` 共享托管列语义。单条语句参数最多 `BATCH_MAX_PARAMS`(900)，超出自动分块、每块独立 execute——**非原子**，要整体回滚请用 `transaction` 包裹。空切片返回 0 且不发 SQL |
| BT2 | **upsert 须提供真实主键** | `upsert_batch` 的 INSERT **写入主键列**（取 `pk_value()`）以便唯一键冲突检测，与纯插入排除主键相反；调用方必须为实体填真实主键，否则冲突判断失效。更新列排除主键/软删列/created_at，保留 updated_at 用本批 now。MySQL 受影响行数语义特殊（插入计 1、更新计 2），返回值只宜判「非零」不宜当作新增行数 |
| BT3 | **upsert 方言与驱动支持** | upsert SQL 尾句由 `NOrmDriver::upsert_sql` 生成：默认实现返回 `InvalidInput`（手写驱动零破坏）；内置 SQLite=`ON CONFLICT(pk) DO UPDATE SET c=excluded.c`、MySQL=`ON DUPLICATE KEY UPDATE c=VALUES(c)`。须在表上定义主键/唯一键，否则 MySQL 的冲突子句不生效（退化为纯插入）；尾句须以空格开头，列名来自 schema 白名单 |
| MYSQL7 | **时区自动转换的边界** | 自动转换只对 `NOrmValue::NOrmDateTime` 生效，`NOrmValue::Text` / `I64` 一律不转（避免误伤 DECIMAL/JSON/普通字符串）。`connect_with_tz` 用 `OptsBuilder::init` 下发 `SET time_zone`，但会话设置对 DATETIME 内部无效——由驱动层墙钟换算补偿，外部直写 DATETIME 的数据不保证正确。**所有 TIMESTAMP/DATETIME 列读回一律 `NOrmValue::NOrmDateTime`，`String` 字段会读到 `InvalidData`，须改用 `NOrmDateTime` 字段**。固定偏移无 DST，不支持命名时区自动夏令时；SQLite 驱动不支持自动转换（按墙钟原样写 TEXT） |

## 4. 使用规则

1. AI 生成代码时，A 类方法视为**不存在**，不要用「看起来应该可以」的方式绕过。
2. B 类可以调用，但必须在代码/注释里体现已知语义（例如手动检查 `err`、导入 `TimeTrait`）。
3. C 类不是 bug，是设计边界；涉及加密、不可信输入时必须显式处理并在 PR/变更说明里写明。
4. 修缺陷时优先修根因（A1 一行修复能连带解决 A2），不要给每个失效方法各打一个补丁。
