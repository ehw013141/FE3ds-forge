import re
from collections import defaultdict
from pathlib import Path
from typing import Dict, List

from core.bin_archive import ISBinArchive
from core.lz11 import decompress_lz11

FE13_ROOT = Path(r"D:\fe13")


def dump_hex_rows(data: bytes, limit: int = 0xA0) -> None:
    chunk = data[:limit]
    for off in range(0, len(chunk), 16):
        row = chunk[off : off + 16]
        hex_str = " ".join(f"{b:02X}" for b in row)
        print(f"  +0x{off:03X}: {hex_str}")


def probe_static() -> None:
    raw, _ = decompress_lz11((FE13_ROOT / r"RomFS\data\person\static.bin.lz").read_bytes())
    arc = ISBinArchive(raw)

    # FE13 原版个人成长率 [HP, Str, Mag, Skl, Spd, Lck, Def, Res]
    targets: Dict[str, bytes] = {
        "PID_クロム": bytes([45, 40, 10, 40, 40, 70, 35, 20]),
        "PID_リズ": bytes([35, 25, 35, 30, 35, 65, 15, 35]),
    }

    print("=== 1. static.bin 人物成长率实锤 ===")
    for pid, expected in targets.items():
        off = arc.sections.get(pid)
        if off is None:
            print(f"[警告] 未找到 {pid}")
            continue
        block = arc.read_bytes(off, 0x238)
        hit = block.find(expected)
        print(f"\n[{pid}] data+0x{off:06X} | 明文成长率命中偏移: {f'+0x{hit:03X}' if hit != -1 else '未直接命中(需核对编码/结构)'}")
        if hit != -1:
            print(f"  匹配字节: {list(block[hit : hit + 8])}")
        dump_hex_rows(block, limit=0x90)


def probe_gamedata_tables() -> None:
    raw, _ = decompress_lz11((FE13_ROOT / r"RomFS\data\GameData.bin.lz").read_bytes())
    arc = ISBinArchive(raw)

    groups: Dict[str, List[tuple[str, int]]] = defaultdict(list)
    for label, off in arc.sections.items():
        prefix = label.split("_", 1)[0] + "_" if "_" in label else "(ROOT)"
        groups[prefix].append((label, off))

    print("\n=== 2. GameData.bin 内部数据表清单 ===")
    for prefix, items in sorted(groups.items(), key=lambda kv: -len(kv[1])):
        stride = items[1][1] - items[0][1] if len(items) >= 2 else 0
        sample = items[0][0]
        print(f"  {prefix:10s} | 数量: {len(items):4d} | 首项偏移: 0x{items[0][1]:06X} | 相邻步长: 0x{stride:X} ({stride}B) | 示例: {sample}")


def probe_romfs_layout() -> None:
    romfs = FE13_ROOT / "RomFS"
    print("\n=== 3. RomFS 顶层目录 ===")
    for p in sorted(romfs.iterdir()):
        kind = "DIR " if p.is_dir() else "FILE"
        print(f"  [{kind}] {p.name}")

    m_dir = romfs / "m"
    if m_dir.exists():
        print("\n=== 4. RomFS\\m 文本目录（前 20 项）===")
        for p in sorted(m_dir.rglob("*"))[:20]:
            rel = p.relative_to(m_dir)
            size = f"{p.stat().st_size} B" if p.is_file() else "DIR"
            print(f"  {str(rel):35s} ({size})")

