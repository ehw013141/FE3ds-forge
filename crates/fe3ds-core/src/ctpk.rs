use crate::bin_archive::ISBinArchive;
use crate::error::{CoreError, CoreResult};
use crate::lz11::decompress_lz11;

const ETC1_MODIFIERS: [[i32; 2]; 8] = [
    [2, 8],
    [5, 17],
    [9, 29],
    [13, 42],
    [18, 60],
    [24, 80],
    [33, 106],
    [47, 183],
];

const PREFERRED_EXPRESSIONS: &[&str] = &[
    "通常.ctpk.lz",
    "微笑.ctpk.lz",
    "キメ.ctpk.lz",
    "笑.ctpk.lz",
];

const PREFERRED_BCH_EXPRESSIONS: &[&str] = &[
    "通常.bch.lz",
    "微笑.bch.lz",
    "キメ.bch.lz",
    "笑.bch.lz",
    "ベース.bch.lz",
];

#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// 从 `RomFS/face/*_fc_通常.arc` 中提取默认表情并解码为 RGBA 图像。
pub fn decode_portrait_from_face_arc(arc_bytes: Vec<u8>) -> CoreResult<DecodedImage> {
    let arc = ISBinArchive::parse(arc_bytes)?;
    let ctpk_entries: Vec<_> = arc
        .sections()
        .iter()
        .filter(|s| s.name.ends_with(".ctpk.lz"))
        .collect();

    if ctpk_entries.is_empty() {
        return Err(CoreError::InvalidTexture("头像归档内未找到 .ctpk.lz 条目".into()));
    }

    let chosen = PREFERRED_EXPRESSIONS
        .iter()
        .find_map(|pref| ctpk_entries.iter().find(|e| e.name == *pref).copied())
        .unwrap_or(ctpk_entries[0]);

    let info = arc.read_slice(chosen.offset, 16)?;
    let comp_size = u32::from_le_bytes([info[8], info[9], info[10], info[11]]) as usize;
    let data_offset = u32::from_le_bytes([info[12], info[13], info[14], info[15]]) as usize;

    let compressed = arc.read_slice(data_offset, comp_size)?;
    let (ctpk_raw, _) = decompress_lz11(compressed)?;
    decode_ctpk(&ctpk_raw)
}
/// 从 FE14 `RomFS/face/face/*_{bu,st}.arc` 中提取默认表情 BCH 并解码为 RGBA 图像。
pub fn decode_portrait_from_fe14_face_arc(arc_bytes: Vec<u8>) -> CoreResult<DecodedImage> {
    let arc = ISBinArchive::parse(arc_bytes)?;
    let bch_entries: Vec<_> = arc
        .sections()
        .iter()
        .filter(|s| s.name.ends_with(".bch.lz"))
        .collect();

    if bch_entries.is_empty() {
        return Err(CoreError::InvalidTexture("FE14 头像归档内未找到 .bch.lz 条目".into()));
    }

    let chosen = PREFERRED_BCH_EXPRESSIONS
        .iter()
        .find_map(|pref| bch_entries.iter().find(|e| e.name == *pref).copied())
        .unwrap_or(bch_entries[0]);

    let info = arc.read_slice(chosen.offset, 16)?;
    let comp_size = u32::from_le_bytes([info[8], info[9], info[10], info[11]]) as usize;
    let rel_off = u32::from_le_bytes([info[12], info[13], info[14], info[15]]) as usize;
    let data_base = arc.section_offset("Data").unwrap_or(0);

    let compressed = arc
        .read_slice(data_base + rel_off, comp_size)
        .or_else(|_| arc.read_slice(rel_off, comp_size))?;
    let (bch_raw, _) = decompress_lz11(compressed)?;
    decode_bch(&bch_raw)
}

