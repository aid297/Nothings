pub mod symmetric;

use std::io::Error;

/// 统一对称加密接口
///
/// 所有对称加密算法（AES、后续的 SM4 等）都实现该 trait，
/// 便于把加密算法作为参数传递、或在运行时切换算法。
///
/// # 示例
/// ```
/// use nothings::secrets::{with_aes_cbc, Symmetric};
///
/// let key = [0x42u8; 32];
/// let iv = [0x24u8; 16];
/// let cipher = with_aes_cbc(&key, &iv);
///
/// let encrypted = cipher.encrypt(b"hello world").unwrap();
/// let decrypted = cipher.decrypt(&encrypted).unwrap();
/// assert_eq!(decrypted, b"hello world");
/// ```
pub trait Symmetric {
    /// 加密数据，返回密文
    fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>, Error>;

    /// 解密密文，返回明文
    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, Error>;
}

/// 创建 AES-GCM 加密器（认证加密，推荐）
///
/// - `key`: 16 / 24 / 32 字节，对应 AES-128 / 192 / 256
/// - `nonce`: 12 字节，每条消息必须唯一
pub fn with_aes_gcm(key: &[u8], nonce: &[u8]) -> Box<dyn Symmetric> {
    Box::new(symmetric::aes::app::Aes::gcm(key, nonce))
}

/// 创建 AES-CBC 加密器（PKCS7 填充）
///
/// - `key`: 16 / 24 / 32 字节
/// - `iv`: 16 字节
pub fn with_aes_cbc(key: &[u8], iv: &[u8]) -> Box<dyn Symmetric> {
    Box::new(symmetric::aes::app::Aes::cbc(key, iv))
}

/// 创建 AES-CTR 加密器（无填充，密文与明文等长）
///
/// - `key`: 16 / 24 / 32 字节
/// - `iv`: 16 字节初始计数器
pub fn with_aes_ctr(key: &[u8], iv: &[u8]) -> Box<dyn Symmetric> {
    Box::new(symmetric::aes::app::Aes::ctr(key, iv))
}

/// 创建 AES-ECB 加密器（PKCS7 填充，不使用 iv）
///
/// ECB 模式下相同明文块产出相同密文块，会泄露数据模式，
/// 仅在需要与既有系统互通时使用。
///
/// - `key`: 16 / 24 / 32 字节
pub fn with_aes_ecb(key: &[u8]) -> Box<dyn Symmetric> {
    Box::new(symmetric::aes::app::Aes::ecb(key))
}
