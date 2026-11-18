# 10 — Rust 公开 API 规格

本文记录**签名无法表达的语义契约**和最易写错的签名。全量公开 item（含精确签名、源码文件与行号、状态标记）在 `../open-api/public-api.json`，模块级完整示例在 `src/**/README.md`。三者冲突时以源码为准。

## 1. 通用契约

| 主题 | 约定 |
| --- | --- |
| 错误出口 | `Result<_, std::io::Error>`（压缩、加密）、`Result<_, String>`（文件系统）、`err: Option<String>` 字段（`mv` / `create` / `up` / `join`）。第三种**不会返回错误**，调用后必须检查字段 |
| 所有权 | `AnySlice::get_data(self)` 消耗自身；`File::remove(self)` / `Dir::remove(self)` / `Dir::up(self)` / `Dir::join(self, ..)` 消耗自身；`HttpResponse::content(self, ..)` / `error(self, ..)` 消耗并返回自身 |
| 返回值是值不是引用 | `AnySlice::first/last/get_value_by_index` 返回克隆的 `Option<T>`；`AnyMap::get_value_by_key` 返回克隆的 `Result<V, Error>`；`to_vec()` 返回内部 `Vec<T>` 的引用 |
| trait bound | 集合运算、拷贝、字符串化普遍要求 `Clone` / `PartialEq` / `Display`；具体见 JSON 的 `signature` 与各模块 README 的约束列 |
| 未实现的 trait | `AnySliceTrait` / `AnyMapTrait` 只有声明没有 impl，**不要 `use`**。`AnyMapTrait` 的 intended signature 是 `every -> bool` / `clean(&mut self) -> &mut Self`；实体 `AnyMap::every` 因 A4 仍返回 `&Self`，二者不一致属已知状态 |
| 状态标记 | JSON 的 `status`：`ok` 可用、`broken` 实现失效（禁止依赖）、`limited` 有限制、`unimplemented` trait 只有声明未实现 |

## 2. 模块契约

### `any_slices::app::AnySlice<T>`（45 个方法）

增强版 `Vec<T>`，链式方法返回 `&mut Self`。

- `new(Vec<T>) -> Self`；`to_vec(&self) -> &Vec<T>`；`set_data(&mut self, Vec<T>) -> &mut Self`；`copy(&self) -> Self`。
- `remove_by_index(&mut self, index: &usize)` 参数是**引用**，越界安全；`remove_by_indexes(&Vec<usize>)` 是空操作（`broken`）。
- `shuffle(&self) -> AnySlice<T>` 返回未打乱的副本（`broken`）；`shuffle_self(&mut self)` 才真正打乱。
- `get_indexes_by_values` 内部 `unwrap`，任一值不存在即 panic；`get_values` / `get_values_by_slicer` 会跳过越界索引。
- `chunk(size)` 的 `size == 0` 会 panic；`set_value` 越界会 panic。
- 集合运算 `union` / `intersection` / `difference`（及 `_slicer` 变体）为 O(n·m) 线性查找，不适合大数据量。

### `any_maps::app::AnyMap<K, V>`（34 个方法）

两个平行 `AnySlice` 实现，保留插入顺序；查找 O(n)，**不是 HashMap 的替代品**。

- 构造器是 `new()` / `with_iter(I: IntoIterator<Item=(K,V)>)` / `with_hashmap(HashMap<K,V>)`（顺序不确定）。`push_datum` **不去重**，重复 key 会出现多次。
- `set_value_by_key(K, V) -> Result<&mut Self, Error>`，key 不存在返回 `Err(NotFound)`；`get_value_by_key(&K) -> Result<V, Error>` 返回克隆值。
- `in_keys` / `in_values` / `not_in_keys` / `not_in_values` 是「**所有**元素都满足」语义（底层 `all()`），不是「任一」。
- `filter` / `remove_by_keys` / `remove_by_values` / `remove_empty` 均因 `AnySlice::remove_by_indexes` 失效而无效（`broken`）；`remove_by_key(&K)` / `remove_by_index(usize)` 可用。
- `every(..) -> &Self` 不是 `bool`，不能作谓词（`broken`）。替：`to_entries().iter().all(..)`。
- `Default` 只为 `AnyMap<(), ()>` 实现，其它类型用 `new()`。

### `compressions`

```rust
pub trait Compressor {
    fn compress(&self, data: &[u8]) -> Result<Vec<u8>, std::io::Error>;
    fn decompress(&self, data: &[u8]) -> Result<Vec<u8>, std::io::Error>;
}
```

