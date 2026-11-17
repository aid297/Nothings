use crate::secrets::symmetric::aes::app::{
    Aes, AesMode, BLOCK_LEN, GCM_NONCE_LEN, GCM_TAG_LEN,
};
use crate::secrets::Symmetric;
use std::io::ErrorKind;

/// 解析十六进制字符串（忽略空白），避免为测试引入额外依赖
fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(s.len().is_multiple_of(2), "hex 字符串长度必须为偶数");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn cipher_for(mode: AesMode, key: &[u8], iv: &[u8]) -> Aes {
    match mode {
        AesMode::Gcm => Aes::gcm(key, iv),
        AesMode::Cbc => Aes::cbc(key, iv),
        AesMode::Ctr => Aes::ctr(key, iv),
        AesMode::Ecb => Aes::ecb(key),
    }
}

const ALL_MODES: [AesMode; 4] = [AesMode::Gcm, AesMode::Cbc, AesMode::Ctr, AesMode::Ecb];
const ALL_KEY_SIZES: [usize; 3] = [16, 24, 32];

fn fixtures() -> Vec<Vec<u8>> {
    vec![
        vec![],
        vec![0x61],
        vec![0x61; 15],
        vec![0x61; BLOCK_LEN],
        vec![0x61; BLOCK_LEN + 1],
        (0..10_000).map(|i| (i % 251) as u8).collect(),
    ]
}

// ─────────────────────────────────────────────
// 已知答案向量（KAT）
// round-trip 测不出填充方式、计数器字节序、tag 位置写错，必须靠 KAT
// ─────────────────────────────────────────────

/// NIST SP 800-38A F.5.1 CTR-AES128，同时验证选用的是 Ctr128BE（大端全 128 位计数器）
#[test]
fn test_kat_aes128_ctr() {
    let key = hex("2b7e151628aed2a6abf7158809cf4f3c");
    let iv = hex("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff");
    let plaintext = hex(
        "6bc1bee22e409f96e93d7e117393172a
         ae2d8a571e03ac9c9eb76fac45af8e51
         30c81c46a35ce411e5fbc1191a0a52ef
         f69f2445df4f9b17ad2b417be66c3710",
    );
    let expected = hex(
        "874d6191b620e3261bef6864990db6ce
         9806f66b7970fdff8617187bb9fffdff
         5ae4df3edbd5d35e5b4f09020db03eab
         1e031dda2fbe03d1792170a0f3009cee",
    );

    let cipher = Aes::ctr(&key, &iv);
    assert_eq!(cipher.encrypt(&plaintext).unwrap(), expected);
    assert_eq!(cipher.decrypt(&expected).unwrap(), plaintext);
}

/// NIST CAVS gcmEncryptExtIV128（AAD 为空），输出为「密文 ‖ 16 字节 tag」
#[test]
fn test_kat_aes128_gcm() {
    let key = hex("7fddb57453c241d03efbed3ac44e371c");
    let nonce = hex("ee283a3fc75575e33efd4887");
    let plaintext = hex("d5de42b461646c255c87bd2962d3b9a2");
    let expected = hex(
        "2ccda4a5415cb91e135c2a0f78c9b2fd
         b36d1df9b9d5e596f83e8b7f52971cb3",
    );

    let cipher = Aes::gcm(&key, &nonce);
    assert_eq!(cipher.encrypt(&plaintext).unwrap(), expected);
    assert_eq!(cipher.decrypt(&expected).unwrap(), plaintext);
}

#[test]
fn test_kat_aes256_gcm() {
    let key = hex("31bdadd96698c204aa9ce1448ea94ae1fb4a9a0b3c9d773b51bb1822666b8f22");
    let nonce = hex("0d18e06c7c725ac9e362e1ce");
    let plaintext = hex("2db5168e932556f8089a0622981d017d");
    let expected = hex(
        "fa4362189661d163fcd6a56d8bf0405a
         d636ac1bbedd5cc3ee727dc2ab4a9489",
    );

    let cipher = Aes::gcm(&key, &nonce);
    assert_eq!(cipher.encrypt(&plaintext).unwrap(), expected);
    assert_eq!(cipher.decrypt(&expected).unwrap(), plaintext);
}

