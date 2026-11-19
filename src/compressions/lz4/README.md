# Lz4 — LZ4 压缩/解压

`Lz4` 提供基于 LZ4 算法的高效压缩/解压功能，支持内存数据和文件压缩，输出标准 LZ4 frame 格式（与 liblz4 兼容）。压缩端基于 liblz4 HC（lz4 crate，C 源码内置于依赖），解压端基于 lz4_flex，两者 frame 互通。

## 快速开始

```rust
use nothings::compressions::lz4::app::Lz4;

let data = b"hello world, this is some test data";

// 默认压缩（级别 1，最快）
let compressed = Lz4::compress(data).unwrap();
let decompressed = Lz4::decompress(&compressed).unwrap();
assert_eq!(decompressed, data);

// 指定压缩级别（1-16 生效，HC 越高压缩率越好）
let compressed = Lz4::compress_with_level(data, 9).unwrap();

// 文件压缩/解压
Lz4::compress_file("input.txt", "output.lz4").unwrap();
Lz4::decompress_file("output.lz4", "output.txt").unwrap();

// 文件压缩（指定级别）
Lz4::compress_file_with_level("input.txt", "output.lz4", 12).unwrap();

// 计算压缩率
let ratio = Lz4::compression_ratio(data, &compressed);
println!("压缩率: {:.2}%", ratio * 100.0);
```

## API 一览

### 内存压缩/解压

| 方法 | 说明 |
|------|------|
| `compress(data)` | 压缩数据（默认级别 1），返回 `Result<Vec<u8>, Error>` |
| `compress_with_level(data, level)` | 指定压缩级别（1-16 有效，>16 钳为 16），返回 `Result<Vec<u8>, Error>` |
| `decompress(data)` | 解压数据，返回 `Result<Vec<u8>, Error>` |
| `compression_ratio(original, compressed)` | 计算压缩率（压缩后大小 / 原始大小），返回 `f64` |

### 文件压缩/解压

| 方法 | 说明 |
|------|------|
| `compress_file(input, output)` | 压缩文件（默认级别 1） |
| `compress_file_with_level(input, output, level)` | 指定压缩级别压缩文件 |
| `decompress_file(input, output)` | 解压文件 |

### 压缩级别说明

压缩端基于 liblz4 HC（`lz4` crate），级别语义：

- `0`：快速模式（默认 `compress()` 用级别 1，即 HC 最低档）
- `1-16`：HC（高压缩）模式，级别越高压缩率越好但越慢
- `>16`：钳制为 16，不会报错

解压端保持 lz4_flex 的 `FrameDecoder`，可读取 liblz4 HC 产出的标准 frame；反之亦可。
实测（局部重复、整体不规则的数据）级别 1 输出 5440 字节、级别 16 输出 2576 字节。
