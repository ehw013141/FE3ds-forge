use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::bin_archive::ISBinArchive;
use crate::cipher::{CipherProfile, GrowthCipher};
use crate::error::{CoreError, CoreResult};
use crate::message::MessageArchive;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FieldKind {
    U8,
    I8,
    U16,
    I16,
    U32,
    U8Array { len: usize },
    I8Array { len: usize },
    CipheredU8Array { len: usize },
}

impl FieldKind {
    pub fn byte_len(&self) -> usize {
        match *self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 => 4,
            Self::U8Array { len } | Self::I8Array { len } | Self::CipheredU8Array { len } => len,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    pub key: String,
    pub label: String,
    pub offset: usize,
    pub kind: FieldKind,
    #[serde(default)]
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    pub min: Option<i64>,
    #[serde(default)]
    pub max: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    pub id: String,
    pub game: String,
    pub title: String,
    pub target_file: String,
    pub entry_prefix: String,
    pub entry_size: usize,
    #[serde(default)]
    pub cipher_profile: Option<CipherProfile>,
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FieldValue {
    Int(i64),
    IntArray(Vec<i64>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableRecord {
    pub index: usize,
    pub id: String,
    pub display_name: String,
    pub offset: usize,
    pub values: BTreeMap<String, FieldValue>,
}

impl TableSchema {
    pub fn from_json_str(json: &str) -> CoreResult<Self> {
        let schema: Self = serde_json::from_str(json)
            .map_err(|e| CoreError::Schema(format!("JSON 解析失败: {e}")))?;
        schema.validate()?;
        Ok(schema)
    }

    pub fn validate(&self) -> CoreResult<()> {
        if self.entry_size == 0 {
            return Err(CoreError::Schema("entry_size 必须大于 0".into()));
        }
        for field in &self.fields {
            let end = field
                .offset
                .checked_add(field.kind.byte_len())
                .ok_or_else(|| CoreError::Schema(format!("字段 {} 偏移溢出", field.key)))?;
            if end > self.entry_size {
                return Err(CoreError::Schema(format!(
                    "字段 {} 区间 [{}..{}) 超出 entry_size {}",
                    field.key, field.offset, end, self.entry_size
                )));
            }
            if let FieldKind::CipheredU8Array { len } = field.kind {
                if len != 8 {
                    return Err(CoreError::Schema(format!(
                        "加密字段 {} 仅支持 8 字节属性组，当前 len={len}",
                        field.key
                    )));
                }
                if self.cipher_profile.is_none() {
                    return Err(CoreError::Schema(format!(
                        "字段 {} 使用了加密类型，但 Schema 未配置 cipher_profile",
                        field.key
                    )));
                }
            }
        }
        Ok(())
    }
}

/// 声明式表格读写引擎：统一承载角色、职业、武器、道具等任意 Schema 表的无损读写。
pub struct TableEngine;

impl TableEngine {
    pub fn read_records(
        schema: &TableSchema,
        archive: &ISBinArchive,
        messages: Option<&MessageArchive>,
        cipher: Option<&GrowthCipher>,
    ) -> CoreResult<Vec<TableRecord>> {
        schema.validate()?;
        let entries = archive.sections_with_prefix(&schema.entry_prefix);
        let mut records = Vec::with_capacity(entries.len());

        for (index, sec) in entries.iter().enumerate() {
            let mut values = BTreeMap::new();
            for field in &schema.fields {
                let raw = archive.read_slice(sec.offset + field.offset, field.kind.byte_len())?;
                let val = decode_field(field, raw, index, schema.cipher_profile, cipher)?;
                values.insert(field.key.clone(), val);
            }

            let display_name = messages
                .and_then(|m| m.resolve_id(&sec.name))
                .unwrap_or(&sec.name)
                .to_string();

            records.push(TableRecord {
                index,
                id: sec.name.clone(),
                display_name,
                offset: sec.offset,
                values,
            });
        }

        Ok(records)
    }

    pub fn write_records(
        schema: &TableSchema,
        archive: &mut ISBinArchive,
        records: &[TableRecord],
        cipher: Option<&GrowthCipher>,
    ) -> CoreResult<usize> {
        schema.validate()?;
        let entries: Vec<(String, usize)> = archive
            .sections_with_prefix(&schema.entry_prefix)
            .into_iter()
            .map(|e| (e.name.clone(), e.offset))
            .collect();

        let mut written = 0usize;
        for rec in records {
            let (entry_name, entry_offset) = entries.get(rec.index).ok_or_else(|| {
                CoreError::Schema(format!("记录序号 {} 越界（表共 {} 项）", rec.index, entries.len()))
            })?;
            if entry_name != &rec.id {
                return Err(CoreError::Schema(format!(
                    "记录 ID 校验不一致：序号 {} 期望 {entry_name}，实际收到 {}",
                    rec.index, rec.id
                )));
            }

            for field in &schema.fields {
                let Some(val) = rec.values.get(&field.key) else {
                    continue;
                };
                let bytes = encode_field(field, val, rec.index, schema.cipher_profile, cipher)?;
                archive.write_slice(*entry_offset + field.offset, &bytes)?;
            }
            written += 1;
        }

        Ok(written)
    }
}

fn decode_field(
    field: &FieldDef,
    raw: &[u8],
    entry_index: usize,
    profile: Option<CipherProfile>,
    cipher: Option<&GrowthCipher>,
) -> CoreResult<FieldValue> {
    match field.kind {
        FieldKind::U8 => Ok(FieldValue::Int(raw[0] as i64)),
        FieldKind::I8 => Ok(FieldValue::Int((raw[0] as i8) as i64)),
        FieldKind::U16 => Ok(FieldValue::Int(u16::from_le_bytes([raw[0], raw[1]]) as i64)),
        FieldKind::I16 => Ok(FieldValue::Int(i16::from_le_bytes([raw[0], raw[1]]) as i64)),
        FieldKind::U32 => Ok(FieldValue::Int(
            u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as i64,
        )),
        FieldKind::U8Array { .. } => Ok(FieldValue::IntArray(
            raw.iter().map(|&b| b as i64).collect(),
        )),
        FieldKind::I8Array { .. } => Ok(FieldValue::IntArray(
            raw.iter().map(|&b| (b as i8) as i64).collect(),
        )),
        FieldKind::CipheredU8Array { .. } => {
            let prof = profile.ok_or_else(|| CoreError::Schema("缺少 cipher_profile".into()))?;
            let c = cipher.ok_or_else(|| CoreError::InvalidCipher("未传入 GrowthCipher 实例".into()))?;
            let arr: [u8; 8] = raw
                .try_into()
                .map_err(|_| CoreError::Schema(format!("字段 {} 长度非 8 字节", field.key)))?;
            let plain = c.decode_stats(prof, entry_index, &arr);
            Ok(FieldValue::IntArray(
                plain.iter().map(|&b| b as i64).collect(),
            ))
        }
    }
}

fn encode_field(
    field: &FieldDef,
    value: &FieldValue,
    entry_index: usize,
    profile: Option<CipherProfile>,
    cipher: Option<&GrowthCipher>,
) -> CoreResult<Vec<u8>> {
    match (&field.kind, value) {
        (FieldKind::U8, FieldValue::Int(v)) => Ok(vec![try_u8(*v, &field.key)?]),
        (FieldKind::I8, FieldValue::Int(v)) => Ok(vec![try_i8(*v, &field.key)? as u8]),
        (FieldKind::U16, FieldValue::Int(v)) => {
            let n = u16::try_from(*v)
                .map_err(|_| CoreError::Schema(format!("字段 {} 值 {v} 超出 u16 范围", field.key)))?;
            Ok(n.to_le_bytes().to_vec())
        }
        (FieldKind::I16, FieldValue::Int(v)) => {
            let n = i16::try_from(*v)
                .map_err(|_| CoreError::Schema(format!("字段 {} 值 {v} 超出 i16 范围", field.key)))?;
            Ok(n.to_le_bytes().to_vec())
        }
        (FieldKind::U32, FieldValue::Int(v)) => {
            let n = u32::try_from(*v)
                .map_err(|_| CoreError::Schema(format!("字段 {} 值 {v} 超出 u32 范围", field.key)))?;
            Ok(n.to_le_bytes().to_vec())
        }
        (FieldKind::U8Array { len }, FieldValue::IntArray(arr)) => {
            ensure_len(arr, *len, &field.key)?;
            arr.iter().map(|&v| try_u8(v, &field.key)).collect()
        }
        (FieldKind::I8Array { len }, FieldValue::IntArray(arr)) => {
            ensure_len(arr, *len, &field.key)?;
            arr.iter().map(|&v| Ok(try_i8(v, &field.key)? as u8)).collect()
        }
        (FieldKind::CipheredU8Array { len }, FieldValue::IntArray(arr)) => {
            ensure_len(arr, *len, &field.key)?;
            let prof = profile.ok_or_else(|| CoreError::Schema("缺少 cipher_profile".into()))?;
            let c = cipher.ok_or_else(|| CoreError::InvalidCipher("未传入 GrowthCipher 实例".into()))?;
            let mut plain = [0u8; 8];
            for (slot, &v) in plain.iter_mut().zip(arr.iter()) {
                *slot = try_u8(v, &field.key)?;
            }
            Ok(c.encode_stats(prof, entry_index, &plain).to_vec())
        }
        _ => Err(CoreError::Schema(format!(
            "字段 {} 的值类型与 Schema 定义不匹配",
            field.key
        ))),
    }
}

fn ensure_len(arr: &[i64], expected: usize, key: &str) -> CoreResult<()> {
    if arr.len() != expected {
        return Err(CoreError::Schema(format!(
            "字段 {key} 数组长度不匹配：期望 {expected}，实际 {}",
            arr.len()
        )));
    }
    Ok(())
}

fn try_u8(v: i64, key: &str) -> CoreResult<u8> {
    u8::try_from(v).map_err(|_| CoreError::Schema(format!("字段 {key} 值 {v} 超出 0..=255 范围")))
}

fn try_i8(v: i64, key: &str) -> CoreResult<i8> {
    i8::try_from(v).map_err(|_| CoreError::Schema(format!("字段 {key} 值 {v} 超出 -128..=127 范围")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lz11::decompress_lz11;
    use std::path::Path;

    #[test]
    fn load_bundled_fe13_schemas() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fe13");
        let char_schema =
            TableSchema::from_json_str(&std::fs::read_to_string(root.join("characters.json")).unwrap())
                .unwrap();
        let class_schema =
            TableSchema::from_json_str(&std::fs::read_to_string(root.join("classes.json")).unwrap())
                .unwrap();
        assert_eq!(char_schema.fields.len(), 4);
        assert_eq!(class_schema.fields.len(), 4);
    }

    #[test]
    fn roundtrip_and_mutate_real_fe13_tables_if_present() {
        let fe13_root = Path::new(r"D:\fe13");
        if !fe13_root.exists() {
            return;
        }
        let schema_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fe13");
        let char_schema = TableSchema::from_json_str(
            &std::fs::read_to_string(schema_dir.join("characters.json")).unwrap(),
        )
        .unwrap();
        let class_schema = TableSchema::from_json_str(
            &std::fs::read_to_string(schema_dir.join("classes.json")).unwrap(),
        )
        .unwrap();

        let (gd_raw, _) =
            decompress_lz11(&std::fs::read(fe13_root.join(r"RomFS\data\GameData.bin.lz")).unwrap())
                .unwrap();
        let gd_arc = ISBinArchive::parse(gd_raw).unwrap();
        let cipher_off = gd_arc.section_offset("CipherData").unwrap();
        let cipher = GrowthCipher::from_table(gd_arc.read_slice(cipher_off, 256).unwrap()).unwrap();

        let (msg_raw, _) =
            decompress_lz11(&std::fs::read(fe13_root.join(r"RomFS\m\GameData.bin.lz")).unwrap())
                .unwrap();
        let messages = MessageArchive::from_archive(&ISBinArchive::parse(msg_raw).unwrap()).unwrap();

        let (st_raw, _) = decompress_lz11(
            &std::fs::read(fe13_root.join(r"RomFS\data\person\static.bin.lz")).unwrap(),
        )
        .unwrap();
        let orig_st_bytes = st_raw.clone();
        let mut st_arc = ISBinArchive::parse(st_raw).unwrap();

        // 1. 无损往返校验：未改动直接写回必须与原文件逐字节一致
        let mut chars =
            TableEngine::read_records(&char_schema, &st_arc, Some(&messages), Some(&cipher)).unwrap();
        assert_eq!(chars.len(), 57);
        TableEngine::write_records(&char_schema, &mut st_arc, &chars, Some(&cipher)).unwrap();
        assert_eq!(st_arc.as_bytes(), orig_st_bytes.as_slice());

        // 2. 内存改写校验：给克罗姆全成长 +30%，重新读取验证
        let chrom = chars.iter_mut().find(|r| r.id == "PID_クロム").unwrap();
        assert_eq!(chrom.display_name, "克罗姆");
        assert_eq!(
            chrom.values.get("personal_growths"),
            Some(&FieldValue::IntArray(vec![45, 40, 10, 40, 40, 70, 35, 20]))
        );
        chrom.values.insert(
            "personal_growths".into(),
            FieldValue::IntArray(vec![75, 70, 40, 70, 70, 100, 65, 50]),
        );
        TableEngine::write_records(&char_schema, &mut st_arc, &chars, Some(&cipher)).unwrap();

        let reread =
            TableEngine::read_records(&char_schema, &st_arc, Some(&messages), Some(&cipher)).unwrap();
        let chrom_after = reread.iter().find(|r| r.id == "PID_クロム").unwrap();
        assert_eq!(
            chrom_after.values.get("personal_growths"),
            Some(&FieldValue::IntArray(vec![75, 70, 40, 70, 70, 100, 65, 50]))
        );

        // 3. 职业表校验
        let classes =
            TableEngine::read_records(&class_schema, &gd_arc, Some(&messages), Some(&cipher))
                .unwrap();
        assert_eq!(classes.len(), 83);
        let lord = classes.iter().find(|r| r.id == "JID_ロード男").unwrap();
        assert_eq!(
            lord.values.get("player_growths"),
            Some(&FieldValue::IntArray(vec![40, 20, 0, 20, 20, 0, 10, 5]))
        );
        assert_eq!(
            lord.values.get("max_caps"),
            Some(&FieldValue::IntArray(vec![60, 27, 20, 25, 26, 30, 26, 25]))
        );
        println!(
            "[PASS] Schema 引擎闭环校验通过：{} 角色 + {} 职业（克罗姆 +30% 写回复读一致）",
            chars.len(),
            classes.len()
        );
    }
    
    #[test]
    fn roundtrip_and_mutate_real_fe14_cci_if_present() {
        let rom_path = Path::new(r"D:\fe14.cci");
        if !rom_path.exists() {
            return;
        }
        let schema_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fe14");
        let char_schema = TableSchema::from_json_str(
            &std::fs::read_to_string(schema_dir.join("characters.json")).unwrap(),
        )
        .unwrap();
        let class_schema = TableSchema::from_json_str(
            &std::fs::read_to_string(schema_dir.join("classes.json")).unwrap(),
        )
        .unwrap();

        let rom = crate::vfs::RomImage::open(rom_path).unwrap();
        let (gd_raw, _) = decompress_lz11(&rom.read_romfs_file("GameData/GameData.bin.lz").unwrap()).unwrap();
        let orig_gd_bytes = gd_raw.clone();
        let mut gd_arc = ISBinArchive::parse(gd_raw).unwrap();

        let (msg_raw, _) = decompress_lz11(&rom.read_romfs_file("m/GameData.bin.lz").unwrap()).unwrap();
        let messages = MessageArchive::from_archive(&ISBinArchive::parse(msg_raw).unwrap()).unwrap();

        // 1. 无损往返校验
        let mut chars = TableEngine::read_records(&char_schema, &gd_arc, Some(&messages), None).unwrap();
        assert_eq!(chars.len(), 255);
        TableEngine::write_records(&char_schema, &mut gd_arc, &chars, None).unwrap();
        assert_eq!(gd_arc.as_bytes(), orig_gd_bytes.as_slice());

        // 2. 菲莉西亚与阿库娅实机成长率校验 + 改写复读
        let felicia = chars.iter_mut().find(|r| r.id == "PID_フェリシア").unwrap();
        assert_ne!(felicia.display_name, felicia.id);
        assert_eq!(
            felicia.values.get("personal_growths"),
            Some(&FieldValue::IntArray(vec![40, 10, 35, 30, 40, 55, 15, 35]))
        );
        felicia.values.insert(
            "personal_growths".into(),
            FieldValue::IntArray(vec![70, 40, 65, 60, 70, 85, 45, 65]),
        );
        TableEngine::write_records(&char_schema, &mut gd_arc, &chars, None).unwrap();

        let reread = TableEngine::read_records(&char_schema, &gd_arc, Some(&messages), None).unwrap();
        let felicia_after = reread.iter().find(|r| r.id == "PID_フェリシア").unwrap();
        assert_eq!(
            felicia_after.values.get("personal_growths"),
            Some(&FieldValue::IntArray(vec![70, 40, 65, 60, 70, 85, 45, 65]))
        );

        // 3. 职业表校验（圣骑士男）
        let classes = TableEngine::read_records(&class_schema, &gd_arc, Some(&messages), None).unwrap();
        assert_eq!(classes.len(), 129);
        let paladin = classes.iter().find(|r| r.id == "JID_パラディン男").unwrap();
        assert_eq!(
            paladin.values.get("player_growths"),
            Some(&FieldValue::IntArray(vec![10, 15, 0, 10, 10, 15, 10, 10]))
        );
        assert_eq!(
            paladin.values.get("max_caps"),
            Some(&FieldValue::IntArray(vec![60, 31, 26, 30, 30, 32, 32, 32]))
        );
        println!(
            "[PASS] FE14 Schema 闭环校验通过：{} 角色 + {} 职业（菲莉西亚译名={}，+30% 写回复读一致）",
            chars.len(),
            classes.len(),
            felicia_after.display_name
        );
    }
}