def probe_phase0_deep() -> None:
    try:
        static_raw, _ = decompress_lz11((FE13_ROOT / r"RomFS\data\person\static.bin.lz").read_bytes())
        s_arc = ISBinArchive(static_raw)
        print("\n=== 5. static.bin 多角色成长区采样 (+0x24..+0x33) ===")
        for pid in ("PID_プレイヤー男", "PID_クロム", "PID_リズ", "PID_フレデリク", "PID_ヴィオール", "PID_ソワレ", "PID_ヴェイク"):
            off = s_arc.sections.get(pid)
            if off is None:
                continue
            blk = s_arc.read_bytes(off, 0x50)
            g1 = blk[0x24:0x2C]
            g2 = blk[0x2C:0x34]
            diff = [(a - b) & 0xFF for a, b in zip(g1, g2)]
            print(f"  {pid:12s} | +24: {g1.hex(' ').upper()} | +2C: {g2.hex(' ').upper()} | diff: {' '.join(f'{x:02X}' for x in diff)}")

        gd_raw, _ = decompress_lz11((FE13_ROOT / r"RomFS\data\GameData.bin.lz").read_bytes())
        g_arc = ISBinArchive(gd_raw)
        print("\n=== 6. GameData.bin 职业结构体采样 (JID_ロード男) ===")
        lord_off = g_arc.sections.get("JID_ロード男")
        if lord_off is not None:
            lord_blk = g_arc.read_bytes(lord_off, 0x80)
            # FE13 领主原版职业成长率 [40, 20, 0, 20, 20, 0, 10, 5]
            hit = lord_blk.find(bytes([40, 20, 0, 20, 20, 0, 10, 5]))
            print(f"  JID_ロード男 data+0x{lord_off:06X} | 明文职业成长率命中: {f'+0x{hit:02X}' if hit != -1 else '未直接命中'}")
            dump_hex_rows(lord_blk, limit=0x80)

        m_dir = FE13_ROOT / "RomFS" / "m"
        non_num = [p.relative_to(m_dir) for p in sorted(m_dir.rglob("*")) if p.is_file() and not p.stem.split(".")[0].isdigit()]
        print(f"\n=== 7. RomFS\\m 非章节编号文本文件 (共 {len(non_num)} 项，展示前 25 项) ===")
        for rel in non_num[:25]:
            print(f"  {str(rel):35s} ({(m_dir / rel).stat().st_size} B)")

        face_dir = FE13_ROOT / "RomFS" / "face"
        if face_dir.exists():
            faces = [p.relative_to(face_dir) for p in sorted(face_dir.rglob("*")) if p.is_file()]
            print(f"\n=== 8. RomFS\\face 头像资源 (共 {len(faces)} 项，展示前 10 项) ===")
            for rel in faces[:10]:
                print(f"  {str(rel):35s} ({(face_dir / rel).stat().st_size} B)")
    except (OSError, ValueError) as exc:
        print(f"[错误] 深度探针执行失败: {exc}")