/// PKCS7 填充结果已与 LibreSSL `openssl enc -aes-128-cbc` 交叉核对
#[test]
fn test_kat_aes128_cbc() {
    let key = [0x42u8; 16];
    let iv = [0x24u8; 16];
    let plaintext = b"hello world! this is my plaintext.";
    let expected = hex(
        "c7fe247ef97b21f07cbdd26cb5d346bf
         d27867cb00d9486723e159978fb9a5f9
         14cfb228a710de4171e396e7b6cf859e",
    );

    let cipher = Aes::cbc(&key, &iv);
    assert_eq!(cipher.encrypt(plaintext).unwrap(), expected);
    assert_eq!(cipher.decrypt(&expected).unwrap(), plaintext);
}

/// PKCS7 填充结果已与 LibreSSL `openssl enc -aes-128-ecb` 交叉核对
#[test]
fn test_kat_aes128_ecb() {
    let key = [0x42u8; 16];
    let plaintext = b"hello world! this is my plaintext.";
    let expected = hex(
        "42b153410851a931eb3e6c048867ae5f
         95eb20b42e176b07840db75688be9c70
         e4670ea0d87a71be5f9f3099b4fff3dc",
    );

    let cipher = Aes::ecb(&key);
    assert_eq!(cipher.encrypt(plaintext).unwrap(), expected);
    assert_eq!(cipher.decrypt(&expected).unwrap(), plaintext);
}

// ─────────────────────────────────────────────
// round-trip 矩阵
// ─────────────────────────────────────────────

#[test]
fn test_roundtrip_all_modes_and_key_sizes() {
    for mode in ALL_MODES {
        for key_len in ALL_KEY_SIZES {
            let key = vec![0x42; key_len];
            let iv = vec![0x24; mode.iv_len()];
            let cipher = cipher_for(mode, &key, &iv);
            for data in fixtures() {
                let ctx = format!("{mode:?} key_len={key_len} data_len={}", data.len());
                let encrypted = cipher
                    .encrypt(&data)
                    .unwrap_or_else(|e| panic!("{ctx}: encrypt 失败 {e}"));
                let decrypted = cipher
                    .decrypt(&encrypted)
                    .unwrap_or_else(|e| panic!("{ctx}: decrypt 失败 {e}"));
                assert_eq!(decrypted, data, "{ctx}: round-trip 不一致");
            }
        }
    }
}

#[test]
fn test_ciphertext_length_by_mode() {
    let data = vec![0x61; 20];
    let key = vec![0x42; 32];

    let gcm = Aes::gcm(&key, &[0x24; GCM_NONCE_LEN]).encrypt(&data).unwrap();
    assert_eq!(gcm.len(), data.len() + GCM_TAG_LEN);

    let ctr = Aes::ctr(&key, &[0x24; BLOCK_LEN]).encrypt(&data).unwrap();
    assert_eq!(ctr.len(), data.len());

    for mode in [AesMode::Cbc, AesMode::Ecb] {
        let iv = vec![0x24; mode.iv_len()];
        let ct = cipher_for(mode, &key, &iv).encrypt(&data).unwrap();
        assert_eq!(ct.len(), 32, "{mode:?} 应向上补齐到 16 的倍数");
    }

    // PKCS7：明文恰好块对齐时补一整块
    let aligned = vec![0x61; BLOCK_LEN];
    let ct = Aes::cbc(&key, &[0x24; BLOCK_LEN]).encrypt(&aligned).unwrap();
    assert_eq!(ct.len(), BLOCK_LEN * 2);
}

#[test]
fn test_empty_plaintext_is_not_noop() {
    for mode in ALL_MODES {
        let key = vec![0x42; 16];
        let iv = vec![0x24; mode.iv_len()];
        let cipher = cipher_for(mode, &key, &iv);
        let encrypted = cipher.encrypt(b"").unwrap();
        // GCM 只有 tag，CBC/ECB 补一整块，CTR 为空
        let expected = match mode {
            AesMode::Gcm => GCM_TAG_LEN,
            AesMode::Ctr => 0,
            AesMode::Cbc | AesMode::Ecb => BLOCK_LEN,
        };
        assert_eq!(encrypted.len(), expected, "{mode:?} 空明文长度不符");
        assert_eq!(cipher.decrypt(&encrypted).unwrap(), b"");
    }
}

