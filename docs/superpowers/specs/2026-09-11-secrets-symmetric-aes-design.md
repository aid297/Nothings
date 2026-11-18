# secrets 模块与对称加密（AES）设计

- 日期：2026-09-11
- 状态：已实现（实现与设计的差异见文末「实现记录」）
- 范围：新增 `nothings::secrets` 模块，定义统一对称加密接口 `Symmetric`，并提供 `secrets::symmetric::aes` 实现

## 背景与目标

`nothings` 目前的 `digests` 模块只提供单向哈希（Bcrypt / MD5 / SHA256），没有可逆加密能力。
本次新增 `secrets` 模块补齐这块，并且要为后续的国密 SM4 预留统一的接口形状。

参照仓库已有的 `compressions` 模块：父模块定义 `Compressor` trait 与 `with_lz4()` 一类工厂函数，
子模块 `compressions::lz4::app::Lz4` 提供具体算法实现。`secrets` 完全对齐这套结构。

## 模块结构

```
src/secrets/
├── mod.rs                    # pub trait Symmetric + 工厂函数
├── README.md
└── symmetric/
    ├── mod.rs                # pub mod aes;
    └── aes/
        ├── mod.rs            # pub mod app; #[cfg(test)] mod test;
        ├── app.rs            # Aes / AesMode
        ├── test.rs
        └── README.md
```

`symmetric` 这一层是分类命名空间，未来可平行扩展 `secrets::asymmetric` 等；
SM4 落在 `secrets::symmetric::sm4`，复用同一个 `Symmetric` trait。

## 依赖

```toml
aes     = "0.9"
cbc     = { version = "0.2",  features = ["alloc"] }
ctr     = { version = "0.10", features = ["alloc"] }
ecb     = { version = "0.2",  features = ["alloc"] }
aes-gcm = { version = "0.11", default-features = false, features = ["aes", "alloc"] }
```

约束与理由：

- 全部为 RustCrypto 纯 Rust 实现，`wasm32-unknown-unknown` 与 `wasm32-wasip1` 均可编译，
  不需要像 `compressions::zstd4` 那样加 `cfg(not(target_arch = "wasm32"))` 分支。
- `aes-gcm` 关闭默认的 `getrandom` feature。仓库已经为 wasm 场景显式配置了
  `getrandom 0.3` / `getrandom 0.4`（`getrandom_04`）的 `wasm_js` feature，
  再引入一份会带来 feature 统一上的风险。随机数统一走已有的 `rand` 依赖。
- 这批版本属于 `cipher 0.5` 系列，API 与旧的 `cipher 0.4` 系列不同
  （`BlockModeEncrypt` / `BlockModeDecrypt` 取代 `BlockEncryptMut` / `BlockDecryptMut`，
  `encrypt_padded_vec` 取代 `encrypt_padded_vec_mut`），实现时以实际编译结果为准。

## 接口

### Symmetric trait（`src/secrets/mod.rs`）

```rust
use std::io::Error;

/// 统一对称加密接口
pub trait Symmetric {
    fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>, Error>;
    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, Error>;
}
```

命名说明：需求描述里写的是 `SymmeticTrait`，Rust 惯例不给 trait 加 `Trait` 后缀，
且仓库已有 `Compressor` 先例，因此定名 `Symmetric`。

工厂函数（对齐 `with_lz4()` / `with_zlib_level()` 风格）：

```rust
pub fn with_aes_gcm(key: &[u8], nonce: &[u8]) -> Box<dyn Symmetric>;
pub fn with_aes_cbc(key: &[u8], iv: &[u8]) -> Box<dyn Symmetric>;
pub fn with_aes_ctr(key: &[u8], iv: &[u8]) -> Box<dyn Symmetric>;
pub fn with_aes_ecb(key: &[u8]) -> Box<dyn Symmetric>;
```

### Aes（`src/secrets/symmetric/aes/app.rs`）

```rust
pub enum AesMode { Gcm, Cbc, Ctr, Ecb }

pub struct Aes {
    mode: AesMode,
    key: Vec<u8>,
    iv: Vec<u8>,
}

impl Aes {
    pub fn gcm(key: &[u8], nonce: &[u8]) -> Self;
    pub fn cbc(key: &[u8], iv: &[u8]) -> Self;
    pub fn ctr(key: &[u8], iv: &[u8]) -> Self;
    pub fn ecb(key: &[u8]) -> Self;

    pub fn mode(&self) -> AesMode;
    pub fn set_key(&mut self, key: &[u8]) -> &Self;
    pub fn set_iv(&mut self, iv: &[u8]) -> &Self;

    /// 生成随机密钥，bits 取 128 / 192 / 256
    pub fn generate_key(bits: usize) -> Result<Vec<u8>, Error>;
    /// 生成 GCM 随机 nonce（12 字节）
    pub fn generate_nonce() -> Vec<u8>;
    /// 生成 CBC / CTR 随机 iv（16 字节）
    pub fn generate_iv() -> Vec<u8>;
}

impl Symmetric for Aes { /* ... */ }
```

