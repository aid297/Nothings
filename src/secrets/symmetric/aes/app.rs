use crate::secrets::Symmetric;
use aes::cipher::{
    BlockModeDecrypt, BlockModeEncrypt, KeyInit, KeyIvInit, StreamCipher, block_padding::Pkcs7,
};
use aes_gcm::AesGcm;
use aes_gcm::aead::Aead;
use rand::Rng;
use std::io::{Error, ErrorKind};

/// AES 分组长度（字节），同时是 CBC / CTR 的 iv 长度
pub const BLOCK_LEN: usize = 16;

/// GCM 推荐 nonce 长度（字节）
pub const GCM_NONCE_LEN: usize = 12;

/// GCM 认证 tag 长度（字节），追加在密文尾部
pub const GCM_TAG_LEN: usize = 16;

/// AES-192-GCM
///
/// `aes-gcm` 只导出了 128 / 256 两个别名，这里按同样的形状补齐 192
type Aes192Gcm = AesGcm<aes::Aes192, aes::cipher::consts::U12>;

/// AES 加密模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AesMode {
    /// GCM 认证加密，密文尾部带 16 字节 tag，篡改可检测
    Gcm,
    /// CBC 密码分组链接，PKCS7 填充
    Cbc,
    /// CTR 计数器模式，无填充，密文与明文等长
    Ctr,
    /// ECB 电子密码本，PKCS7 填充，不使用 iv
    Ecb,
}

impl AesMode {
    /// 该模式所需的 iv / nonce 长度（字节），ECB 不使用 iv 故为 0
    pub fn iv_len(&self) -> usize {
        match self {
            AesMode::Gcm => GCM_NONCE_LEN,
            AesMode::Cbc | AesMode::Ctr => BLOCK_LEN,
            AesMode::Ecb => 0,
        }
    }
}

fn invalid_key_len(len: usize) -> Error {
    Error::new(
        ErrorKind::InvalidInput,
        format!("AES 密钥长度必须为 16 / 24 / 32 字节，实际为 {len}"),
    )
}

fn invalid_iv_len(mode: AesMode, len: usize) -> Error {
    let what = if mode == AesMode::Gcm { "nonce" } else { "iv" };
    Error::new(
        ErrorKind::InvalidInput,
        format!(
            "{mode:?} 模式 {what} 长度必须为 {} 字节，实际为 {len}",
            mode.iv_len()
        ),
    )
}

fn gcm_encrypt_failed() -> Error {
    Error::new(
        ErrorKind::InvalidInput,
        "GCM 加密失败：明文长度超过 GCM 单条消息上限",
    )
}

fn decrypt_failed(mode: AesMode) -> Error {
    Error::new(
        ErrorKind::InvalidData,
        match mode {
            AesMode::Gcm => "GCM 解密失败：密文被篡改、tag 校验不通过，或 key / nonce 不匹配",
            AesMode::Cbc | AesMode::Ecb => {
                "解密失败：PKCS7 填充非法，或密文长度不是 16 的倍数，或 key / iv 不匹配"
            }
            AesMode::Ctr => "CTR 解密失败：key / iv 不匹配",
        },
    )
}

// ─────────────────────────────────────────────
// 各模式的泛型实现，按密钥长度单态化
// ─────────────────────────────────────────────

fn gcm_encrypt_with<C>(key: &[u8], nonce: &[u8], data: &[u8]) -> Result<Vec<u8>, Error>
where
    C: Aead + KeyInit,
{
    let cipher = C::new_from_slice(key).map_err(|_| invalid_key_len(key.len()))?;
    let nonce = <&aes_gcm::aead::Nonce<C>>::try_from(nonce)
        .map_err(|_| invalid_iv_len(AesMode::Gcm, nonce.len()))?;
    cipher
        .encrypt(nonce, data)
        .map_err(|_| gcm_encrypt_failed())
}

fn gcm_decrypt_with<C>(key: &[u8], nonce: &[u8], data: &[u8]) -> Result<Vec<u8>, Error>
where
    C: Aead + KeyInit,
{
    let cipher = C::new_from_slice(key).map_err(|_| invalid_key_len(key.len()))?;
    let nonce = <&aes_gcm::aead::Nonce<C>>::try_from(nonce)
        .map_err(|_| invalid_iv_len(AesMode::Gcm, nonce.len()))?;
    cipher
        .decrypt(nonce, data)
        .map_err(|_| decrypt_failed(AesMode::Gcm))
}

