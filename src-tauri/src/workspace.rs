use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fe3ds_core::bin_archive::ISBinArchive;
use fe3ds_core::cipher::GrowthCipher;
use fe3ds_core::ctpk::{
    decode_portrait_from_face_arc, decode_portrait_from_fe14_face_arc, encode_png,
};
use fe3ds_core::exefs_patcher::{
    build_azahar_cheat_multi, build_exp_patches, build_ips, locate_exp_site, ExpPatchMode,
    CODE_VADDR_BASE,
};
use fe3ds_core::exporter::{export_mod_bundle, ExportReport, ExportRequest, RomFsFilePatch};
use fe3ds_core::lz11::decompress_lz11;
use fe3ds_core::message::MessageArchive;
use fe3ds_core::schema::{TableEngine, TableRecord, TableSchema};
use fe3ds_core::vfs::{ExtractReport, RomImage, RomMetadata};
use serde::{Deserialize, Serialize};

const BUILTIN_FE13_SCHEMAS: &[&str] = &[
    include_str!("../../schemas/fe13/characters.json"),
    include_str!("../../schemas/fe13/classes.json"),
];

const BUILTIN_FE14_SCHEMAS: &[&str] = &[
    include_str!("../../schemas/fe14/characters.json"),
    include_str!("../../schemas/fe14/classes.json"),
];

const BUILTIN_FE15_SCHEMAS: &[&str] = &[
    include_str!("../../schemas/fe15/characters.json"),
    include_str!("../../schemas/fe15/classes.json"),
];

#[derive(Default)]
pub struct WorkspaceState {
    pub inner: Mutex<Option<LoadedWorkspace>>,
}

struct LoadedArchive {
    rel_path: String,
    is_lz13: bool,
    base_archive: ISBinArchive,
}

struct LoadedTable {
    archive_key: String,
    schema: TableSchema,
    original_records: Vec<TableRecord>,
    current_records: Vec<TableRecord>,
}

enum RomSource {
    Directory {
        romfs_dir: PathBuf,
        exefs_dir: Option<PathBuf>,
    },
    Image(RomImage),
}

impl RomSource {
    fn source_kind(&self) -> &'static str {
        match self {
            Self::Directory { .. } => "directory",
            Self::Image(_) => "image",
        }
    }

    fn rom_meta(&self) -> Option<RomMetadata> {
        match self {
            Self::Directory { .. } => None,
            Self::Image(rom) => Some(rom.metadata().clone()),
        }
    }

    fn has_romfs_file(&self, rel_path: &str) -> bool {
        match self {
            Self::Directory { romfs_dir, .. } => join_rel(romfs_dir, rel_path).is_file(),
            Self::Image(rom) => rom.has_romfs_file(rel_path),
        }
    }

    fn read_romfs_file(&self, rel_path: &str) -> Result<Vec<u8>, String> {
        match self {
            Self::Directory { romfs_dir, .. } => {
                let p = join_rel(romfs_dir, rel_path);
                std::fs::read(&p).map_err(|e| format!("读取文件 {} 失败: {e}", p.display()))
            }
            Self::Image(rom) => rom
                .read_romfs_file(rel_path)
                .map_err(|e| format!("读取镜像文件 {rel_path} 失败: {e}")),
        }
    }

    fn read_romfs_lz(&self, rel_path: &str) -> Result<(Vec<u8>, bool), String> {
        let raw = self.read_romfs_file(rel_path)?;
        decompress_lz11(&raw).map_err(|e| format!("解压文件 {rel_path} 失败: {e}"))
    }

    fn read_code_bin(&self) -> Result<Option<Vec<u8>>, String> {
        match self {
            Self::Directory { exefs_dir, .. } => {
                let Some(dir) = exefs_dir else {
                    return Ok(None);
                };
                let p = dir.join("code.bin");
                if !p.is_file() {
                    return Ok(None);
                }
                std::fs::read(&p)
                    .map(Some)
                    .map_err(|e| format!("读取 {} 失败: {e}", p.display()))
            }
            Self::Image(rom) => rom
                .read_code_bin()
                .map_err(|e| format!("读取镜像 ExeFS/.code 失败: {e}")),
        }
    }

    fn extract_to_dir(&self, output_dir: &Path) -> Result<ExtractReport, String> {
        match self {
            Self::Directory { .. } => Err("当前加载的是已解包目录，无需再次解包".to_string()),
            Self::Image(rom) => rom
                .extract_to_dir(output_dir)
                .map_err(|e| format!("解包 ROM 失败: {e}")),
        }
    }
}