- 工厂函数返回 `Box<dyn Compressor>`：`with_lz4()` / `with_lz4_level(u32)` / `with_zlib()` / `with_zlib_level(u32)` / `with_zstd()` / `with_zstd_level(i32)`。**只有 `with_zstd_level` 是 `i32`**。
- 三个算法类型 `compressions::{lz4,zlib,zstd4}::app::{Lz4,Zlib,Zstd}` 接口一致，全部是关联函数：`new()` / `with_level(level)` / `compress(&[u8])` / `compress_with_level(&[u8], level)` / `decompress(&[u8])` / `compress_file(&str,&str) -> Result<(), Error>` / `compress_file_with_level(..)` / `decompress_file(&str,&str) -> Result<(), Error>` / `compression_ratio(&[u8],&[u8]) -> f64`。
- 级别：Lz4 1-16（**参数不影响输出**，实测 1/8/16 逐字节相同）；Zlib 0-9（默认 6，0 = 仅存储）；Zstd 1-21（默认 3）。
- `Lz4` 输出标准 LZ4 frame；解压必须用同一算法，magic 校验失败返回 `Err`。

### `digests`

| 类型 | 关键签名 |
| --- | --- |
| `bcrypt::Bcrypt` | `new(&'a str)`、`set_origin(&mut self, &'a str) -> &Self`、`set_cost(&mut self, u32) -> &Self`、`hash(&self) -> Result<String, BcryptError>`、`check(&self, &String) -> Result<bool, BcryptError>` |
| `md5::Md5Hash` / `sha::Sha256Hash` | `new(&'a str)`、`set_origin(&mut self, &'a str) -> &Self`、`hash(&self) -> String`（32 / 64 位小写 hex） |

- Bcrypt cost 4-31，默认 12，加盐所以每次结果不同；`check` 的入参是 `&String` 不是 `&str`。
- 摘要 API 基于 `&str`，**不接受任意二进制**；MD5 不用于安全场景。

### `secrets`

```rust
pub trait Symmetric {
    fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>, std::io::Error>;
    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, std::io::Error>;
}
```

- 工厂：`with_aes_gcm(key, nonce)` / `with_aes_cbc(key, iv)` / `with_aes_ctr(key, iv)` / `with_aes_ecb(key)`，均返回 `Box<dyn Symmetric>`。
- `secrets::symmetric::aes::app::Aes`：`gcm` / `cbc` / `ctr` / `ecb` 构造（不落错）、`mode() -> AesMode`、`set_key(&[u8]) -> &mut Self`、`set_iv(&[u8]) -> &mut Self`、`generate_key(bits: usize) -> Result<Vec<u8>, Error>`（128/192/256）、`generate_nonce() -> Vec<u8>`（12 字节）、`generate_iv() -> Vec<u8>`（16 字节）；`AesMode::iv_len()`（ECB 为 0）。
- 常量：`BLOCK_LEN = 16`、`GCM_NONCE_LEN = 12`、`GCM_TAG_LEN = 16`。
- 参数规格：GCM key 16/24/32 + 12 字节 nonce，输出「密文 ‖ 16 字节 tag」；CBC/CTR 16 字节 iv，CBC 用 PKCS7 向上块对齐，CTR 与明文等长（`Ctr128BE`，与 OpenSSL/Java 默认一致）；ECB 不用 iv + PKCS7。
- 错误：`InvalidInput`（key 非 16/24/32、iv/nonce 长度不符、`generate_key` 非法 bits、GCM 明文超长）、`InvalidData`（tag 校验失败、PKCS7 非法）。
- 安全契约：只有 GCM 校验完整性；GCM 同一密钥下 nonce 绝不能重复；CBC/CTR/ECB 不校验篡改；key/iv 存在普通 `Vec<u8>`，`Drop` 不清零。

### `filesystem`

- `file::File` 与 `dir::Dir` 字段全部 `pub`，没有 `Debug` / `Clone`；路径参数统一 `&str`，不接受 `Path` / `PathBuf`。
- `File::write` 是**覆盖**不是追加；`read()` 依赖构造时的 `exist` 快照（文件后来才创建会误报不存在）。
- `File::mv` 返回 `&Self`、失败写 `err`，成功后**不清除旧 `err`**；调用前先 `f.err = None`。`Dir::mv` 无此问题。
- `Dir::new` 递归扫描整棵子树（无惰性接口）；`Dir::up` 对相对路径失效；`Dir::join` 是空操作；`Dir::set_mode` 在非 Unix 总返回 `Err`。
- `zip` / `unzip` 接收 `&dyn Compressor`，产物是**自定义 TLV 归档 + 压缩**，不是 ZIP；不保存空目录；`unzip` 不校验路径逃逸，**不要用于不可信归档**。

