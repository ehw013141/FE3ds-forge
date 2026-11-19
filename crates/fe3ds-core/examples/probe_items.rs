use std::fs;
use std::path::Path;

fn read_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

fn read_cbytes(buf: &[u8], off: usize) -> Option<&[u8]> {
    if off >= buf.len() {
        return None;
    }
    let end = buf[off..].iter().position(|&b| b == 0)?;
    if end == 0 || end > 128 {
        return None;
    }
    Some(&buf[off..off + end])
}

fn fmt_raw_str(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes {
        if (0x20..=0x7E).contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("\\x{:02X}", b));
        }
    }
    s
}

fn dump_hex(buf: &[u8], start: usize, len: usize) {
    let end = (start + len).min(buf.len());
    for (row, chunk) in buf[start..end].chunks(16).enumerate() {
        print!("    +{:02X}:", row * 16);
        for b in chunk {
            print!(" {:02X}", b);
        }
        println!();
    }
}

/// 解析指向 raw[0x20..] 的相对指针（可跨越 data 区指向尾部 label 区）
fn resolve_str<'a>(raw: &'a [u8], ptr_val: u32) -> Option<&'a [u8]> {
    let abs = 0x20usize.checked_add(ptr_val as usize)?;
    read_cbytes(raw, abs)
}

fn probe_table(
    label: &str,
    path: &str,
    table_start: usize,
    stride: usize,
    iid_rel: usize,
    max_scan: usize,
    interesting_indices: &[usize],
) {
    println!("\n================ {label} (start=0x{table_start:X}, stride=0x{stride:X}/{stride}B) ================");
    if !Path::new(path).exists() {
        println!("  [不存在] {path}");
        return;
    }
    let raw_lz = fs::read(path).expect("读取失败");
    let (raw, _) = fe3ds_core::lz11::decompress_lz11(&raw_lz).expect("解压失败");
    let data_size = read_u32(&raw, 0x04) as usize;
    let data = &raw[0x20..0x20 + data_size];

    // 检查 table_start 之前 16 字节（排查数组计数头/标志头）
    if table_start >= 16 {
        print!("  表前 16B (data+0x{:X}):", table_start - 16);
        for b in &data[table_start - 16..table_start] {
            print!(" {:02X}", b);
        }
        println!();
    }

    // 连续扫描直到不再指向 IID_
    let mut count = 0usize;
    let mut samples: Vec<(usize, String, String)> = Vec::new();
    for idx in 0..max_scan {
        let off = table_start + idx * stride;
        if off + stride > data.len() {
            break;
        }
        let iid_ptr = read_u32(data, off + iid_rel);
        let Some(iid_b) = resolve_str(&raw, iid_ptr) else {
            break;
        };
        if !iid_b.starts_with(b"IID_") {
            break;
        }
        count += 1;
        let miid_ptr = read_u32(data, off + iid_rel + 4);
        let miid = resolve_str(&raw, miid_ptr)
            .map(fmt_raw_str)
            .unwrap_or_else(|| format!("0x{miid_ptr:X}"));
        samples.push((idx, fmt_raw_str(iid_b), miid));
    }
    println!("  连续有效 IID_ 条目数: {count}");
    if let Some(last) = samples.last() {
        println!("  末条 [{}]: {} (MIID={})", last.0, last.1, last.2);
    }

    // 打印索引清单（前 15 条，方便辨认剑/弓/魔法/伤药位置）
    println!("  前 15 条索引:");
    for (idx, iid, miid) in samples.iter().take(15) {
        println!("    [{idx:3}] {iid} | {miid}");
    }

    // 打印指定样本的完整字节，以及关键字匹配样本（必杀/弓/魔法/伤药）
    // SJIS: 殺=8E45, 弓=8B7C, ファイアー=8374 8340 8343, 傷薬=8F9D 96F2, 鉄の弓=9353 82CC 8B7C
    let mut to_dump: Vec<usize> = interesting_indices.to_vec();
    for (idx, iid, _) in &samples {
        if iid.contains("\\x8E\\x45") // 必殺 / キル
            || iid.contains("\\x8B\\x7C") // 弓
            || iid.contains("\\x83\\x74\\x83\\x40\\x83\\x43") // ファイアー
            || iid.contains("\\x8F\\x9D\\x96\\xF2") // 傷薬
            || iid.contains("\\x83\\x4C\\x83\\x8B") // キル(ソード)
        {
            if to_dump.len() < 12 && !to_dump.contains(idx) {
                to_dump.push(*idx);
            }
        }
    }
    to_dump.sort_unstable();
    to_dump.dedup();

    for idx in to_dump {
        if idx >= count {
            continue;
        }
        let off = table_start + idx * stride;
        let (_, ref iid, ref miid) = samples[idx];
        println!("  --- 样本 [{idx}] {iid} ({miid}) @ data+0x{off:X} ---");
        dump_hex(data, off, stride);
    }
}