def probe_cipher_and_assets() -> None:
    known: Dict[int, int] = {
        0: 0x00, 5: 0x69, 10: 0xDF, 15: 0xBB, 20: 0xDD,
        25: 0x9E, 30: 0x3D, 35: 0xD1, 40: 0x02, 45: 0x91,
        50: 0xDA, 55: 0xEB, 60: 0xF0, 65: 0xD7, 70: 0xF7,
    }
    try:
        code = (FE13_ROOT / r"ExeFS\code.bin").read_bytes()
        print("\n=== 9. code.bin 成长率映射表扫描 ===")
        fwd_hits: List[int] = []
        for i in range(0, len(code) - 71):
            base = code[i]
            if ((code[i + 5] - base) & 0xFF) != 0x69:
                continue
            if ((code[i + 10] - base) & 0xFF) != 0xDF:
                continue
            if all(((code[i + g] - base) & 0xFF) == fg for g, fg in known.items()):
                fwd_hits.append(i)
        print(f"  正向表命中: {[f'0x{x:06X}' for x in fwd_hits]}")

        inv_hits: List[tuple[int, int, int]] = []
        scan_rules = (
            (1, 0x00, b"\x00." + re.escape(b"\x28")),
            (1, 0xDD, b"\x14.\x0a"),
            (-1, 0x00, re.escape(b"\x28") + b".\x00"),
            (-1, 0xDD, b"\x0a.\x14"),
        )
        seen: set[tuple[int, int, int]] = set()
        for sign, anchor_fg, pat in scan_rules:
            for m in re.finditer(pat, code, re.DOTALL):
                p = m.start() if sign == 1 else m.start() + 2
                for c in range(256):
                    t0 = p - ((c + sign * anchor_fg) & 0xFF)
                    if t0 < 0 or t0 + 256 > len(code):
                        continue
                    key = (t0, c, sign)
                    if key in seen:
                        continue
                    if all(code[t0 + ((c + sign * fg) & 0xFF)] == g for g, fg in known.items()):
                        seen.add(key)
                        inv_hits.append(key)
        print(f"  逆向表命中 (table_off, shift_c, sign): {[(f'0x{t:06X}', f'0x{c:02X}', s) for t, c, s in inv_hits]}")

        print("\n=== 10. RomFS\\m\\GameData.bin.lz 汉化文本格式探测 ===")
        msg_raw, _ = decompress_lz11((FE13_ROOT / r"RomFS\m\GameData.bin.lz").read_bytes())
        m_arc = ISBinArchive(msg_raw)
        print(f"  解压大小: {len(msg_raw)} B | Header: {m_arc.header}")
        print(f"  ptr2 前 5 项: {list(m_arc.sections.items())[:5]}")
        dump_hex_rows(msg_raw, limit=0x80)
        tag = b"MPID_\x83N\x83\x8d\x83\x80\x00"  # Shift-JIS: MPID_クロム
        pos = msg_raw.find(tag)
        if pos != -1:
            rel_tag = pos - ISBinArchive.HEADER_SIZE
            ptr_bytes = rel_tag.to_bytes(4, "little")
            ref = msg_raw.find(ptr_bytes, ISBinArchive.HEADER_SIZE)
            print(f"  MPID_クロム 位置: data+0x{rel_tag:06X} | 指针引用位置: 0x{ref:06X}")
            if ref != -1 and ref >= 4:
                txt_rel = int.from_bytes(msg_raw[ref - 4 : ref], "little")
                txt_off = ISBinArchive.HEADER_SIZE + txt_rel
                end_u16 = txt_off
                while end_u16 + 1 < len(msg_raw) and msg_raw[end_u16 : end_u16 + 2] != b"\x00\x00":
                    end_u16 += 2
                raw_u16 = msg_raw[txt_off:end_u16]
                print(f"  对应文本偏移: data+0x{txt_rel:06X} | UTF-16LE 原始字节: {raw_u16.hex(' ').upper()}")
                print(f"  标准 UTF-16LE 解码结果: {raw_u16.decode('utf-16-le', errors='replace')}")

        print("\n=== 11. RomFS\\face 头像容器与命名探测 ===")
        face_dir = FE13_ROOT / "RomFS" / "face"
        chrom_faces = [p.name for p in sorted(face_dir.glob("*クロム*"))]
        print(f"  库洛姆相关文件 ({len(chrom_faces)} 项): {chrom_faces[:6]}")
        sample_face = face_dir / (chrom_faces[0] if chrom_faces else "001_ボス_fc_通常.arc")
        f_bytes = sample_face.read_bytes()
        print(f"  样本 {sample_face.name} ({len(f_bytes)} B) 头部前 32B:")
        dump_hex_rows(f_bytes, limit=0x20)
        if f_bytes[:1] == b"\x11":
            dec_face, _ = decompress_lz11(f_bytes)
            print(f"  LZ11 解压后大小: {len(dec_face)} B | 前 48B:")
            dump_hex_rows(dec_face, limit=0x30)
    except (OSError, ValueError) as exc:
        print(f"[错误] 收官探针执行失败: {exc}")
def _find_perm256(buf: bytes, stride: int) -> List[int]:
    hits: List[int] = []
    span = 256 * stride
    limit = len(buf) - span
    if limit < 0:
        return hits
    i = 0
    while i <= limit:
        if len(set(buf[i : i + 16 * stride : stride])) == 16:
            if stride == 1:
                if len(set(buf[i : i + 252])) == 252:
                    for t0 in range(max(0, i - 3), min(limit, i) + 1):
                        if len(set(buf[t0 : t0 + 256])) == 256:
                            hits.append(t0)
                            i = t0 + 256
                            break
                    else:
                        i += 4
                    continue
            elif len(set(buf[i : i + span : stride])) == 256:
                hits.append(i)
                i += span
                continue
        i += 4
    return hits