pub struct LoadedWorkspace {
    root_dir: PathBuf,
    game_id: String,
    game_title: String,
    message_count: usize,
    cipher: Option<GrowthCipher>,
    archives: HashMap<String, LoadedArchive>,
    table_order: Vec<String>,
    tables: HashMap<String, LoadedTable>,
    code_bin: Option<Vec<u8>>,
    exp_site_offset: Option<usize>,
    exp_mode: ExpPatchMode,
    source: RomSource,
    portrait_cache: HashMap<String, Option<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableMeta {
    pub id: String,
    pub title: String,
    pub target_file: String,
    pub record_count: usize,
    pub modified_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSummary {
    pub root_dir: String,
    pub source_kind: String,
    pub rom_meta: Option<RomMetadata>,
    pub game_id: String,
    pub game_title: String,
    pub message_count: usize,
    pub has_code_bin: bool,
    pub exp_site_offset: Option<usize>,
    pub exp_site_vaddr: Option<u32>,
    pub exp_mode: ExpPatchMode,
    pub tables: Vec<TableMeta>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableDataPayload {
    pub schema: TableSchema,
    pub records: Vec<TableRecord>,
    pub original_records: Vec<TableRecord>,
    pub modified_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpPatchPreview {
    pub mode: ExpPatchMode,
    pub active: bool,
    pub file_offset: Option<usize>,
    pub vaddr: Option<u32>,
    pub ips_bytes: Option<usize>,
    pub cheat_text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportWorkspaceArgs {
    pub output_dir: String,
    pub export_ips: bool,
    pub export_cheat: bool,
    pub cheat_title: String,
}

impl LoadedWorkspace {
    pub fn load_from_path(path: &Path) -> Result<Self, String> {
        if path.is_dir() {
            let romfs_dir = resolve_subdir(path, &["RomFS", "ExtractedRomFS", "romfs"])
                .or_else(|| {
                    (path.join("GameData").is_dir() || path.join("data").is_dir())
                        .then(|| path.to_path_buf())
                })
                .ok_or_else(|| {
                    "未在所选目录中找到 RomFS（或 ExtractedRomFS / GameData / data）目录"
                        .to_string()
                })?;
            let exefs_dir = resolve_subdir(path, &["ExeFS", "ExtractedExeFS", "exefs"])
                .or_else(|| path.join("code.bin").is_file().then(|| path.to_path_buf()));
            Self::load_from_source(
                path,
                RomSource::Directory {
                    romfs_dir,
                    exefs_dir,
                },
            )
        } else if path.is_file() {
            let rom = RomImage::open(path).map_err(|e| format!("{e}"))?;
            Self::load_from_source(path, RomSource::Image(rom))
        } else {
            Err(format!("路径不存在或不可访问: {}", path.display()))
        }
    }


    fn load_from_source(root_path: &Path, source: RomSource) -> Result<Self, String> {
        let (game_id, game_title, primary_gd_rel, primary_gd_romfs_path) =
            if source.has_romfs_file("data/person/static.bin.lz")
                && source.has_romfs_file("data/GameData.bin.lz")
            {
                (
                    "fe13",
                    "火焰纹章：觉醒 (FE13)",
                    "data/GameData.bin.lz",
                    "RomFS/data/GameData.bin.lz",
                )
            } else if source.has_romfs_file("GameData/GameData.bin.lz") {
                (
                    "fe14",
                    "火焰纹章 if (FE14)",
                    "GameData/GameData.bin.lz",
                    "RomFS/GameData/GameData.bin.lz",
                )
            } else if source.has_romfs_file("Data/Person.bin.lz")
                && source.has_romfs_file("Data/Job.bin.lz")
            {
                (
                    "fe15",
                    "火焰纹章 回声：另一位英雄王 (FE15)",
                    "Data/Person.bin.lz",
                    "RomFS/Data/Person.bin.lz",
                )
            } else {
                return Err(
                    "未识别到受支持的 3DS 火焰纹章游戏数据（当前支持 FE13《觉醒》、FE14《if》、FE15《回声》）"
                        .to_string(),
                );
            };

        let (gd_raw, gd_lz13) = source.read_romfs_lz(primary_gd_rel)?;
        let gd_arc = ISBinArchive::parse(gd_raw)
            .map_err(|e| format!("解析 GameData.bin 失败: {e}"))?;

        let cipher = match gd_arc.section_offset("CipherData") {
            Some(off) => {
                let tbl = gd_arc
                    .read_slice(off, 256)
                    .map_err(|e| format!("读取 CipherData 失败: {e}"))?;
                Some(
                    GrowthCipher::from_table(tbl)
                        .map_err(|e| format!("初始化成长率加解密器失败: {e}"))?,
                )
            }
            None => None,
        };

        let msg_candidates: &[&str] = if game_id == "fe15" {
            &[
                "m/@J/Name.bin.lz",
                "m/@J/Job.bin.lz",
                "m/@E/Name.bin.lz",
                "m/@E/Job.bin.lz",
                "m/Name.bin.lz",
                "m/Job.bin.lz",
            ]
        } else {
            &["m/GameData.bin.lz", "m/@E/GameData.bin.lz", "m/@J/GameData.bin.lz"]
        };

        let mut merged_messages: Option<MessageArchive> = None;
        for &p in msg_candidates {
            if !source.has_romfs_file(p) {
                continue;
            }
            let (msg_raw, _) = source.read_romfs_lz(p)?;
            let msg_arc = ISBinArchive::parse(msg_raw)
                .map_err(|e| format!("解析文本包 {p} 失败: {e}"))?;
            let part = MessageArchive::from_archive(&msg_arc)
                .map_err(|e| format!("读取文本词条 {p} 失败: {e}"))?;
            match &mut merged_messages {
                Some(acc) => acc.merge(part),
                None => merged_messages = Some(part),
            }
            if game_id != "fe15" {
                break;
            }
        }
        let messages = merged_messages;
        let message_count = messages.as_ref().map_or(0, MessageArchive::len);

        let mut archives: HashMap<String, LoadedArchive> = HashMap::new();
        archives.insert(
            primary_gd_rel.to_ascii_lowercase(),
            LoadedArchive {
                rel_path: primary_gd_romfs_path.to_string(),
                is_lz13: gd_lz13,
                base_archive: gd_arc,
            },
        );

        let schemas = load_game_schemas(game_id)?;
        let mut table_order = Vec::with_capacity(schemas.len());
        let mut tables = HashMap::with_capacity(schemas.len());

        for schema in schemas {
            let rel_in_romfs = strip_romfs_prefix(&schema.target_file);
            let archive_key = rel_in_romfs.to_ascii_lowercase();

            if !archives.contains_key(&archive_key) {
                if !source.has_romfs_file(&rel_in_romfs) {
                    continue;
                }
                let (raw, is_lz13) = source.read_romfs_lz(&rel_in_romfs)?;
                let base_archive = ISBinArchive::parse(raw)
                    .map_err(|e| format!("解析 {} 失败: {e}", schema.target_file))?;
                archives.insert(
                    archive_key.clone(),
                    LoadedArchive {
                        rel_path: schema.target_file.clone(),
                        is_lz13,
                        base_archive,
                    },
                );
            }

            let arc = &archives[&archive_key].base_archive;
            let records =
                TableEngine::read_records(&schema, arc, messages.as_ref(), cipher.as_ref())
                    .map_err(|e| format!("读取数据表 {} 失败: {e}", schema.id))?;

            let table_id = schema.id.clone();
            table_order.push(table_id.clone());
            tables.insert(
                table_id,
                LoadedTable {
                    archive_key,
                    schema,
                    original_records: records.clone(),
                    current_records: records,
                },
            );
        }

        let code_bin = source.read_code_bin()?;
        let exp_site_offset = code_bin
            .as_deref()
            .and_then(|bytes| locate_exp_site(game_id, bytes).ok());

        Ok(Self {
            root_dir: root_path.to_path_buf(),
            game_id: game_id.to_string(),
            game_title: game_title.to_string(),
            message_count,
            cipher,
            archives,
            table_order,
            tables,
            code_bin,
            exp_site_offset,
            exp_mode: ExpPatchMode::Vanilla,
            source,
            portrait_cache: HashMap::new(),
        })
    }

    pub fn summary(&self) -> WorkspaceSummary {
        let tables = self
            .table_order
            .iter()
            .filter_map(|id| self.tables.get(id))
            .map(|t| TableMeta {
                id: t.schema.id.clone(),
                title: t.schema.title.clone(),
                target_file: t.schema.target_file.clone(),
                record_count: t.current_records.len(),
                modified_count: count_modified(&t.original_records, &t.current_records),
            })
            .collect();

        WorkspaceSummary {
            root_dir: self.root_dir.to_string_lossy().into_owned(),
            source_kind: self.source.source_kind().to_string(),
            rom_meta: self.source.rom_meta(),
            game_id: self.game_id.clone(),
            game_title: self.game_title.clone(),
            message_count: self.message_count,
            has_code_bin: self.code_bin.is_some(),
            exp_site_offset: self.exp_site_offset,
            exp_site_vaddr: self
                .exp_site_offset
                .map(|off| (off as u32) + CODE_VADDR_BASE),
            exp_mode: self.exp_mode,
            tables,
        }
    }

    pub fn get_table_data(&self, table_id: &str) -> Result<TableDataPayload, String> {
        let table = self
            .tables
            .get(table_id)
            .ok_or_else(|| format!("未找到数据表: {table_id}"))?;
        Ok(TableDataPayload {
            schema: table.schema.clone(),
            records: table.current_records.clone(),
            original_records: table.original_records.clone(),
            modified_count: count_modified(&table.original_records, &table.current_records),
        })
    }

    pub fn save_table_records(
        &mut self,
        table_id: &str,
        records: Vec<TableRecord>,
    ) -> Result<usize, String> {
        let (archive_key, schema) = {
            let table = self
                .tables
                .get(table_id)
                .ok_or_else(|| format!("未找到数据表: {table_id}"))?;
            (table.archive_key.clone(), table.schema.clone())
        };

        let mut test_arc = self
            .archives
            .get(&archive_key)
            .ok_or_else(|| format!("缺少底层归档缓存: {archive_key}"))?
            .base_archive
            .clone();

        TableEngine::write_records(&schema, &mut test_arc, &records, self.cipher.as_ref())
            .map_err(|e| format!("数据校验失败，已拒绝保存: {e}"))?;

        let table = self
            .tables
            .get_mut(table_id)
            .ok_or_else(|| format!("未找到数据表: {table_id}"))?;
        table.current_records = records;
        Ok(count_modified(&table.original_records, &table.current_records))
    }

    pub fn set_exp_patch(&mut self, mode: ExpPatchMode) -> Result<ExpPatchPreview, String> {
        if mode == ExpPatchMode::Vanilla {
            self.exp_mode = mode;
            return Ok(ExpPatchPreview {
                mode,
                active: false,
                file_offset: self.exp_site_offset,
                vaddr: self
                    .exp_site_offset
                    .map(|off| (off as u32) + CODE_VADDR_BASE),
                ips_bytes: None,
                cheat_text: None,
            });
        }

        let code = self
            .code_bin
            .as_deref()
            .ok_or_else(|| "当前目录未找到 ExeFS/code.bin，无法配置经验补丁".to_string())?;
        let patches = build_exp_patches(&self.game_id, code, mode)
            .map_err(|e| format!("经验补丁生成失败: {e}"))?;
        if patches.is_empty() {
            return Err("无效的经验补丁状态".to_string());
        }

        let ips = build_ips(&patches).map_err(|e| format!("构建 IPS 预览失败: {e}"))?;
        let default_title = format!("{} Custom EXP", self.game_id.to_ascii_uppercase());
        let cheat = build_azahar_cheat_multi(&default_title, &patches)
            .map_err(|e| format!("构建金手指预览失败: {e}"))?;

        self.exp_mode = mode;
        Ok(ExpPatchPreview {
            mode,
            active: true,
            file_offset: Some(patches[0].file_offset),
            vaddr: Some((patches[0].file_offset as u32) + CODE_VADDR_BASE),
            ips_bytes: Some(ips.len()),
            cheat_text: Some(cheat),
        })
    }

    pub fn export(&self, args: &ExportWorkspaceArgs) -> Result<ExportReport, String> {
        let out_dir = PathBuf::from(args.output_dir.trim());
        if out_dir.as_os_str().is_empty() {
            return Err("导出目录不能为空".to_string());
        }

        let mut dirty_by_archive: HashMap<String, Vec<&LoadedTable>> = HashMap::new();
        for table_id in &self.table_order {
            if let Some(table) = self.tables.get(table_id) {
                if count_modified(&table.original_records, &table.current_records) > 0 {
                    dirty_by_archive
                        .entry(table.archive_key.clone())
                        .or_default()
                        .push(table);
                }
            }
        }

        let mut romfs_patches = Vec::new();
        for (archive_key, dirty_tables) in dirty_by_archive {
            let loaded = self
                .archives
                .get(&archive_key)
                .ok_or_else(|| format!("缺少底层归档缓存: {archive_key}"))?;
            let mut working = loaded.base_archive.clone();
            for table in dirty_tables {
                TableEngine::write_records(
                    &table.schema,
                    &mut working,
                    &table.current_records,
                    self.cipher.as_ref(),
                )
                .map_err(|e| format!("写回 {} 失败: {e}", table.schema.id))?;
            }
            romfs_patches.push(RomFsFilePatch {
                rel_path: loaded.rel_path.clone(),
                raw_data: working.into_bytes(),
                add_lz13_wrapper: loaded.is_lz13,
            });
        }

        let mut code_patches = Vec::new();
        if self.exp_mode != ExpPatchMode::Vanilla {
            let code = self
                .code_bin
                .as_deref()
                .ok_or_else(|| "缺少 ExeFS/code.bin，无法导出经验补丁".to_string())?;
            let recs = build_exp_patches(&self.game_id, code, self.exp_mode)
                .map_err(|e| format!("生成经验补丁失败: {e}"))?;
            code_patches.extend(recs);
        }

        if romfs_patches.is_empty() && code_patches.is_empty() {
            return Err("当前尚未修改任何成长率或经验规则，无需导出".to_string());
        }

        let title = if args.cheat_title.trim().is_empty() {
            format!("{} Custom EXP", self.game_id.to_ascii_uppercase())
        } else {
            args.cheat_title.trim().to_string()
        };

        let req = ExportRequest {
            output_dir: out_dir,
            romfs_patches,
            code_patches,
            export_ips: args.export_ips,
            export_cheat: args.export_cheat,
            cheat_title: title,
        };

        export_mod_bundle(&req).map_err(|e| format!("导出补丁失败: {e}"))
    }

    pub fn get_portrait_data_url(&mut self, record_id: &str) -> Result<Option<String>, String> {
        if let Some(cached) = self.portrait_cache.get(record_id) {
            return Ok(cached.clone());
        }

        let Some(stem) = record_id.strip_prefix("PID_") else {
            self.portrait_cache.insert(record_id.to_string(), None);
            return Ok(None);
        };

        let decoded = match self.game_id.as_str() {
            "fe13" => {
                let rel_path = format!("face/{stem}_fc_通常.arc");
                if !self.source.has_romfs_file(&rel_path) {
                    self.portrait_cache.insert(record_id.to_string(), None);
                    return Ok(None);
                }
                let arc_bytes = self.source.read_romfs_file(&rel_path)?;
                decode_portrait_from_face_arc(arc_bytes)
                    .map_err(|e| format!("解码头像 {record_id} 失败: {e}"))?
            }
            "fe14" => {
                let mapped_stem = match stem {
                    "プレイヤー男" => "aマイユニ男1",
                    "プレイヤー女" => "aマイユニ女1",
                    other => other,
                };
                let candidates = [
                    format!("face/face/{mapped_stem}_bu.arc"),
                    format!("face/face/{mapped_stem}_st.arc"),
                ];
                let Some(rel_path) = candidates.into_iter().find(|p| self.source.has_romfs_file(p))
                else {
                    self.portrait_cache.insert(record_id.to_string(), None);
                    return Ok(None);
                };
                let arc_bytes = self.source.read_romfs_file(&rel_path)?;
                decode_portrait_from_fe14_face_arc(arc_bytes)
                    .map_err(|e| format!("解码 FE14 头像 {record_id} 失败: {e}"))?
            }
            "fe15" => {
                let candidates = [
                    format!("face/face/player/{stem}_bu.arc"),
                    format!("face/face/enemy/{stem}_bu.arc"),
                    format!("face/face/{stem}_bu.arc"),
                ];
                let Some(rel_path) = candidates.into_iter().find(|p| self.source.has_romfs_file(p))
                else {
                    self.portrait_cache.insert(record_id.to_string(), None);
                    return Ok(None);
                };
                let arc_bytes = self.source.read_romfs_file(&rel_path)?;
                decode_portrait_from_fe14_face_arc(arc_bytes)
                    .map_err(|e| format!("解码 FE15 头像 {record_id} 失败: {e}"))?
            }
            _ => {
                self.portrait_cache.insert(record_id.to_string(), None);
                return Ok(None);
            }
        };
        
        let png_bytes = encode_png(&decoded)
            .map_err(|e| format!("编码头像 PNG 失败: {e}"))?;

        let data_url = format!("data:image/png;base64,{}", encode_base64(&png_bytes));
        self.portrait_cache
            .insert(record_id.to_string(), Some(data_url.clone()));
        Ok(Some(data_url))
    }

    pub fn extract_rom(&self, output_dir: &str) -> Result<ExtractReport, String> {
        let out = PathBuf::from(output_dir.trim());
        if out.as_os_str().is_empty() {
            return Err("解包目标目录不能为空".to_string());
        }
        self.source.extract_to_dir(&out)
    }
}

fn join_rel(base: &Path, rel_path: &str) -> PathBuf {
    let mut p = base.to_path_buf();
    for seg in rel_path.split('/') {
        if !seg.is_empty() {
            p.push(seg);
        }
    }
    p
}

fn resolve_subdir(root: &Path, candidates: &[&str]) -> Option<PathBuf> {
    candidates
        .iter()
        .map(|name| root.join(name))
        .find(|p| p.is_dir())
}

fn strip_romfs_prefix(path: &str) -> String {
    let norm = path.replace('\\', "/");
    if norm.to_ascii_lowercase().starts_with("romfs/") {
        norm[6..].to_string()
    } else {
        norm
    }
}

fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn count_modified(orig: &[TableRecord], curr: &[TableRecord]) -> usize {
    orig.iter()
        .zip(curr.iter())
        .filter(|(a, b)| a.values != b.values)
        .count()
}

fn load_game_schemas(game: &str) -> Result<Vec<TableSchema>, String> {
    let mut schemas = Vec::new();
    let mut index_by_id: HashMap<String, usize> = HashMap::new();

    let builtins: &[&str] = match game {
        "fe13" => BUILTIN_FE13_SCHEMAS,
        "fe14" => BUILTIN_FE14_SCHEMAS,
        "fe15" => BUILTIN_FE15_SCHEMAS,
        _ => &[],
    };
    for &raw in builtins {
        let schema = TableSchema::from_json_str(raw)
            .map_err(|e| format!("内置 Schema 无效: {e}"))?;
        index_by_id.insert(schema.id.clone(), schemas.len());
        schemas.push(schema);
    }

    // 支持开源贡献者在本地 `schemas/<game>/` 目录新增或覆盖自定义 JSON Schema
    let mut extra_dirs = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        extra_dirs.push(cwd.join("schemas").join(game));
        if let Some(parent) = cwd.parent() {
            extra_dirs.push(parent.join("schemas").join(game));
        }
    }

    for dir in extra_dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
            .collect();
        files.sort();

        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let schema = TableSchema::from_json_str(&text)
                .map_err(|e| format!("外部 Schema {} 无效: {e}", file.display()))?;
            if let Some(&existing_idx) = index_by_id.get(&schema.id) {
                schemas[existing_idx] = schema;
            } else {
                index_by_id.insert(schema.id.clone(), schemas.len());
                schemas.push(schema);
            }
        }
        break;
    }

    Ok(schemas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fe3ds_core::schema::FieldValue;

    #[test]
    fn workspace_end_to_end_on_real_fe13_if_present() {
        let fe13_root = Path::new(r"D:\fe13");
        if !fe13_root.exists() {
            return;
        }

        let mut ws = LoadedWorkspace::load_from_path(fe13_root).expect("加载 FE13 工作区失败");
        let summary = ws.summary();
        assert_eq!(summary.game_id, "fe13");
        assert_eq!(summary.exp_site_offset, Some(0x1862E0));
        assert!(summary.tables.len() >= 2);

        let mut char_data = ws.get_table_data("fe13_characters").expect("读取角色表失败");
        let chrom = char_data
            .records
            .iter_mut()
            .find(|r| r.id == "PID_クロム")
            .expect("未找到克罗姆");
        assert_eq!(chrom.display_name, "克罗姆");
        chrom.values.insert(
            "personal_growths".into(),
            FieldValue::IntArray(vec![75, 70, 40, 70, 70, 100, 65, 50]),
        );

        let modified = ws
            .save_table_records("fe13_characters", char_data.records)
            .expect("保存角色表失败");
        assert_eq!(modified, 1);

        let preview = ws
            .set_exp_patch(ExpPatchMode::Fixed { value: 25 })
            .expect("配置固定 25 经验失败");
        assert!(preview.active);
        assert_eq!(preview.vaddr, Some(0x002862E0));

        let tmp_out = std::env::temp_dir().join(format!(
            "fe3ds_forge_ws_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp_out);

        let report = ws
            .export(&ExportWorkspaceArgs {
                output_dir: tmp_out.to_string_lossy().into_owned(),
                export_ips: true,
                export_cheat: true,
                cheat_title: "FE13 Fixed 25 EXP".into(),
            })
            .expect("导出补丁包失败");
        assert_eq!(report.romfs_count, 1);
        assert_eq!(report.ips_bytes, Some(21));
        assert!(report.cheat_written);

        let _ = std::fs::remove_dir_all(&tmp_out);
        println!("[PASS] Workspace 端到端闭环通过：加载 -> 改克罗姆成长 + 固定 25 经验 -> 导出校验完成");
    }

    #[test]
    fn workspace_end_to_end_on_real_fe14_if_present() {
        let fe14_rom = Path::new(r"D:\fe14.cci");
        if !fe14_rom.exists() {
            return;
        }

        let mut ws = LoadedWorkspace::load_from_path(fe14_rom).expect("加载 FE14 工作区失败");
        let summary = ws.summary();
        assert_eq!(summary.game_id, "fe14");
        assert_eq!(summary.exp_site_offset, Some(0x42D7DC));
        assert!(summary.tables.len() >= 2);

        let mut char_data = ws.get_table_data("fe14_characters").expect("读取角色表失败");
        let felicia = char_data
            .records
            .iter_mut()
            .find(|r| r.id == "PID_フェリシア")
            .expect("未找到菲丽希娅");
        assert_eq!(felicia.display_name, "菲丽希娅");
        felicia.values.insert(
            "personal_growths".into(),
            FieldValue::IntArray(vec![70, 40, 65, 60, 70, 85, 45, 65]),
        );

        let modified = ws
            .save_table_records("fe14_characters", char_data.records)
            .expect("保存角色表失败");
        assert_eq!(modified, 1);

        let preview = ws
            .set_exp_patch(ExpPatchMode::Multiplier {
                preset: fe3ds_core::exefs_patcher::ExpMultiplier::Double,
            })
            .expect("配置 2x 经验失败");
        assert!(preview.active);
        assert_eq!(preview.vaddr, Some(0x0052D7DC));

        let tmp_out = std::env::temp_dir().join(format!(
            "fe3ds_forge_ws_fe14_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp_out);

        let report = ws
            .export(&ExportWorkspaceArgs {
                output_dir: tmp_out.to_string_lossy().into_owned(),
                export_ips: true,
                export_cheat: true,
                cheat_title: String::new(),
            })
            .expect("导出 FE14 补丁包失败");
        assert_eq!(report.romfs_count, 1);
        assert_eq!(report.ips_bytes, Some(33));
        assert!(report.cheat_written);
        assert!(tmp_out.join("romfs/GameData/GameData.bin.lz").is_file());

        let _ = std::fs::remove_dir_all(&tmp_out);
        println!("[PASS] FE14 Workspace 端到端闭环通过：秒开 .cci -> 改菲丽希娅成长 + 2x 经验 -> 导出校验完成");
    }

    #[test]
    fn workspace_end_to_end_on_real_fe15_if_present() {
        let fe15_dir = Path::new(r"D:\fe15");
        if !fe15_dir.exists() {
            return;
        }

        let mut ws = LoadedWorkspace::load_from_path(fe15_dir).expect("加载 FE15 工作区失败");
        let summary = ws.summary();
        assert_eq!(summary.game_id, "fe15");
        assert_eq!(summary.exp_site_offset, Some(0x350B6C));
        assert!(summary.tables.len() >= 2);

        let mut char_data = ws.get_table_data("fe15_characters").expect("读取 FE15 角色表失败");
        let alm = char_data
            .records
            .iter_mut()
            .find(|r| r.id == "PID_アルム")
            .expect("未找到阿鲁姆");
        assert_ne!(alm.display_name, alm.id);
        alm.values.insert(
            "personal_growths".into(),
            FieldValue::IntArray(vec![90, 80, 85, 75, 65, 80, 33]),
        );

        let modified = ws
            .save_table_records("fe15_characters", char_data.records)
            .expect("保存 FE15 角色表失败");
        assert_eq!(modified, 1);

        let preview = ws
            .set_exp_patch(ExpPatchMode::Multiplier {
                preset: fe3ds_core::exefs_patcher::ExpMultiplier::Double,
            })
            .expect("配置 FE15 2x 经验失败");
        assert!(preview.active);
        assert_eq!(preview.vaddr, Some(0x00450B6C));

        let tmp_out = std::env::temp_dir().join(format!(
            "fe3ds_forge_ws_fe15_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp_out);

        let report = ws
            .export(&ExportWorkspaceArgs {
                output_dir: tmp_out.to_string_lossy().into_owned(),
                export_ips: true,
                export_cheat: true,
                cheat_title: String::new(),
            })
            .expect("导出 FE15 补丁包失败");
        assert_eq!(report.romfs_count, 1);
        assert_eq!(report.ips_bytes, Some(5 + (3 + 2 + 4) * 2 + 3));
        assert!(report.cheat_written);
        assert!(tmp_out.join("romfs/Data/Person.bin.lz").is_file());

        let _ = std::fs::remove_dir_all(&tmp_out);
        println!(
            "[PASS] FE15 Workspace 端到端闭环通过：{} 角色 + 73 职业 + 双位点 2x EXP 导出校验完成",
            summary.tables[0].record_count
        );
    }
}