fn cbc_encrypt_with<C>(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>, Error>
where
    C: BlockModeEncrypt + KeyIvInit,
{
    Ok(C::new_from_slices(key, iv)
        .map_err(|_| invalid_iv_len(AesMode::Cbc, iv.len()))?
        .encrypt_padded_vec::<Pkcs7>(data))
}

fn cbc_decrypt_with<C>(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>, Error>
where
    C: BlockModeDecrypt + KeyIvInit,
{
    C::new_from_slices(key, iv)
        .map_err(|_| invalid_iv_len(AesMode::Cbc, iv.len()))?
        .decrypt_padded_vec::<Pkcs7>(data)
        .map_err(|_| decrypt_failed(AesMode::Cbc))
}

fn ctr_apply_with<C>(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>, Error>
where
    C: StreamCipher + KeyIvInit,
{
    let mut cipher =
        C::new_from_slices(key, iv).map_err(|_| invalid_iv_len(AesMode::Ctr, iv.len()))?;
    let mut buf = data.to_vec();
    cipher.apply_keystream(&mut buf);
    Ok(buf)
}

fn ecb_encrypt_with<C>(key: &[u8], data: &[u8]) -> Result<Vec<u8>, Error>
where
    C: BlockModeEncrypt + KeyInit,
{
    Ok(C::new_from_slice(key)
        .map_err(|_| invalid_key_len(key.len()))?
        .encrypt_padded_vec::<Pkcs7>(data))
}

fn ecb_decrypt_with<C>(key: &[u8], data: &[u8]) -> Result<Vec<u8>, Error>
where
    C: BlockModeDecrypt + KeyInit,
{
    C::new_from_slice(key)
        .map_err(|_| invalid_key_len(key.len()))?
        .decrypt_padded_vec::<Pkcs7>(data)
        .map_err(|_| decrypt_failed(AesMode::Ecb))
}

/// AES 对称加密工具
///
/// 支持 GCM / CBC / CTR / ECB 四种模式与 128 / 192 / 256 位密钥，
/// 构造时持有 key 与 iv，之后可反复调用 [`Symmetric::encrypt`] / [`Symmetric::decrypt`]。
///
/// key 与 iv 的长度校验发生在加解密时而非构造时，失败返回 `std::io::Error`。
///
/// # 示例
/// ```
/// use nothings::secrets::symmetric::aes::app::Aes;
/// use nothings::secrets::Symmetric;
///
/// let cipher = Aes::gcm(&Aes::generate_key(256).unwrap(), &Aes::generate_nonce());
/// let encrypted = cipher.encrypt(b"hello world").unwrap();
/// assert_eq!(cipher.decrypt(&encrypted).unwrap(), b"hello world");
/// ```
pub struct Aes {
    mode: AesMode,
    key: Vec<u8>,
    iv: Vec<u8>,
}

impl Aes {
    /// 创建 AES-GCM 加密器
    ///
    /// - `key`: 16 / 24 / 32 字节
    /// - `nonce`: 12 字节，同一 key 下每条消息必须唯一，重复使用会破坏安全性
    ///
    /// 密文格式为「密文 ‖ 16 字节 tag」，解密时校验 tag，失败返回 `ErrorKind::InvalidData`
    ///
    /// # 示例
    /// ```
    /// use nothings::secrets::symmetric::aes::app::Aes;
    /// use nothings::secrets::Symmetric;
    ///
    /// let cipher = Aes::gcm(&[0x42; 32], &[0x24; 12]);
    /// let encrypted = cipher.encrypt(b"hello world").unwrap();
    /// assert_eq!(encrypted.len(), b"hello world".len() + 16);
    /// assert_eq!(cipher.decrypt(&encrypted).unwrap(), b"hello world");
    /// ```
    pub fn gcm(key: &[u8], nonce: &[u8]) -> Self {
        Aes {
            mode: AesMode::Gcm,
            key: key.to_vec(),
            iv: nonce.to_vec(),
        }
    }

    /// 创建 AES-CBC 加密器（PKCS7 填充）
    ///
    /// - `key`: 16 / 24 / 32 字节
    /// - `iv`: 16 字节
    ///
    /// # 示例
    /// ```
    /// use nothings::secrets::symmetric::aes::app::Aes;
    /// use nothings::secrets::Symmetric;
    ///
    /// let cipher = Aes::cbc(&[0x42; 16], &[0x24; 16]);
    /// let encrypted = cipher.encrypt(b"hello world").unwrap();
    /// assert_eq!(cipher.decrypt(&encrypted).unwrap(), b"hello world");
    /// ```
    pub fn cbc(key: &[u8], iv: &[u8]) -> Self {
        Aes {
            mode: AesMode::Cbc,
            key: key.to_vec(),
            iv: iv.to_vec(),
        }
    }

    /// 创建 AES-CTR 加密器（无填充，密文与明文等长）
    ///
    /// 计数器为 16 字节大端自增，与 OpenSSL、Java `AES/CTR/NoPadding` 的默认行为一致
    ///
    /// - `key`: 16 / 24 / 32 字节
    /// - `iv`: 16 字节初始计数器
    ///
    /// # 示例
    /// ```
    /// use nothings::secrets::symmetric::aes::app::Aes;
    /// use nothings::secrets::Symmetric;
    ///
    /// let cipher = Aes::ctr(&[0x42; 16], &[0x24; 16]);
    /// let encrypted = cipher.encrypt(b"hello world").unwrap();
    /// assert_eq!(encrypted.len(), b"hello world".len());
    /// assert_eq!(cipher.decrypt(&encrypted).unwrap(), b"hello world");
    /// ```
    pub fn ctr(key: &[u8], iv: &[u8]) -> Self {
        Aes {
            mode: AesMode::Ctr,
            key: key.to_vec(),
            iv: iv.to_vec(),
        }
    }

    /// 创建 AES-ECB 加密器（PKCS7 填充，不使用 iv）
    ///
    /// ECB 模式下相同明文块产出相同密文块，会泄露数据模式，
    /// 仅在需要与既有系统互通时使用
    ///
    /// - `key`: 16 / 24 / 32 字节
    ///
    /// # 示例
    /// ```
    /// use nothings::secrets::symmetric::aes::app::Aes;
    /// use nothings::secrets::Symmetric;
    ///
    /// let cipher = Aes::ecb(&[0x42; 16]);
    /// let encrypted = cipher.encrypt(b"hello world").unwrap();
    /// assert_eq!(cipher.decrypt(&encrypted).unwrap(), b"hello world");
    /// ```
    pub fn ecb(key: &[u8]) -> Self {
        Aes {
            mode: AesMode::Ecb,
            key: key.to_vec(),
            iv: Vec::new(),
        }
    }

    /// 获取当前加密模式
    pub fn mode(&self) -> AesMode {
        self.mode
    }

    /// 替换密钥
    ///
    /// # 示例
    /// ```
    /// use nothings::secrets::symmetric::aes::app::Aes;
    ///
    /// let mut cipher = Aes::cbc(&[0x42; 16], &[0x24; 16]);
    /// cipher.set_key(&[0x11; 32]).set_iv(&[0x22; 16]);
    /// assert_eq!(cipher.mode().iv_len(), 16);
    /// ```
    pub fn set_key(&mut self, key: &[u8]) -> &mut Self {
        self.key = key.to_vec();
        self
    }

    /// 替换 iv（GCM 模式下即 nonce）
    pub fn set_iv(&mut self, iv: &[u8]) -> &mut Self {
        self.iv = iv.to_vec();
        self
    }

    /// 生成随机密钥
    ///
    /// # 参数
    /// - `bits`: 密钥位数，取 128 / 192 / 256
    ///
    /// # 错误
    /// `bits` 不是上述取值时返回 `ErrorKind::InvalidInput`
    ///
    /// # 示例
    /// ```
    /// use nothings::secrets::symmetric::aes::app::Aes;
    ///
    /// assert_eq!(Aes::generate_key(128).unwrap().len(), 16);
    /// assert_eq!(Aes::generate_key(192).unwrap().len(), 24);
    /// assert_eq!(Aes::generate_key(256).unwrap().len(), 32);
    /// assert!(Aes::generate_key(64).is_err());
    /// ```
    pub fn generate_key(bits: usize) -> Result<Vec<u8>, Error> {
        let len = match bits {
            128 => 16,
            192 => 24,
            256 => 32,
            _ => {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    format!("AES 密钥位数必须为 128 / 192 / 256，实际为 {bits}"),
                ));
            }
        };
        Ok(random_bytes(len))
    }

    /// 生成 GCM 随机 nonce（12 字节）
    ///
    /// # 示例
    /// ```
    /// use nothings::secrets::symmetric::aes::app::Aes;
    ///
    /// assert_eq!(Aes::generate_nonce().len(), 12);
    /// ```
    pub fn generate_nonce() -> Vec<u8> {
        random_bytes(GCM_NONCE_LEN)
    }

    /// 生成 CBC / CTR 随机 iv（16 字节）
    ///
    /// # 示例
    /// ```
    /// use nothings::secrets::symmetric::aes::app::Aes;
    ///
    /// assert_eq!(Aes::generate_iv().len(), 16);
    /// ```
    pub fn generate_iv() -> Vec<u8> {
        random_bytes(BLOCK_LEN)
    }
}

fn random_bytes(len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    rand::rng().fill_bytes(&mut buf);
    buf
}

impl Symmetric for Aes {
    fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>, Error> {
        match self.mode {
            AesMode::Gcm => match self.key.len() {
                16 => gcm_encrypt_with::<aes_gcm::Aes128Gcm>(&self.key, &self.iv, data),
                24 => gcm_encrypt_with::<Aes192Gcm>(&self.key, &self.iv, data),
                32 => gcm_encrypt_with::<aes_gcm::Aes256Gcm>(&self.key, &self.iv, data),
                n => Err(invalid_key_len(n)),
            },
            AesMode::Cbc => match self.key.len() {
                16 => cbc_encrypt_with::<cbc::Encryptor<aes::Aes128>>(&self.key, &self.iv, data),
                24 => cbc_encrypt_with::<cbc::Encryptor<aes::Aes192>>(&self.key, &self.iv, data),
                32 => cbc_encrypt_with::<cbc::Encryptor<aes::Aes256>>(&self.key, &self.iv, data),
                n => Err(invalid_key_len(n)),
            },
            AesMode::Ctr => match self.key.len() {
                16 => ctr_apply_with::<ctr::Ctr128BE<aes::Aes128>>(&self.key, &self.iv, data),
                24 => ctr_apply_with::<ctr::Ctr128BE<aes::Aes192>>(&self.key, &self.iv, data),
                32 => ctr_apply_with::<ctr::Ctr128BE<aes::Aes256>>(&self.key, &self.iv, data),
                n => Err(invalid_key_len(n)),
            },
            AesMode::Ecb => match self.key.len() {
                16 => ecb_encrypt_with::<ecb::Encryptor<aes::Aes128>>(&self.key, data),
                24 => ecb_encrypt_with::<ecb::Encryptor<aes::Aes192>>(&self.key, data),
                32 => ecb_encrypt_with::<ecb::Encryptor<aes::Aes256>>(&self.key, data),
                n => Err(invalid_key_len(n)),
            },
        }
    }

    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, Error> {
        match self.mode {
            AesMode::Gcm => match self.key.len() {
                16 => gcm_decrypt_with::<aes_gcm::Aes128Gcm>(&self.key, &self.iv, data),
                24 => gcm_decrypt_with::<Aes192Gcm>(&self.key, &self.iv, data),
                32 => gcm_decrypt_with::<aes_gcm::Aes256Gcm>(&self.key, &self.iv, data),
                n => Err(invalid_key_len(n)),
            },
            AesMode::Cbc => match self.key.len() {
                16 => cbc_decrypt_with::<cbc::Decryptor<aes::Aes128>>(&self.key, &self.iv, data),
                24 => cbc_decrypt_with::<cbc::Decryptor<aes::Aes192>>(&self.key, &self.iv, data),
                32 => cbc_decrypt_with::<cbc::Decryptor<aes::Aes256>>(&self.key, &self.iv, data),
                n => Err(invalid_key_len(n)),
            },
            // CTR 加解密同构，均为异或密钥流
            AesMode::Ctr => self.encrypt(data),
            AesMode::Ecb => match self.key.len() {
                16 => ecb_decrypt_with::<ecb::Decryptor<aes::Aes128>>(&self.key, data),
                24 => ecb_decrypt_with::<ecb::Decryptor<aes::Aes192>>(&self.key, data),
                32 => ecb_decrypt_with::<ecb::Decryptor<aes::Aes256>>(&self.key, data),
                n => Err(invalid_key_len(n)),
            },
        }
    }
}
