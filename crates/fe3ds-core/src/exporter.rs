use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::exefs_patcher::{build_azahar_cheat_multi, build_ips, CodePatchRecord};
use crate::lz11::compress_lz11;

#[derive(Debug, Clone)]
pub struct RomFsFilePatch {
    /// 相对路径（可带或不带 `RomFS/` 前缀，例如 `RomFS/data/person/static.bin.lz`）
    pub rel_path: String,
    /// 已修改的未压缩 `.bin` 原始字节
    pub raw_data: Vec<u8>,
    /// 是否保留 FE14/FE15 的 0x13 外层封装头
    pub add_lz13_wrapper: bool,
}

#[derive(Debug, Clone)]
pub struct ExportRequest {
    pub output_dir: PathBuf,
    pub romfs_patches: Vec<RomFsFilePatch>,
    pub code_patches: Vec<CodePatchRecord>,
    pub export_ips: bool,
    pub export_cheat: bool,
    pub cheat_title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportReport {
    pub written_files: Vec<PathBuf>,
    pub romfs_count: usize,
    pub ips_bytes: Option<usize>,
    pub cheat_written: bool,
}

/// 将 RomFS 数据表与 ExeFS 代码补丁导出为标准 LayeredFS 目录结构及可选金手指。
pub fn export_mod_bundle(req: &ExportRequest) -> CoreResult<ExportReport> {
    let mut written_files = Vec::new();

    for patch in &req.romfs_patches {
        let clean_rel = sanitize_romfs_rel_path(&patch.rel_path)?;
        let dest = req.output_dir.join("romfs").join(clean_rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let compressed = compress_lz11(&patch.raw_data, patch.add_lz13_wrapper);
        std::fs::write(&dest, compressed)?;
        written_files.push(dest);
    }

    let mut ips_bytes = None;
    if req.export_ips && !req.code_patches.is_empty() {
        let ips = build_ips(&req.code_patches)?;
        let dest = req.output_dir.join("exefs").join("code.ips");
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        ips_bytes = Some(ips.len());
        std::fs::write(&dest, ips)?;
        written_files.push(dest);
    }

    let mut cheat_written = false;
    if req.export_cheat && !req.code_patches.is_empty() {
        let cheat_text = build_azahar_cheat_multi(&req.cheat_title, &req.code_patches)?;
        let dest = req.output_dir.join("cheats.txt");
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, cheat_text)?;
        written_files.push(dest);
        cheat_written = true;
    }

    Ok(ExportReport {
        written_files,
        romfs_count: req.romfs_patches.len(),
        ips_bytes,
        cheat_written,
    })
}

fn sanitize_romfs_rel_path(raw: &str) -> CoreResult<PathBuf> {
    let normalized = raw.replace('\\', "/");
    let trimmed = if normalized.to_ascii_lowercase().starts_with("romfs/") {
        &normalized[6..]
    } else {
        &normalized
    };

    let mut clean = PathBuf::new();
    for comp in Path::new(trimmed).components() {
        match comp {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(CoreError::Schema(format!(
                    "非法的 RomFS 相对路径（禁止越级或绝对路径）: {raw}"
                )));
            }
        }
    }

    if clean.as_os_str().is_empty() {
        return Err(CoreError::Schema("RomFS 目标相对路径不能为空".into()));
    }
    Ok(clean)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exefs_patcher::{build_fe13_exp_patch, ExpPatchMode, FE13_EXP_AOB};
    use crate::lz11::decompress_lz11;

    #[test]
    fn reject_path_traversal() {
        assert!(sanitize_romfs_rel_path("../escape.bin.lz").is_err());
        assert!(sanitize_romfs_rel_path(r"C:\Windows\evil.bin").is_err());
        assert!(sanitize_romfs_rel_path("RomFS/").is_err());
        assert_eq!(
            sanitize_romfs_rel_path("RomFS/data/person/static.bin.lz").unwrap(),
            PathBuf::from("data/person/static.bin.lz")
        );
    }

    #[test]
    fn export_layeredfs_bundle_to_temp_dir() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "fe3ds_forge_export_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp_dir);

        let mut fake_code = vec![0u8; 0x200];
        fake_code[0x100..0x110].copy_from_slice(&FE13_EXP_AOB);
        let code_patch = build_fe13_exp_patch(&fake_code, ExpPatchMode::Fixed { value: 25 })
            .unwrap()
            .unwrap();

        let sample_raw = b"FE3DS_FORGE_SAMPLE_ARCHIVE_PAYLOAD".to_vec();
        let req = ExportRequest {
            output_dir: tmp_dir.clone(),
            romfs_patches: vec![RomFsFilePatch {
                rel_path: "RomFS/data/person/static.bin.lz".into(),
                raw_data: sample_raw.clone(),
                add_lz13_wrapper: false,
            }],
            code_patches: vec![code_patch],
            export_ips: true,
            export_cheat: true,
            cheat_title: "FE13 Fixed 25 EXP".into(),
        };

        let report = export_mod_bundle(&req).expect("导出补丁包失败");
        assert_eq!(report.romfs_count, 1);
        assert_eq!(report.ips_bytes, Some(21));
        assert!(report.cheat_written);

        let lz_bytes = std::fs::read(tmp_dir.join("romfs/data/person/static.bin.lz")).unwrap();
        let (decoded, is_lz13) = decompress_lz11(&lz_bytes).unwrap();
        assert_eq!(decoded, sample_raw);
        assert!(!is_lz13);

        let ips_bytes = std::fs::read(tmp_dir.join("exefs/code.ips")).unwrap();
        assert!(ips_bytes.starts_with(b"PATCH") && ips_bytes.ends_with(b"EOF"));

        let cheat_txt = std::fs::read_to_string(tmp_dir.join("cheats.txt")).unwrap();
        assert!(cheat_txt.contains("*citra_enabled"));

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}