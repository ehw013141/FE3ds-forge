use std::path::Path;

use fe3ds_core::bin_archive::ISBinArchive;
use fe3ds_core::ctpk::decode_portrait_from_fe14_face_arc;
use fe3ds_core::lz11::decompress_lz11;
use fe3ds_core::message::MessageArchive;

fn main() {
    let root = Path::new(r"D:\fe15");
    let romfs = root.join("RomFS");

    println!("=== 1. 角色表 Data/Person.bin.lz 探针 ===");
    inspect_table(&romfs.join(r"Data\Person.bin.lz"), "PID_", &["PID_アルム", "PID_セリカ", "PID_ルカ"]);

    println!("\n=== 2. 职业表 Data/Job.bin.lz 探针 ===");
    inspect_table(&romfs.join(r"Data\Job.bin.lz"), "JID_", &["JID_村人男", "JID_ナイト男", "JID_パラディン男"]);

    println!("\n=== 3. 文本包 m/@J/ 探针 ===");
    for name in ["Person.bin.lz", "Job.bin.lz", "GameData.bin.lz", "System.bin.lz"] {
        let p = romfs.join(r"m\@J").join(name);
        if p.is_file() {
            if let Ok(raw) = std::fs::read(&p) {
                if let Ok((dec, lz13)) = decompress_lz11(&raw) {
                    if let Ok(arc) = ISBinArchive::parse(dec) {
                        if let Ok(msg) = MessageArchive::from_archive(&arc) {
                            println!(
                                "  [OK] m/@J/{name} | lz13={lz13} | 词条数={} | 阿鲁姆={:?} | 村人={:?}",
                                msg.len(),
                                msg.resolve_id("PID_アルム"),
                                msg.resolve_id("JID_村人")
                            );
                        }
                    }
                }
            }
        }
    }

    println!("\n=== 4. 头像 BCH 兼容性探针 ===");
    for cand in [
        r"face\face\アルム_bu.arc",
        r"face\face\アルム_tk.arc",
        r"face\face\セリカ_bu.arc",
    ] {
        let p = romfs.join(cand);
        if p.is_file() {
            match std::fs::read(&p)
                .map_err(|e| format!("{e}"))
                .and_then(|b| decode_portrait_from_fe14_face_arc(b).map_err(|e| format!("{e}")))
            {
                Ok(img) => println!("  [OK] {cand} -> 直接兼容 FE14 BCH 解码 ({}x{})", img.width, img.height),
                Err(e) => println!("  [ERR] {cand} -> {e}"),
            }
        }
    }

    println!("\n=== 5. ExeFS/code.bin 100 经验封顶候选扫描 ===");
    let code_path = root.join(r"ExeFS\code.bin");
    if let Ok(code) = std::fs::read(&code_path) {
        inspect_code_bin(&code);
    }
}

fn inspect_table(path: &Path, prefix: &str, samples: &[&str]) {
    let raw = std::fs::read(path).expect("读取表文件失败");
    let (dec, lz13) = decompress_lz11(&raw).expect("LZ 解压失败");
    let arc = ISBinArchive::parse(dec).expect("ISBinArchive 解析失败");
    let items = arc.sections_with_prefix(prefix);

    println!(
        "  文件={} | orig={}B -> raw={}B (lz13={lz13}) | CipherData={} | ptr2总数={} | {prefix}项数={}",
        path.file_name().unwrap().to_string_lossy(),
        raw.len(),
        arc.as_bytes().len(),
        arc.section_offset("CipherData").is_some(),
        arc.sections().len(),
        items.len()
    );

    if items.len() >= 2 {
        let strides: Vec<usize> = items.windows(2).take(10).map(|w| w[1].offset.saturating_sub(w[0].offset)).collect();
        println!("  前 10 组相邻步长: {strides:?}");
        for s in items.iter().take(6) {
            println!("    {:20} @ data+0x{:X}", s.name, s.offset);
        }
    }

    let stride = if items.len() >= 2 {
        items[1].offset.saturating_sub(items[0].offset).min(0x100)
    } else {
        0x80
    };

    for &target in samples {
        if let Some(sec) = items.iter().find(|s| s.name == target) {
            println!("\n  样本 [{target}] @ data+0x{:X} ({stride} B):", sec.offset);
            if let Ok(slice) = arc.read_slice(sec.offset, stride) {
                dump_hex(slice);
            }
        }
    }
}

fn inspect_code_bin(code: &[u8]) {
    println!("  code.bin 大小: {} B (0x{:X})", code.len(), code.len());
    let words = code.len() / 4;
    let mut hits = 0usize;
    for idx in 4..words.saturating_sub(6) {
        let w = read_u32(code, idx * 4);
        if (w & 0xFFF0_FFFF) != 0xE350_0064 {
            continue;
        }
        let rn = (w >> 16) & 0x0F;
        let mut is_upper_cap = false;
        for step in 1..=3 {
            let nw = read_u32(code, (idx + step) * 4);
            let rd = (nw >> 12) & 0x0F;
            let cond = nw >> 28;
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
        ("候选#07 (0x350B70)", 0x350B70usize),
        ("候选#10 (0x353774)", 0x353774usize),
    ] {
        if target_off >= 0x20 && target_off + 0x40 <= code.len() {
            println!("\n  --- {label} 周边指令 ---");
            for off in (target_off - 0x20..target_off + 0x40).step_by(4) {
                let mark = if off == target_off { " <<" } else { "" };
                println!("    0x{off:06X} (0x{:08X}): {:08X}{mark}", off + 0x0010_0000, read_u32(code, off));
            }
        }
    }
}

fn read_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

fn dump_hex(slice: &[u8]) {
    for (row, chunk) in slice.chunks(16).enumerate() {
        print!("    +{:02X}:", row * 16);
        for b in chunk {
            print!(" {b:02X}");
        }
        println!();
    }
}