use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use fe3ds_core::bin_archive::ISBinArchive;
use fe3ds_core::lz11::decompress_lz11;
use fe3ds_core::message::MessageArchive;
use fe3ds_core::vfs::RomImage;

enum ProbeSource {
    Dir {
        romfs_root: PathBuf,
        exefs_root: Option<PathBuf>,
        files: Vec<String>,
    },
    Rom(RomImage),
}

impl ProbeSource {
    fn open(path: &Path) -> Result<Self, String> {
        if path.is_file() {
            let rom = RomImage::open(path).map_err(|e| format!("打开镜像失败: {e}"))?;
            Ok(Self::Rom(rom))
        } else if path.is_dir() {
            let romfs_root = ["RomFS", "ExtractedRomFS", "romfs"]
                .iter()
                .map(|s| path.join(s))
                .find(|p| p.is_dir())
                .or_else(|| {
                    (path.join("GameData").is_dir() || path.join("data").is_dir())
                        .then(|| path.to_path_buf())
                })
                .ok_or_else(|| {
                    "目录中未找到 RomFS 子目录，且当前目录不含 GameData / data".to_string()
                })?;
            let exefs_root = ["ExeFS", "ExtractedExeFS", "exefs"]
                .iter()
                .map(|s| path.join(s))
                .find(|p| p.is_dir())
                .or_else(|| path.join("code.bin").is_file().then(|| path.to_path_buf()));
            let mut files = Vec::new();
            walk_rel(&romfs_root, &romfs_root, &mut files)?;
            files.sort();
            Ok(Self::Dir {
                romfs_root,
                exefs_root,
                files,
            })
        } else {
            Err(format!("路径不存在: {}", path.display()))
        }
    }

    fn list_files(&self) -> Vec<String> {
        match self {
            Self::Dir { files, .. } => files.clone(),
            Self::Rom(rom) => rom.list_romfs_files().into_iter().map(String::from).collect(),
        }
    }

    fn read_romfs(&self, rel: &str) -> Result<Vec<u8>, String> {
        match self {
            Self::Dir { romfs_root, .. } => {
                let mut p = romfs_root.clone();
                for seg in rel.split('/') {
                    p.push(seg);
                }
                fs::read(&p).map_err(|e| format!("读取 {} 失败: {e}", p.display()))
            }
            Self::Rom(rom) => rom
                .read_romfs_file(rel)
                .map_err(|e| format!("读取镜像 {rel} 失败: {e}")),
        }
    }

    fn read_code_bin(&self) -> Result<Option<Vec<u8>>, String> {
        match self {
            Self::Dir { exefs_root, .. } => {
                let Some(dir) = exefs_root else {
                    return Ok(None);
                };
                let p = dir.join("code.bin");
                if !p.is_file() {
                    return Ok(None);
                }
                fs::read(&p)
                    .map(Some)
                    .map_err(|e| format!("读取 code.bin 失败: {e}"))
            }
            Self::Rom(rom) => rom
                .read_code_bin()
                .map_err(|e| format!("读取镜像 .code 失败: {e}")),
        }
    }
}

