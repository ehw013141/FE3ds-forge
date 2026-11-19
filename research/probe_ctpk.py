import struct
from pathlib import Path
from probe_fe13 import FE13_ROOT, ISBinArchive, decompress_lz11

CTPK_FORMATS = {
    0: "RGBA8", 1: "RGB8", 2: "RGBA5551", 3: "RGB565", 4: "RGBA4444",
    5: "LA8", 6: "HILO8", 7: "L8", 8: "A8", 9: "LA4", 10: "L4", 11: "A4",
    12: "ETC1", 13: "ETC1A4",
}


def inspect_face_arc(arc_path: Path) -> None:
    arc = ISBinArchive(arc_path.read_bytes())
    entries = [(k, v) for k, v in arc.sections.items() if k.endswith(".ctpk.lz")]
    print(f"\n[{arc_path.name}] 包含 {len(entries)} 个表情贴图，前 5 项:")
    for name, info_off in entries[:5]:
        u0, u1, u2, u3 = struct.unpack_from("<4I", arc.read_bytes(info_off, 16))
        print(f"  {name:18s} | info@0x{info_off:X} -> ({u0}, 0x{u1:X}, 0x{u2:X}, 0x{u3:X})")

    # 直接解压首个表情（Data+0 起点）并解析 CTPK 头
    first_lz = arc.read_bytes(0, min(len(arc.raw) - ISBinArchive.HEADER_SIZE, 0x20000))
    ctpk, is_lz13 = decompress_lz11(first_lz)
    magic, ver, tex_cnt, data_off, data_sz, crc_off, info_off = struct.unpack_from("<4sHHIIII", ctpk, 0)
    print(f"  首贴图解压: {len(ctpk)} B (lz13={is_lz13}) | magic={magic} ver={ver} count={tex_cnt} data_off=0x{data_off:X}")
    for i in range(min(tex_cnt, 3)):
        ent_off = 0x20 + i * 0x20
        name_off, tex_sz, tex_off, fmt, w, h, mip, typ = struct.unpack_from("<IIIIHHBB", ctpk, ent_off)
        fmt_name = CTPK_FORMATS.get(fmt, f"UNKNOWN({fmt})")
        print(f"    tex[{i}]: {w}x{h} | fmt={fmt} ({fmt_name}) | size={tex_sz}B | tex_off=0x{tex_off:X} | mip={mip} | typ={typ}")


def probe_pid_face_mapping() -> None:
    st_raw, _ = decompress_lz11((FE13_ROOT / r"RomFS\data\person\static.bin.lz").read_bytes())
    st_arc = ISBinArchive(st_raw)
    face_dir = FE13_ROOT / "RomFS" / "face"

    matched, missed = [], []
    for pid in st_arc.sections:
        stem = pid.removeprefix("PID_")
        cand = face_dir / f"{stem}_fc_通常.arc"
        if cand.is_file():
            matched.append(pid)
        else:
            missed.append(pid)

    print(f"\n=== PID -> *_fc_通常.arc 直接映射统计 ({len(matched)}/{len(st_arc.sections)}) ===")
    print(f"  未直接命中 ({len(missed)} 项，展示前 10 项): {missed[:10]}")


if __name__ == "__main__":
    try:
        for sample in ("クロム_fc_通常.arc", "リズ_fc_通常.arc"):
            p = FE13_ROOT / "RomFS" / "face" / sample
            if p.is_file():
                inspect_face_arc(p)
        probe_pid_face_mapping()
    except (OSError, ValueError, struct.error) as exc:
        print(f"[错误] CTPK 探测失败: {exc}")