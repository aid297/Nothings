# Aes — AES 对称加密

`Aes` 提供 AES-128 / 192 / 256 的加密与解密，支持 **GCM / CBC / CTR / ECB** 四种模式。基于 RustCrypto 纯 Rust 实现，可用于 wasm 环境。

实现了 [`Symmetric`](../../README.md) trait，因此可以作为 `Box<dyn Symmetric>` 或 `&dyn Symmetric` 传递。

## 快速开始

```rust
use nothings::secrets::symmetric::aes::app::Aes;
use nothings::secrets::Symmetric;

// 随机生成密钥与 nonce（推荐做法）
let key = Aes::generate_key(256).unwrap();   // 32 字节
let nonce = Aes::generate_nonce();           // 12 字节

let cipher = Aes::gcm(&key, &nonce);
let encrypted = cipher.encrypt(b"hello world").unwrap();
let decrypted = cipher.decrypt(&encrypted).unwrap();
assert_eq!(decrypted, b"hello world");
```

⚠️ `nonce` / `iv` 不是秘密，需要**随密文一起存储或传输**，否则无法解密。

## 四种模式对比

| 模式 | 构造 | iv / nonce | 填充 | 输出长度 | 完整性校验 |
|------|------|-----------|------|---------|-----------|
| **GCM** | `Aes::gcm(key, nonce)` | 12 字节 | 无 | 明文 + 16 字节 tag | ✅ |
| **CBC** | `Aes::cbc(key, iv)` | 16 字节 | PKCS7 | 向上补齐到 16 的倍数 | ❌ |
| **CTR** | `Aes::ctr(key, iv)` | 16 字节 | 无 | 与明文等长 | ❌ |
| **ECB** | `Aes::ecb(key)` | 不使用 | PKCS7 | 向上补齐到 16 的倍数 | ❌ |

密钥长度三者通用：16 字节 = AES-128，24 字节 = AES-192，32 字节 = AES-256。

除非有历史系统互通需求，**一律优先 GCM**——它是唯一能检出密文被篡改的模式。

## API 一览

### 构造

| 方法 | 说明 |
|------|------|
| `Aes::gcm(key, nonce)` | GCM 模式，`nonce` 12 字节 |
| `Aes::cbc(key, iv)` | CBC 模式，`iv` 16 字节，PKCS7 填充 |
| `Aes::ctr(key, iv)` | CTR 模式，`iv` 16 字节初始计数器 |
| `Aes::ecb(key)` | ECB 模式，不使用 iv |

四个构造函数都**不会失败**——密钥与 iv 的长度校验推迟到加解密时进行，
与仓库里 `Lz4::with_level` / `Bcrypt::new` 的风格一致。

### 读写参数

| 方法 | 说明 |
|------|------|
| `mode()` | 返回当前 `AesMode` |
| `set_key(key) -> &mut Self` | 替换密钥，可链式调用 |
| `set_iv(iv) -> &mut Self` | 替换 iv（GCM 模式下即 nonce），可链式调用 |

```rust
use nothings::secrets::symmetric::aes::app::Aes;

let mut cipher = Aes::cbc(&[0x42; 16], &[0x24; 16]);
cipher.set_key(&[0x11; 32]).set_iv(&[0x22; 16]);
```

### 随机生成

| 方法 | 说明 |
|------|------|
| `generate_key(bits)` | 生成随机密钥，`bits` 取 128 / 192 / 256，返回 `Result<Vec<u8>, Error>` |
| `generate_nonce()` | 生成 GCM 随机 nonce，12 字节 |
| `generate_iv()` | 生成 CBC / CTR 随机 iv，16 字节 |

熵源为仓库已有的 `rand` 依赖，wasm 浏览器环境由 Cargo.toml 中配置的 `getrandom` + `wasm_js` 兜底。

### 加解密（来自 `Symmetric` trait）

| 方法 | 说明 |
|------|------|
| `encrypt(data) -> Result<Vec<u8>, Error>` | 加密 |
| `decrypt(data) -> Result<Vec<u8>, Error>` | 解密 |

调用前需 `use nothings::secrets::Symmetric;`。

### 常量

| 常量 | 值 | 说明 |
|------|-----|------|
| `BLOCK_LEN` | 16 | AES 分组长度，也是 CBC / CTR 的 iv 长度 |
| `GCM_NONCE_LEN` | 12 | GCM 推荐 nonce 长度 |
| `GCM_TAG_LEN` | 16 | GCM 认证 tag 长度，追加在密文尾部 |