def probe_phase0_final() -> None:
    try:
        print("\n=== 12. 全量 256 置换表扫描 (u8/u16/u32) ===")
        scan_targets = [
            ("ExeFS/code.bin", (FE13_ROOT / r"ExeFS\code.bin").read_bytes()),
            ("RomFS/params.bin", decompress_lz11((FE13_ROOT / r"RomFS\params.bin.lz").read_bytes())[0]),
            ("RomFS/data/GameData.bin", decompress_lz11((FE13_ROOT / r"RomFS\data\GameData.bin.lz").read_bytes())[0]),
        ]
        for label, blob in scan_targets:
            for stride in (1, 2, 4):
                hits = _find_perm256(blob, stride)
                if hits:
                    print(f"  [命中] {label} (stride={stride}): {[f'0x{x:06X}' for x in hits[:8]]}")

        print("\n=== 13. code.bin 成长率读取点 (+0x24/+0x2C) ARM 定位 ===")
        code = scan_targets[0][1]
        words = [int.from_bytes(code[i : i + 4], "little") for i in range(0, len(code) - 3, 4)]
        pair_hits: List[int] = []
        for idx, w in enumerate(words):
            is_24 = (w & 0x0FF00FFF) == 0x05D00024 or (w & 0x0FE00FFF) == 0x02800024
            if not is_24:
                continue
            window = words[max(0, idx - 12) : min(len(words), idx + 16)]
            if any((x & 0x0FF00FFF) == 0x05D0002C or (x & 0x0FE00FFF) == 0x0280002C for x in window):
                if not pair_hits or idx - pair_hits[-1] > 16:
                    pair_hits.append(idx)
        print(f"  命中候选点共 {len(pair_hits)} 处，展示前 5 处:")
        for idx in pair_hits[:5]:
            start = max(0, idx - 4)
            end = min(len(words), idx + 14)
            dump = " ".join(f"{words[k]:08X}" for k in range(start, end))
            print(f"  @0x{idx * 4:06X} (vaddr 0x{idx * 4 + 0x100000:08X}):\n    {dump}")

        print("\n=== 14. 汉化文本抽检 (RomFS\\m\\GameData.bin.lz) ===")
        msg_raw, _ = decompress_lz11((FE13_ROOT / r"RomFS\m\GameData.bin.lz").read_bytes())
        m_arc = ISBinArchive(msg_raw)
        samples = ["MPID_デフォルト名", "MPID_クロム", "MPID_リズ", "MPID_フレデリク"]
        for prefix in ("MJID_", "MIID_", "MSID_"):
            first = next((k for k in m_arc.sections if k.startswith(prefix)), None)
            if first:
                samples.append(first)
        for key in samples:
            rel = m_arc.sections.get(key)
            if rel is None:
                continue
            off = ISBinArchive.HEADER_SIZE + rel
            end = off
            while end + 1 < len(msg_raw) and msg_raw[end : end + 2] != b"\x00\x00":
                end += 2
            text = msg_raw[off:end].decode("utf-16-le", errors="replace")
            print(f"  {key:24s} -> {text}")

        print("\n=== 15. 头像容器内部结构 (クロム_fc_通常.arc) ===")
        fc_path = FE13_ROOT / "RomFS" / "face" / "クロム_fc_通常.arc"
        fc_arc = ISBinArchive(fc_path.read_bytes())
        print(f"  Header: {fc_arc.header}")
        print(f"  ptr2 前 8 项: {list(fc_arc.sections.items())[:8]}")
        first_off = next(iter(fc_arc.sections.values()), 0)
        dump_hex_rows(fc_arc.read_bytes(first_off, 0x40), limit=0x40)
    except (OSError, ValueError) as exc:
        print(f"[错误] 终局探针执行失败: {exc}")

