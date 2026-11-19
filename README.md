# nothings

一个实用的 Rust 工具库。核心是 `AnySlice<T>`（增强版 `Vec<T>`）与 `AnyMap<K, V>`（有序键值对映射），另外提供 LZ4 / zlib / Zstandard 压缩、AES 对称加密、Bcrypt / MD5 / SHA256 摘要、文件与目录操作、统一 HTTP 响应结构、时间字符串解析、声明式结构体校验、单例宏。另有独立的 ORM crate：`nothings-orm`（核心）、`nothings-orm-sqlite`（SQLite 驱动）与 `nothings-orm-mysql`（MySQL 驱动）。

## 安装

```toml
[dependencies]
nothings = "0.0.9"
```

- MSRV：edition 2024（需 Rust 1.89+，由 `aes` / `aes-gcm` 依赖决定）
- 平台：所有模块无条件参与编译，无平台差异（zstd 为 C 绑定，需原生工具链）

## 模块目录

每个二级模块都有独立文档，下表链接到各自的 README。

| 模块 | 导入路径 | 说明 | 文档 |
|------|----------|------|------|
| **AnySlice** | `nothings::any_slices::app` | 增强版 `Vec<T>`，45 个方法：链式增删改、集合运算、`pluck`、分块等 | [README](src/any_slices/README.md) |
| **AnyMap** | `nothings::any_maps::app` | 有序键值对映射，基于平行 `AnySlice` 实现，34 个方法 | [README](src/any_maps/README.md) |
| **Compressions** | `nothings::compressions` | 压缩模块总览：`Compressor` trait 与 `with_lz4()` / `with_zlib()` / `with_zstd()` 等 6 个工厂函数，用于把压缩算法作为参数传递 | [README](src/compressions/README.md) |
| **Lz4** | `nothings::compressions::lz4::app` | LZ4 frame 压缩/解压，纯 Rust 实现 | [README](src/compressions/lz4/README.md) |
| **Zlib** | `nothings::compressions::zlib::app` | zlib 压缩/解压，0-9 级压缩级别 | [README](src/compressions/zlib/README.md) |
| **Zstd** | `nothings::compressions::zstd4::app` | Zstandard 压缩/解压，1-21 级压缩级别 | [README](src/compressions/zstd4/README.md) |
| **Digests** | `nothings::digests` | `Bcrypt` 密码哈希与验证、`Md5Hash`、`Sha256Hash` | [README](src/digests/README.md) |
| **Secrets** | `nothings::secrets` | 加密模块总览：`Symmetric` trait 与 `with_aes_gcm()` / `with_aes_cbc()` / `with_aes_ctr()` / `with_aes_ecb()` 4 个工厂函数，用于把加密算法作为参数传递 | [README](src/secrets/README.md) |
| **Aes** | `nothings::secrets::symmetric::aes::app` | AES-128/192/256 加密解密，GCM / CBC / CTR / ECB 四种模式，纯 Rust 实现 | [README](src/secrets/symmetric/aes/README.md) |
| **Filesystem** | `nothings::filesystem` | `File` / `Dir`：读写、移动、删除、权限设置、整目录打包/还原 | [README](src/filesystem/README.md) |
| **HttpResponse** | `nothings::http_responses` | 可序列化的统一响应结构 `HttpResponse<T>`（`ok` / `bad_request` 等 7 个构造器） | [README](src/http_responses/README.md) |
| **Texts / Timer** | `nothings::texts::timer` | 将 `"1h30m"`、`"2w3d"` 等字符串解析为 `std::time::Duration` | [README](src/texts/README.md) |
| **Validations** | `nothings::validations` | 基于 `#[derive(Nothings)]` 的声明式结构体字段校验 | [README](src/validations/README.md) |
| **Singletons** | `nothings::impl_singleton!` | 通过宏为任意类型生成 `instance()` / `default()` / `with(...)` 单例接口（外部 crate 可用） | [README](src/singletons/README.md) |

### 独立 crate（不在 `nothings` 内，按需引入）

```toml
[dependencies]
nothings-orm = "0.0.1"          # ORM 核心，零 DB 依赖
nothings-orm-sqlite = "0.0.1"   # SQLite 驱动（rusqlite bundled）
nothings-orm-mysql = "0.0.1"    # MySQL 驱动（同步 mysql crate，纯 Rust）
```

