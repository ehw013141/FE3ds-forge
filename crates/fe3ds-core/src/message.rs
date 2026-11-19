use std::collections::HashMap;

use crate::bin_archive::ISBinArchive;
use crate::error::CoreResult;

/// 3DS 火焰纹章文本档案解析器（如 `RomFS/m/GameData.bin.lz`），自动解码原生 UTF-16LE 译名。
#[derive(Debug, Clone, Default)]
pub struct MessageArchive {
    entries: HashMap<String, String>,
}

impl MessageArchive {
    pub fn from_archive(archive: &ISBinArchive) -> CoreResult<Self> {
        let data = archive.data_slice();
        let mut entries = HashMap::with_capacity(archive.sections().len());

        for sec in archive.sections() {
            if sec.offset >= data.len() {
                continue;
            }
            let text = read_utf16le_z(&data[sec.offset..]);
            entries.insert(sec.name.clone(), text);
        }

        Ok(Self { entries })
    }

    pub fn get(&self, message_key: &str) -> Option<&str> {
        self.entries.get(message_key).map(String::as_str)
    }

    /// 将数据表标签（如 `PID_クロム` / `JID_ロード男` / `IID_鉄の剣`）自动映射为对应文本键 `M*`。
    pub fn resolve_id(&self, data_id: &str) -> Option<&str> {
        if let Some(exact) = self.get(data_id) {
            return Some(exact);
        }
        let msg_key = format!("M{data_id}");
        self.get(&msg_key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// 合并另一份文本包的词条（用于 FE15 等将角色/职业译名拆分在不同 `.bin.lz` 的作品）。
    pub fn merge(&mut self, other: Self) {
        self.entries.extend(other.entries);
    }
}

fn read_utf16le_z(bytes: &[u8]) -> String {
    let mut units = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        let u = u16::from_le_bytes([bytes[i], bytes[i + 1]]);
        if u == 0 {
            break;
        }
        units.push(u);
        i += 2;
    }
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lz11::decompress_lz11;
    use std::path::Path;

    #[test]
    fn decode_real_fe13_chinese_messages_if_present() {
        let path = Path::new(r"D:\fe13\RomFS\m\GameData.bin.lz");
        if !path.exists() {
            return;
        }
        let (raw, _) = decompress_lz11(&std::fs::read(path).unwrap()).unwrap();
        let arc = ISBinArchive::parse(raw).unwrap();
        let msgs = MessageArchive::from_archive(&arc).unwrap();

        assert_eq!(msgs.resolve_id("PID_クロム"), Some("克罗姆"));
        assert_eq!(msgs.resolve_id("PID_リズ"), Some("莉兹"));
        assert_eq!(msgs.resolve_id("PID_フレデリク"), Some("弗雷德里克"));
        println!("[PASS] 成功解析汉化文本 {} 条（PID_クロム -> 克罗姆）", msgs.len());
    }
}