def probe_cipher_exact() -> None:
    try:
        code = (FE13_ROOT / r"ExeFS\code.bin").read_bytes()
        gd_raw, _ = decompress_lz11((FE13_ROOT / r"RomFS\data\GameData.bin.lz").read_bytes())
        g_arc = ISBinArchive(gd_raw)

        print("\n=== 16. 256 置换表内容与归属核验 ===")
        for label, blob, off in (
            ("code.bin @0x36E594", code, 0x36E594),
            ("code.bin @0x36E9BB", code, 0x36E9BB),
            ("GameData.bin @0x04001C", gd_raw, 0x04001C),
        ):
            tbl = blob[off : off + 256]
            print(f"  [{label}] 前 48B:")
            dump_hex_rows(tbl, limit=0x30)
            if "GameData" in label:
                rel = off - ISBinArchive.HEADER_SIZE
                owner = max(
                    ((o, k) for k, o in g_arc.sections.items() if o <= rel),
                    default=(0, "(UNKNOWN)"),
                )
                print(f"  所属 ptr2 区块: {owner[1]} (data+0x{owner[0]:06X}, 相对偏移 +0x{rel - owner[0]:X})")

        print("\n=== 17. ARM32 目标函数反汇编数据 (0x21B310 & 0x0AE5D4) ===")
        for title, start, count in (
            ("BL 目标 @0x21B310 (vaddr 0x0031B310)", 0x21B310, 40),
            ("+0x2C 后续 @0x0AE5D4 (vaddr 0x001AE5D4)", 0x0AE5D4, 16),
        ):
            words = [
                int.from_bytes(code[start + i * 4 : start + (i + 1) * 4], "little")
                for i in range(count)
            ]
            print(f"  {title}:")
            for row in range(0, len(words), 8):
                addr = start + row * 4
                line = " ".join(f"{w:08X}" for w in words[row : row + 8])
                print(f"    0x{addr:06X}: {line}")

        print("\n=== 18. 置换表交叉引用扫描 (vaddr 0x0046E594 / 0x0046E9BB) ===")
        for file_off in (0x36E594, 0x36E9BB):
            vaddr = file_off + 0x100000
            needle = vaddr.to_bytes(4, "little")
            hits: List[int] = []
            pos = 0
            while True:
                idx = code.find(needle, pos)
                if idx == -1:
                    break
                hits.append(idx)
                pos = idx + 4
            print(f"  vaddr 0x{vaddr:08X} 引用点: {[f'0x{h:06X}' for h in hits[:8]]}")
    except (OSError, ValueError) as exc:
        print(f"[错误] 精确探测失败: {exc}")