// ─────────────────────────────────────────────
// 非法输入
// ─────────────────────────────────────────────

#[test]
fn test_invalid_key_length() {
    for mode in ALL_MODES {
        for bad_len in [0usize, 1, 15, 17, 20, 31, 33, 64] {
            let key = vec![0x42; bad_len];
            let iv = vec![0x24; 16];
            let cipher = cipher_for(mode, &key, &iv);
            let err = cipher
                .encrypt(b"data")
                .expect_err(&format!("{mode:?} key_len={bad_len} 应当报错"));
            assert_eq!(err.kind(), ErrorKind::InvalidInput, "{mode:?} {bad_len}");

            let err = cipher
                .decrypt(b"data")
                .expect_err(&format!("{mode:?} key_len={bad_len} decrypt 应当报错"));
            assert_eq!(err.kind(), ErrorKind::InvalidInput, "{mode:?} {bad_len}");
        }
    }
}

#[test]
fn test_invalid_iv_length() {
    let key = vec![0x42; 32];
    for mode in [AesMode::Gcm, AesMode::Cbc, AesMode::Ctr] {
        for bad_len in [0usize, 1, 11, 13, 15, 17, 32] {
            let iv = vec![0x24; bad_len];
            let cipher = cipher_for(mode, &key, &iv);
            let err = cipher
                .encrypt(b"data")
                .expect_err(&format!("{mode:?} iv_len={bad_len} 应当报错"));
            assert_eq!(err.kind(), ErrorKind::InvalidInput, "{mode:?} iv_len={bad_len}");
        }
    }
}

#[test]
fn test_generate_key_invalid_bits() {
    for bits in [0usize, 64, 127, 129, 512] {
        assert_eq!(Aes::generate_key(bits).unwrap_err().kind(), ErrorKind::InvalidInput);
    }
}

// ─────────────────────────────────────────────
// 解密失败
// ─────────────────────────────────────────────

#[test]
fn test_gcm_detects_tampering() {
    let cipher = Aes::gcm(&[0x42; 32], &[0x24; GCM_NONCE_LEN]);
    let mut encrypted = cipher.encrypt(b"top secret payload").unwrap();

    for idx in [0usize, encrypted.len() / 2, encrypted.len() - 1] {
        let mut tampered = encrypted.clone();
        tampered[idx] ^= 0x01;
        let err = cipher.decrypt(&tampered).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidData, "翻转第 {idx} 字节应被检出");
    }

    // 截断 tag
    encrypted.truncate(encrypted.len() - 1);
    assert_eq!(cipher.decrypt(&encrypted).unwrap_err().kind(), ErrorKind::InvalidData);
}

#[test]
fn test_gcm_wrong_key_or_nonce_fails() {
    let nonce = vec![0x24; GCM_NONCE_LEN];
    let encrypted = Aes::gcm(&[0x42; 32], &nonce).encrypt(b"payload").unwrap();

    let wrong_key = Aes::gcm(&[0x43; 32], &nonce);
    assert_eq!(wrong_key.decrypt(&encrypted).unwrap_err().kind(), ErrorKind::InvalidData);

    let wrong_nonce = Aes::gcm(&[0x42; 32], &[0x25; GCM_NONCE_LEN]);
    assert_eq!(wrong_nonce.decrypt(&encrypted).unwrap_err().kind(), ErrorKind::InvalidData);
}

#[test]
fn test_cbc_rejects_malformed_ciphertext() {
    let cipher = Aes::cbc(&[0x42; 16], &[0x24; BLOCK_LEN]);

    // 长度不是 16 的倍数
    assert_eq!(
        cipher.decrypt(&[0u8; 17]).unwrap_err().kind(),
        ErrorKind::InvalidData
    );

    // 全零密文解密后填充几乎必然非法
    assert_eq!(
        cipher.decrypt(&[0u8; 32]).unwrap_err().kind(),
        ErrorKind::InvalidData
    );

    // ECB 同理
    let ecb = Aes::ecb(&[0x42; 16]);
    assert_eq!(ecb.decrypt(&[0u8; 15]).unwrap_err().kind(), ErrorKind::InvalidData);
}