### `http_responses::HttpResponse<T>`

```rust
#[derive(Serialize)]
pub struct HttpResponse<T> {
    pub code: u32, pub msg: String, pub content: Option<T>,
    #[serde(skip)] pub errors: Vec<std::fmt::Error>,
}
```

- 7 个构造器：`ok(Option<&str>)` 200 / `created(Option<&str>)` 201 / `updated(Option<String>)` 202 / `deleted(Option<&str>)` 204 / `bad_request(&str)` 400 / `un_authorization(Option<&str>)` 401 / `internal_server_error(&str)` 500。**`updated` 的参数是 `Option<String>`**，其余可选参数是 `Option<&str>`；`bad_request` 与 `internal_server_error` 必填。
- `content(T) -> Self`、`error(std::fmt::Error) -> Self` 链式。`errors` 被 `#[serde(skip)]`，`std::fmt::Error` 不携带信息，无实际作用。
- 只派生 `Serialize`：没有 `Debug` / `Clone` / `PartialEq` / `Deserialize`，不能 `{:?}`、`assert_eq!`、反序列化。字段可 `pub` 直接改，用于绕过无法自定义状态码的限制（`empty()` 私有）。
- 序列化形状固定 `{"code":..,"msg":..,"content":..}`，`content: None` 输出 `null` 不省略。

### `texts::timer`

- 只有 `nothings::texts::timer` 公开；`texts::prelude`、`texts::volumer` 是私有 mod。
- 常量 `SECOND` / `MINUTE` / `HOUR` / `DAY` / `WEEK` 为 `std::time::Duration`。
- `Time` 实现 `TimeTrait`：`new() -> Time`、`with_text(&str) -> Result<Time, String>`、`get_duration() -> Duration`、`to_chinese() -> Result<String, String>`。**调用关联函数必须同时 `use TimeTrait`**。
- 单位 `s/m/h/d/w` 可组合；纯数字按秒；末尾无单位数字按「前一个单位的下一级」处理（w→d→h→m→s）；未知单位或单位在前返回 `Err`。

### `validations`

- `#[derive(Nothings)]` 由 `nothings-macros` 提供，主 crate 通过 `validations::Nothings` 重新导出；字段属性 `#[validator[...]]`，键为 `rule`（必填）/ `name`（必填）/ `kind` / `nested`。
- 规则串 `(rule)(rule)...`；操作符 `> < >= <= == !=`；内置 `required`（等价 `!`）、`min` / `max`、`size`、`in` / `in!=`、`ex:fn-a,fn-b`。按顺序执行，第一个失败即返回 `Option<ValidationError>`。
- `check::Check::new(data)` + `checker::Checker::check() -> Option<ValidationError>`；`ValidationError { field, message }`。
- `validation::Validation` 静态方法：`set_slice_split_char` / `slice_split_char`、`set_error_split_char` / `error_split_char`、`register_ex_check_fn(key, F)` / `has_ex_check_fn` / `call_ex_check_fn`。分隔符是**进程级全局**配置，默认 `,`（`SliceSplitChar::Comma`）、`\n`（`ErrorSplitChar::Console`）。
- 自定义校验函数签名 `Fn(&str, &str, &str) -> Option<ValidationError>`（field_name / value 字符串 / kind）。
- `Option<T>` 字段：有 `required` 且为 `None` → 报错；无 `required` 且为 `None` → 跳过全部校验。`nested` 递归解析子字段并加前缀（`name` 优先，否则用 Rust 字段名）。

### `singletons`

- `impl_singleton!` 用 `#[macro_export]` 定义在 `src/singletons/app.rs`，导出位置是 **crate 根** `nothings::impl_singleton!`。
- 生成 `instance() -> &'static Mutex<T>`、`default() -> Self`（新建实例，不是单例）、`with(FnOnce(&mut T) -> R) -> R`。要求类型有 `fn default_new() -> Self`、每个类型只能调用一次。
- **外部 crate 不可用**：宏展开引用 `pub(crate) trait Singleton`，报 `E0603` 导致整个宏失效。替代写法见 `src/singletons/README.md`。`with()` 内部 `.lock().unwrap()`，锁中毒后所有调用 panic。

### `coroutines`

空占位（`mod.rs` 只有 `pub mod app;`，`app.rs` 为 0 字节），不要基于它生成代码。

### 独立 crate：`nothings-orm` / `nothings-orm-sqlite` / `nothings-orm-mysql`

不是 `nothings` 的模块，而是三个独立 crate。完整设计见 `spec/50-orm-design.md` 与 `spec/51-mysql-driver-design.md`，用法见各自 README。