def probe_cipher_seed_rule() -> None:
    try:
        gd_raw, _ = decompress_lz11((FE13_ROOT / r"RomFS\data\GameData.bin.lz").read_bytes())
        g_arc = ISBinArchive(gd_raw)
        cipher = g_arc.read_bytes(g_arc.sections["CipherData"], 256)
        inv = {v: i for i, v in enumerate(cipher)}
        zero_idx = inv[0]

        known: Dict[int, int] = {
            0: 0x00, 5: 0x69, 10: 0xDF, 15: 0xBB, 20: 0xDD,
            25: 0x9E, 30: 0x3D, 35: 0xD1, 40: 0x02, 45: 0x91,
            50: 0xDA, 55: 0xEB, 60: 0xF0, 65: 0xD7, 70: 0xF7,
        }
        all_ok = all(((inv[g] - zero_idx) & 0xFF) == d for g, d in known.items())
        print("\n=== 19. CipherData 全量校验 ===")
        print(f"  inv[0] = 0x{zero_idx:02X} | 15 组已知成长率校验: {'PASS' if all_ok else 'FAIL'}")

        s_raw, _ = decompress_lz11((FE13_ROOT / r"RomFS\data\person\static.bin.lz").read_bytes())
        s_arc = ISBinArchive(s_raw)
        print("\n=== 20. static.bin 前 15 角色 seed 与明文成长率还原 ===")
        prev_seed: int | None = None
        for idx, (pid, off) in enumerate(list(s_arc.sections.items())[:15]):
            blk = s_arc.read_bytes(off, 0x50)
            seed = ((blk[0x2C] - zero_idx) & 0xFF) ^ 0x26
            delta = (seed - prev_seed) & 0xFF if prev_seed is not None else 0
            prev_seed = seed
            growths = [
                cipher[(blk[0x24 + i] - (((seed - i * 0x3D) & 0xFF) ^ 0x26)) & 0xFF]
                for i in range(8)
            ]
            zeros = [
                cipher[(blk[0x2C + i] - (((seed - i * 0x3D) & 0xFF) ^ 0x26)) & 0xFF]
                for i in range(8)
            ]
            id_bytes = blk[0x3C:0x4A].hex(" ").upper()
            print(f"  [{idx:02d}] {pid:12s} | seed=0x{seed:02X} (+0x{delta:02X}) | g={growths} | z={zeros} | +3C..49: {id_bytes}")

        print("\n=== 21. GameData.bin 前 10 职业 seed 与明文成长率还原 ===")
        jids = [(k, v) for k, v in g_arc.sections.items() if k.startswith("JID_")][:10]
        prev_jseed: int | None = None
        for idx, (jid, off) in enumerate(jids):
            blk = g_arc.read_bytes(off, 0x38)
            found_seed: int | None = None
            dec_p: List[int] = []
            for cand in range(256):
                p_vals = [
                    cipher[(blk[0x20 + i] - (((cand - i * 0x0D) & 0xFF) ^ 0x07)) & 0xFF]
                    for i in range(8)
                ]
                e_vals = [
                    cipher[(blk[0x28 + i] - (((cand - i * 0x0D) & 0xFF) ^ 0x07)) & 0xFF]
                    for i in range(8)
                ]
                if all(v <= 100 and v % 5 == 0 for v in p_vals + e_vals):
                    found_seed = cand
                    dec_p = p_vals
                    break
            delta = (found_seed - prev_jseed) & 0xFF if (found_seed is not None and prev_jseed is not None) else 0
            prev_jseed = found_seed
            s_str = f"0x{found_seed:02X}" if found_seed is not None else "NONE"
            print(f"  [{idx:02d}] {jid:16s} | seed={s_str} (+0x{delta:02X}) | player_g={dec_p} | +00..03={blk[0:4].hex(' ').upper()}")

        code = (FE13_ROOT / r"ExeFS\code.bin").read_bytes()
        c_pos = code.find(b"CipherData\x00")
        print("\n=== 22. code.bin CipherData 字符串位置 ===")
        print(f"  CipherData 偏移: {f'0x{c_pos:06X} (vaddr 0x{c_pos + 0x100000:08X})' if c_pos != -1 else '未命中'}")
    except (OSError, ValueError, KeyError) as exc:
        print(f"[错误] seed 规律探测失败: {exc}")

