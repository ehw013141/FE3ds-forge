use crate::error::{CoreError, CoreResult};

const MAX_WINDOW: usize = 4096;
const MAX_MATCH: usize = 0x10110;
const HASH_BITS: usize = 15;
const HASH_SIZE: usize = 1 << HASH_BITS;
const HASH_MASK: usize = HASH_SIZE - 1;
const NIL: u32 = u32::MAX;

#[inline]
fn hash3(data: &[u8], pos: usize) -> usize {
    (((data[pos] as usize) << 10) ^ ((data[pos + 1] as usize) << 5) ^ (data[pos + 2] as usize))
        & HASH_MASK
}

/// 解压 3DS 火焰纹章的 `.bin.lz` 文件。
/// 返回 `(解压后的原始字节, 是否包含 FE14/FE15 的 0x13 外层封装头)`。
pub fn decompress_lz11(data: &[u8]) -> CoreResult<(Vec<u8>, bool)> {
    if data.len() < 4 {
        return Err(CoreError::InvalidLz11("数据长度不足 4 字节".into()));
    }

    let mut has_lz13_wrapper = false;
    let mut pos = 0usize;

    if data[0] == 0x13 && data.len() >= 8 && data[4] == 0x11 {
        has_lz13_wrapper = true;
        pos = 4;
    }

    if data[pos] != 0x11 {
        return Err(CoreError::InvalidLz11(format!(
            "不支持的压缩魔数: 0x{:02X}（期望 0x11）",
            data[pos]
        )));
    }

    let mut decomp_size = (data[pos + 1] as usize)
        | ((data[pos + 2] as usize) << 8)
        | ((data[pos + 3] as usize) << 16);
    pos += 4;

    if decomp_size == 0 {
        if pos + 4 > data.len() {
            return Err(CoreError::InvalidLz11("扩展长度字段截断".into()));
        }
        decomp_size = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
            as usize;
        pos += 4;
    }

    let mut out = Vec::with_capacity(decomp_size);

    while out.len() < decomp_size && pos < data.len() {
        let flags = data[pos];
        pos += 1;

        for bit in (0..8).rev() {
            if out.len() >= decomp_size || pos >= data.len() {
                break;
            }

            if (flags & (1 << bit)) == 0 {
                out.push(data[pos]);
                pos += 1;
            } else {
                if pos + 2 > data.len() {
                    return Err(CoreError::InvalidLz11("压缩引用指令截断 (2B)".into()));
                }
                let b1 = data[pos];
                let b2 = data[pos + 1];
                let indicator = b1 >> 4;

                let (length, disp) = match indicator {
                    0 => {
                        if pos + 3 > data.len() {
                            return Err(CoreError::InvalidLz11("压缩引用指令截断 (3B)".into()));
                        }
                        let b3 = data[pos + 2];
                        let len = ((((b1 & 0x0F) as usize) << 4) | ((b2 >> 4) as usize)) + 0x11;
                        let d = ((((b2 & 0x0F) as usize) << 8) | (b3 as usize)) + 1;
                        pos += 3;
                        (len, d)
                    }
                    1 => {
                        if pos + 4 > data.len() {
                            return Err(CoreError::InvalidLz11("压缩引用指令截断 (4B)".into()));
                        }
                        let b3 = data[pos + 2];
                        let b4 = data[pos + 3];
                        let len = ((((b1 & 0x0F) as usize) << 12)
                            | ((b2 as usize) << 4)
                            | ((b3 >> 4) as usize))
                            + 0x111;
                        let d = ((((b3 & 0x0F) as usize) << 8) | (b4 as usize)) + 1;
                        pos += 4;
                        (len, d)
                    }
                    _ => {
                        let len = (indicator as usize) + 1;
                        let d = ((((b1 & 0x0F) as usize) << 8) | (b2 as usize)) + 1;
                        pos += 2;
                        (len, d)
                    }
                };

                if disp == 0 || disp > out.len() {
                    return Err(CoreError::InvalidLz11(format!(
                        "非法回溯偏移 disp={disp}，当前已输出={}",
                        out.len()
                    )));
                }

                for _ in 0..length {
                    if out.len() >= decomp_size {
                        break;
                    }
                    let byte = out[out.len() - disp];
                    out.push(byte);
                }
            }
        }
    }

    if out.len() != decomp_size {
        return Err(CoreError::InvalidLz11(format!(
            "解压流意外终止：期望 {decomp_size} B，实际得到 {} B",
            out.len()
        )));
    }

    Ok((out, has_lz13_wrapper))
}

