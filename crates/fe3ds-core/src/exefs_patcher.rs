use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::growth_rules::WeaponExpPreset;

pub const CODE_VADDR_BASE: u32 = 0x0010_0000;

/// FE13 `code.bin` 经验结算点 AOB 特征码（对应文件偏移 `0x1862E0` / 内存 `0x002862E0`）
pub const FE13_EXP_AOB: [u8; 16] = [
    0x64, 0x00, 0x51, 0xE3, 0x01, 0x40, 0xA0, 0xE1,
    0x8D, 0x60, 0xD5, 0xE5, 0x68, 0x00, 0x95, 0xE5,
];
/// FE14 `code.bin` 经验结算入口 AOB 特征码（对应 `Unit::AddExp`，文件偏移 `0x42D7DC` / 内存 `0x0052D7DC`）
pub const FE14_EXP_AOB: [u8; 20] = [
    0x64, 0x00, 0x51, 0xE3, // CMP   R1, #0x64
    0x00, 0x40, 0xA0, 0xE1, // MOV   R4, R0
    0x64, 0x10, 0xA0, 0xC3, // MOVGT R1, #0x64
    0xA0, 0x00, 0x94, 0xE5, // LDR   R0, [R4, #0xA0]
    0x01, 0x50, 0xA0, 0xE1, // MOV   R5, R1
];
/// FE15 `code.bin` 经验结算双位点 AOB 特征码（`0x350B6C` 战斗经验 / `0x353770` 行动经验）
pub const FE15_EXP_AOB: [u8; 24] = [
    0x80, 0x00, 0xA0, 0x11, // MOVNE R0, R0, LSL #1 (SID_エリート)
    0x64, 0x00, 0x50, 0xE3, // CMP   R0, #0x64
    0x64, 0x00, 0xA0, 0xA3, // MOVGE R0, #0x64
    0x01, 0x00, 0x00, 0xAA, // BGE   +1
    0x01, 0x00, 0x50, 0xE3, // CMP   R0, #1
    0x01, 0x00, 0xA0, 0xD3, // MOVLE R0, #1
];

/// FE14 用杖武器熟练度 AOB（`0x332010`，`+8` 为 `MOV R2, #2`）
pub const FE14_WEXP_STAFF_AOB: [u8; 16] = [
    0x00, 0x60, 0xA0, 0xE1, // MOV  R6, R0
    0x3C, 0x00, 0x95, 0xE5, // LDR  R0, [R5, #0x3C]
    0x02, 0x20, 0xA0, 0xE3, // MOV  R2, #2
    0x07, 0x10, 0xA0, 0xE1, // MOV  R1, R7
];

/// FE14 攻方战后武器熟练度结算 AOB（`0x442278`，无 BL 偏移）
pub const FE14_WEXP_ATK_AOB: [u8; 20] = [
    0xD1, 0x00, 0xD0, 0xE1, // LDRSB R0, [R0, #1]
    0xD1, 0x10, 0xD7, 0xE1, // LDRSB R1, [R7, #1]
    0x05, 0x00, 0x80, 0xE0, // ADD   R0, R0, R5
    0x10, 0x20, 0xD0, 0xE5, // LDRB  R2, [R0, #0x10]
    0x06, 0x00, 0xA0, 0xE1, // MOV   R0, R6
];

