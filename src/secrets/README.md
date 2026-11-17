# Secrets — 加密模块总览与统一接口

`secrets` 模块提供可逆的对称加密能力，并通过 `Symmetric` trait 把具体算法抽象成**可以当参数传递的值**。

与 `digests` 的区别：`digests`（MD5 / SHA256 / Bcrypt）是**单向**的，只能校验不能还原；`secrets` 是**可逆**的，加密后能用同一把密钥解回原文。

- 只关心「用 AES 加密一段字节」→ 直接用 `Aes`，见 [symmetric/aes/README.md](symmetric/aes/README.md)
- 需要「让调用方决定用哪种算法」→ 用本页的 `Symmetric` trait 与工厂函数

## 目录结构

```
secrets
└── symmetric          对称加密（加解密同一把密钥）
    └── aes            AES，支持 GCM / CBC / CTR / ECB
```

`symmetric` 这一层是分类命名空间，后续的国密 **SM4** 会落在 `secrets::symmetric::sm4`，
复用同一个 `Symmetric` trait——届时只需实现 trait 并新增 `with_sm4_*` 工厂函数，调用方代码不用改。

## Symmetric trait

```rust
pub trait Symmetric {
    fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>, std::io::Error>;
    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, std::io::Error>;
}
```

错误类型沿用 `std::io::Error`，与 `compressions::Compressor` 保持一致：

| `ErrorKind` | 含义 |
|-------------|------|
| `InvalidInput` | 密钥长度不是 16 / 24 / 32 字节，或 iv / nonce 长度不符合该模式要求 |
| `InvalidData` | 解密失败：GCM tag 校验不通过、CBC / ECB 的 PKCS7 填充非法、密钥或 iv 不匹配 |

## 工厂函数

定义在 `nothings::secrets` 下，返回 `Box<dyn Symmetric>`：

| 函数 | 签名 | 说明 |
|------|------|------|
| `with_aes_gcm(key, nonce)` | `(&[u8], &[u8]) -> Box<dyn Symmetric>` | AES-GCM 认证加密，**推荐** |
| `with_aes_cbc(key, iv)` | `(&[u8], &[u8]) -> Box<dyn Symmetric>` | AES-CBC，PKCS7 填充 |
| `with_aes_ctr(key, iv)` | `(&[u8], &[u8]) -> Box<dyn Symmetric>` | AES-CTR，密文与明文等长 |
| `with_aes_ecb(key)` | `(&[u8]) -> Box<dyn Symmetric>` | AES-ECB，⚠️ 会泄露数据模式 |

## 快速开始

```rust
use nothings::secrets::{with_aes_cbc, with_aes_gcm, Symmetric};

let data = b"hello world, this is some secret data";

// Box<dyn Symmetric> 直接调用 trait 方法
let cipher = with_aes_gcm(&[0x42; 32], &[0x24; 12]);
let encrypted = cipher.encrypt(data).unwrap();
assert_eq!(cipher.decrypt(&encrypted).unwrap(), data);

// 典型用法：把算法作为参数传进业务函数
fn seal(data: &[u8], cipher: &dyn Symmetric) -> Vec<u8> {
    cipher.encrypt(data).unwrap()
}

assert!(!seal(data, &*with_aes_cbc(&[0x42; 16], &[0x24; 16])).is_empty());
```

也可以直接构造 `Aes` 实例并传引用，省去 `Box`：

```rust
use nothings::secrets::symmetric::aes::app::Aes;
use nothings::secrets::Symmetric;

let cipher = Aes::gcm(&Aes::generate_key(256).unwrap(), &Aes::generate_nonce());
let encrypted = cipher.encrypt(b"hello").unwrap();
assert_eq!(cipher.decrypt(&encrypted).unwrap(), b"hello");
```

## 如何选择模式

| 场景 | 推荐 |
|------|------|
| 新系统、没有历史包袱 | **GCM** —— 密文自带 16 字节 tag，被篡改会解密失败 |
| 需要与 Java / Go / OpenSSL 老系统互通 | CBC（对方多半是 `AES/CBC/PKCS5Padding`） |
| 需要密文长度与明文严格相等（定长字段、流式） | CTR |
| 仅当对接的旧系统只支持 ECB | ECB —— ⚠️ 相同明文块产出相同密文块 |

⚠️ **CBC / CTR / ECB 都不校验密文完整性**。攻击者改动密文不会被发现，
CBC 下还可能被 padding oracle 攻击还原明文。除非有互通需求，一律优先 GCM。

⚠️ **GCM 的 nonce 在同一把密钥下绝不能重复**。重复使用 nonce 会让攻击者恢复出认证子密钥，
进而伪造任意密文。每条消息都用 `Aes::generate_nonce()` 生成新值，并随密文一起存储或传输。

## wasm 支持

`secrets` 全模块基于 RustCrypto 纯 Rust 实现，在 `wasm32-unknown-unknown` 与 `wasm32-wasip1` 下均可编译，
不需要像 `compressions::zstd4` 那样加 `#[cfg(not(target_arch = "wasm32"))]` 包裹。

`Aes::generate_key` / `generate_nonce` / `generate_iv` 走仓库已有的 `rand` 依赖，
浏览器环境由 Cargo.toml 里配置的 `getrandom` + `wasm_js` 提供熵源。

## 与 digests 的配合

常见做法是「用 SHA256 把任意长度的口令规整成 AES 密钥」：

```rust
use nothings::digests::sha::Sha256Hash;
use nothings::secrets::symmetric::aes::app::Aes;
use nothings::secrets::Symmetric;

// Sha256Hash::hash() 返回 64 个十六进制字符，解码后正好是 32 字节密钥
let digest = Sha256Hash::new("my-passphrase").hash();
let key: Vec<u8> = (0..digest.len())
    .step_by(2)
    .map(|i| u8::from_str_radix(&digest[i..i + 2], 16).unwrap())
    .collect();
assert_eq!(key.len(), 32);

let cipher = Aes::gcm(&key, &Aes::generate_nonce());
let encrypted = cipher.encrypt(b"payload").unwrap();
assert!(!encrypted.is_empty());
```

⚠️ 注意十六进制字符串**不能直接** `into_bytes()` 当密钥——那会得到 64 个 ASCII 字节，
既不是合法密钥长度，也白白浪费了一半熵。

⚠️ 直接用口令的 SHA256 当密钥**不抗暴力破解**（SHA256 太快，可被 GPU 批量穷举）。
面向用户口令的场景应先经 Argon2 / bcrypt 等慢哈希拉伸。

## 未提供的能力

| 能力 | 说明 |
|------|------|
| GCM 的 AAD（附加认证数据） | `Symmetric::encrypt(&self, data)` 的签名容纳不下，需要时再加 `encrypt_with_aad` |
| base64 / hex 编解码 | 加密输出保持 `Vec<u8>`，由调用方自行编码 |
| 文件加解密 | `compressions` 有 `compress_file`，加密侧暂未提供 |
| wasm C-ABI 导出 | `wasm_exports` 中暂无 AES 导出函数 |