`set_key` / `set_iv` 返回 `&Self`，对齐 `Bcrypt::set_origin` / `Sha256Hash::set_origin` 的既有风格。

随机数来自已有的 `rand` 依赖，wasm 场景由仓库现有的 `getrandom` 配置兜底。

### 各模式参数规格

| 模式 | key 长度 | iv / nonce | 填充 | 输出长度 |
| --- | --- | --- | --- | --- |
| GCM | 16 / 24 / 32 | 12 字节 nonce | 无（流式） | 明文长度 + 16 字节 tag |
| CBC | 16 / 24 / 32 | 16 字节 | PKCS7 | 向上块对齐 |
| CTR | 16 / 24 / 32 | 16 字节初始计数器 | 无 | 与明文等长 |
| ECB | 16 / 24 / 32 | 不使用（留空） | PKCS7 | 向上块对齐 |

CTR 变体选 `Ctr128BE`：把整个 16 字节 iv 当作大端 128 位计数器自增，
与 OpenSSL、Java `AES/CTR/NoPadding` 的默认行为一致，便于跨语言互通。

GCM 密文格式为「密文 ‖ 16 字节 tag」，即 `aes-gcm` 的默认输出，
解密时对 tag 做认证校验，校验失败即报错。

### 错误处理

沿用 `std::io::Error`，与 `compressions::Compressor` 保持一致，不新增公开错误类型。

构造函数不落错（对齐 `Lz4::with_level`、`Bcrypt::new` 的既有风格），
key / iv 长度校验在 `encrypt` / `decrypt` 内部完成，trait 是唯一的错误出口：

- `ErrorKind::InvalidInput` —— key 长度不是 16 / 24 / 32；iv 或 nonce 长度不符合该模式要求；
  `generate_key` 收到非法 bits
- `ErrorKind::InvalidData` —— 解密失败：GCM tag 认证不通过、CBC / ECB PKCS7 去填充失败、
  CTR 输入无需校验故一般不触发

已知取舍：`Aes::cbc(&[1, 2, 3], iv)` 可以构造成功，直到调用 `encrypt` 才报错。
换来的是构造函数签名简洁、与仓库其它模块一致。

## 明确不做（YAGNI）

- **GCM 的 AAD（附加认证数据）**：`Symmetric::encrypt(&self, data)` 的签名容纳不下，
  确有需求时再加 `encrypt_with_aad`，不影响现有调用方。
- **base64 / hex 编解码辅助**：仓库没有该依赖，加密输出保持 `Vec<u8>`，由调用方自行编码。
- **文件加解密（`encrypt_file` / `decrypt_file`）**：`compressions` 有对应能力，
  但加密场景更常走内存，需要时再补。
- **`wasm_exports` 的 C-ABI 导出**：本次不做，后续按 Go 宿主需求再补
  `nothings_aes_encrypt` / `nothings_aes_decrypt`。

## 测试计划（`src/secrets/symmetric/aes/test.rs`）

1. **Round-trip**：4 模式 × 3 密钥长度，覆盖空数据、1 字节、恰好 16 字节、跨块、10KB 数据。
2. **已知答案向量（KAT）**：AES-128/256-CBC、AES-256-GCM、AES-128-CTR 的 NIST / RFC 公开向量。
   round-trip 测试无法发现填充方式、计数器字节序、tag 位置写错的问题，KAT 是必须的。
3. **非法输入**：key 长度非 16/24/32 → `InvalidInput`；CBC/CTR iv 非 16 字节、
   GCM nonce 非 12 字节 → `InvalidInput`。
4. **篡改检测**：GCM 密文改动 1 字节 → `InvalidData`；CBC 密文破坏填充 → `InvalidData`。
5. **确定性**：同 key + iv + 明文两次加密结果完全一致（GCM 复用 nonce 同样确定性，
   测试仅验证行为，不代表推荐这么用）。
6. **模式隔离**：同一明文在 CBC 与 CTR 下密文不同；用错模式解密失败或产出错误明文。
7. **随机生成器**：`generate_key(128/192/256)` 长度正确、非法 bits 报错；
   `generate_nonce()` 12 字节、`generate_iv()` 16 字节，多次调用不重复。
8. **工厂函数**：`with_aes_gcm` 等返回的 `Box<dyn Symmetric>` 可正常 round-trip。
9. **文档测试**：`app.rs` 中每个公开方法带可运行的 `# 示例`，对齐 `Lz4` 的文档密度。

