use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};

/// 成长率字节流混淆参数（可通过 Schema 扩展到不同表或作品）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CipherProfile {
    pub c1: u8,
    pub c5: u8,
    pub idx_mul: u8,
    pub stat_step: u8,
    pub xor_mask: u8,
}

impl CipherProfile {
    /// FE13 `RomFS/data/person/static.bin` 人物成长率（57 角色全量验真）
    pub const FE13_PERSON: Self = Self {
        c1: 0x18,
        c5: 0x22,
        idx_mul: 0x63,
        stat_step: 0x3D,
        xor_mask: 0x26,
    };

    /// FE13 `RomFS/data/GameData.bin` 职业成长率（83 职业全量验真）
    pub const FE13_JOB: Self = Self {
        c1: 0x39,
        c5: 0x22,
        idx_mul: 0x23,
        stat_step: 0x0D,
        xor_mask: 0x07,
    };

    #[inline]
    pub fn key_byte(&self, entry_index: usize, stat_index: usize) -> u8 {
        let seed = self
            .c5
            .wrapping_add(self.idx_mul.wrapping_mul((entry_index as u8) ^ self.c1));
        seed.wrapping_sub((stat_index as u8).wrapping_mul(self.stat_step)) ^ self.xor_mask
    }
}

/// 基于 `GameData.bin -> CipherData` 的 256 字节双向无损成长率加解密器。
#[derive(Debug, Clone)]
pub struct GrowthCipher {
    decode_table: [u8; 256],
    encode_table: [u8; 256],
}

impl GrowthCipher {
    pub fn from_table(table: &[u8]) -> CoreResult<Self> {
        if table.len() != 256 {
            return Err(CoreError::InvalidCipher(format!(
                "CipherData 长度必须为 256 字节，实际得到 {} 字节",
                table.len()
            )));
        }

        let mut decode_table = [0u8; 256];
        let mut encode_table = [0u8; 256];
        let mut seen = [false; 256];

        for (i, &val) in table.iter().enumerate() {
            if seen[val as usize] {
                return Err(CoreError::InvalidCipher(format!(
                    "CipherData 不是合法双射置换表：值 0x{val:02X} 重复出现"
                )));
            }
            seen[val as usize] = true;
            decode_table[i] = val;
            encode_table[val as usize] = i as u8;
        }

        Ok(Self {
            decode_table,
            encode_table,
        })
    }

    pub fn decode_stats(
        &self,
        profile: CipherProfile,
        entry_index: usize,
        encrypted: &[u8; 8],
    ) -> [u8; 8] {
        let mut out = [0u8; 8];
        for (stat_idx, &enc) in encrypted.iter().enumerate() {
            let k = profile.key_byte(entry_index, stat_idx);
            out[stat_idx] = self.decode_table[enc.wrapping_sub(k) as usize];
        }
        out
    }

    pub fn encode_stats(
        &self,
        profile: CipherProfile,
        entry_index: usize,
        plain: &[u8; 8],
    ) -> [u8; 8] {
        let mut out = [0u8; 8];
        for (stat_idx, &val) in plain.iter().enumerate() {
            let k = profile.key_byte(entry_index, stat_idx);
            out[stat_idx] = self.encode_table[val as usize].wrapping_add(k);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bin_archive::ISBinArchive;
    use crate::lz11::decompress_lz11;
    use std::path::Path;

    #[test]
    fn reject_non_bijective_table() {
        assert!(GrowthCipher::from_table(&[0u8; 10]).is_err());
        assert!(GrowthCipher::from_table(&[0u8; 256]).is_err());
    }

    #[test]
    fn roundtrip_synthetic_cipher() {
        let table: Vec<u8> = (0..=255u8).rev().collect();
        let cipher = GrowthCipher::from_table(&table).unwrap();
        let plain = [45, 40, 10, 40, 40, 70, 35, 20];

        for profile in [CipherProfile::FE13_PERSON, CipherProfile::FE13_JOB] {
            for idx in [0, 3, 8, 56, 82] {
                let enc = cipher.encode_stats(profile, idx, &plain);
                let dec = cipher.decode_stats(profile, idx, &enc);
                assert_eq!(dec, plain);
            }
        }
    }

    #[test]
    fn verify_real_fe13_growths_if_present() {
        let gd_path = Path::new(r"D:\fe13\RomFS\data\GameData.bin.lz");
        let st_path = Path::new(r"D:\fe13\RomFS\data\person\static.bin.lz");
        if !gd_path.exists() || !st_path.exists() {
            return;
        }

        let (gd_raw, _) = decompress_lz11(&std::fs::read(gd_path).unwrap()).unwrap();
        let gd_arc = ISBinArchive::parse(gd_raw).unwrap();
        let cipher_off = gd_arc.section_offset("CipherData").expect("缺少 CipherData");
        let cipher = GrowthCipher::from_table(gd_arc.read_slice(cipher_off, 256).unwrap()).unwrap();

        let (st_raw, _) = decompress_lz11(&std::fs::read(st_path).unwrap()).unwrap();
        let st_arc = ISBinArchive::parse(st_raw).unwrap();

        for (idx, sec) in st_arc.sections().iter().enumerate() {
            let enc: [u8; 8] = st_arc.read_slice(sec.offset + 0x24, 8).unwrap().try_into().unwrap();
            let plain = cipher.decode_stats(CipherProfile::FE13_PERSON, idx, &enc);
            let re_enc = cipher.encode_stats(CipherProfile::FE13_PERSON, idx, &plain);
            assert_eq!(re_enc, enc, "人物加密往返不一致: {}", sec.name);

            if sec.name == "PID_クロム" {
                assert_eq!(plain, [45, 40, 10, 40, 40, 70, 35, 20]);
            } else if sec.name == "PID_リズ" {
                assert_eq!(plain, [35, 25, 35, 30, 35, 65, 15, 35]);
            }
        }

        let jobs = gd_arc.sections_with_prefix("JID_");
        for (idx, sec) in jobs.iter().enumerate() {
            let enc: [u8; 8] = gd_arc.read_slice(sec.offset + 0x20, 8).unwrap().try_into().unwrap();
            let plain = cipher.decode_stats(CipherProfile::FE13_JOB, idx, &enc);
            let re_enc = cipher.encode_stats(CipherProfile::FE13_JOB, idx, &plain);
            assert_eq!(re_enc, enc, "职业加密往返不一致: {}", sec.name);

            if sec.name == "JID_ロード男" {
                assert_eq!(plain, [40, 20, 0, 20, 20, 0, 10, 5]);
            }
        }

        println!(
            "[PASS] FE13 全量成长率加解密校验通过：{} 名角色 + {} 个职业",
            st_arc.sections().len(),
            jobs.len()
        );
    }
}