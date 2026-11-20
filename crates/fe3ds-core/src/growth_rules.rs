use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SupportSpeedPreset {
    #[default]
    Vanilla,
    /// 2 倍速（升级点数约减半）
    Fast2x,
    /// 4 倍速（升级点数约压至 1/4）
    Fast4x,
    /// 极速（C/B/A/S 压至 1/2/3/4 点，几乎 1 战升级）
    Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WeaponExpPreset {
    #[default]
    Vanilla,
    /// 2 倍熟练度 / 战技 SP 减半
    Fast2x,
    /// 5 倍熟练度 / 战技 SP 压至 1/5
    Fast5x,
    /// 10 倍熟练度 / 战技 SP 压至 1/10
    Fast10x,
    /// 一击满级 / 1 战习得全部战技
    Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GrowthRulesConfig {
    pub support_speed: SupportSpeedPreset,
    pub weapon_exp: WeaponExpPreset,
}

impl GrowthRulesConfig {
    pub fn is_vanilla(&self) -> bool {
        self.support_speed == SupportSpeedPreset::Vanilla
            && self.weapon_exp == WeaponExpPreset::Vanilla
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrowthPatchStat {
    pub modified_entries: usize,
    pub sample_before: Vec<u8>,
    pub sample_after: Vec<u8>,
}

fn read_u32(buf: &[u8], off: usize) -> Result<u32, String> {
    let slice = buf
        .get(off..off + 4)
        .ok_or_else(|| format!("读取 u32 越界: 0x{off:X}"))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u16(buf: &[u8], off: usize) -> Result<u16, String> {
    let slice = buf
        .get(off..off + 2)
        .ok_or_else(|| format!("读取 u16 越界: 0x{off:X}"))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_cstr_bytes(buf: &[u8], off: usize) -> Option<&[u8]> {
    if off >= buf.len() {
        return None;
    }
    let end = buf[off..].iter().position(|&b| b == 0)?;
    if end == 0 || end > 128 {
        return None;
    }
    Some(&buf[off..off + end])
}

/// 解析 ISBin 头部并返回 (data_size, ptr1_count, ptr2_count)
fn parse_isbin_header(raw: &[u8]) -> Result<(usize, usize, usize), String> {
    if raw.len() < 0x20 {
        return Err(format!("ISBin 长度不足: {} B", raw.len()));
    }
    let data_size = read_u32(raw, 0x04)? as usize;
    let ptr1_count = read_u32(raw, 0x08)? as usize;
    let ptr2_count = read_u32(raw, 0x0C)? as usize;
    if 0x20 + data_size > raw.len() {
        return Err(format!(
            "ISBin data_size 越界: 0x{data_size:X} > 0x{:X}",
            raw.len()
        ));
    }
    Ok((data_size, ptr1_count, ptr2_count))
}

/// 在 ISBin 的 ptr2 标签表中按精确字节名查找 data 区偏移
fn find_ptr2_label(raw: &[u8], target_label: &[u8]) -> Result<Option<usize>, String> {
    let (data_size, ptr1_count, ptr2_count) = parse_isbin_header(raw)?;
    let ptr2_base = 0x20 + data_size + ptr1_count * 4;
    let label_base = ptr2_base + ptr2_count * 8;
    if label_base > raw.len() {
        return Err("ISBin ptr2 区域越界".to_string());
    }
    for i in 0..ptr2_count {
        let data_off = read_u32(raw, ptr2_base + i * 8)? as usize;
        let lbl_off = read_u32(raw, ptr2_base + i * 8 + 4)? as usize;
        if let Some(name) = read_cstr_bytes(raw, label_base + lbl_off) {
            if name == target_label {
                return Ok(Some(data_off));
            }
        }
    }
    Ok(None)
}

/// 按倍率缩放支援阈值数组（保留锁定占位值 sentinel，保证有效档位严格递增且不超过原值）
pub fn scale_support_thresholds(
    orig: &[u8],
    sentinel: u8,
    preset: SupportSpeedPreset,
) -> Vec<u8> {
    if preset == SupportSpeedPreset::Vanilla {
        return orig.to_vec();
    }
    let mut out = orig.to_vec();
    let mut prev: u8 = 0;
    for slot in &mut out {
        let v = *slot;
        if v == 0 || v == sentinel {
            continue;
        }
        let scaled = match preset {
            SupportSpeedPreset::Vanilla => v,
            SupportSpeedPreset::Fast2x => ((v as u16 + 1) / 2) as u8,
            SupportSpeedPreset::Fast4x => ((v as u16 + 2) / 4).max(1) as u8,
            SupportSpeedPreset::Instant => prev.saturating_add(1),
        };
        let next = scaled.max(prev.saturating_add(1)).min(v);
        *slot = next;
        prev = next;
    }
    out
}

/// FE13 支援度加速：修改 `data/person/static.bin.lz` 全员 52 支援槽阈值（99 为空/锁定）
pub fn apply_fe13_support_speed(
    raw: &mut [u8],
    preset: SupportSpeedPreset,
) -> Result<GrowthPatchStat, String> {
    let (data_size, _, _) = parse_isbin_header(raw)?;
    let data = &mut raw[0x20..0x20 + data_size];
    let char_count = read_u32(data, 0x04)? as usize;
    const STRIDE: usize = 0x238;
    const BASE: usize = 0x10;
    const SUPPORT_OFFSET: usize = 0x80;
    const MAX_SLOTS: usize = 52;
    const LOCKED: u8 = 99;

    if BASE + char_count * STRIDE > data.len() {
        return Err(format!(
            "FE13 static.bin 角色表越界: count={char_count}, data_len=0x{:X}",
            data.len()
        ));
    }

    let mut modified_entries = 0usize;
    let mut sample_before = Vec::new();
    let mut sample_after = Vec::new();

    for i in 0..char_count {
        let rec_off = BASE + i * STRIDE;
        for s in 0..MAX_SLOTS {
            let slot_off = rec_off + SUPPORT_OFFSET + s * 8;
            let partner_ptr = read_u32(data, slot_off)?;
            if partner_ptr == 0 {
                continue;
            }
            let before = data[slot_off + 4..slot_off + 8].to_vec();
            if before[0] == 0 || before[0] == LOCKED {
                continue;
            }
            let after = scale_support_thresholds(&before, LOCKED, preset);
            if sample_before.is_empty() {
                sample_before = before.clone();
                sample_after = after.clone();
            }
            if after != before {
                data[slot_off + 4..slot_off + 8].copy_from_slice(&after);
                modified_entries += 1;
            }
        }
    }

    Ok(GrowthPatchStat {
        modified_entries,
        sample_before,
        sample_after,
    })
}

/// FE13 武器熟练度加速：修改 `data/GameData.bin.lz` 中 `ItemData` 的单次 WExp (+0x1D)
pub fn apply_fe13_weapon_exp(
    raw: &mut [u8],
    preset: WeaponExpPreset,
) -> Result<GrowthPatchStat, String> {
    if preset == WeaponExpPreset::Vanilla {
        return Ok(GrowthPatchStat {
            modified_entries: 0,
            sample_before: vec![],
            sample_after: vec![],
        });
    }
    let (data_size, _, _) = parse_isbin_header(raw)?;
    let item_off = find_ptr2_label(raw, b"ItemData")?
        .ok_or_else(|| "FE13 GameData 缺少 ItemData 标签".to_string())?;
    let count_off = find_ptr2_label(raw, b"ItemDataNum")?
        .ok_or_else(|| "FE13 GameData 缺少 ItemDataNum 标签".to_string())?;

    let data = &mut raw[0x20..0x20 + data_size];
    let item_count = read_u32(data, count_off)? as usize;
    const STRIDE: usize = 0x38;
    const WEXP_REL: usize = 0x1D;
    const RANK_A_CAP: u8 = 91;

    if item_off + item_count * STRIDE > data.len() {
        return Err("FE13 ItemData 范围越界".to_string());
    }

    let mut modified_entries = 0usize;
    let mut sample_before = Vec::new();
    let mut sample_after = Vec::new();

    for i in 0..item_count {
        let pos = item_off + i * STRIDE + WEXP_REL;
        let orig = data[pos];
        if orig == 0 {
            continue;
        }
        let next = match preset {
            WeaponExpPreset::Vanilla => orig,
            WeaponExpPreset::Fast2x => orig.saturating_mul(2).min(RANK_A_CAP),
            WeaponExpPreset::Fast5x => orig.saturating_mul(5).min(RANK_A_CAP),
            WeaponExpPreset::Fast10x => orig.saturating_mul(10).min(RANK_A_CAP),
            WeaponExpPreset::Instant => RANK_A_CAP,
        };
        if sample_before.is_empty() {
            sample_before = vec![orig];
            sample_after = vec![next];
        }
        if next != orig {
            data[pos] = next;
            modified_entries += 1;
        }
    }

    Ok(GrowthPatchStat {
        modified_entries,
        sample_before,
        sample_after,
    })
}

/// FE14 支援度加速：通过 `GameData/GameData.bin.lz` 头部动态定位支援表（0xFF 为无 S 级）
pub fn apply_fe14_support_speed(
    raw: &mut [u8],
    preset: SupportSpeedPreset,
) -> Result<GrowthPatchStat, String> {
    let (data_size, _, _) = parse_isbin_header(raw)?;
    let data = &mut raw[0x20..0x20 + data_size];

    let char_header_off = read_u32(data, 0x08)? as usize;
    let support_table_off = read_u32(data, char_header_off + 0x08)? as usize;
    let char_count = read_u32(data, support_table_off)? as usize;
    if char_count == 0 || char_count > 256 {
        return Err(format!("FE14 支援表角色数异常: {char_count}"));
    }

    let mut modified_entries = 0usize;
    let mut sample_before = Vec::new();
    let mut sample_after = Vec::new();

    for i in 0..char_count {
        let sub_off = read_u32(data, support_table_off + 4 + i * 4)? as usize;
        let pair_count = read_u16(data, sub_off + 2)? as usize;
        for j in 0..pair_count {
            let rec_off = sub_off + 4 + j * 12;
            if rec_off + 12 > data.len() {
                return Err(format!("FE14 支援记录越界: 0x{rec_off:X}"));
            }
            let before = data[rec_off + 4..rec_off + 8].to_vec();
            let after = scale_support_thresholds(&before, 0xFF, preset);
            if sample_before.is_empty() && before[3] != 0xFF {
                sample_before = before.clone();
                sample_after = after.clone();
            }
            if after != before {
                data[rec_off + 4..rec_off + 8].copy_from_slice(&after);
                modified_entries += 1;
            }
        }
    }

    Ok(GrowthPatchStat {
        modified_entries,
        sample_before,
        sample_after,
    })
}

/// FE15 支援度加速：动态定位 `Data/Reliance.bin.lz` 内的 `REXID_` 支援曲线表
pub fn apply_fe15_support_speed(
    raw: &mut [u8],
    preset: SupportSpeedPreset,
) -> Result<GrowthPatchStat, String> {
    let (data_size, _, _) = parse_isbin_header(raw)?;
    let mut curve_offsets = Vec::new();

    for rel in (0..data_size.saturating_sub(8)).step_by(4) {
        let ptr_val = read_u32(raw, 0x20 + rel)? as usize;
        if let Some(name) = read_cstr_bytes(raw, 0x20 + ptr_val) {
            if name.starts_with(b"REXID_") {
                let c = raw[0x20 + rel + 4];
                let b = raw[0x20 + rel + 5];
                let a = raw[0x20 + rel + 6];
                let pad = raw[0x20 + rel + 7];
                if c > 0 && c < b && b < a && pad == 0 {
                    curve_offsets.push(0x20 + rel + 4);
                }
            }
        }
    }

    if curve_offsets.is_empty() {
        return Err("FE15 Reliance.bin 未找到有效的 REXID_ 支援曲线表".to_string());
    }

    let mut modified_entries = 0usize;
    let mut sample_before = Vec::new();
    let mut sample_after = Vec::new();

    for off in curve_offsets {
        let before = raw[off..off + 3].to_vec();
        let after = scale_support_thresholds(&before, 0xFF, preset);
        if sample_before.is_empty() {
            sample_before = before.clone();
            sample_after = after.clone();
        }
        if after != before {
            raw[off..off + 3].copy_from_slice(&after);
            modified_entries += 1;
        }
    }

    Ok(GrowthPatchStat {
        modified_entries,
        sample_before,
        sample_after,
    })
}

/// FE15 武器战技熟练度加速：修改 `Data/Skill.bin.lz` 战技习得所需 SP (+0x0D)
pub fn apply_fe15_art_sp(
    raw: &mut [u8],
    preset: WeaponExpPreset,
) -> Result<GrowthPatchStat, String> {
    if preset == WeaponExpPreset::Vanilla {
        return Ok(GrowthPatchStat {
            modified_entries: 0,
            sample_before: vec![],
            sample_after: vec![],
        });
    }
    let (data_size, _, _) = parse_isbin_header(raw)?;
    let data = &mut raw[0x20..0x20 + data_size];

    // Skill.bin 头部偏移 0x400 处存放表元信息：+0x04=count(u16), +0x06=stride(u16=0x68), +0x08=start(u32)
    let count = read_u16(data, 0x404)? as usize;
    let stride = read_u16(data, 0x406)? as usize;
    let start = read_u32(data, 0x408)? as usize;
    if stride != 0x68 || start + count * stride > data.len() {
        return Err(format!(
            "FE15 Skill.bin 表头校验失败: count={count}, stride=0x{stride:X}, start=0x{start:X}"
        ));
    }

    let mut modified_entries = 0usize;
    let mut sample_before = Vec::new();
    let mut sample_after = Vec::new();

    for i in 0..count {
        let off = start + i * stride;
        let is_art = data[off + 0x0C];
        let orig_sp = data[off + 0x0D];
        if is_art != 1 || orig_sp == 0 {
            continue;
        }
        let next_sp = match preset {
            WeaponExpPreset::Vanilla => orig_sp,
            WeaponExpPreset::Fast2x => ((orig_sp as u16 + 1) / 2).max(1) as u8,
            WeaponExpPreset::Fast5x => ((orig_sp as u16 + 4) / 5).max(1) as u8,
            WeaponExpPreset::Fast10x => ((orig_sp as u16 + 9) / 10).max(1) as u8,
            WeaponExpPreset::Instant => 1,
        };
        if sample_before.is_empty() {
            sample_before = vec![orig_sp];
            sample_after = vec![next_sp];
        }
        if next_sp != orig_sp {
            data[off + 0x0D] = next_sp;
            modified_entries += 1;
        }
    }

    Ok(GrowthPatchStat {
        modified_entries,
        sample_before,
        sample_after,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn scale_thresholds_preserves_monotonicity_and_sentinels() {
        assert_eq!(
            scale_support_thresholds(&[5, 10, 16, 22], 99, SupportSpeedPreset::Fast2x),
            vec![3, 5, 8, 11]
        );
        assert_eq!(
            scale_support_thresholds(&[4, 10, 18, 99], 99, SupportSpeedPreset::Instant),
            vec![1, 2, 3, 99]
        );
        assert_eq!(
            scale_support_thresholds(&[3, 7, 12, 255], 255, SupportSpeedPreset::Fast4x),
            vec![1, 2, 3, 255]
        );
        assert_eq!(
            scale_support_thresholds(&[20, 45, 90], 255, SupportSpeedPreset::Instant),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn verify_real_fe13_growth_rules_if_present() {
        let static_path = Path::new(r"D:\fe13\RomFS\data\person\static.bin.lz");
        let gd_path = Path::new(r"D:\fe13\RomFS\data\GameData.bin.lz");
        if !static_path.exists() || !gd_path.exists() {
            return;
        }
        let (mut raw_static, _) =
            crate::lz11::decompress_lz11(&fs::read(static_path).unwrap()).unwrap();
        let sup = apply_fe13_support_speed(&mut raw_static, SupportSpeedPreset::Instant).unwrap();
        assert_eq!(sup.modified_entries, 632);
        assert_eq!(sup.sample_after[..3], [1, 2, 3]);

        let (mut raw_gd, _) = crate::lz11::decompress_lz11(&fs::read(gd_path).unwrap()).unwrap();
        let wexp = apply_fe13_weapon_exp(&mut raw_gd, WeaponExpPreset::Fast5x).unwrap();
        assert_eq!(wexp.modified_entries, 144);
        assert_eq!(wexp.sample_before, vec![1]);
        assert_eq!(wexp.sample_after, vec![5]);
    }

    #[test]
    fn verify_real_fe14_support_rule_if_present() {
        let gd_path = Path::new(r"D:\fe14\RomFS\GameData\GameData.bin.lz");
        if !gd_path.exists() {
            return;
        }
        let (mut raw_gd, _) = crate::lz11::decompress_lz11(&fs::read(gd_path).unwrap()).unwrap();
        let sup = apply_fe14_support_speed(&mut raw_gd, SupportSpeedPreset::Fast2x).unwrap();
        assert_eq!(sup.modified_entries, 1234);
        assert_eq!(sup.sample_before, vec![3, 7, 12, 18]);
        assert_eq!(sup.sample_after, vec![2, 4, 6, 9]);
    }

    #[test]
    fn verify_real_fe15_growth_rules_if_present() {
        let rel_path = Path::new(r"D:\fe15\RomFS\Data\Reliance.bin.lz");
        let skill_path = Path::new(r"D:\fe15\RomFS\Data\Skill.bin.lz");
        if !rel_path.exists() || !skill_path.exists() {
            return;
        }
        let (mut raw_rel, _) = crate::lz11::decompress_lz11(&fs::read(rel_path).unwrap()).unwrap();
        let sup = apply_fe15_support_speed(&mut raw_rel, SupportSpeedPreset::Fast4x).unwrap();
        assert_eq!(sup.modified_entries, 3);
        assert_eq!(sup.sample_before, vec![20, 45, 90]);
        assert_eq!(sup.sample_after, vec![5, 11, 23]);

        let (mut raw_sk, _) = crate::lz11::decompress_lz11(&fs::read(skill_path).unwrap()).unwrap();
        let sp = apply_fe15_art_sp(&mut raw_sk, WeaponExpPreset::Instant).unwrap();
        assert_eq!(sp.modified_entries, 54);
        assert_eq!(sp.sample_after, vec![1]);
    }
}