| crate | 说明 | 文档 |
|-------|------|------|
| **nothings-orm** | `#[derive(NOrmEntity)]` 派生宏、静态 schema、`NOrmDriver` / `NOrmRepository` / `NOrmQueryBuilder`、CRUD 与事务；`NOrmCondition` / `NOrmOperator` 支持 OR、嵌套、NOT、比较运算 | [README](nothings-orm/README.md) |

支持软删除（`#[entity(soft_delete)]` + `unscoped()` / `hard_delete()`）、时间戳钩子（`created_at` / `updated_at`）与零依赖 `NOrmDateTime::now_utc()`，详见 [cookbook](docs/orm-cookbook.md)。
| **nothings-orm-sqlite** | `NOrmSqliteDriver`，rusqlite bundled 实现，仅 native | [README](nothings-orm-sqlite/README.md) |
| **nothings-orm-mysql** | `NOrmMysqlDriver`，同步 `mysql` crate（`minimal-rust`，无 C/TLS 依赖）；集成测试需真实 MySQL | [README](nothings-orm-mysql/README.md) |

**完整实战演示（推荐从这里开始）**：[docs/orm-cookbook.md](docs/orm-cookbook.md) —— 覆盖实体定义、CRUD、查询构建、事务、错误处理、类型映射、两个驱动与自定义 NOrmDriver，示例全部实测可跑。

设计文档：[docs/ai/spec/50-orm-design.md](docs/ai/spec/50-orm-design.md)（ORM 核心）、[docs/ai/spec/51-mysql-driver-design.md](docs/ai/spec/51-mysql-driver-design.md)（MySQL 驱动）。根 crate 零 DB 依赖，wasm 移除后所有 crate 都是纯 native。

### 暂无独立文档的模块

| 模块 | 导入路径 | 说明 |
|------|----------|------|
| **Coroutines** | `nothings::coroutines` | 预留模块，`mod.rs` 只有 `pub mod app;` 一行，`app.rs` 为 0 字节，当前没有任何内容 |

## 快速开始

下面只给最小可用示例，完整 API 见各模块 README。

### AnySlice — 增强版 Vec

```rust
use nothings::any_slices::app::AnySlice;

let mut slice = AnySlice::new(vec![1, 2, 3, 4, 5]);

// has 的参数是 &Vec<T>；first/last 返回克隆出的 Option<T>
assert!(slice.has(&vec![3]));
assert_eq!(slice.first(), Some(1));

// 链式操作
slice.push(6).push(7);

// 集合运算
let a = AnySlice::new(vec![1, 2, 3]);
let b = vec![2, 3, 4];
assert_eq!(a.union(&b).to_vec(),        &vec![1, 2, 3, 4]);
assert_eq!(a.intersection(&b).to_vec(), &vec![2, 3]);
assert_eq!(a.difference(&b).to_vec(),   &vec![1]);
```

详见 [src/any_slices/README.md](src/any_slices/README.md)。

### AnyMap — 有序键值对映射

```rust
use nothings::any_maps::app::AnyMap;

// 构造器名为 with_iter / with_hashmap
let mut map = AnyMap::with_iter(vec![("alice", 90), ("bob", 85), ("charlie", 92)]);

// get_value_by_key 返回 Result<V, Error>（克隆值，不是引用）
assert_eq!(map.get_value_by_key(&"alice").unwrap(), 90);
assert!(map.has(&"bob"));
assert!(map.in_value(&85));

map.push_datum("dave", 88);
map.each(|_key, value| value + 1);

assert_eq!(map.to_entries().len(), 4);
println!("{}", map.to_string(Some(", ")));
// alice: 91, bob: 86, charlie: 93, dave: 89
```

详见 [src/any_maps/README.md](src/any_maps/README.md)。

### 压缩 / 解压

三种算法接口一致，均为关联函数。

```rust
use nothings::compressions::lz4::app::Lz4;
use nothings::compressions::zlib::app::Zlib;

let data = b"hello world, this is some test data";

// LZ4：默认级别 1
let compressed = Lz4::compress(data).unwrap();
assert_eq!(Lz4::decompress(&compressed).unwrap(), data);

// Zlib：默认级别 6，可指定 0-9
let compressed = Zlib::compress_with_level(data, 9).unwrap();
assert_eq!(Zlib::decompress(&compressed).unwrap(), data);

// 文件压缩/解压（路径需真实存在，否则返回 Err）
Lz4::compress_file("input.txt", "output.lz4").unwrap();
Lz4::decompress_file("output.lz4", "output.txt").unwrap();
```

```rust
use nothings::compressions::zstd4::app::Zstd;
let compressed = Zstd::compress_with_level(b"hello", 19).unwrap();
assert_eq!(Zstd::decompress(&compressed).unwrap(), b"hello");
```

