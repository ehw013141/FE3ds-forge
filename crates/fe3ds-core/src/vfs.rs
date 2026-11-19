use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;

use crate::error::{CoreError, CoreResult};

const INVALID_META_OFFSET: u32 = 0xFFFF_FFFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RomFormat {
    Ncsd,
    Cia,
    Ncch,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RomMetadata {
    pub format: RomFormat,
    pub title_id: String,
    pub product_code: String,
    pub romfs_file_count: usize,
    pub has_code_bin: bool,
    pub code_compressed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractReport {
    pub output_dir: String,
    pub romfs_files: usize,
    pub exefs_files: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone)]
struct FileExtent {
    rel_path: String,
    abs_offset: u64,
    size: u64,
}

/// 纯 Rust 3DS ROM 虚拟文件系统（支持已解密的 `.3ds` / `.cia` / `.cxi` 按需直读与全量解包）。
pub struct RomImage {
    file: Mutex<File>,
    file_len: u64,
    metadata: RomMetadata,
    exefs_entries: Vec<FileExtent>,
    exefs_index: HashMap<String, usize>,
    romfs_entries: Vec<FileExtent>,
    romfs_index: HashMap<String, usize>,
}

impl RomImage {
    pub fn open(path: &Path) -> CoreResult<Self> {
        let mut file = File::open(path)?;
        let file_len = file.metadata()?.len();
        if file_len < 0x200 {
            return Err(CoreError::InvalidRom("文件体积过小，不是有效的 3DS ROM 镜像".into()));
        }

        let (format, ncch_offset) = detect_ncch_offset(&mut file, file_len)?;
        let ncch_hdr = read_at(&mut file, ncch_offset, 0x200)?;
        if &ncch_hdr[0x100..0x104] != b"NCCH" {
            return Err(CoreError::InvalidRom(
                "未在主分区检测到明文 NCCH 头，该 ROM 可能仍处于加密状态，请先解密后再加载".into(),
            ));
        }

        let program_id = read_u64_le(&ncch_hdr, 0x118);
        let product_code = std::str::from_utf8(&ncch_hdr[0x150..0x160])
            .unwrap_or("")
            .trim_end_matches('\0')
            .to_string();
        let exheader_size = read_u32_le(&ncch_hdr, 0x180) as u64;
        let media_unit_shift = ncch_hdr[0x18E];
        if media_unit_shift > 16 {
            return Err(CoreError::InvalidRom(format!(
                "非法的 NCCH 介质单元移位值: {media_unit_shift}"
            )));
        }
        let media_unit = 0x200u64 << media_unit_shift;
        let no_crypto = (ncch_hdr[0x18F] & 0x04) != 0;

        let exefs_off_mu = read_u32_le(&ncch_hdr, 0x1A0) as u64;
        let exefs_size_mu = read_u32_le(&ncch_hdr, 0x1A4) as u64;
        let romfs_off_mu = read_u32_le(&ncch_hdr, 0x1B0) as u64;
        let romfs_size_mu = read_u32_le(&ncch_hdr, 0x1B4) as u64;

        let code_compressed = if exheader_size >= 0x200 {
            let exhdr = read_at(&mut file, ncch_offset + 0x200, 0x200)?;
            (exhdr[0x0D] & 0x01) != 0
        } else {
            false
        };

        let mut exefs_entries = Vec::new();
        let mut exefs_index = HashMap::new();
        if exefs_off_mu > 0 && exefs_size_mu > 0 {
            let exefs_start = ncch_offset
                .checked_add(exefs_off_mu * media_unit)
                .ok_or_else(|| CoreError::InvalidRom("ExeFS 偏移溢出".into()))?;
            let exefs_len = exefs_size_mu * media_unit;
            exefs_entries = parse_exefs(&mut file, exefs_start, exefs_len, file_len)?;
            for (idx, ent) in exefs_entries.iter().enumerate() {
                exefs_index.insert(ent.rel_path.to_ascii_lowercase(), idx);
            }
        }

        let mut romfs_entries = Vec::new();
        let mut romfs_index = HashMap::new();
        if romfs_off_mu > 0 && romfs_size_mu > 0 {
            let romfs_start = ncch_offset
                .checked_add(romfs_off_mu * media_unit)
                .ok_or_else(|| CoreError::InvalidRom("RomFS 偏移溢出".into()))?;
            let romfs_len = romfs_size_mu * media_unit;
            romfs_entries = parse_romfs(&mut file, romfs_start, romfs_len, file_len, no_crypto)?;
            for (idx, ent) in romfs_entries.iter().enumerate() {
                romfs_index.insert(ent.rel_path.to_ascii_lowercase(), idx);
            }
        }

        let has_code_bin = exefs_index.contains_key(".code");
        let metadata = RomMetadata {
            format,
            title_id: format!("{program_id:016X}"),
            product_code,
            romfs_file_count: romfs_entries.len(),
            has_code_bin,
            code_compressed,
        };

        Ok(Self {
            file: Mutex::new(file),
            file_len,
            metadata,
            exefs_entries,
            exefs_index,
            romfs_entries,
            romfs_index,
        })
    }

    pub fn metadata(&self) -> &RomMetadata {
        &self.metadata
    }

    pub fn has_romfs_file(&self, rel_path: &str) -> bool {
        self.romfs_index.contains_key(&normalize_romfs_key(rel_path))
    }

    pub fn list_romfs_files(&self) -> Vec<&str> {
        self.romfs_entries.iter().map(|e| e.rel_path.as_str()).collect()
    }

    pub fn read_romfs_file(&self, rel_path: &str) -> CoreResult<Vec<u8>> {
        let key = normalize_romfs_key(rel_path);
        let &idx = self
            .romfs_index
            .get(&key)
            .ok_or_else(|| CoreError::InvalidRom(format!("RomFS 中不存在文件: {rel_path}")))?;
        let ent = &self.romfs_entries[idx];
        self.read_extent(ent)
    }

    pub fn read_code_bin(&self) -> CoreResult<Option<Vec<u8>>> {
        let Some(&idx) = self.exefs_index.get(".code") else {
            return Ok(None);
        };
        let raw = self.read_extent(&self.exefs_entries[idx])?;
        if self.metadata.code_compressed {
            Ok(Some(decompress_blz(&raw)?))
        } else {
            Ok(Some(raw))
        }
    }

    /// 将镜像内的 `RomFS` 与 `ExeFS` 完整解包到指定目录（`.code` 自动解压为 `ExeFS/code.bin`）。
    pub fn extract_to_dir(&self, output_dir: &Path) -> CoreResult<ExtractReport> {
        let romfs_root = output_dir.join("RomFS");
        let exefs_root = output_dir.join("ExeFS");
        fs::create_dir_all(&romfs_root)?;
        fs::create_dir_all(&exefs_root)?;

        let mut total_bytes = 0u64;

        for ent in &self.romfs_entries {
            let dest = safe_join(&romfs_root, &ent.rel_path)?;
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            let data = self.read_extent(ent)?;
            total_bytes += data.len() as u64;
            fs::write(dest, data)?;
        }

        let mut exefs_count = 0usize;
        for ent in &self.exefs_entries {
            let raw = self.read_extent(ent)?;
            let (out_name, bytes) = if ent.rel_path.eq_ignore_ascii_case(".code") {
                let decoded = if self.metadata.code_compressed {
                    decompress_blz(&raw)?
                } else {
                    raw
                };
                ("code.bin".to_string(), decoded)
            } else {
                let clean = ent.rel_path.trim_start_matches('.');
                let name = if clean.contains('.') {
                    clean.to_string()
                } else {
                    format!("{clean}.bin")
                };
                (name, raw)
            };
            let dest = safe_join(&exefs_root, &out_name)?;
            total_bytes += bytes.len() as u64;
            fs::write(dest, bytes)?;
            exefs_count += 1;
        }

        Ok(ExtractReport {
            output_dir: output_dir.to_string_lossy().into_owned(),
            romfs_files: self.romfs_entries.len(),
            exefs_files: exefs_count,
            total_bytes,
        })
    }

    fn read_extent(&self, ent: &FileExtent) -> CoreResult<Vec<u8>> {
        let end = ent
            .abs_offset
            .checked_add(ent.size)
            .ok_or_else(|| CoreError::InvalidRom("文件区段偏移溢出".into()))?;
        if end > self.file_len {
            return Err(CoreError::InvalidRom(format!(
                "文件 {} 超出 ROM 镜像长度",
                ent.rel_path
            )));
        }
        let mut guard = self
            .file
            .lock()
            .map_err(|_| CoreError::InvalidRom("ROM 文件句柄锁异常".into()))?;
        read_at(&mut guard, ent.abs_offset, ent.size as usize)
    }
}

/// 3DS `ExeFS/.code` 使用的任天堂后向 LZSS（BLZ / `MI_UncompressBackward`）解压器。
pub fn decompress_blz(data: &[u8]) -> CoreResult<Vec<u8>> {
    if data.len() < 8 {
        return Err(CoreError::InvalidRom("BLZ 数据过短，缺少 8 字节尾部头".into()));
    }

    let len = data.len();
    let top_bottom = read_u32_le(data, len - 8);
    let extra_size = read_u32_le(data, len - 4) as usize;
    let footer_size = (top_bottom >> 24) as usize;
    let comp_region_size = (top_bottom & 0x00FF_FFFF) as usize;

    if comp_region_size == 0 {
        return Ok(data.to_vec());
    }
    if footer_size < 8 || footer_size > comp_region_size || comp_region_size > len {
        return Err(CoreError::InvalidRom(format!(
            "非法的 BLZ 尾部参数: footer={footer_size}, comp_region={comp_region_size}, len={len}"
        )));
    }

    let out_len = len
        .checked_add(extra_size)
        .ok_or_else(|| CoreError::InvalidRom("BLZ 解压目标长度溢出".into()))?;
    let stop_in = len - comp_region_size;

    let mut out = vec![0u8; out_len];
    out[..stop_in].copy_from_slice(&data[..stop_in]);

    let mut in_pos = len - footer_size;
    let mut out_pos = out_len;

    while in_pos > stop_in && out_pos > stop_in {
        in_pos -= 1;
        let flags = data[in_pos];

        for bit in 0..8 {
            if in_pos <= stop_in || out_pos <= stop_in {
                break;
            }
            if (flags & (0x80 >> bit)) == 0 {
                in_pos -= 1;
                out_pos -= 1;
                out[out_pos] = data[in_pos];
            } else {
                if in_pos < stop_in + 2 {
                    return Err(CoreError::InvalidRom("BLZ 回溯指令截断".into()));
                }
                let b1 = data[in_pos - 1] as usize;
                let b2 = data[in_pos - 2] as usize;
                in_pos -= 2;

                let copy_len = (b1 >> 4) + 3;
                let disp = (((b1 & 0x0F) << 8) | b2) + 3;

                for _ in 0..copy_len {
                    if out_pos <= stop_in {
                        return Err(CoreError::InvalidRom("BLZ 解压写入下溢".into()));
                    }
                    out_pos -= 1;
                    let src_idx = out_pos + disp;
                    if src_idx >= out_len {
                        return Err(CoreError::InvalidRom("BLZ 回溯偏移越界".into()));
                    }
                    out[out_pos] = out[src_idx];
                }
            }
        }
    }

    Ok(out)
}

fn detect_ncch_offset(file: &mut File, file_len: u64) -> CoreResult<(RomFormat, u64)> {
    let head = read_at(file, 0, 0x200)?;

    if &head[0x100..0x104] == b"NCSD" {
        let shift = head[0x18E];
        if shift > 16 {
            return Err(CoreError::InvalidRom(format!(
                "非法的 NCSD 介质单元移位值: {shift}"
            )));
        }
        let media_unit = 0x200u64 << shift;
        let part0_off = read_u32_le(&head, 0x120) as u64;
        let part0_size = read_u32_le(&head, 0x124) as u64;
        if part0_size == 0 {
            return Err(CoreError::InvalidRom("NCSD 主游戏分区 (Partition 0) 为空".into()));
        }
        let abs = part0_off * media_unit;
        if abs + 0x200 > file_len {
            return Err(CoreError::InvalidRom("NCSD 分区偏移超出文件末尾".into()));
        }
        return Ok((RomFormat::Ncsd, abs));
    }

    if &head[0x100..0x104] == b"NCCH" {
        return Ok((RomFormat::Ncch, 0));
    }

    // 尝试解析 CIA 容器头（固定 header_size == 0x2020）
    let header_size = read_u32_le(&head, 0x00) as u64;
    let cia_type = u16::from_le_bytes([head[0x04], head[0x05]]);
    let cia_ver = u16::from_le_bytes([head[0x06], head[0x07]]);
    if header_size == 0x2020 && cia_type == 0 && cia_ver == 0 {
        let cert_sz = read_u32_le(&head, 0x08) as u64;
        let ticket_sz = read_u32_le(&head, 0x0C) as u64;
        let tmd_sz = read_u32_le(&head, 0x10) as u64;

        let content_off = align_up(header_size, 64)
            + align_up(cert_sz, 64)
            + align_up(ticket_sz, 64)
            + align_up(tmd_sz, 64);
        if content_off + 0x200 > file_len {
            return Err(CoreError::InvalidRom("CIA 内容区偏移超出文件末尾".into()));
        }
        return Ok((RomFormat::Cia, content_off));
    }

    Err(CoreError::InvalidRom(
        "无法识别的镜像格式（仅支持已解密的 .3ds / .cci / .cia / .cxi 文件）".into(),
    ))
}

fn parse_exefs(
    file: &mut File,
    exefs_start: u64,
    exefs_len: u64,
    file_len: u64,
) -> CoreResult<Vec<FileExtent>> {
    if exefs_start + 0x200 > file_len || exefs_len < 0x200 {
        return Err(CoreError::InvalidRom("ExeFS 头越界".into()));
    }
    let hdr = read_at(file, exefs_start, 0x200)?;
    let mut out = Vec::new();
    for i in 0..10 {
        let base = i * 16;
        let name_bytes = &hdr[base..base + 8];
        let name = std::str::from_utf8(name_bytes)
            .unwrap_or("")
            .trim_end_matches('\0');
        let rel_off = read_u32_le(&hdr, base + 8) as u64;
        let size = read_u32_le(&hdr, base + 12) as u64;
        if name.is_empty() || size == 0 {
            continue;
        }
        let abs_offset = exefs_start + 0x200 + rel_off;
        if abs_offset + size > file_len {
            return Err(CoreError::InvalidRom(format!(
                "ExeFS 条目 {name} 超出文件边界"
            )));
        }
        out.push(FileExtent {
            rel_path: name.to_string(),
            abs_offset,
            size,
        });
    }
    Ok(out)
}

fn parse_romfs(
    file: &mut File,
    romfs_start: u64,
    romfs_len: u64,
    file_len: u64,
    no_crypto: bool,
) -> CoreResult<Vec<FileExtent>> {
    if romfs_start + 0x60 > file_len || romfs_len < 0x60 {
        return Err(CoreError::InvalidRom("RomFS IVFC 头越界".into()));
    }
    let ivfc = read_at(file, romfs_start, 0x60)?;
    if &ivfc[0..4] != b"IVFC" {
        if !no_crypto {
            return Err(CoreError::InvalidRom(
                "检测到该 ROM 的 RomFS 仍处于加密状态（未设置 NoCrypto 标志且缺少 IVFC 头），请先解密 ROM 后再加载"
                    .into(),
            ));
        }
        return Err(CoreError::InvalidRom("RomFS 缺少有效的 IVFC 头".into()));
    }

    let master_hash_sz = read_u32_le(&ivfc, 0x08) as u64;
    let lv1_sz = read_u64_le(&ivfc, 0x14);
    let lv1_blk_log2 = read_u32_le(&ivfc, 0x1C);
    let lv2_sz = read_u64_le(&ivfc, 0x2C);
    let lv2_blk_log2 = read_u32_le(&ivfc, 0x34);
    let lv3_blk_log2 = read_u32_le(&ivfc, 0x4C);

    if lv1_blk_log2 > 24 || lv2_blk_log2 > 24 || lv3_blk_log2 > 24 {
        return Err(CoreError::InvalidRom("IVFC 块大小字段异常".into()));
    }

    let lv1_blk = 1u64 << lv1_blk_log2;
    let lv2_blk = 1u64 << lv2_blk_log2;
    let lv3_blk = 1u64 << lv3_blk_log2;

    // 优先尝试标准 3DS 布局（Level 3 紧跟 Master Hash 对齐后，通常为 +0x1000），兼容 Level 1/2 前置布局
    let cand_std = romfs_start + align_up(0x60 + master_hash_sz, lv3_blk);
    let cand_alt = romfs_start
        + align_up(0x60 + master_hash_sz, lv1_blk)
        + align_up(lv1_sz, lv1_blk)
        + align_up(lv2_sz, lv2_blk);

    let (lv3_start, lv3_hdr) = [cand_std, cand_alt]
        .into_iter()
        .find_map(|off| {
            if off + 0x28 > file_len {
                return None;
            }
            let hdr = read_at(file, off, 0x28).ok()?;
            if is_valid_lv3_header(&hdr) {
                Some((off, hdr))
            } else {
                None
            }
        })
        .ok_or_else(|| CoreError::InvalidRom("无法定位有效的 RomFS Level 3 文件系统头".into()))?;

    let dir_meta_off = read_u32_le(&lv3_hdr, 0x0C) as u64;
    let dir_meta_len = read_u32_le(&lv3_hdr, 0x10) as usize;
    let file_meta_off = read_u32_le(&lv3_hdr, 0x1C) as u64;
    let file_meta_len = read_u32_le(&lv3_hdr, 0x20) as usize;
    let file_data_off = read_u32_le(&lv3_hdr, 0x24) as u64;

    let dir_meta = read_at(file, lv3_start + dir_meta_off, dir_meta_len)?;
    let file_meta = read_at(file, lv3_start + file_meta_off, file_meta_len)?;
    let data_base = lv3_start + file_data_off;

    let mut entries = Vec::new();
    let mut visited_dirs = HashSet::new();
    let mut stack: Vec<(u32, String)> = vec![(0, String::new())];

    while let Some((dir_off, prefix)) = stack.pop() {
        if dir_off == INVALID_META_OFFSET || !visited_dirs.insert(dir_off) {
            continue;
        }
        let d = dir_off as usize;
        if d + 0x18 > dir_meta.len() {
            return Err(CoreError::InvalidRom("RomFS 目录元数据越界".into()));
        }

        let sibling_dir = read_u32_le(&dir_meta, d + 0x04);
        let child_dir = read_u32_le(&dir_meta, d + 0x08);
        let first_file = read_u32_le(&dir_meta, d + 0x0C);
        let name_len = read_u32_le(&dir_meta, d + 0x14) as usize;

        if d + 0x18 + name_len > dir_meta.len() {
            return Err(CoreError::InvalidRom("RomFS 目录名越界".into()));
        }

        let dir_name = decode_utf16le(&dir_meta[d + 0x18..d + 0x18 + name_len])?;
        let current_prefix = if dir_name.is_empty() {
            prefix.clone()
        } else if prefix.is_empty() {
            dir_name
        } else {
            format!("{prefix}/{dir_name}")
        };

        // 兄弟目录与子目录压栈（先兄弟后子目录，保证顺序稳定）
        if sibling_dir != INVALID_META_OFFSET {
            stack.push((sibling_dir, prefix));
        }
        if child_dir != INVALID_META_OFFSET {
            stack.push((child_dir, current_prefix.clone()));
        }

        // 遍历当前目录下的文件链表
        let mut f_off = first_file;
        let mut visited_files = HashSet::new();
        while f_off != INVALID_META_OFFSET && visited_files.insert(f_off) {
            let f = f_off as usize;
            if f + 0x20 > file_meta.len() {
                return Err(CoreError::InvalidRom("RomFS 文件元数据越界".into()));
            }
            let next_file = read_u32_le(&file_meta, f + 0x04);
            let rel_data_off = read_u64_le(&file_meta, f + 0x08);
            let data_size = read_u64_le(&file_meta, f + 0x10);
            let f_name_len = read_u32_le(&file_meta, f + 0x1C) as usize;

            if f + 0x20 + f_name_len > file_meta.len() {
                return Err(CoreError::InvalidRom("RomFS 文件名越界".into()));
            }
            let file_name = decode_utf16le(&file_meta[f + 0x20..f + 0x20 + f_name_len])?;
            let rel_path = if current_prefix.is_empty() {
                file_name
            } else {
                format!("{current_prefix}/{file_name}")
            };

            let abs_offset = data_base
                .checked_add(rel_data_off)
                .ok_or_else(|| CoreError::InvalidRom("RomFS 文件数据偏移溢出".into()))?;

            entries.push(FileExtent {
                rel_path,
                abs_offset,
                size: data_size,
            });
            f_off = next_file;
        }
    }

    Ok(entries)
}

fn is_valid_lv3_header(hdr: &[u8]) -> bool {
    if hdr.len() < 0x28 {
        return false;
    }
    let header_len = read_u32_le(hdr, 0x00);
    let dir_hash_off = read_u32_le(hdr, 0x04);
    let dir_meta_off = read_u32_le(hdr, 0x0C);
    let file_meta_off = read_u32_le(hdr, 0x1C);
    let file_data_off = read_u32_le(hdr, 0x24);
    header_len == 0x28
        && dir_hash_off == 0x28
        && dir_meta_off >= dir_hash_off
        && file_meta_off >= dir_meta_off
        && file_data_off >= file_meta_off
}

fn normalize_romfs_key(rel_path: &str) -> String {
    let mut norm = rel_path.replace('\\', "/");
    while norm.starts_with('/') {
        norm.remove(0);
    }
    let lower = norm.to_ascii_lowercase();
    if let Some(stripped) = lower.strip_prefix("romfs/") {
        stripped.to_string()
    } else {
        lower
    }
}

fn safe_join(root: &Path, rel: &str) -> CoreResult<PathBuf> {
    let mut out = root.to_path_buf();
    for seg in rel.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." || seg.contains('\\') || seg.contains(':') {
            return Err(CoreError::InvalidRom(format!("检测到非法路径片段: {rel}")));
        }
        out.push(seg);
    }
    Ok(out)
}