fn walk_rel(base: &Path, cur: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries = fs::read_dir(cur).map_err(|e| format!("遍历目录失败: {e}"))?;
    for ent in entries.flatten() {
        let p = ent.path();
        if p.is_dir() {
            walk_rel(base, &p, out)?;
        } else if p.is_file() {
            if let Ok(rel) = p.strip_prefix(base) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(())
}

fn main() {
    let Some(arg) = env::args().nth(1) else {
        eprintln!("用法: cargo run -p fe3ds-core --example probe_fe14 -- \"D:\\path\\to\\fe14.cci\"");
        std::process::exit(1);
    };

    let path = Path::new(&arg);
    let src = match ProbeSource::open(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[错误] {e}");
            std::process::exit(1);
        }
    };

    println!("=== 1. 基础信息 ===");
    if let ProbeSource::Rom(ref rom) = src {
        let m = rom.metadata();
        println!(
            "格式={:?} | TitleID={} | ProductCode={} | RomFS文件数={} | code_compressed={}",
            m.format, m.title_id, m.product_code, m.romfs_file_count, m.code_compressed
        );
    }

    let files = src.list_files();
    println!("RomFS 总文件数: {}", files.len());

    print_filtered(&files, "GameData 目录样本", |f| {
        f.to_ascii_lowercase().starts_with("gamedata/")
            || f.to_ascii_lowercase().contains("gamedata.bin")
    }, 15);
    print_filtered(&files, "文本 m/ 目录样本", |f| {
        f.to_ascii_lowercase().starts_with("m/") && f.to_ascii_lowercase().contains("gamedata")
    }, 10);
    print_filtered(&files, "头像 face/ 非饰品样本", |f| {
        let l = f.to_ascii_lowercase();
        l.starts_with("face/") && !l.contains("accessory")
    }, 20);
    print_filtered(&files, "菲莉西亚头像样本", |f| {
        f.to_ascii_lowercase().starts_with("face/") && f.contains("フェリシア")
    }, 10);
    for arc_path in files
        .iter()
        .filter(|f| f.to_ascii_lowercase().starts_with("face/face/") && f.contains("フェリシア"))
    {
        inspect_face_sample(&src, arc_path);
    }

    println!("\n=== 2. GameData.bin.lz 结构探针 ===");
    if let Some(gd_path) = files.iter().find(|f| {
        let l = f.to_ascii_lowercase();
        l == "gamedata/gamedata.bin.lz" || l == "data/gamedata.bin.lz"
    }) {
        inspect_gamedata(&src, gd_path);
    } else {
        println!("[提示] 未找到 GameData/GameData.bin.lz");
    }

    println!("\n=== 3. 文本包探针 ===");
    for msg_path in files
        .iter()
        .filter(|f| {
            let l = f.to_ascii_lowercase();
            l.starts_with("m/") && l.ends_with("gamedata.bin.lz")
        })
        .take(5)
    {
        match src
            .read_romfs(msg_path)
            .and_then(|raw| decompress_lz11(&raw).map_err(|e| format!("{e}")))
            .and_then(|(raw, is_lz13)| {
                let mut mpid_samples: Vec<(Vec<u8>, String)> = Vec::new();
                let mut pos = 0usize;
                while pos + 5 < raw.len() && mpid_samples.len() < 6 {
                    if &raw[pos..pos + 5] == b"MPID_" && (pos == 0 || raw[pos - 1] == 0) {
                        if let Some(nul) = raw[pos..].iter().position(|&b| b == 0) {
                            let bytes = raw[pos..pos + nul].to_vec();
                            let lossy = String::from_utf8_lossy(&bytes).into_owned();
                            mpid_samples.push((bytes, lossy));
                            pos += nul + 1;
                            continue;
                        }
                    }
                    pos += 1;
                }
                let arc = ISBinArchive::parse(raw).map_err(|e| format!("{e}"))?;
                let msg = MessageArchive::from_archive(&arc).map_err(|e| format!("{e}"))?;
                Ok((msg.len(), is_lz13, mpid_samples))
            }) {
            Ok((count, is_lz13, samples)) => {
                println!("  [OK] {msg_path} | lz13={is_lz13} | 词条数={count}");
                for (raw_b, lossy) in samples {
                    println!("    key={lossy} | hex={raw_b:02X?}");
                }
            }
            Err(e) => println!("  [FAIL] {msg_path} | {e}"),
        }
    }

    println!("\n=== 4. ExeFS/code.bin 100 经验封顶候选扫描 ===");
    match src.read_code_bin() {
        Ok(Some(code)) => inspect_code_bin(&code),
        Ok(None) => println!("  未找到 ExeFS/code.bin"),
        Err(e) => println!("  [错误] {e}"),
    }
}

fn print_filtered<F: Fn(&str) -> bool>(files: &[String], title: &str, pred: F, limit: usize) {
    let matched: Vec<&String> = files.iter().filter(|f| pred(f)).collect();
    println!("\n[{title}] 命中 {} 项（展示前 {} 项）:", matched.len(), limit);
    for f in matched.iter().take(limit) {
        println!("  {f}");
    }
}

fn inspect_gamedata(src: &ProbeSource, rel: &str) {
    let Ok(comp) = src.read_romfs(rel) else {
        println!("  读取 {rel} 失败");
        return;
    };
    let Ok((raw, is_lz13)) = decompress_lz11(&comp) else {
        println!("  解压 {rel} 失败");
        return;
    };

    let data_size = read_u32(&raw, 4) as usize;
    let ptr1_cnt = read_u32(&raw, 8);
    let ptr2_cnt = read_u32(&raw, 12);
    println!(
        "  文件={rel} | orig={}B -> raw={}B (lz13={is_lz13}) | data_size=0x{data_size:X} ptr1={ptr1_cnt} ptr2={ptr2_cnt}",
        comp.len(),
        raw.len()
    );

    let Ok(arc) = ISBinArchive::parse(raw.clone()) else {
        println!("  ISBinArchive 解析失败");
        return;
    };
    println!(
        "  CipherData 存在: {} | ptr2 节总数: {}",
        arc.section_offset("CipherData").is_some(),
        arc.sections().len()
    );
    let ptr2_pid = arc.sections().iter().filter(|s| s.name.starts_with("PID_")).count();
    let ptr2_jid = arc.sections().iter().filter(|s| s.name.starts_with("JID_")).count();
    println!("  ptr2 内含 PID_={ptr2_pid} 项, JID_={ptr2_jid} 项");
    println!("  前 15 个 ptr2 节:");
    for sec in arc.sections().iter().take(15) {
        println!("    {:28} @ data+0x{:X}", sec.name, sec.offset);
    }

    if let Ok(hdr_bytes) = arc.read_slice(0, 0x60.min(data_size)) {
        println!("  Data+0x00 头指针区 (u32 LE):");
        for row in 0..(hdr_bytes.len() / 16) {
            let base = row * 16;
            println!(
                "    +0x{:02X}: {:08X} {:08X} {:08X} {:08X}",
                base,
                read_u32(hdr_bytes, base),
                read_u32(hdr_bytes, base + 4),
                read_u32(hdr_bytes, base + 8),
                read_u32(hdr_bytes, base + 12),
            );
        }
    }

    probe_label_stride(&raw, data_size, b"PID_", "角色表 (PID_)");
    probe_label_stride(&raw, data_size, b"JID_", "职业表 (JID_)");
    let data = &raw[0x20..(0x20 + data_size).min(raw.len())];
    println!("\n  --- 表头前导字节核查 ---");
    if data.len() >= 0xDD8 {
        println!("  PID 表前导 data+0xDC0..0xDD8:");
        dump_hex(&data[0xDC0..0xDD8]);
    }
    if data.len() >= 0xE9F8 {
        println!("  JID 表前导 data+0xE9E8..0xE9F8:");
        dump_hex(&data[0xE9E8..0xE9F8]);
    }

    // 转储典型真实角色：#4 菲莉西亚(0x1038)、#5 乔卡(0x10D0)、#9 阿库娅(0x1330)
    for (idx, off) in [(4usize, 0x1038usize), (5, 0x10D0), (9, 0x1330)] {
        if off + 0x98 <= data.len() {
            let str_off = read_u32(data, off) as usize;
            let raw_lbl = read_cstr_bytes(&raw, 0x20 + str_off);
            println!("\n  PID[{idx}] @ data+0x{off:X} | label_hex={raw_lbl:02X?}");
            dump_hex(&data[off..off + 0x98]);
        }
    }

    // 转储典型真实职业：#1 暗夜王子/血统(0xEA78)、#7 圣骑士男(0xED78)
    for (idx, off) in [(1usize, 0xEA78usize), (7, 0xED78)] {
        if off + 0x80 <= data.len() {
            let str_off = read_u32(data, off) as usize;
            let raw_lbl = read_cstr_bytes(&raw, 0x20 + str_off);
            println!("\n  JID[{idx}] @ data+0x{off:X} | label_hex={raw_lbl:02X?}");
            dump_hex(&data[off..off + 0x80]);
        }
    }
}

fn probe_label_stride(raw: &[u8], data_size: usize, prefix: &[u8], title: &str) {
    let data_end = (0x20 + data_size).min(raw.len());
    let data_slice = &raw[0x20..data_end];

    let mut labels = Vec::new();
    let mut i = 0x20usize;
    while i + prefix.len() < raw.len() {
        if &raw[i..i + prefix.len()] == prefix && (i == 0 || raw[i - 1] == 0) {
            if let Some(nul) = raw[i..].iter().position(|&b| b == 0) {
                let s = String::from_utf8_lossy(&raw[i..i + nul]).into_owned();
                labels.push((i - 0x20, s));
                i += nul + 1;
                continue;
            }
        }
        i += 1;
    }

    println!("\n  --- {title}: 扫描到 {} 个标签 ---", labels.len());
    let mut ptr_hits: Vec<(usize, String)> = Vec::new();
    for (rel_off, name) in labels.iter().take(12) {
        let needle = (*rel_off as u32).to_le_bytes();
        let mut found = Vec::new();
        for pos in (0..data_slice.len().saturating_sub(4)).step_by(4) {
            if data_slice[pos..pos + 4] == needle {
                found.push(pos);
            }
        }
        if let Some(&first_ref) = found.first() {
            ptr_hits.push((first_ref, name.clone()));
        }
        println!("    {name:24} (str@data+0x{rel_off:X}) -> 首次引用 data+0x{:X?}", found.iter().take(3).collect::<Vec<_>>());
    }

    ptr_hits.sort_by_key(|(off, _)| *off);
    if ptr_hits.len() >= 2 {
        let deltas: Vec<usize> = ptr_hits
            .windows(2)
            .map(|w| w[1].0.saturating_sub(w[0].0))
            .collect();
        println!("    相邻记录引用步长 (十进制 / 十六进制): {:?} / {:X?}", deltas, deltas);

        let rec_off = ptr_hits[0].0;
        let dump_len = deltas.first().copied().unwrap_or(0x80).clamp(0x40, 0xB0);
        if rec_off + dump_len <= data_slice.len() {
            println!("    首条记录 [{}] @ data+0x{:X} ({} B):", ptr_hits[0].1, rec_off, dump_len);
            dump_hex(&data_slice[rec_off..rec_off + dump_len]);
        }
    }
}

fn inspect_code_bin(code: &[u8]) {
    println!("  code.bin 大小: {} B (0x{:X})", code.len(), code.len());
    let words = code.len() / 4;
    let mut hits = 0usize;
    for idx in 4..words.saturating_sub(6) {
        let w = read_u32(code, idx * 4);
        // 仅匹配 ARM32: CMP Rn, #100 (0xE35?0064)
        if (w & 0xFFF0_FFFF) != 0xE350_0064 {
            continue;
        }
        let rn = (w >> 16) & 0x0F;
        let mut is_upper_cap = false;
        for step in 1..=3 {
            let nw = read_u32(code, (idx + step) * 4);
            let rd = (nw >> 12) & 0x0F;
            let cond = nw >> 28;
            // 同寄存器上限钳位: MOVGT(0xC) / MOVGE(0xA) / MOVHI(0x8) / MOVCS(0x2) Rd, #100
            if rd == rn
                && (nw & 0x0FFF_0FFF) == 0x03A0_0064
                && matches!(cond, 0xC | 0xA | 0x8 | 0x2)
            {
                is_upper_cap = true;
                break;
            }
        }
        if is_upper_cap {
            let off = idx * 4;
            let vaddr = off + 0x0010_0000;
            print!("  [封顶 #{:02}] off=0x{off:06X} (vaddr=0x{vaddr:08X}) R{rn}:", hits + 1);
            for k in (idx - 3)..=(idx + 4) {
                print!(" {:08X}", read_u32(code, k * 4));
            }
            println!();
            hits += 1;
        }
    }
    println!("  全文件共命中 {hits} 处同寄存器 100 上限封顶指令。");
    for (label, target_off) in [
        ("候选#10 (0x4257B8)", 0x4257B8usize),
        ("候选#12 (0x42D7DC)", 0x42D7DCusize),
    ] {
        if target_off >= 0x30 && target_off + 0x60 <= code.len() {
            println!("\n  --- {label} 周边指令 ---");
            for off in (target_off - 0x30..target_off + 0x50).step_by(4) {
                let mark = if off == target_off { " <<" } else { "" };
                println!("    0x{off:06X} (0x{:08X}): {:08X}{mark}", off + 0x0010_0000, read_u32(code, off));
            }
        }
    }
}

fn read_cstr_bytes(buf: &[u8], off: usize) -> Vec<u8> {
    if off >= buf.len() {
        return Vec::new();
    }
    let end = buf[off..]
        .iter()
        .position(|&b| b == 0)
        .map_or(buf.len(), |n| off + n);
    buf[off..end].to_vec()
}

fn dump_hex(bytes: &[u8]) {
    for (row, chunk) in bytes.chunks(16).enumerate() {
        print!("      +{:02X}:", row * 16);
        for b in chunk {
            print!(" {:02X}", b);
        }
        println!();
    }
}

fn inspect_face_sample(src: &ProbeSource, rel: &str) {
    let Ok(bytes) = src.read_romfs(rel) else {
        println!("  读取头像 {rel} 失败");
        return;
    };
    let Ok(arc) = ISBinArchive::parse(bytes) else {
        println!("  头像 {rel} 不是标准 ISBinArchive");
        return;
    };
    let data_base = arc.section_offset("Data").unwrap_or(0);
    println!("  [头像归档] {rel} | ptr2={} | Data@0x{data_base:X}", arc.sections().len());
    for sec in arc.sections().iter().filter(|s| s.name.ends_with(".bch.lz")).take(8) {
        if let Ok(info) = arc.read_slice(sec.offset, 16) {
            let idx = read_u32(info, 4);
            let sz = read_u32(info, 8) as usize;
            let off = read_u32(info, 12) as usize;
            println!("    entry: {:18} idx={idx} sz=0x{sz:X} off=0x{off:X}", sec.name);
        }
    }
    let target = arc
        .sections()
        .iter()
        .find(|s| s.name.contains("通常") && s.name.ends_with(".bch.lz"))
        .or_else(|| arc.sections().iter().find(|s| s.name.ends_with(".bch.lz")));
    let Some(sec) = target else {
        return;
    };
    let Ok(info) = arc.read_slice(sec.offset, 16) else {
        return;
    };
    let sz = read_u32(info, 8) as usize;
    let off = read_u32(info, 12) as usize;
    let payload = arc
        .read_slice(data_base + off, sz)
        .or_else(|_| arc.read_slice(off, sz));
    let Ok(slice) = payload else {
        println!("    读取 {} 载荷失败", sec.name);
        return;
    };
    let Ok((dec, is_lz13)) = decompress_lz11(slice) else {
        println!("    解压 {} 失败 (前16B):", sec.name);
        dump_hex(&slice[..slice.len().min(16)]);
        return;
    };
    let magic = String::from_utf8_lossy(&dec[..dec.len().min(4)]);
    println!("    [BCH] {} -> {}B (lz13={is_lz13}) | magic={magic}", sec.name, dec.len());
    dump_hex(&dec[..dec.len().min(0x60)]);
    if dec.len() >= 0x34 && &dec[..4] == b"BCH\0" {
        let main_off = read_u32(&dec, 0x08) as usize;
        let str_off = read_u32(&dec, 0x0C) as usize;
        let gpu_off = read_u32(&dec, 0x10) as usize;
        let dat_off = read_u32(&dec, 0x14) as usize;
        let gpu_sz = read_u32(&dec, 0x28) as usize;
        let dat_sz = read_u32(&dec, 0x2C) as usize;
        println!(
            "    BCH sections: main=0x{main_off:X} str=0x{str_off:X} gpu=0x{gpu_off:X}(0x{gpu_sz:X}B) data=0x{dat_off:X}(0x{dat_sz:X}B)"
        );
        if main_off < dec.len() {
            let m_end = (main_off + 0x60).min(dec.len());
            println!("    main_header @ 0x{main_off:X}:");
            dump_hex(&dec[main_off..m_end]);
        }
        if gpu_off < dec.len() {
            let g_end = (gpu_off + gpu_sz.min(0x80)).min(dec.len());
            println!("    gpu_cmds @ 0x{gpu_off:X}:");
            dump_hex(&dec[gpu_off..g_end]);
        }
        if str_off < dec.len() {
            let s_end = (str_off + 0x60).min(dec.len());
            println!("    strings @ 0x{str_off:X}: {:?}", String::from_utf8_lossy(&dec[str_off..s_end]));
        }
    }
}
#[inline]
fn read_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}