fn list_msg_files(label: &str, dir: &str) {
    println!("\n--- 消息文件扫描 {label}: {dir} ---");
    if !Path::new(dir).exists() {
        println!("  [目录不存在]");
        return;
    }
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let lower = name.to_lowercase();
        if lower.contains("gamedata") || lower.contains("item") || lower.contains("name") {
            println!("  {name}");
        }
    }
}

fn check_ptr2_iid(label: &str, path: &str) {
    let raw_lz = fs::read(path).expect("读取失败");
    let (raw, _) = fe3ds_core::lz11::decompress_lz11(&raw_lz).expect("解压失败");
    let data_size = read_u32(&raw, 0x04) as usize;
    let ptr1_count = read_u32(&raw, 0x08) as usize;
    let ptr2_count = read_u32(&raw, 0x0C) as usize;
    let ptr2_base = 0x20 + data_size + ptr1_count * 4;
    let label_base = ptr2_base + ptr2_count * 8;

    let mut iid_labels = Vec::new();
    for i in 0..ptr2_count {
        let data_off = read_u32(&raw, ptr2_base + i * 8) as usize;
        let lbl_off = read_u32(&raw, ptr2_base + i * 8 + 4) as usize;
        if let Some(b) = read_cbytes(&raw, label_base + lbl_off) {
            if b.starts_with(b"IID_") {
                iid_labels.push((data_off, fmt_raw_str(b)));
            }
        }
    }
    println!("\n[ptr2 IID_ 核验] {label}: 命中 {} 条", iid_labels.len());
    for (off, name) in iid_labels.iter().take(4) {
        println!("  data+0x{off:06X} -> {name}");
    }
}

fn main() {
    check_ptr2_iid("FE13 GameData", r"D:\fe13\RomFS\data\GameData.bin.lz");
    check_ptr2_iid("FE14 GameData", r"D:\fe14\RomFS\GameData\GameData.bin.lz");
    check_ptr2_iid("FE15 Item", r"D:\fe15\RomFS\Data\Item.bin.lz");

    // FE13: [8]必杀剑, [9]雷剑, [60]斧/弓区, [75]弓区, [100]魔法区
    probe_table(
        "FE13 ItemData",
        r"D:\fe13\RomFS\data\GameData.bin.lz",
        0x3A2F8,
        0x38,
        0x08,
        202,
        &[8, 9, 60, 75, 100],
    );

    // FE14: [11]必杀剑, [12]雷剑, [90]弓/暗器区, [130]魔法区
    probe_table(
        "FE14 ItemData",
        r"D:\fe14\RomFS\GameData\GameData.bin.lz",
        0x14F48,
        0x68,
        0x08,
        355,
        &[11, 12, 90, 130],
    );

    // FE15: [6]勇者剑, [35]弓区, [55]盾牌/戒指区, [80]装饰/食物区, [130]魔法区
    probe_table(
        "FE15 ItemData",
        r"D:\fe15\RomFS\Data\Item.bin.lz",
        0x754,
        0xE0,
        0x00,
        258,
        &[6, 35, 55, 80, 130],
    );
}