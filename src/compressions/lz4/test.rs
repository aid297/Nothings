use crate::compressions::lz4::app::Lz4;

// ─────────────────────────────────────────────
// compress / decompress
// ─────────────────────────────────────────────

#[test]
fn test_compress_decompress() {
    let data = b"hello world, this is attr test for lz4 compression";
    let compressed = Lz4::compress(data).unwrap();
    let decompressed = Lz4::decompress(&compressed).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn test_compress_empty() {
    let data = b"";
    let compressed = Lz4::compress(data).unwrap();
    let decompressed = Lz4::decompress(&compressed).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn test_compress_large_data() {
    let data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
    let compressed = Lz4::compress(&data).unwrap();
    let decompressed = Lz4::decompress(&compressed).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn test_compress_repetitive_data() {
    let data = "abcdefghij".repeat(1000);
    let compressed = Lz4::compress(data.as_bytes()).unwrap();
    // 重复数据应该有较好的压缩率
    assert!(compressed.len() < data.len());
    let decompressed = Lz4::decompress(&compressed).unwrap();
    assert_eq!(decompressed, data.as_bytes());
}

#[test]
fn test_decompress_invalid_data() {
    let invalid = b"this is not lz4 compressed data";
    assert!(Lz4::decompress(invalid).is_err());
}

// ─────────────────────────────────────────────
// compress_with_level
// ─────────────────────────────────────────────

#[test]
fn test_compress_with_level_1() {
    let data = b"hello world, level 1 compression test";
    let compressed = Lz4::compress_with_level(data, 1).unwrap();
    let decompressed = Lz4::decompress(&compressed).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn test_compress_with_level_9() {
    let data = b"hello world, level 9 compression test with higher ratio";
    let compressed = Lz4::compress_with_level(data, 9).unwrap();
    let decompressed = Lz4::decompress(&compressed).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn test_compress_with_level_16() {
    let data = b"hello world, level 16 max compression test";
    let compressed = Lz4::compress_with_level(data, 16).unwrap();
    let decompressed = Lz4::decompress(&compressed).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn test_higher_level_better_compression() {
    // 完全规则的数据在任何 HC 级别下都已最优，必须用局部重复、整体不可预测的数据
    let data = pseudo_random_words(2000);
    let compressed_l1 = Lz4::compress_with_level(data.as_bytes(), 1).unwrap();
    let compressed_l16 = Lz4::compress_with_level(data.as_bytes(), 16).unwrap();
    // HC 级别必须真正影响输出：体积显著下降，且字节序列不同
    assert!(
        compressed_l16.len() < compressed_l1.len(),
        "level 16 应比 level 1 更小（实测 {} vs {}）",
        compressed_l16.len(),
        compressed_l1.len()
    );
    assert_ne!(
        compressed_l16, compressed_l1,
        "level 1 与 level 16 的压缩输出不应相同"
    );
}

/// 用线性同余伪随机选取词汇，生成「局部有重复、整体不规则」的测试数据（确定性）
fn pseudo_random_words(n: usize) -> String {
    let words = [
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india",
        "juliet",
    ];
    let mut seed = 12345678u64;
    let mut out = String::new();
    for _ in 0..n {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        out.push_str(words[(seed >> 33) as usize % words.len()]);
        out.push(' ');
    }
    out
}

#[test]
fn test_compress_with_level_produces_standard_frame() {
    // liblz4 HC 输出必须是标准 LZ4 frame，与 lz4_flex 解码器兼容
    let data = b"standard frame check with HC encoder".repeat(8);
    for level in [1u32, 9, 16, 100] {
        let compressed = Lz4::compress_with_level(&data, level).unwrap();
        assert_eq!(&compressed[..4], &[0x04, 0x22, 0x4d, 0x18]);
        // >16 钳制行为：不 panic 且可解压
        assert_eq!(Lz4::decompress(&compressed).unwrap(), data);
    }
}

// ─────────────────────────────────────────────
// compress_file / decompress_file
// ─────────────────────────────────────────────

#[test]
fn test_compress_decompress_file() {
    let input_path = "/tmp/lz4_test_input.txt";
    let compressed_path = "/tmp/lz4_test_compressed.lz4";
    let output_path = "/tmp/lz4_test_output.txt";

    let original = "hello world, file compression test!";
    std::fs::write(input_path, original).unwrap();

    Lz4::compress_file(input_path, compressed_path).unwrap();
    Lz4::decompress_file(compressed_path, output_path).unwrap();

    let result = std::fs::read_to_string(output_path).unwrap();
    assert_eq!(result, original);

    // 清理
    let _ = std::fs::remove_file(input_path);
    let _ = std::fs::remove_file(compressed_path);
    let _ = std::fs::remove_file(output_path);
}

#[test]
fn test_compress_file_with_level() {
    let input_path = "/tmp/lz4_test_level_input.txt";
    let compressed_path = "/tmp/lz4_test_level_compressed.lz4";
    let output_path = "/tmp/lz4_test_level_output.txt";

    let original = "hello world, file compression with level 12!";
    std::fs::write(input_path, original).unwrap();

    Lz4::compress_file_with_level(input_path, compressed_path, 12).unwrap();
    Lz4::decompress_file(compressed_path, output_path).unwrap();

    let result = std::fs::read_to_string(output_path).unwrap();
    assert_eq!(result, original);

    // 清理
    let _ = std::fs::remove_file(input_path);
    let _ = std::fs::remove_file(compressed_path);
    let _ = std::fs::remove_file(output_path);
}

#[test]
fn test_compress_file_not_found() {
    let result = Lz4::compress_file("/tmp/nonexistent_file_12345.txt", "/tmp/out.lz4");
    assert!(result.is_err());
}

#[test]
fn test_decompress_file_not_found() {
    let result = Lz4::decompress_file("/tmp/nonexistent_file_12345.lz4", "/tmp/out.txt");
    assert!(result.is_err());
}

// ─────────────────────────────────────────────
// compression_ratio
// ─────────────────────────────────────────────

#[test]
fn test_compression_ratio_empty() {
    let ratio = Lz4::compression_ratio(&[], &[1, 2, 3]);
    assert_eq!(ratio, 0.0);
}

#[test]
fn test_compression_ratio_normal() {
    let original = vec![0u8; 1000];
    let compressed = vec![0u8; 100];
    let ratio = Lz4::compression_ratio(&original, &compressed);
    assert!((ratio - 0.1).abs() < f64::EPSILON);
}

#[test]
fn test_compression_ratio_with_real_data() {
    let data = "abcdefghij".repeat(1000);
    let compressed = Lz4::compress(data.as_bytes()).unwrap();
    let ratio = Lz4::compression_ratio(data.as_bytes(), &compressed);
    // 重复数据压缩率应该 < 1.0
    assert!(ratio < 1.0);
}