/// 解码 3DS `BCH` 容器的首张贴图（解析 PICA200 GPU 命令流，支持 `ETC1` 与 `ETC1A4`）。
pub fn decode_bch(bch: &[u8]) -> CoreResult<DecodedImage> {
    if bch.len() < 0x30 || &bch[0..4] != b"BCH\0" {
        return Err(CoreError::InvalidTexture("无效的 BCH 文件头".into()));
    }

    let gpu_off = u32::from_le_bytes([bch[0x10], bch[0x11], bch[0x12], bch[0x13]]) as usize;
    let data_off = u32::from_le_bytes([bch[0x14], bch[0x15], bch[0x16], bch[0x17]]) as usize;
    let gpu_size = u32::from_le_bytes([bch[0x28], bch[0x29], bch[0x2A], bch[0x2B]]) as usize;
    let data_size = u32::from_le_bytes([bch[0x2C], bch[0x2D], bch[0x2E], bch[0x2F]]) as usize;

    let gpu_end = gpu_off
        .checked_add(gpu_size)
        .ok_or_else(|| CoreError::InvalidTexture("BCH GPU 命令区偏移溢出".into()))?;
    let gpu_cmds = bch
        .get(gpu_off..gpu_end)
        .ok_or_else(|| CoreError::InvalidTexture("BCH GPU 命令区越界".into()))?;

    let mut width = 0usize;
    let mut height = 0usize;
    let mut tex_rel_off = 0usize;
    let mut format: Option<u32> = None;

    for chunk in gpu_cmds.chunks_exact(8) {
        let param = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let cmd = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        match cmd & 0xFFFF {
            0x0082 if width == 0 => {
                height = (param & 0xFFFF) as usize;
                width = ((param >> 16) & 0xFFFF) as usize;
            }
            0x0085 if format.is_none() => {
                tex_rel_off = param as usize;
            }
            0x008E if format.is_none() => {
                format = Some(param);
            }
            _ => {}
        }
    }

    if width == 0 || height == 0 || width % 8 != 0 || height % 8 != 0 {
        return Err(CoreError::InvalidTexture(format!(
            "不支持的 BCH 贴图尺寸: {width}x{height}"
        )));
    }

    let has_alpha = match format {
        Some(12) => false,
        Some(13) => true,
        other => {
            return Err(CoreError::InvalidTexture(format!(
                "暂不支持的 BCH 像素格式: {other:?}"
            )));
        }
    };

    let start = data_off
        .checked_add(tex_rel_off)
        .ok_or_else(|| CoreError::InvalidTexture("BCH 贴图数据偏移溢出".into()))?;
    let end = data_off
        .checked_add(data_size)
        .ok_or_else(|| CoreError::InvalidTexture("BCH 贴图数据长度溢出".into()))?;
    let pixel_data = bch
        .get(start..end)
        .ok_or_else(|| CoreError::InvalidTexture("BCH 贴图数据超出文件边界".into()))?;

    let rgba = decode_etc1_tiled(pixel_data, width, height, has_alpha)?;
    Ok(DecodedImage {
        width: width as u32,
        height: height as u32,
        rgba,
    })
}
/// 解码 3DS `CTPK` 容器的首张贴图（支持格式 12 `ETC1` 与格式 13 `ETC1A4`）。
pub fn decode_ctpk(ctpk: &[u8]) -> CoreResult<DecodedImage> {
    if ctpk.len() < 0x40 || &ctpk[0..4] != b"CTPK" {
        return Err(CoreError::InvalidTexture("无效的 CTPK 文件头".into()));
    }

    let tex_count = u16::from_le_bytes([ctpk[6], ctpk[7]]);
    if tex_count == 0 {
        return Err(CoreError::InvalidTexture("CTPK 不包含任何贴图".into()));
    }

    let data_section_off = u32::from_le_bytes([ctpk[8], ctpk[9], ctpk[10], ctpk[11]]) as usize;
    let tex_size = u32::from_le_bytes([ctpk[0x24], ctpk[0x25], ctpk[0x26], ctpk[0x27]]) as usize;
    let tex_rel_off = u32::from_le_bytes([ctpk[0x28], ctpk[0x29], ctpk[0x2A], ctpk[0x2B]]) as usize;
    let format = u32::from_le_bytes([ctpk[0x2C], ctpk[0x2D], ctpk[0x2E], ctpk[0x2F]]);
    let width = u16::from_le_bytes([ctpk[0x30], ctpk[0x31]]) as usize;
    let height = u16::from_le_bytes([ctpk[0x32], ctpk[0x33]]) as usize;

    if width == 0 || height == 0 || width % 8 != 0 || height % 8 != 0 {
        return Err(CoreError::InvalidTexture(format!(
            "不支持的贴图尺寸: {width}x{height}（需为 8 的倍数）"
        )));
    }

    let start = data_section_off
        .checked_add(tex_rel_off)
        .ok_or_else(|| CoreError::InvalidTexture("贴图数据偏移溢出".into()))?;
    let end = start
        .checked_add(tex_size)
        .ok_or_else(|| CoreError::InvalidTexture("贴图数据长度溢出".into()))?;
    let pixel_data = ctpk
        .get(start..end)
        .ok_or_else(|| CoreError::InvalidTexture("贴图数据超出 CTPK 文件边界".into()))?;

    let has_alpha = match format {
        12 => false,
        13 => true,
        other => {
            return Err(CoreError::InvalidTexture(format!(
                "暂不支持的 CTPK 像素格式: {other}"
            )));
        }
    };

    let rgba = decode_etc1_tiled(pixel_data, width, height, has_alpha)?;
    Ok(DecodedImage {
        width: width as u32,
        height: height as u32,
        rgba,
    })
}