#[test]
fn test_ctr_never_fails_on_arbitrary_bytes() {
    // CTR 是纯异或密钥流，无填充、无 tag，任意长度输入都不会报错
    let cipher = Aes::ctr(&[0x42; 16], &[0x24; BLOCK_LEN]);
    let out = cipher.decrypt(&hex("00ff10")).unwrap();
    assert_eq!(out.len(), 3);
}

// ─────────────────────────────────────────────
// 确定性与模式隔离
// ─────────────────────────────────────────────

#[test]
fn test_same_params_produce_same_ciphertext() {
    for mode in ALL_MODES {
        let key = vec![0x42; 32];
        let iv = vec![0x24; mode.iv_len()];
        let a = cipher_for(mode, &key, &iv).encrypt(b"payload").unwrap();
        let b = cipher_for(mode, &key, &iv).encrypt(b"payload").unwrap();
        assert_eq!(a, b, "{mode:?} 应当是确定性的");
    }
}

#[test]
fn test_different_iv_produces_different_ciphertext() {
    for mode in [AesMode::Gcm, AesMode::Cbc, AesMode::Ctr] {
        let key = vec![0x42; 32];
        let iv_a = vec![0x24; mode.iv_len()];
        let iv_b = vec![0x25; mode.iv_len()];
        let a = cipher_for(mode, &key, &iv_a)
            .encrypt(b"payload")
            .unwrap();
        let b = cipher_for(mode, &key, &iv_b)
            .encrypt(b"payload")
            .unwrap();
        assert_ne!(a, b, "{mode:?} 换 iv 后密文必须不同");
    }
}

#[test]
fn test_modes_produce_different_ciphertext() {
    let key = vec![0x42; 16];
    let iv = vec![0x24; BLOCK_LEN];
    let plaintext = b"the same plaintext for every mode";

    let cbc = Aes::cbc(&key, &iv).encrypt(plaintext).unwrap();
    let ctr = Aes::ctr(&key, &iv).encrypt(plaintext).unwrap();
    let ecb = Aes::ecb(&key).encrypt(plaintext).unwrap();
    let gcm = Aes::gcm(&key, &iv[..GCM_NONCE_LEN]).encrypt(plaintext).unwrap();

    assert_ne!(cbc, ctr);
    assert_ne!(cbc, ecb);
    assert_ne!(ctr, ecb);
    assert_ne!(gcm, ctr);
}

#[test]
fn test_cross_mode_decrypt_does_not_yield_plaintext() {
    let key = vec![0x42; 16];
    let iv = vec![0x24; BLOCK_LEN];
    let plaintext = b"the same plaintext for every mode";

    let cbc_ct = Aes::cbc(&key, &iv).encrypt(plaintext).unwrap();
    let ecb = Aes::ecb(&key);
    match ecb.decrypt(&cbc_ct) {
        Ok(out) => assert_ne!(out, plaintext, "用 ECB 解 CBC 密文不应得到正确明文"),
        Err(e) => assert_eq!(e.kind(), ErrorKind::InvalidData),
    }
}

/// ECB 的已知弱点：相同明文块产出相同密文块。此处固化行为，提醒调用方谨慎选用
#[test]
fn test_ecb_repeats_identical_blocks() {
    let cipher = Aes::ecb(&[0x42; 16]);
    let encrypted = cipher.encrypt(&[0x61; BLOCK_LEN * 2]).unwrap();
    assert_eq!(&encrypted[..BLOCK_LEN], &encrypted[BLOCK_LEN..BLOCK_LEN * 2]);
}

// ─────────────────────────────────────────────
// 构造、setter 与随机生成
// ─────────────────────────────────────────────

#[test]
fn test_mode_and_iv_len() {
    assert_eq!(Aes::gcm(&[], &[]).mode(), AesMode::Gcm);
    assert_eq!(Aes::cbc(&[], &[]).mode(), AesMode::Cbc);
    assert_eq!(Aes::ctr(&[], &[]).mode(), AesMode::Ctr);
    assert_eq!(Aes::ecb(&[]).mode(), AesMode::Ecb);

    assert_eq!(AesMode::Gcm.iv_len(), GCM_NONCE_LEN);
    assert_eq!(AesMode::Cbc.iv_len(), BLOCK_LEN);
    assert_eq!(AesMode::Ctr.iv_len(), BLOCK_LEN);
    assert_eq!(AesMode::Ecb.iv_len(), 0);
}