fn decode_utf16le(bytes: &[u8]) -> CoreResult<String> {
    if bytes.len() % 2 != 0 {
        return Err(CoreError::InvalidRom("UTF-16LE 字节长度非偶数".into()));
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&units)
        .map_err(|e| CoreError::InvalidRom(format!("UTF-16LE 文件名解码失败: {e}")))
}

fn read_at(file: &mut File, offset: u64, len: usize) -> CoreResult<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; len];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

#[inline]
fn align_up(val: u64, align: u64) -> u64 {
    if align <= 1 {
        return val;
    }
    let rem = val % align;
    if rem == 0 { val } else { val + (align - rem) }
}

#[inline]
fn read_u32_le(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

#[inline]
fn read_u64_le(buf: &[u8], off: usize) -> u64 {
    u64::from_le_bytes([
        buf[off],
        buf[off + 1],
        buf[off + 2],
        buf[off + 3],
        buf[off + 4],
        buf[off + 5],
        buf[off + 6],
        buf[off + 7],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompress_blz_synthetic_roundtrip() {
        // 构造输出 "PRE_" + "ABC" * 6 (22 字节)，其中后 18 字节经 BLZ 压缩
        let mut blz = Vec::new();
        blz.extend_from_slice(b"PRE_");
        // 压缩流（正序）：backref(len=15, disp=3) -> [0x00, 0xC0], literals "ABC", flags=0x10
        blz.extend_from_slice(&[0x00, 0xC0, b'A', b'B', b'C', 0x10]);
        let top_bottom: u32 = (8u32 << 24) | 14u32;
        let extra_size: u32 = 4;
        blz.extend_from_slice(&top_bottom.to_le_bytes());
        blz.extend_from_slice(&extra_size.to_le_bytes());

        let decoded = decompress_blz(&blz).expect("BLZ 解压失败");
        assert_eq!(decoded, b"PRE_ABCABCABCABCABCABC");
    }

    #[test]
    fn parse_and_extract_synthetic_ncch() {
        let mut img = vec![0u8; 0x3000];
        // 1. NCCH 头 @ 0x0
        img[0x100..0x104].copy_from_slice(b"NCCH");
        img[0x118..0x120].copy_from_slice(&0x0004_0000_0004_C200u64.to_le_bytes());
        img[0x150..0x15A].copy_from_slice(b"CTR-P-AFEJ");
        img[0x180..0x184].copy_from_slice(&0x400u32.to_le_bytes()); // exheader_size
        img[0x18F] = 0x04; // NoCrypto
        img[0x1A0..0x1A4].copy_from_slice(&4u32.to_le_bytes()); // exefs @ 0x800
        img[0x1A4..0x1A8].copy_from_slice(&2u32.to_le_bytes()); // exefs_size = 0x400
        img[0x1B0..0x1B4].copy_from_slice(&8u32.to_le_bytes()); // romfs @ 0x1000
        img[0x1B4..0x1B8].copy_from_slice(&16u32.to_le_bytes()); // romfs_size = 0x2000

        // 2. ExHeader @ 0x200: 标记 .code 为 BLZ 压缩
        img[0x200 + 0x0D] = 0x01;

        // 3. ExeFS @ 0x800: 放入 BLZ 压缩的 .code
        let mut blz_code = b"CODE".to_vec();
        blz_code.extend_from_slice(&[0x00, 0xC0, b'X', b'Y', b'Z', 0x10]);
        blz_code.extend_from_slice(&((8u32 << 24) | 14u32).to_le_bytes());
        blz_code.extend_from_slice(&4u32.to_le_bytes());

        img[0x800..0x805].copy_from_slice(b".code");
        img[0x808..0x80C].copy_from_slice(&0u32.to_le_bytes());
        img[0x80C..0x810].copy_from_slice(&(blz_code.len() as u32).to_le_bytes());
        img[0xA00..0xA00 + blz_code.len()].copy_from_slice(&blz_code);

        // 4. RomFS IVFC @ 0x1000 -> Level 3 @ 0x2000
        img[0x1000..0x1004].copy_from_slice(b"IVFC");
        img[0x1004..0x1008].copy_from_slice(&0x10000u32.to_le_bytes());
        img[0x1008..0x100C].copy_from_slice(&0x20u32.to_le_bytes());
        img[0x101C..0x1020].copy_from_slice(&12u32.to_le_bytes());
        img[0x1034..0x1038].copy_from_slice(&12u32.to_le_bytes());
        img[0x104C..0x1050].copy_from_slice(&12u32.to_le_bytes()); // 0x1000 block

        let lv3 = 0x2000usize;
        // Dir metadata: root (0x18B) + child "data" (0x20B)
        let mut dir_meta = Vec::new();
        // root @ 0x00
        dir_meta.extend_from_slice(&0u32.to_le_bytes()); // parent
        dir_meta.extend_from_slice(&INVALID_META_OFFSET.to_le_bytes()); // sibling
        dir_meta.extend_from_slice(&0x18u32.to_le_bytes()); // child_dir = 0x18
        dir_meta.extend_from_slice(&INVALID_META_OFFSET.to_le_bytes()); // first_file
        dir_meta.extend_from_slice(&INVALID_META_OFFSET.to_le_bytes()); // hash_next
        dir_meta.extend_from_slice(&0u32.to_le_bytes()); // name_len = 0
        // "data" @ 0x18
        dir_meta.extend_from_slice(&0u32.to_le_bytes()); // parent
        dir_meta.extend_from_slice(&INVALID_META_OFFSET.to_le_bytes()); // sibling
        dir_meta.extend_from_slice(&INVALID_META_OFFSET.to_le_bytes()); // child_dir
        dir_meta.extend_from_slice(&0u32.to_le_bytes()); // first_file = 0
        dir_meta.extend_from_slice(&INVALID_META_OFFSET.to_le_bytes()); // hash_next
        let dname: Vec<u8> = "data".encode_utf16().flat_map(u16::to_le_bytes).collect();
        dir_meta.extend_from_slice(&(dname.len() as u32).to_le_bytes());
        dir_meta.extend_from_slice(&dname);

        // File metadata: "test.bin" @ 0x00
        let payload = b"FE13_VFS_OK";
        let mut file_meta = Vec::new();
        file_meta.extend_from_slice(&0x18u32.to_le_bytes()); // parent
        file_meta.extend_from_slice(&INVALID_META_OFFSET.to_le_bytes()); // sibling
        file_meta.extend_from_slice(&0u64.to_le_bytes()); // data_off
        file_meta.extend_from_slice(&(payload.len() as u64).to_le_bytes()); // data_size
        file_meta.extend_from_slice(&INVALID_META_OFFSET.to_le_bytes()); // hash_next
        let fname: Vec<u8> = "test.bin".encode_utf16().flat_map(u16::to_le_bytes).collect();
        file_meta.extend_from_slice(&(fname.len() as u32).to_le_bytes());
        file_meta.extend_from_slice(&fname);

        let dir_hash_off = 0x28u32;
        let dir_meta_off = 0x2Cu32;
        let file_hash_off = dir_meta_off + dir_meta.len() as u32;
        let file_meta_off = file_hash_off + 4;
        let file_data_off = align_up((file_meta_off + file_meta.len() as u32) as u64, 16) as u32;

        img[lv3..lv3 + 4].copy_from_slice(&0x28u32.to_le_bytes());
        img[lv3 + 4..lv3 + 8].copy_from_slice(&dir_hash_off.to_le_bytes());
        img[lv3 + 8..lv3 + 12].copy_from_slice(&4u32.to_le_bytes());
        img[lv3 + 12..lv3 + 16].copy_from_slice(&dir_meta_off.to_le_bytes());
        img[lv3 + 16..lv3 + 20].copy_from_slice(&(dir_meta.len() as u32).to_le_bytes());
        img[lv3 + 20..lv3 + 24].copy_from_slice(&file_hash_off.to_le_bytes());
        img[lv3 + 24..lv3 + 28].copy_from_slice(&4u32.to_le_bytes());
        img[lv3 + 28..lv3 + 32].copy_from_slice(&file_meta_off.to_le_bytes());
        img[lv3 + 32..lv3 + 36].copy_from_slice(&(file_meta.len() as u32).to_le_bytes());
        img[lv3 + 36..lv3 + 40].copy_from_slice(&file_data_off.to_le_bytes());

        img[lv3 + dir_meta_off as usize..lv3 + dir_meta_off as usize + dir_meta.len()]
            .copy_from_slice(&dir_meta);
        img[lv3 + file_meta_off as usize..lv3 + file_meta_off as usize + file_meta.len()]
            .copy_from_slice(&file_meta);
        img[lv3 + file_data_off as usize..lv3 + file_data_off as usize + payload.len()]
            .copy_from_slice(payload);

        let tmp_rom = std::env::temp_dir().join(format!("fe3ds_vfs_test_{}.cxi", std::process::id()));
        let tmp_out = std::env::temp_dir().join(format!("fe3ds_vfs_out_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp_out);
        fs::write(&tmp_rom, &img).unwrap();

        let rom = RomImage::open(&tmp_rom).expect("打开合成 NCCH 失败");
        assert_eq!(rom.metadata().format, RomFormat::Ncch);
        assert_eq!(rom.metadata().title_id, "000400000004C200");
        assert_eq!(rom.metadata().product_code, "CTR-P-AFEJ");
        assert_eq!(rom.read_romfs_file("RomFS/data/test.bin").unwrap(), payload);
        assert_eq!(
            rom.read_code_bin().unwrap().unwrap(),
            b"CODEXYZXYZXYZXYZXYZXYZ"
        );

        let report = rom.extract_to_dir(&tmp_out).expect("解包失败");
        assert_eq!(report.romfs_files, 1);
        assert_eq!(report.exefs_files, 1);
        assert_eq!(
            fs::read(tmp_out.join("RomFS/data/test.bin")).unwrap(),
            payload
        );
        assert_eq!(
            fs::read(tmp_out.join("ExeFS/code.bin")).unwrap(),
            b"CODEXYZXYZXYZXYZXYZXYZ"
        );

        let _ = fs::remove_file(&tmp_rom);
        let _ = fs::remove_dir_all(&tmp_out);
    }

    #[test]
    fn reject_invalid_rom_header() {
        let tmp = std::env::temp_dir().join(format!("fe3ds_vfs_bad_{}.3ds", std::process::id()));
        fs::write(&tmp, vec![0xAAu8; 0x400]).unwrap();
        assert!(RomImage::open(&tmp).is_err());
        let _ = fs::remove_file(&tmp);
    }
}