`AesMode::iv_len()` 返回该模式所需的 iv 长度（ECB 为 0），便于写通用代码：

```rust
use nothings::secrets::symmetric::aes::app::{Aes, AesMode};

for mode in [AesMode::Gcm, AesMode::Cbc, AesMode::Ctr, AesMode::Ecb] {
    println!("{mode:?} 需要 {} 字节 iv", mode.iv_len());
}
```

## 错误码

| `ErrorKind` | 触发条件 |
|-------------|---------|
| `InvalidInput` | 密钥长度不是 16 / 24 / 32；iv / nonce 长度不符合模式要求；`generate_key` 收到非法 `bits` |
| `InvalidData` | GCM tag 校验不通过（密文被篡改、密钥或 nonce 不匹配）；CBC / ECB 的 PKCS7 填充非法或密文长度不是 16 的倍数 |

CTR 模式是纯异或密钥流，无填充也无 tag，任意长度输入都不会解密失败——
这也意味着**它无法告诉你密文有没有被改动**。

## 与其它系统互通

四种模式的密文格式均为裸密文（无自定义头），可直接与下列实现对互通：

| 本模块 | OpenSSL CLI | Java JCE |
|--------|-------------|----------|
| `Aes::cbc` | `openssl enc -aes-256-cbc` | `AES/CBC/PKCS5Padding` |
| `Aes::ctr` | `openssl enc -aes-256-ctr` | `AES/CTR/NoPadding` |
| `Aes::ecb` | `openssl enc -aes-256-ecb` | `AES/ECB/PKCS5Padding` |
| `Aes::gcm` | `openssl enc` 不支持 GCM | `AES/GCM/NoPadding` |

要点：

- Java 的 `PKCS5Padding` 在 AES（16 字节分组）下等价于本模块的 PKCS7
- CTR 采用 `Ctr128BE`：把整个 16 字节 iv 当作**大端 128 位计数器**自增，
  与 OpenSSL、Java 的默认行为一致。若对方用的是 32 位或 64 位计数器变体则无法互通
- GCM 的输出是「密文 ‖ 16 字节 tag」，Java 侧 `AES/GCM/NoPadding` 的 `doFinal` 结果同样是这个拼接顺序；
  若对方要求 tag 单独存放，需自行切分最后 16 字节
- CBC / ECB 的密文长度必然是 16 的倍数，明文恰好块对齐时会多补一整块（PKCS7 规定）

CBC 与 ECB 的填充结果已与 LibreSSL 逐字节核对，CTR 与 GCM 已用 NIST 公开向量核对，见 `test.rs` 中的 KAT 测试。

## 安全注意事项

- **GCM 的 nonce 在同一密钥下绝不能重复**。重复会让攻击者恢复出 GHASH 认证子密钥，
  进而伪造任意密文。每条消息都调用 `generate_nonce()`，并把 nonce 与密文一起保存
- **CBC / CTR / ECB 的 iv 也必须每条消息唯一**（不必保密，但不可预测、不可重用），
  否则相同明文会产出相同密文，泄露数据模式
- **ECB 会泄露数据模式**：相同明文块产出相同密文块。`test.rs` 里有一条测试专门固化了这个行为。
  仅在对接只支持 ECB 的旧系统时使用
- **CBC 存在 padding oracle 风险**：若解密失败信息会返回给攻击者，可能被逐字节还原明文。
  对外提供服务时优先换 GCM
- 密钥不要硬编码在源码里，也不要写进日志

## 已知限制

| 限制 | 说明 |
|------|------|
| 不支持 GCM 的 AAD | `Symmetric::encrypt(&self, data)` 签名容纳不下附加认证数据，需要时再加 `encrypt_with_aad` |
| 不做 base64 / hex 编解码 | 输出保持 `Vec<u8>`，由调用方自行编码 |
| 不提供文件加解密 | `compressions` 有 `compress_file`，加密侧暂未提供 |
| GCM nonce 固定 12 字节 | 底层 `aes-gcm` 支持其它长度，但 12 字节是标准且性能最好的选择，未开放配置 |
| 密钥不做内存擦除 | `Aes` 把 key / iv 存在普通 `Vec<u8>` 里，`Drop` 时不会清零，密钥字节可能残留在进程内存或 swap 中。底层 crate 提供了 `zeroize` feature，但本模块未启用。对内存取证有要求的场景需自行评估 |