def probe_verify_all_seeds() -> None:
    try:
        gd_raw, _ = decompress_lz11((FE13_ROOT / r"RomFS\data\GameData.bin.lz").read_bytes())
        g_arc = ISBinArchive(gd_raw)
        cipher = g_arc.read_bytes(g_arc.sections["CipherData"], 256)
        inv = {v: i for i, v in enumerate(cipher)}
        zero_idx = inv[0]

        s_raw, _ = decompress_lz11((FE13_ROOT / r"RomFS\data\person\static.bin.lz").read_bytes())
        s_arc = ISBinArchive(s_raw)
        p_items = list(s_arc.sections.items())
        p_seeds: List[tuple[int, str, int, List[int]]] = []
        for idx, (pid, off) in enumerate(p_items):
            blk = s_arc.read_bytes(off, 0x38)
            seed = ((blk[0x2C] - zero_idx) & 0xFF) ^ 0x26
            zeros = [
                cipher[(blk[0x2C + i] - (((seed - i * 0x3D) & 0xFF) ^ 0x26)) & 0xFF]
                for i in range(8)
            ]
            p_seeds.append((idx, pid, seed, zeros))

        print(f"\n=== 23. 全量 static.bin 拟合 (共 {len(p_seeds)} 角色) ===")
        nonzero_z = [(i, pid, z) for i, pid, _, z in p_seeds if any(z)]
        print(f"  +0x2C 全零校验异常数: {len(nonzero_z)} {nonzero_z[:3]}")
        p_matches: List[tuple[int, int]] = []
        for c1 in range(256):
            c5 = (p_seeds[0][2] - 0x63 * (0 ^ c1)) & 0xFF
            if all(((c5 + 0x63 * (i ^ c1)) & 0xFF) == s for i, _, s, _ in p_seeds):
                p_matches.append((c1, c5))
        print(f"  100% 命中 (C1_p, C5_p): {[(f'0x{a:02X}', f'0x{b:02X}') for a, b in p_matches]}")
        if p_matches:
            c1_p, c5_p = p_matches[0]
            for idx in (0, 15, 30, len(p_items) - 1):
                pid, off = p_items[idx]
                blk = s_arc.read_bytes(off, 0x30)
                sd = (c5_p + 0x63 * (idx ^ c1_p)) & 0xFF
                g = [cipher[(blk[0x24 + i] - (((sd - i * 0x3D) & 0xFF) ^ 0x26)) & 0xFF] for i in range(8)]
                print(f"  抽检 [{idx:02d}] {pid:14s} -> g={g}")

        j_items = [(k, v) for k, v in g_arc.sections.items() if k.startswith("JID_")]
        print(f"\n=== 24. 全量 GameData.bin 职业拟合 (共 {len(j_items)} 职业) ===")
        j_best: List[tuple[int, int, int]] = []
        for c1 in range(256):
            c5 = (0xED - 0x23 * (0 ^ c1)) & 0xFF
            ok_cnt = 0
            for idx, (_, off) in enumerate(j_items):
                blk = g_arc.read_bytes(off, 0x30)
                sd = (c5 + 0x23 * (idx ^ c1)) & 0xFF
                vals = [
                    cipher[(blk[base + i] - (((sd - i * 0x0D) & 0xFF) ^ 0x07)) & 0xFF]
                    for base in (0x20, 0x28)
                    for i in range(8)
                ]
                if all(v <= 150 and v % 5 == 0 for v in vals):
                    ok_cnt += 1
            if ok_cnt == len(j_items):
                j_best.append((ok_cnt, c1, c5))
        print(f"  100% 命中 (count, C1_j, C5_j): {[(n, f'0x{a:02X}', f'0x{b:02X}') for n, a, b in j_best]}")
        if j_best:
            _, c1_j, c5_j = j_best[0]
            for idx in (0, 20, 50, len(j_items) - 1):
                jid, off = j_items[idx]
                blk = g_arc.read_bytes(off, 0x30)
                sd = (c5_j + 0x23 * (idx ^ c1_j)) & 0xFF
                pg = [cipher[(blk[0x20 + i] - (((sd - i * 0x0D) & 0xFF) ^ 0x07)) & 0xFF] for i in range(8)]
                print(f"  抽检 [{idx:02d}] {jid:18s} -> player_g={pg}")

        code = (FE13_ROOT / r"ExeFS\code.bin").read_bytes()
        needle = (0x2B0094 + 0x100000).to_bytes(4, "little")
        refs: List[int] = []
        pos = 0
        while (hit := code.find(needle, pos)) != -1:
            refs.append(hit)
            pos = hit + 4
        print(f"\n=== 25. code.bin CipherData 字符串引用点 ===\n  refs: {[f'0x{r:06X}' for r in refs[:8]]}")
    except (OSError, ValueError, KeyError) as exc:
        print(f"[错误] 全量拟合失败: {exc}")

if __name__ == "__main__":
    probe_static()
    probe_gamedata_tables()
    probe_romfs_layout()
    probe_phase0_deep()
    probe_cipher_and_assets()
    probe_phase0_final()
    probe_cipher_exact()
    probe_cipher_seed_rule()
    probe_verify_all_seeds()