/// 将解码后的 RGBA 图像编码为内存 PNG 字节流。
pub fn encode_png(image: &DecodedImage) -> CoreResult<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| CoreError::InvalidTexture(format!("写入 PNG 头失败: {e}")))?;
        writer
            .write_image_data(&image.rgba)
            .map_err(|e| CoreError::InvalidTexture(format!("写入 PNG 像素失败: {e}")))?;
    }
    Ok(out)
}

fn decode_etc1_tiled(
    src: &[u8],
    width: usize,
    height: usize,
    has_alpha: bool,
) -> CoreResult<Vec<u8>> {
    let block_bytes = if has_alpha { 16 } else { 8 };
    let expected = (width / 4) * (height / 4) * block_bytes;
    if src.len() < expected {
        return Err(CoreError::InvalidTexture(format!(
            "ETC1 像素流截断：期望至少 {expected} B，实际 {} B",
            src.len()
        )));
    }

    let mut rgba = vec![0u8; width * height * 4];
    let mut cursor = 0usize;

    for ty in (0..height).step_by(8) {
        for tx in (0..width).step_by(8) {
            for sub in 0..4 {
                let bx = tx + (sub & 1) * 4;
                let by = ty + (sub >> 1) * 4;

                let alpha_bits = if has_alpha {
                    let a = u64::from_le_bytes(src[cursor..cursor + 8].try_into().unwrap());
                    cursor += 8;
                    Some(a)
                } else {
                    None
                };

                let color_bits = u64::from_le_bytes(src[cursor..cursor + 8].try_into().unwrap());
                cursor += 8;

                decode_etc1_4x4(color_bits, alpha_bits, bx, by, width, &mut rgba);
            }
        }
    }

    Ok(rgba)
}