也可通过统一的 `Compressor` trait 把算法作为参数传递：

```rust
use nothings::compressions::{with_lz4, with_zlib, Compressor};

fn pack(data: &[u8], c: &dyn Compressor) -> Vec<u8> {
    c.compress(data).unwrap()
}

let _ = pack(b"hello", &*with_lz4());
let _ = pack(b"hello", &*with_zlib());
```

### 摘要与密码哈希

```rust
use nothings::digests::bcrypt::Bcrypt;
use nothings::digests::md5::Md5Hash;
use nothings::digests::sha::Sha256Hash;

// Bcrypt：默认 cost 12（范围 4-31）
let plaintext = "my_password";
let hashed = Bcrypt::new(plaintext).hash().unwrap();
assert!(Bcrypt::new(plaintext).check(&hashed).unwrap());

let mut bcrypt = Bcrypt::new(plaintext);
bcrypt.set_cost(10);
let _ = bcrypt.hash().unwrap();

// MD5 → 32 位十六进制
assert_eq!(Md5Hash::new("hello world").hash(), "5eb63bbbe01eeed093cb22bb8f5acdc3");

// SHA256 → 64 位十六进制
assert_eq!(
    Sha256Hash::new("hello world").hash(),
    "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
);
```

### 对称加密

```rust
use nothings::secrets::symmetric::aes::app::Aes;
use nothings::secrets::{with_aes_cbc, Symmetric};

// AES-GCM：随机密钥 + 随机 nonce，密文自带 16 字节认证 tag
let key = Aes::generate_key(256).unwrap();   // 32 字节
let nonce = Aes::generate_nonce();           // 12 字节，需随密文一起保存

let cipher = Aes::gcm(&key, &nonce);
let encrypted = cipher.encrypt(b"hello world").unwrap();
assert_eq!(encrypted.len(), b"hello world".len() + 16);
assert_eq!(cipher.decrypt(&encrypted).unwrap(), b"hello world");

// 篡改一个字节，GCM 会解密失败
let mut tampered = encrypted.clone();
tampered[0] ^= 0x01;
assert!(cipher.decrypt(&tampered).is_err());

// 工厂函数：把算法作为参数传递
fn seal(data: &[u8], cipher: &dyn Symmetric) -> Vec<u8> {
    cipher.encrypt(data).unwrap()
}

assert!(!seal(b"hello", &*with_aes_cbc(&[0x42; 16], &[0x24; 16])).is_empty());
```

CBC / CTR / ECB 三种模式不校验密文完整性，仅在与既有系统互通时使用，详见 [Aes README](src/secrets/symmetric/aes/README.md)。

### Filesystem — 文件与目录

```rust
use nothings::filesystem::file::File;
use nothings::filesystem::dir::Dir;

// File：写入是覆盖，不是追加
let mut f = File::new("note.txt");
f.write(b"hello").unwrap();
assert_eq!(f.read().unwrap(), b"hello");
assert_eq!(f.ext, "txt");

// mv 不返回 Result，失败信息写进公开的 err 字段，必须手动检查
let mut f = File::new("note.txt");
f.mv("moved.txt");
assert!(f.err.is_none());

// Dir::new 会递归扫描整棵子树，大目录慎用
let d = Dir::new_or_create("/tmp/demo").unwrap();
println!("{} 个条目，空={}", d.count(), d.is_empty());
```

⚠️ `Dir::zip` 产出的**不是标准 ZIP 文件**（自定义 TLV 归档 + 压缩，magic 不是 `PK\x03\x04`），只能用 `Dir::unzip` 配同一算法还原；空目录不会被保存。详见 [src/filesystem/README.md](src/filesystem/README.md)。

### HttpResponse — 统一响应结构

```rust
use nothings::http_responses::HttpResponse;

let ok: HttpResponse<i32> = HttpResponse::ok(None).content(42);
assert_eq!(ok.code, 200);
assert_eq!(serde_json::to_string(&ok).unwrap(), r#"{"code":200,"msg":"OK","content":42}"#);

let err: HttpResponse<()> = HttpResponse::un_authorization(Some("请先登录"));
assert_eq!(err.code, 401);
```

7 个构造器覆盖 200 / 201 / 202 / 204 / 400 / 401 / 500。只派生了 `Serialize`，**没有 `Debug` / `Clone` / `Deserialize`**，且状态码无法自定义（初始化入口 `empty()` 是私有的，只能靠直接改 `pub` 字段绕过）。详见 [src/http_responses/README.md](src/http_responses/README.md)。