#[test]
fn test_constructor_does_not_validate() {
    // 构造函数不落错，非法长度延迟到加解密时才报（与 Lz4::with_level / Bcrypt::new 一致）
    let cipher = Aes::cbc(&[1, 2, 3], &[4, 5]);
    assert_eq!(cipher.mode(), AesMode::Cbc);
    assert_eq!(cipher.encrypt(b"x").unwrap_err().kind(), ErrorKind::InvalidInput);
}

#[test]
fn test_set_key_and_set_iv_chain() {
    let mut cipher = Aes::cbc(&[0x42; 16], &[0x24; BLOCK_LEN]);
    cipher.set_key(&[0x11; 32]).set_iv(&[0x22; BLOCK_LEN]);

    let expected = Aes::cbc(&[0x11; 32], &[0x22; BLOCK_LEN])
        .encrypt(b"payload")
        .unwrap();
    assert_eq!(cipher.encrypt(b"payload").unwrap(), expected);
}

#[test]
fn test_set_key_switches_key_size() {
    let mut cipher = Aes::gcm(&[0x42; 16], &[0x24; GCM_NONCE_LEN]);
    let encrypted = cipher.encrypt(b"payload").unwrap();
    assert!(!encrypted.is_empty());

    // 换成 256 位密钥后，旧密文必须解不开
    cipher.set_key(&[0x42; 32]);
    assert_eq!(cipher.decrypt(&encrypted).unwrap_err().kind(), ErrorKind::InvalidData);
}

#[test]
fn test_generate_key_lengths() {
    assert_eq!(Aes::generate_key(128).unwrap().len(), 16);
    assert_eq!(Aes::generate_key(192).unwrap().len(), 24);
    assert_eq!(Aes::generate_key(256).unwrap().len(), 32);
}

#[test]
fn test_generate_is_random() {
    assert_ne!(Aes::generate_nonce(), Aes::generate_nonce());
    assert_ne!(Aes::generate_iv(), Aes::generate_iv());
    assert_ne!(Aes::generate_key(256).unwrap(), Aes::generate_key(256).unwrap());

    assert_eq!(Aes::generate_nonce().len(), GCM_NONCE_LEN);
    assert_eq!(Aes::generate_iv().len(), BLOCK_LEN);
}

#[test]
fn test_generated_material_roundtrips() {
    for mode in ALL_MODES {
        let key = Aes::generate_key(256).unwrap();
        let iv = match mode {
            AesMode::Gcm => Aes::generate_nonce(),
            AesMode::Ecb => vec![],
            _ => Aes::generate_iv(),
        };
        let cipher = cipher_for(mode, &key, &iv);
        let encrypted = cipher.encrypt(b"generated key material").unwrap();
        assert_eq!(cipher.decrypt(&encrypted).unwrap(), b"generated key material");
    }
}

// ─────────────────────────────────────────────
// 工厂函数
// ─────────────────────────────────────────────

#[test]
fn test_factory_functions() {
    use crate::secrets::{with_aes_cbc, with_aes_ctr, with_aes_ecb, with_aes_gcm};

    let key = vec![0x42; 24];
    let cases: Vec<Box<dyn Symmetric>> = vec![
        with_aes_gcm(&key, &[0x24; GCM_NONCE_LEN]),
        with_aes_cbc(&key, &[0x24; BLOCK_LEN]),
        with_aes_ctr(&key, &[0x24; BLOCK_LEN]),
        with_aes_ecb(&key),
    ];

    for cipher in cases {
        let encrypted = cipher.encrypt(b"via factory").unwrap();
        assert_eq!(cipher.decrypt(&encrypted).unwrap(), b"via factory");
    }
}

#[test]
fn test_factory_matches_direct_construction() {
    use crate::secrets::with_aes_cbc;

    let key = vec![0x42; 16];
    let iv = vec![0x24; BLOCK_LEN];
    assert_eq!(
        with_aes_cbc(&key, &iv).encrypt(b"payload").unwrap(),
        Aes::cbc(&key, &iv).encrypt(b"payload").unwrap()
    );
}