/// FE14 守方战后武器熟练度结算 AOB（`0x442414`，无 BL 偏移）
pub const FE14_WEXP_DEF_AOB: [u8; 20] = [
    0xD1, 0x00, 0xD0, 0xE1, // LDRSB R0, [R0, #1]
    0xD1, 0x10, 0xD6, 0xE1, // LDRSB R1, [R6, #1]
    0x05, 0x00, 0x80, 0xE0, // ADD   R0, R0, R5
    0x10, 0x20, 0xD0, 0xE5, // LDRB  R2, [R0, #0x10]
    0x07, 0x00, 0xA0, 0xE1, // MOV   R0, R7
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpMultiplier {
    Half,
    OnePointFive,
    Double,
    Triple,
    Quadruple,
    Quintuple,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ExpPatchMode {
    Vanilla,
    Multiplier { preset: ExpMultiplier },
    Fixed { value: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodePatchRecord {
    pub file_offset: usize,
    pub payload: Vec<u8>,
}

/// 在 `code.bin` 字节流中扫描 AOB 特征码并返回首次命中文件偏移。
pub fn find_aob(code: &[u8], pattern: &[u8]) -> Option<usize> {
    if pattern.is_empty() || pattern.len() > code.len() {
        return None;
    }
    code.windows(pattern.len()).position(|w| w == pattern)
}
/// 扫描 `code.bin` 中所有命中的 AOB 特征码偏移。
pub fn find_all_aob(code: &[u8], pattern: &[u8]) -> Vec<usize> {
    if pattern.is_empty() || pattern.len() > code.len() {
        return Vec::new();
    }
    code.windows(pattern.len())
        .enumerate()
        .filter_map(|(i, w)| (w == pattern).then_some(i))
        .collect()
}

/// 定位 FE13 战后经验结算注入点。
pub fn locate_fe13_exp_site(code: &[u8]) -> CoreResult<usize> {
    find_aob(code, &FE13_EXP_AOB)
        .ok_or_else(|| CoreError::PatternNotFound("FE13 经验结算 AOB 特征码未命中".into()))
}
/// 定位 FE14 战后经验结算注入点。
pub fn locate_fe14_exp_site(code: &[u8]) -> CoreResult<usize> {
    find_aob(code, &FE14_EXP_AOB)
        .ok_or_else(|| CoreError::PatternNotFound("FE14 经验结算 AOB 特征码未命中".into()))
}

/// 按作品 ID 定位战后经验注入点。
/// 定位 FE15 战斗与行动经验结算注入点（返回所有命中偏移）。
pub fn locate_fe15_exp_sites(code: &[u8]) -> CoreResult<Vec<usize>> {
    let sites = find_all_aob(code, &FE15_EXP_AOB);
    if sites.is_empty() {
        return Err(CoreError::PatternNotFound("FE15 经验结算 AOB 特征码未命中".into()));
    }
    Ok(sites)
}

/// 按作品 ID 定位战后经验主注入点。
pub fn locate_exp_site(game_id: &str, code: &[u8]) -> CoreResult<usize> {
    match game_id {
        "fe13" => locate_fe13_exp_site(code),
        "fe14" => locate_fe14_exp_site(code),
        "fe15" => locate_fe15_exp_sites(code).map(|v| v[0]),
        other => Err(CoreError::Schema(format!("暂不支持该作品的经验补丁: {other}"))),
    }
}
/// 生成 FE13 经验修改的 8 字节 ARM32 补丁指令（`Vanilla` 模式返回 `Ok(None)`）。
pub fn build_fe13_exp_patch(
    code: &[u8],
    mode: ExpPatchMode,
) -> CoreResult<Option<CodePatchRecord>> {
    let insn1: u32 = match mode {
        ExpPatchMode::Vanilla => return Ok(None),
        ExpPatchMode::Multiplier { preset } => match preset {
            ExpMultiplier::Half => 0xE1A0_40A1,         // MOV R4, R1, LSR #1
            ExpMultiplier::OnePointFive => 0xE081_40A1, // ADD R4, R1, R1, LSR #1
            ExpMultiplier::Double => 0xE1A0_4081,       // MOV R4, R1, LSL #1
            ExpMultiplier::Triple => 0xE081_4081,       // ADD R4, R1, R1, LSL #1
            ExpMultiplier::Quadruple => 0xE1A0_4101,    // MOV R4, R1, LSL #2
            ExpMultiplier::Quintuple => 0xE081_4101,    // ADD R4, R1, R1, LSL #2
        },
        ExpPatchMode::Fixed { value } => {
            if !(1..=100).contains(&value) {
                return Err(CoreError::Schema(format!(
                    "固定经验值必须在 1..=100 之间，当前值: {value}"
                )));
            }
            0xE3A0_4000 | (value as u32) // MOV R4, #value
        }
    };

    let file_offset = locate_fe13_exp_site(code)?;
    let insn2: u32 = 0xE354_0064; // CMP R4, #0x64（保留原版 100 经验封顶）
    let mut payload = Vec::with_capacity(8);
    payload.extend_from_slice(&insn1.to_le_bytes());
    payload.extend_from_slice(&insn2.to_le_bytes());

    Ok(Some(CodePatchRecord {
        file_offset,
        payload,
    }))
}
/// 生成 FE14 经验修改的 20 字节 ARM32 补丁指令（`Vanilla` 模式返回 `Ok(None)`）。
pub fn build_fe14_exp_patch(
    code: &[u8],
    mode: ExpPatchMode,
) -> CoreResult<Option<CodePatchRecord>> {
    let insns: [u32; 5] = match mode {
        ExpPatchMode::Vanilla => return Ok(None),
        ExpPatchMode::Multiplier { preset } => {
            let scale_insn: u32 = match preset {
                ExpMultiplier::Half => 0xE1A0_50A1,         // MOV R5, R1, LSR #1
                ExpMultiplier::OnePointFive => 0xE081_50A1, // ADD R5, R1, R1, LSR #1
                ExpMultiplier::Double => 0xE1A0_5081,       // MOV R5, R1, LSL #1
                ExpMultiplier::Triple => 0xE081_5081,       // ADD R5, R1, R1, LSL #1
                ExpMultiplier::Quadruple => 0xE1A0_5101,    // MOV R5, R1, LSL #2
                ExpMultiplier::Quintuple => 0xE081_5101,    // ADD R5, R1, R1, LSL #2
            };
            [
                0xE1A0_4000, // MOV   R4, R0
                scale_insn,  // R5 = scaled(R1)
                0xE355_0064, // CMP   R5, #0x64
                0xC3A0_5064, // MOVGT R5, #0x64
                0xE594_00A0, // LDR   R0, [R4, #0xA0]
            ]
        }
        ExpPatchMode::Fixed { value } => {
            if !(1..=100).contains(&value) {
                return Err(CoreError::Schema(format!(
                    "固定经验值必须在 1..=100 之间，当前值: {value}"
                )));
            }
            [
                0xE1A0_4000,                  // MOV   R4, R0
                0xE351_0000,                  // CMP   R1, #0
                0xC3A0_5000 | (value as u32), // MOVGT R5, #value
                0xD3A0_5000,                  // MOVLE R5, #0
                0xE594_00A0,                  // LDR   R0, [R4, #0xA0]
            ]
        }
    };

    let file_offset = locate_fe14_exp_site(code)?;
    let mut payload = Vec::with_capacity(20);
    for insn in insns {
        payload.extend_from_slice(&insn.to_le_bytes());
    }

    Ok(Some(CodePatchRecord {
        file_offset,
        payload,
    }))
}

/// 按作品 ID 生成战后经验补丁记录。
pub fn build_exp_patch(
    game_id: &str,
    code: &[u8],
    mode: ExpPatchMode,
) -> CoreResult<Option<CodePatchRecord>> {
    match game_id {
        "fe13" => build_fe13_exp_patch(code, mode),
        "fe14" => build_fe14_exp_patch(code, mode),
        other => Err(CoreError::Schema(format!("暂不支持该作品的经验补丁: {other}"))),
    }
}
/// 生成 FE15 经验修改补丁（同时覆盖战斗与白魔法/行动经验双位点，每处仅改 4 字节）。
pub fn build_fe15_exp_patches(
    code: &[u8],
    mode: ExpPatchMode,
) -> CoreResult<Vec<CodePatchRecord>> {
    let insn: u32 = match mode {
        ExpPatchMode::Vanilla => return Ok(Vec::new()),
        ExpPatchMode::Multiplier { preset } => match preset {
            ExpMultiplier::Half => 0xE1A0_00A0,         // MOV R0, R0, LSR #1
            ExpMultiplier::OnePointFive => 0xE080_00A0, // ADD R0, R0, R0, LSR #1
            ExpMultiplier::Double => 0xE1A0_0080,       // MOV R0, R0, LSL #1
            ExpMultiplier::Triple => 0xE080_0080,       // ADD R0, R0, R0, LSL #1
            ExpMultiplier::Quadruple => 0xE1A0_0100,    // MOV R0, R0, LSL #2
            ExpMultiplier::Quintuple => 0xE080_0100,    // ADD R0, R0, R0, LSL #2
        },
        ExpPatchMode::Fixed { value } => {
            if !(1..=100).contains(&value) {
                return Err(CoreError::Schema(format!(
                    "固定经验值必须在 1..=100 之间，当前值: {value}"
                )));
            }
            0xE3A0_0000 | (value as u32) // MOV R0, #value
        }
    };

    let sites = locate_fe15_exp_sites(code)?;
    let payload = insn.to_le_bytes().to_vec();
    Ok(sites
        .into_iter()
        .map(|file_offset| CodePatchRecord {
            file_offset,
            payload: payload.clone(),
        })
        .collect())
}

/// 按作品 ID 生成全部战后经验补丁记录（支持 FE13/FE14 单点与 FE15 双点）。
pub fn build_exp_patches(
    game_id: &str,
    code: &[u8],
    mode: ExpPatchMode,
) -> CoreResult<Vec<CodePatchRecord>> {
    match game_id {
        "fe13" => Ok(build_fe13_exp_patch(code, mode)?.into_iter().collect()),
        "fe14" => Ok(build_fe14_exp_patch(code, mode)?.into_iter().collect()),
        "fe15" => build_fe15_exp_patches(code, mode),
        other => Err(CoreError::Schema(format!("暂不支持该作品的经验补丁: {other}"))),
    }
}
/// 生成 FE14 武器熟练度加速补丁（攻方战后 + 守方战后 + 用杖，共 3 处零副作用注入点）。
pub fn build_fe14_wexp_patches(
    code: &[u8],
    preset: WeaponExpPreset,
) -> CoreResult<Vec<CodePatchRecord>> {
    let (battle_scale_insn, staff_val): (u32, u8) = match preset {
        WeaponExpPreset::Vanilla => return Ok(Vec::new()),
        WeaponExpPreset::Fast2x => (0xE1A0_2082, 4),   // MOV R2, R2, LSL #1  (2x)
        WeaponExpPreset::Fast5x => (0xE082_2102, 10),  // ADD R2, R2, R2, LSL #2 (5x)
        WeaponExpPreset::Fast10x => (0xE1A0_2182, 16), // MOV R2, R2, LSL #3  (8x)
        WeaponExpPreset::Instant => (0xE1A0_2402, 251), // MOV R2, R2, LSL #8 (自动封顶至职业上限)
    };

    let staff_site = find_aob(code, &FE14_WEXP_STAFF_AOB)
        .ok_or_else(|| CoreError::PatternNotFound("FE14 用杖熟练度 AOB 未命中".into()))?;
    let atk_site = find_aob(code, &FE14_WEXP_ATK_AOB)
        .ok_or_else(|| CoreError::PatternNotFound("FE14 攻方武器熟练度 AOB 未命中".into()))?;
    let def_site = find_aob(code, &FE14_WEXP_DEF_AOB)
        .ok_or_else(|| CoreError::PatternNotFound("FE14 守方武器熟练度 AOB 未命中".into()))?;

    // 合并重复的武器类型读取，腾出 1 条指令槽位原位缩放 R2（保持 R2==0 不变）
    let battle_insns: [u32; 4] = [
        0xE1D0_10D1,       // LDRSB R1, [R0, #1]
        0xE081_0005,       // ADD   R0, R1, R5
        0xE5D0_2010,       // LDRB  R2, [R0, #0x10]
        battle_scale_insn, // 缩放 R2
    ];
    let mut battle_payload = Vec::with_capacity(16);
    for insn in battle_insns {
        battle_payload.extend_from_slice(&insn.to_le_bytes());
    }

    let staff_insn: u32 = 0xE3A0_2000 | (staff_val as u32); // MOV R2, #staff_val

    Ok(vec![
        CodePatchRecord {
            file_offset: staff_site + 8,
            payload: staff_insn.to_le_bytes().to_vec(),
        },
        CodePatchRecord {
            file_offset: atk_site,
            payload: battle_payload.clone(),
        },
        CodePatchRecord {
            file_offset: def_site,
            payload: battle_payload,
        },
    ])
}
/// 构建标准 3DS IPS 补丁（可直接保存为 `mods/{TitleID}/exefs/code.ips`）。
pub fn build_ips(records: &[CodePatchRecord]) -> CoreResult<Vec<u8>> {
    let mut sorted: Vec<&CodePatchRecord> = records.iter().collect();
    sorted.sort_by_key(|r| r.file_offset);

    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(b"PATCH");

    for rec in sorted {
        if rec.payload.is_empty() {
            continue;
        }
        if rec.file_offset > 0x00FF_FFFF {
            return Err(CoreError::OutOfBounds(format!(
                "标准 IPS 偏移超出 24 位上限: 0x{:X}",
                rec.file_offset
            )));
        }
        if rec.file_offset == 0x45_4F46 {
            return Err(CoreError::OutOfBounds(
                "IPS 偏移命中 EOF 保留字 0x454F46，需向前扩展 1 字节对齐".into(),
            ));
        }
        if rec.payload.len() > u16::MAX as usize {
            return Err(CoreError::OutOfBounds(format!(
                "单条 IPS 补丁长度超出 65535 字节: {}",
                rec.payload.len()
            )));
        }

        out.push(((rec.file_offset >> 16) & 0xFF) as u8);
        out.push(((rec.file_offset >> 8) & 0xFF) as u8);
        out.push((rec.file_offset & 0xFF) as u8);
        out.extend_from_slice(&(rec.payload.len() as u16).to_be_bytes());
        out.extend_from_slice(&rec.payload);
    }

    out.extend_from_slice(b"EOF");
    Ok(out)
}

/// 生成 Azahar / Citra 原生 `.txt` 金手指文本（自动启用 `*citra_enabled`）。
pub fn build_azahar_cheat(title: &str, record: &CodePatchRecord) -> CoreResult<String> {
    if record.payload.is_empty() || record.payload.len() % 4 != 0 {
        return Err(CoreError::Schema(format!(
            "ARM32 金手指负载长度必须是 4 的正整数倍，当前为 {} 字节",
            record.payload.len()
        )));
    }
    let base_vaddr = (record.file_offset as u32)
        .checked_add(CODE_VADDR_BASE)
        .ok_or_else(|| CoreError::OutOfBounds("内存虚拟地址溢出".into()))?;

    let mut lines = vec![format!("[{title}]"), "*citra_enabled".to_string()];
    for (idx, chunk) in record.payload.chunks_exact(4).enumerate() {
        let word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let vaddr = base_vaddr + (idx as u32) * 4;
        lines.push(format!("{vaddr:08X} {word:08X}"));
    }

    Ok(lines.join("\n") + "\n")
}
/// 将多条补丁记录合并为单条 Azahar / Citra `.txt` 金手指。
pub fn build_azahar_cheat_multi(title: &str, records: &[CodePatchRecord]) -> CoreResult<String> {
    let mut lines = vec![format!("[{title}]"), "*citra_enabled".to_string()];
    for record in records {
        if record.payload.is_empty() || record.payload.len() % 4 != 0 {
            return Err(CoreError::Schema(format!(
                "ARM32 金手指负载长度必须是 4 的正整数倍，当前为 {} 字节",
                record.payload.len()
            )));
        }
        let base_vaddr = (record.file_offset as u32)
            .checked_add(CODE_VADDR_BASE)
            .ok_or_else(|| CoreError::OutOfBounds("内存虚拟地址溢出".into()))?;
        for (idx, chunk) in record.payload.chunks_exact(4).enumerate() {
            let word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let vaddr = base_vaddr + (idx as u32) * 4;
            lines.push(format!("{vaddr:08X} {word:08X}"));
        }
    }
    Ok(lines.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn generate_ips_and_cheat_from_synthetic_code() {
        let mut fake_code = vec![0u8; 0x200];
        fake_code[0x100..0x110].copy_from_slice(&FE13_EXP_AOB);

        let rec = build_fe13_exp_patch(&fake_code, ExpPatchMode::Fixed { value: 25 })
            .unwrap()
            .unwrap();
        assert_eq!(rec.file_offset, 0x100);

        let ips = build_ips(&[rec.clone()]).unwrap();
        assert_eq!(&ips[..5], b"PATCH");
        assert_eq!(&ips[5..8], &[0x00, 0x01, 0x00]);
        assert_eq!(&ips[8..10], &[0x00, 0x08]);
        assert_eq!(&ips[ips.len() - 3..], b"EOF");

        let cheat = build_azahar_cheat("FE13 Fixed 25 EXP", &rec).unwrap();
        assert!(cheat.contains("[FE13 Fixed 25 EXP]\n*citra_enabled\n00100100 E3A04019\n00100104 E3540064\n"));
    }

    #[test]
    fn reject_invalid_fixed_exp_range() {
        assert!(build_fe13_exp_patch(&FE13_EXP_AOB, ExpPatchMode::Fixed { value: 0 }).is_err());
        assert!(build_fe13_exp_patch(&FE13_EXP_AOB, ExpPatchMode::Fixed { value: 101 }).is_err());
    }

    #[test]
    fn verify_real_fe13_code_bin_if_present() {
        let code_path = Path::new(r"D:\fe13\ExeFS\code.bin");
        if !code_path.exists() {
            return;
        }
        let code = std::fs::read(code_path).unwrap();
        let rec = build_fe13_exp_patch(&code, ExpPatchMode::Fixed { value: 25 })
            .unwrap()
            .unwrap();
        assert_eq!(rec.file_offset, 0x1862E0);

        let cheat = build_azahar_cheat("FE13 Fixed 25 EXP", &rec).unwrap();
        assert!(cheat.contains("002862E0 E3A04019"));
        assert!(cheat.contains("002862E4 E3540064"));

        let ips = build_ips(&[rec]).unwrap();
        assert_eq!(ips.len(), 5 + 3 + 2 + 8 + 3);
        println!(
            "[PASS] FE13 code.bin AOB 命中 0x1862E0 | IPS={}B | Azahar 金手指生成正常",
            ips.len()
        );
    }
    #[test]
    fn generate_fe14_exp_patch_from_synthetic_code() {
        let mut fake_code = vec![0u8; 0x200];
        fake_code[0x120..0x134].copy_from_slice(&FE14_EXP_AOB);

        let rec = build_exp_patch("fe14", &fake_code, ExpPatchMode::Multiplier { preset: ExpMultiplier::Double })
            .unwrap()
            .unwrap();
        assert_eq!(rec.file_offset, 0x120);
        assert_eq!(rec.payload.len(), 20);
        let cheat = build_azahar_cheat("FE14 2x EXP", &rec).unwrap();
        assert!(cheat.contains("00100124 E1A05081"));
        assert!(cheat.contains("00100130 E59400A0"));
    }

    #[test]
    fn verify_real_fe14_cci_code_bin_if_present() {
        let rom_path = Path::new(r"D:\fe14.cci");
        if !rom_path.exists() {
            return;
        }
        let rom = crate::vfs::RomImage::open(rom_path).unwrap();
        let code = rom.read_code_bin().unwrap().unwrap();
        let rec = build_exp_patch(
            "fe14",
            &code,
            ExpPatchMode::Multiplier {
                preset: ExpMultiplier::Double,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(rec.file_offset, 0x42D7DC);
        let ips = build_ips(&[rec]).unwrap();
        assert_eq!(ips.len(), 5 + 3 + 2 + 20 + 3);
        println!(
            "[PASS] FE14 code.bin AOB 命中 0x42D7DC | IPS={}B | 2x EXP 补丁生成正常",
            ips.len()
        );
    }
    
    #[test]
    fn verify_real_fe14_wexp_code_bin_if_present() {
        let code_path = Path::new(r"D:\fe14\ExeFS\code.bin");
        if !code_path.exists() {
            return;
        }
        let code = std::fs::read(code_path).unwrap();
        let recs = build_fe14_wexp_patches(&code, WeaponExpPreset::Fast5x).unwrap();
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0].file_offset, 0x332018);
        assert_eq!(recs[1].file_offset, 0x442278);
        assert_eq!(recs[2].file_offset, 0x442414);

        let ips = build_ips(&recs).unwrap();
        assert!(ips.starts_with(b"PATCH") && ips.ends_with(b"EOF"));
        let cheat = build_azahar_cheat_multi("FE14 5x Weapon EXP", &recs).unwrap();
        assert!(cheat.contains("00432018 E3A0200A"));
        assert!(cheat.contains("00542284 E0822102"));
        assert!(cheat.contains("00542420 E0822102"));
    }
}