### Timer — 时间字符串解析

```rust
use nothings::texts::timer::{Time, TimeTrait};
use std::time::Duration;

// Time 是实现 TimeTrait 的结构体，调用关联函数需把 TimeTrait 一起导入
// 注意导入路径是 texts::timer，texts::prelude 与 texts::volumer 为私有模块
let t = Time::with_text("1h30m").unwrap();
assert_eq!(t.get_duration(), Duration::from_secs(5400));
assert_eq!(t.to_chinese(), Ok("1小时30分钟".to_string()));
```

支持单位 `s` / `m` / `h` / `d` / `w`，可组合书写；详见 [src/texts/README.md](src/texts/README.md)。

### Validations — 声明式校验

```rust
use nothings::validations::check::Check;
use nothings::validations::checker::Checker;
use nothings::validations::Nothings;

#[derive(Nothings)]
struct Register {
    #[validator[rule="(required)(min>=6)(max<=20)" name="用户名" kind="string"]]
    username: String,
}

let form = Register { username: "alice".into() };

let checker = Check::new(form);
match checker.check() {
    None => println!("校验通过"),
    Some(err) => println!("校验失败: {} - {}", err.field, err.message),
}
```

规则用 `(...)` 包裹并连续书写，属性名为 `rule` / `name` / `kind` / `nested`；完整规则与操作符列表见 [src/validations/README.md](src/validations/README.md)。

### Singletons

`impl_singleton!` 宏外部 crate 可用：`Singleton` trait 已公开，`instance()` / `with()` / `default()` 均可调用。用法见 [src/singletons/README.md](src/singletons/README.md)，跨 crate 回归守护见 `tests/singletons.rs`。

## 已知问题

以下问题仍存在，修复前请勿依赖（A1-A7 已于 2026-11-19 修复并从表中移除）：

| 模块 | 问题 | 影响与替代方案 |
|------|------|---------------|
| AnySlice | `get_indexes_by_values` 内部 `unwrap` | 任一值不存在即 panic；`AnyMap::get_indexes_by_values` 无此问题（用 `filter_map`） |
| Filesystem | `File::mv` 成功后不清除 `err` | 前一次失败的错误会一直残留，导致误判；调用前先 `f.err = None`。`Dir::mv` 无此问题 |
| Filesystem | `Dir::zip` 产出的不是标准 ZIP，且丢失空目录 | 自定义 TLV 归档 + 压缩，magic 不是 `PK\x03\x04`，系统解压工具打不开；只能用 `Dir::unzip` 配同一算法还原 |
| Filesystem | `Dir::new` 会递归扫描整棵子树 | 对大目录开销巨大且无惰性接口；只需浅层信息时直接用 `std::fs::read_dir` |
| Filesystem | `Dir::up` 对相对路径失效 | `Path::new("demo").parent()` 是空串，触发「无法返回上级目录」并原样返回自身；先 `canonicalize` |
| HttpResponse | 状态码无法自定义，`errors` 字段无实际作用 | 初始化入口 `empty()` 是私有的，只能改 `pub` 字段绕过；`std::fmt::Error` 不携带信息且被 `#[serde(skip)]` |
| HttpResponse | 没有 `Debug` / `Clone` / `PartialEq` / `Deserialize` | 不能 `{:?}` 打印、不能 `assert_eq!`、不能反序列化；只适合做服务端出参 |
| nothings-orm-mysql | 标识符不加反引号，保留字列名会报 1064 | `E::COLUMNS` / `E::TABLE` 避开 MySQL 保留字；DDL 列名与 `#[entity(column = ...)]` 字面一致 |
| nothings-orm-mysql | `NOrmQueryBuilder::offset()` 单独调用会生成 MySQL 不支持的 `SELECT ... OFFSET n` | 必须同时调用 `limit()`；`one()` 自带 `limit(1)`，`get` / `find_one` 不受影响 |
| nothings-orm-mysql | `execute_ddl` 不参数化且实测允许多语句；集成测试强制 DSN | DDL 只用固定字面量、绝不接用户输入；跑测试前设置 `NOTHINGS_MYSQL_DSN` |
| nothings-orm-mysql | 时区自动转换只对 `NOrmValue::NOrmDateTime` 生效，TIMESTAMP/DATETIME 读回一律 `NOrmValue::NOrmDateTime` | 时间列字段改用 `NOrmDateTime`（`String` 会读失败）；SQLite 不支持自动转换；用 `connect_with_tz` 配置双时区 |

## License

MIT