## 文档

- 新增 `src/secrets/README.md`、`src/secrets/symmetric/aes/README.md`
- 更新根 `README.md`：模块目录表新增 Secrets / Symmetric / Aes 三行，
  快速开始新增「对称加密」小节，平台说明补充 AES 在 wasm 下可用

## 验收标准

- `cargo test` 全绿（含文档测试）
- `cargo build --target wasm32-unknown-unknown` 通过
- `cargo doc --no-deps` 无告警
- 后续新增 SM4 时，只需实现 `Symmetric` 并加一个 `with_sm4_*` 工厂函数，无需改动 trait

## 实现记录

实现与上述设计的差异，以及落地时才暴露的事实：

### setter 返回 `&mut Self` 而非 `&Self`

设计里写的是对齐 `Bcrypt::set_origin(&mut self) -> &Self`。实现改为返回 `&mut Self`：
`&Self` 会让 `cipher.set_key(k).set_iv(v)` 无法编译（第二次调用需要 `&mut`），
仓库现有的 `Bcrypt` / `Sha256Hash` 存在同样的问题。新代码不复制这个缺陷，
既有模块本次不动。

### 补齐 `Aes192Gcm` 类型别名

`aes-gcm` 0.11 只导出 `Aes128Gcm` 与 `Aes256Gcm`，没有 192。
但其 `AesGcm<Aes, NonceSize, TagSize>` 的各 trait 实现都是对底层分组密码泛型的，
因此在 `app.rs` 中自定义 `type Aes192Gcm = AesGcm<aes::Aes192, aes::cipher::consts::U12>;`
即可，无需为 GCM 模式砍掉 AES-192 支持。四种模式 × 三种密钥长度的组合因此是完整的。

### 加密与解密的错误文案分开

GCM 加密失败（明文超长）与解密失败（tag 校验不通过）语义完全不同，
若共用一条文案会让调用方在加密路径上看到「密文被篡改」这种误导信息。
实现拆成 `gcm_encrypt_failed()`（`InvalidInput`）与 `decrypt_failed(mode)`（`InvalidData`）。

### MSRV 从 1.85 抬升到 1.89

`aes` 0.9.3 声明 `rust-version = 1.89`，高于 edition 2024 本身要求的 1.85。
这是一个**对下游有影响的变化**，已在根 README 的安装说明中标注。
如需保持 1.85，只能退回 `cipher 0.4` 系列（`aes` 0.8 / `aes-gcm` 0.10），
但那是旧 API，不建议。

### `AesMode::iv_len()` 是设计外新增

写通用测试与文档示例时反复需要「该模式要几字节 iv」，
于是把它做成 `AesMode` 的公开方法（ECB 返回 0）。属于设计里没写、实现中自然长出来的 API。

### 密钥不做内存擦除

`Aes` 把 key / iv 存在普通 `Vec<u8>` 中，`Drop` 时不清零。
底层 crate 都提供 `zeroize` feature 但本次未启用。
已在 Aes README 的「已知限制」中如实披露，供有内存取证要求的调用方评估。

### 验证结果

- `cargo test`：304 个单元测试 + 21 个文档测试全部通过（新增 29 个）
- `cargo build --target wasm32-unknown-unknown` 与 `--target wasm32-wasip1` 均通过
- `cargo doc --no-deps` 无 secrets 相关告警
- `cargo clippy --all-targets`：secrets 模块 0 告警
  （仓库其它模块存量 136 处告警，本次未处理）
- KAT 向量来源：CBC / ECB 取自 `cbc` / `ecb` crate 自带 doctest 并用 LibreSSL
  `openssl enc` 独立复算逐字节核对；CTR 取自 NIST SP 800-38A F.5.1 并用 LibreSSL 复算；
  GCM 取自 `aes-gcm` crate 内置的 NIST CAVS `gcmEncryptExtIV128.rsp` 向量
- 未执行 rustfmt：仓库整体非 rustfmt 格式（存量 136 处差异），
  且 rustfmt 会把 test.rs 中多行书写的 NIST 向量压成单行，损害可读性

---

## 落地差异（2026-11-18 补记）

本文是 2026-09-11 的历史实现记录，上文关于 wasm 的描述保留原样，不再反映当前代码。

- 仓库已于 2026-11-18 彻底移除 wasm 支持：删除 `cdylib`、`src/wasm_exports/`
  及其 13 个 C-ABI 导出、`wasm_js` 相关 target 依赖。
- `secrets` / AES 实现本身不受影响，仍为纯 Rust；本文描述的 API、KAT 验证结论与限制继续有效。
- 若未来单独起 wasm 版本，随机数熵源与 C-ABI 契约需在新仓库中重新设计。