fn decode_etc1_4x4(
    block: u64,
    alpha_bits: Option<u64>,
    bx: usize,
    by: usize,
    stride_w: usize,
    out: &mut [u8],
) {
    let flip = ((block >> 32) & 1) != 0;
    let diff = ((block >> 33) & 1) != 0;
    let table2 = ((block >> 34) & 7) as usize;
    let table1 = ((block >> 37) & 7) as usize;

    let (r1, g1, b1, r2, g2, b2) = if diff {
        let r1_5 = ((block >> 59) & 0x1F) as i32;
        let g1_5 = ((block >> 51) & 0x1F) as i32;
        let b1_5 = ((block >> 43) & 0x1F) as i32;
        let dr = sign_extend_3((block >> 56) & 7);
        let dg = sign_extend_3((block >> 48) & 7);
        let db = sign_extend_3((block >> 40) & 7);
        (
            expand_5to8(r1_5),
            expand_5to8(g1_5),
            expand_5to8(b1_5),
            expand_5to8((r1_5 + dr).clamp(0, 31)),
            expand_5to8((g1_5 + dg).clamp(0, 31)),
            expand_5to8((b1_5 + db).clamp(0, 31)),
        )
    } else {
        (
            expand_4to8(((block >> 60) & 0x0F) as i32),
            expand_4to8(((block >> 52) & 0x0F) as i32),
            expand_4to8(((block >> 44) & 0x0F) as i32),
            expand_4to8(((block >> 56) & 0x0F) as i32),
            expand_4to8(((block >> 48) & 0x0F) as i32),
            expand_4to8(((block >> 40) & 0x0F) as i32),
        )
    };

    for px in 0..4 {
        for py in 0..4 {
            let bit_idx = px * 4 + py;
            let lsb = ((block >> bit_idx) & 1) as usize;
            let msb = ((block >> (bit_idx + 16)) & 1) as usize;

            let use_first = if flip { py < 2 } else { px < 2 };
            let (br, bg, bb, tbl) = if use_first {
                (r1, g1, b1, table1)
            } else {
                (r2, g2, b2, table2)
            };

            let mag = ETC1_MODIFIERS[tbl][lsb];
            let delta = if msb == 0 { mag } else { -mag };

            let a = match alpha_bits {
                Some(bits) => {
                    let a4 = ((bits >> (bit_idx * 4)) & 0x0F) as u8;
                    (a4 << 4) | a4
                }
                None => 0xFF,
            };

            let dst = ((by + py) * stride_w + (bx + px)) * 4;
            out[dst] = (br + delta).clamp(0, 255) as u8;
            out[dst + 1] = (bg + delta).clamp(0, 255) as u8;
            out[dst + 2] = (bb + delta).clamp(0, 255) as u8;
            out[dst + 3] = a;
        }
    }
}

#[inline]
fn sign_extend_3(v: u64) -> i32 {
    let n = (v & 7) as i32;
    if n >= 4 { n - 8 } else { n }
}

#[inline]
fn expand_4to8(v: i32) -> i32 {
    (v << 4) | v
}

#[inline]
fn expand_5to8(v: i32) -> i32 {
    (v << 3) | (v >> 2)
}

#[cfg(test)]

mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn reject_invalid_ctpk_header() {
        assert!(decode_ctpk(b"NOT_CTPK").is_err());
    }

    #[test]
    fn decode_real_fe13_chrom_portrait_if_present() {
        let path = Path::new(r"D:\fe13\RomFS\face\クロム_fc_通常.arc");
        if !path.exists() {
            return;
        }
        let img = decode_portrait_from_face_arc(std::fs::read(path).unwrap())
            .expect("解码克罗姆头像失败");
        assert_eq!(img.width, 128);
        assert_eq!(img.height, 128);
        assert_eq!(img.rgba.len(), 128 * 128 * 4);

        let png_bytes = encode_png(&img).expect("编码 PNG 失败");
        assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        println!(
            "[PASS] 成功解码克罗姆 128x128 ETC1A4 头像 -> PNG {} B",
            png_bytes.len()
        );
    }

    #[test]
    fn decode_real_fe14_felicia_portrait_if_present() {
        let rom_path = Path::new(r"D:\fe14.cci");
        if !rom_path.exists() {
            return;
        }
        let rom = crate::vfs::RomImage::open(rom_path).unwrap();
        let arc_bytes = rom.read_romfs_file("face/face/フェリシア_bu.arc").unwrap();
        let img = decode_portrait_from_fe14_face_arc(arc_bytes).expect("解码菲丽希娅 BCH 头像失败");
        assert_eq!(img.width, 128);
        assert_eq!(img.height, 128);
        assert_eq!(img.rgba.len(), 128 * 128 * 4);

        let png_bytes = encode_png(&img).expect("编码 PNG 失败");
        assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        println!(
            "[PASS] 成功解码 FE14 菲丽希娅 128x128 BCH(ETC1A4) 头像 -> PNG {} B",
            png_bytes.len()
        );
    }
}