| 类型 | 关键签名 | 契约 |
| --- | --- | --- |
| `Value` | `Null / Bool / I64 / F64 / Text / Blob` | 运行时列值；`ToValue` / `FromValue` 双向转换；`Option<T>` 的 Null → None |
| `SqlType` | `Integer / Real / Text / Blob` | `HasSqlType::SQL_TYPE` 静态标签 |
| `Row` / `OwnedRow` | `column_count` / `column_name` / `value` | `Driver::query` 必须即时物化为 owned 行 |
| `Schema` | `const TABLE` / `const COLUMNS` / `fn pk()` | 由 `#[derive(Entity)]` 生成 |
| `Entity` | `from_row<R: Row>` / `pk_value` / `column_value` | 全部由宏生成，调用方需 `use nothings_orm::entity::Entity as EntityTrait` 才能调 trait 方法 |
| `Driver` | `execute` / `query` / `last_insert_id` / `begin` / `commit` / `rollback` | 后端抽象；所有 SQL 必须参数化；驱动 crate 需显式 `use nothings_orm::driver::Driver` 才能 impl |
| `Repository` | `insert` / `get` / `find_one` / `find_all` / `update` / `delete` / `query` / `transaction` | 泛型 `D: Driver`；`get` 未找到返 `NotFound`，`find_one` 返 `Ok(None)` |
| `QueryBuilder` | `r#where` / `order_by` / `limit` / `offset` / `all` / `one` / `count` | 链式 `&mut Self`；列名必须在 schema 白名单内；MySQL 下 `offset()` 必须搭配 `limit()` |
| `SqliteDriver` | `open(&str)` / `in_memory()` / `execute_batch(&str)` | rusqlite bundled；`execute_batch` 仅供 DDL/测试 |
| `MysqlDriver` | `connect(&str)` / `connect_with_tz(&str, UtcOffset, UtcOffset)` / `execute_ddl(&str)` | 同步 `mysql` crate（minimal-rust）；`execute_ddl` 单条原始 DDL，不参数化；`impl Driver` 的 6 个方法在 JSON 中记为 `inherited (trait method)` |
| `UtcOffset` / `DateTime` | `UtcOffset::parse` / `DateTime::new` / `parse` / `from_unix_secs` | `nothings-orm::value` 的零依赖时间类型；`Value::DateTime` 是时区转换的类型标记，`Text`/`I64` 不转 |

派生宏生成的方法（`Schema` / `Entity` 的 impl）**不进 `public-api.json`**——生成器只扫源码文本，不展开过程宏。查这些 API 看 `nothings-orm/README.md`。

## 3. 易错签名速查

| 项 | 实际形态 |
| --- | --- |
| `AnySlice::remove_by_index` | `index: &usize`（引用）；`AnyMap::remove_by_index` 是 `usize`（值） |
| `AnySlice::has` / `union` | 参数是 `&Vec<T>`，另有 `*_slicer` 版本接 `&AnySlice<T>` |
| `HttpResponse::updated` | `Option<String>`，其余可选 msg 是 `Option<&str>` |
| `with_zstd_level` | `i32`；`with_lz4_level` / `with_zlib_level` 是 `u32` |
| `Bcrypt::check` | `&String`；`set_origin` / `set_cost` 返回 `&Self`（无法连续链式） |
| `Aes::set_key` / `set_iv` | 返回 `&mut Self`（可连续链式） |
| `File::zip` / `Dir::zip` | 都返回 `Result<File, String>`（新 `File` 对象），不是 `()` |
| 压缩 `compress_file*` | 返回 `Result<(), Error>`，不是压缩后的字节 |
| `AnyMap::every` | `-> &Self`，不是 `bool` |
| `Entity::from_row` | trait 方法，需导入 `nothings_orm::entity::Entity as EntityTrait`；crate 根的同名 `Entity` 是派生宏 |
| `QueryBuilder::r#where` | raw identifier，列名必须在 `E::COLUMNS` 白名单内 |
| `SqliteDriver::execute_batch` | 原始 SQL，不参数化，禁止传用户输入 |
| `MysqlDriver::execute_ddl` | 单条原始 DDL，不参数化；实测客户端允许多语句，禁止传用户输入 |
| `MysqlDriver` 的 `Driver` 方法 | trait 方法在 `public-api.json` 中记为 `inherited (trait method)`；驱动 crate 需显式 `use nothings_orm::driver::Driver` |
| `nothings-orm-mysql` 的 `query` | 原样保留 `AS __count` 别名与列名大小写；`QueryBuilder::offset()` 必须搭配 `limit()` |