/// 使用定长哈希链将原始数据重压为标准 3DS LZ11（可选外层 0x13 封装头）。
pub fn compress_lz11(raw: &[u8], add_lz13_wrapper: bool) -> Vec<u8> {
    let n = raw.len();
    let mut out = Vec::with_capacity(n / 2 + 16);

    if add_lz13_wrapper {
        out.push(0x13);
        out.push((n & 0xFF) as u8);
        out.push(((n >> 8) & 0xFF) as u8);
        out.push(((n >> 16) & 0xFF) as u8);
    }

    out.push(0x11);
    if n > 0 && n < 0x100_0000 {
        out.push((n & 0xFF) as u8);
        out.push(((n >> 8) & 0xFF) as u8);
        out.push(((n >> 16) & 0xFF) as u8);
    } else {
        out.extend_from_slice(&[0, 0, 0]);
        out.extend_from_slice(&(n as u32).to_le_bytes());
    }

    if n == 0 {
        return out;
    }

    let mut head = vec![NIL; HASH_SIZE];
    let mut prev = vec![NIL; n];
    let mut pos = 0usize;

    while pos < n {
        let flag_pos = out.len();
        out.push(0);
        let mut flags = 0u8;

        for bit in (0..8).rev() {
            if pos >= n {
                break;
            }

            let mut best_len = 0usize;
            let mut best_disp = 0usize;

            if pos + 3 <= n {
                let limit_start = pos.saturating_sub(MAX_WINDOW);
                let mut cand = head[hash3(raw, pos)];
                let mut steps = 0usize;

                while cand != NIL && steps < 64 {
                    let c = cand as usize;
                    if c < limit_start {
                        break;
                    }
                    if raw[c] == raw[pos]
                        && raw[c + 1] == raw[pos + 1]
                        && raw[c + 2] == raw[pos + 2]
                    {
                        let mut match_len = 3usize;
                        let max_possible = MAX_MATCH.min(n - pos);
                        while match_len < max_possible
                            && raw[c + match_len] == raw[pos + match_len]
                        {
                            match_len += 1;
                        }
                        if match_len > best_len {
                            best_len = match_len;
                            best_disp = pos - c;
                            if best_len >= 128 {
                                break;
                            }
                        }
                    }
                    cand = prev[c];
                    steps += 1;
                }
            }

            if best_len >= 3 {
                flags |= 1 << bit;
                let d = best_disp - 1;
                if best_len <= 16 {
                    out.push((((best_len - 1) << 4) | ((d >> 8) & 0x0F)) as u8);
                    out.push((d & 0xFF) as u8);
                } else if best_len <= 272 {
                    let l = best_len - 0x11;
                    out.push((l >> 4) as u8);
                    out.push((((l & 0x0F) << 4) | ((d >> 8) & 0x0F)) as u8);
                    out.push((d & 0xFF) as u8);
                } else {
                    let l = best_len - 0x111;
                    out.push((0x10 | ((l >> 12) & 0x0F)) as u8);
                    out.push(((l >> 4) & 0xFF) as u8);
                    out.push((((l & 0x0F) << 4) | ((d >> 8) & 0x0F)) as u8);
                    out.push((d & 0xFF) as u8);
                }

                let end_pos = (pos + best_len).min(n.saturating_sub(2));
                for p in pos..end_pos {
                    let h = hash3(raw, p);
                    prev[p] = head[h];
                    head[h] = p as u32;
                }
                pos += best_len;
            } else {
                if pos + 3 <= n {
                    let h = hash3(raw, pos);
                    prev[pos] = head[h];
                    head[h] = pos as u32;
                }
                out.push(raw[pos]);
                pos += 1;
            }
        }

        out[flag_pos] = flags;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn roundtrip_synthetic_and_edge_cases() {
        let cases: Vec<Vec<u8>> = vec![
            vec![],
            b"FE13".to_vec(),
            vec![0xAA; 500],
            (0..=255u8).cycle().take(4096).collect(),
        ];

        for raw in cases {
            for lz13 in [false, true] {
                let packed = compress_lz11(&raw, lz13);
                let (unpacked, detected_lz13) = decompress_lz11(&packed).expect("解压失败");
                assert_eq!(unpacked, raw);
                assert_eq!(detected_lz13, lz13);
            }
        }
    }

    #[test]
    fn reject_invalid_stream() {
        assert!(decompress_lz11(&[0x00, 0x01]).is_err());
        assert!(decompress_lz11(&[0x10, 0x04, 0x00, 0x00, 1, 2, 3, 4]).is_err());
    }

    #[test]
    fn roundtrip_real_fe13_files_if_present() {
        let files = [
            r"D:\fe13\RomFS\data\person\static.bin.lz",
            r"D:\fe13\RomFS\data\GameData.bin.lz",
            r"D:\fe13\RomFS\m\GameData.bin.lz",
        ];
        for path_str in files {
            let path = Path::new(path_str);
            if !path.exists() {
                continue;
            }
            let orig = std::fs::read(path).expect("读取样本失败");
            let (raw, is_lz13) = decompress_lz11(&orig).expect("解压样本失败");
            let repacked = compress_lz11(&raw, is_lz13);
            let (verify, _) = decompress_lz11(&repacked).expect("二次解压失败");
            assert_eq!(raw, verify, "无损往返校验失败: {path_str}");
            println!(
                "[PASS] {} | orig={}B -> raw={}B -> repacked={}B",
                path_str,
                orig.len(),
                raw.len(),
                repacked.len()
            );
        }
    }
}