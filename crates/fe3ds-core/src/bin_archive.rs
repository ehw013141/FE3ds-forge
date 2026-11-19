use std::collections::HashMap;

use encoding_rs::SHIFT_JIS;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};

pub const HEADER_SIZE: usize = 0x20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinHeader {
    pub file_size: u32,
    pub data_size: u32,
    pub ptr1_count: u32,
    pub ptr2_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionEntry {
    pub name: String,
    pub offset: usize,
}

/// Intelligent Systems 3DS 通用 `.bin` 容器（适用于 FE13 / FE14 / FE15）。
/// 保留 `ptr2` 的原始定义顺序（加解密密钥流依赖条目序号），同时提供 O(1) 标签检索。
#[derive(Debug, Clone)]
pub struct ISBinArchive {
    raw: Vec<u8>,
    pub header: BinHeader,
    pub ptr1: Vec<u32>,
    sections: Vec<SectionEntry>,
    section_map: HashMap<String, usize>,
}

impl ISBinArchive {
    pub fn parse(raw: Vec<u8>) -> CoreResult<Self> {
        if raw.len() < HEADER_SIZE {
            return Err(CoreError::InvalidArchive("文件长度不足 32 字节标准头".into()));
        }

        let file_size = read_u32_le(&raw, 0)?;
        let data_size = read_u32_le(&raw, 4)?;
        let ptr1_count = read_u32_le(&raw, 8)?;
        let ptr2_count = read_u32_le(&raw, 12)?;

        let data_size_usize = data_size as usize;
        let ptr1_count_usize = ptr1_count as usize;
        let ptr2_count_usize = ptr2_count as usize;

        let ptr1_offset = HEADER_SIZE
            .checked_add(data_size_usize)
            .ok_or_else(|| CoreError::InvalidArchive("data_size 溢出".into()))?;
        let ptr1_bytes = ptr1_count_usize
            .checked_mul(4)
            .ok_or_else(|| CoreError::InvalidArchive("ptr1_count 溢出".into()))?;
        let ptr2_offset = ptr1_offset
            .checked_add(ptr1_bytes)
            .ok_or_else(|| CoreError::InvalidArchive("ptr2 偏移溢出".into()))?;
        let ptr2_bytes = ptr2_count_usize
            .checked_mul(8)
            .ok_or_else(|| CoreError::InvalidArchive("ptr2_count 溢出".into()))?;
        let label_region_offset = ptr2_offset
            .checked_add(ptr2_bytes)
            .ok_or_else(|| CoreError::InvalidArchive("label 偏移溢出".into()))?;

        if label_region_offset > raw.len() {
            return Err(CoreError::InvalidArchive(format!(
                "归档结构越界：标签区起点 0x{label_region_offset:X} 超出文件长度 0x{:X}",
                raw.len()
            )));
        }

        let mut ptr1 = Vec::with_capacity(ptr1_count_usize);
        for i in 0..ptr1_count_usize {
            ptr1.push(read_u32_le(&raw, ptr1_offset + i * 4)?);
        }

        let mut sections = Vec::with_capacity(ptr2_count_usize);
        let mut section_map = HashMap::with_capacity(ptr2_count_usize);
        for i in 0..ptr2_count_usize {
            let entry_off = ptr2_offset + i * 8;
            let data_rel_off = read_u32_le(&raw, entry_off)? as usize;
            let label_rel_off = read_u32_le(&raw, entry_off + 4)? as usize;
            let label_abs = label_region_offset
                .checked_add(label_rel_off)
                .ok_or_else(|| CoreError::InvalidArchive("标签偏移溢出".into()))?;
            let name = read_sjis_cstr(&raw, label_abs)?;

            section_map.entry(name.clone()).or_insert(data_rel_off);
            sections.push(SectionEntry {
                name,
                offset: data_rel_off,
            });
        }

        Ok(Self {
            raw,
            header: BinHeader {
                file_size,
                data_size,
                ptr1_count,
                ptr2_count,
            },
            ptr1,
            sections,
            section_map,
        })
    }

    pub fn sections(&self) -> &[SectionEntry] {
        &self.sections
    }

    pub fn section_offset(&self, name: &str) -> Option<usize> {
        self.section_map.get(name).copied()
    }

    /// 按原始定义顺序筛选指定前缀（如 `JID_`、`IID_`）的所有标签项。
    pub fn sections_with_prefix<'a>(&'a self, prefix: &'a str) -> Vec<&'a SectionEntry> {
        self.sections
            .iter()
            .filter(|entry| entry.name.starts_with(prefix))
            .collect()
    }

    /// 返回数据区只读切片（不含 0x20 头部，不越界进入 ptr1/ptr2）。
    pub fn data_slice(&self) -> &[u8] {
        &self.raw[HEADER_SIZE..HEADER_SIZE + self.header.data_size as usize]
    }

    pub fn read_slice(&self, data_rel_off: usize, len: usize) -> CoreResult<&[u8]> {
        let end = data_rel_off
            .checked_add(len)
            .ok_or_else(|| CoreError::OutOfBounds("读取长度溢出".into()))?;
        let data_limit = self.header.data_size as usize;
        if end > data_limit {
            return Err(CoreError::OutOfBounds(format!(
                "读取区间 [0x{data_rel_off:X}..0x{end:X}) 超出数据区大小 0x{data_limit:X}"
            )));
        }
        Ok(&self.raw[HEADER_SIZE + data_rel_off..HEADER_SIZE + end])
    }

    /// 定长原地覆写数据区字节，严格校验边界以确保 `ptr1`/`ptr2` 指针零错位。
    pub fn write_slice(&mut self, data_rel_off: usize, payload: &[u8]) -> CoreResult<()> {
        let end = data_rel_off
            .checked_add(payload.len())
            .ok_or_else(|| CoreError::OutOfBounds("写入长度溢出".into()))?;
        let data_limit = self.header.data_size as usize;
        if end > data_limit {
            return Err(CoreError::OutOfBounds(format!(
                "写入区间 [0x{data_rel_off:X}..0x{end:X}) 超出数据区大小 0x{data_limit:X}"
            )));
        }
        self.raw[HEADER_SIZE + data_rel_off..HEADER_SIZE + end].copy_from_slice(payload);
        Ok(())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.raw
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.raw
    }
}

#[inline]
fn read_u32_le(buf: &[u8], offset: usize) -> CoreResult<u32> {
    let slice = buf
        .get(offset..offset + 4)
        .ok_or_else(|| CoreError::OutOfBounds(format!("无法在 0x{offset:X} 读取 u32")))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_sjis_cstr(buf: &[u8], start: usize) -> CoreResult<String> {
    if start > buf.len() {
        return Err(CoreError::OutOfBounds(format!(
            "Shift-JIS 字符串起点越界: 0x{start:X}"
        )));
    }
    let slice = &buf[start..];
    let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    let (decoded, _) = SHIFT_JIS.decode_without_bom_handling(&slice[..end]);
    Ok(decoded.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_mutate_synthetic_archive() {
        let data = vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        let label = b"TEST_ENTRY\0";
        let file_size = (HEADER_SIZE + data.len() + 8 + label.len()) as u32;

        let mut raw = Vec::new();
        raw.extend_from_slice(&file_size.to_le_bytes());
        raw.extend_from_slice(&(data.len() as u32).to_le_bytes());
        raw.extend_from_slice(&0u32.to_le_bytes());
        raw.extend_from_slice(&1u32.to_le_bytes());
        raw.extend_from_slice(&[0u8; 16]);
        raw.extend_from_slice(&data);
        raw.extend_from_slice(&4u32.to_le_bytes());
        raw.extend_from_slice(&0u32.to_le_bytes());
        raw.extend_from_slice(label);

        let mut arc = ISBinArchive::parse(raw).expect("解析合成归档失败");
        assert_eq!(arc.section_offset("TEST_ENTRY"), Some(4));
        assert_eq!(arc.read_slice(4, 4).unwrap(), &[0x55, 0x66, 0x77, 0x88]);

        arc.write_slice(4, &[0xAA, 0xBB]).unwrap();
        assert_eq!(arc.read_slice(4, 4).unwrap(), &[0xAA, 0xBB, 0x77, 0x88]);
        assert!(arc.write_slice(6, &[1, 2, 3]).is_err());
    }
}