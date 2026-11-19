from pathlib import Path
from core.lz11 import decompress_lz11, compress_lz11
from core.bin_archive import ISBinArchive
from core.exefs_patcher import FE13_EXP_AOB, find_aob

FE13_ROOT = Path(r"D:\fe13")


def verify_bin(rel_path: str):
    path = FE13_ROOT / rel_path
    orig_lz = path.read_bytes()
    raw, is_lz13 = decompress_lz11(orig_lz)

    # 往返校验：重新压缩后再解压，必须与原始解压字节 100% 一致
    recompressed = compress_lz11(raw, add_lz13_wrapper=is_lz13)
    roundtrip, _ = decompress_lz11(recompressed)
    assert raw == roundtrip, f"{rel_path} 往返校验失败！"

    arc = ISBinArchive(raw)
    print(f"\n=== {rel_path} ===")
    print(f"原始压缩: {len(orig_lz)} B | 解压后: {len(raw)} B | 重压缩: {len(recompressed)} B | 无损校验: PASS")
    print(f"Header: {arc.header}")
    print(f"命名表 (ptr2 共 {len(arc.sections)} 项，展示前 15 项):")
    for idx, (name, off) in enumerate(list(arc.sections.items())[:15]):
        print(f"  [{idx:02d}] {name:30s} -> data+0x{off:06X}")


def verify_code():
    code_path = FE13_ROOT / "ExeFS" / "code.bin"
    code = code_path.read_bytes()
    off = find_aob(code, FE13_EXP_AOB)
    print("\n=== ExeFS/code.bin ===")
    if off is not None:
        print(f"经验结算 AOB 命中！文件偏移: 0x{off:06X} | 内存地址: 0x{off + 0x100000:08X}")
    else:
        print("未命中默认 AOB，需要检查版本差异。")


if __name__ == "__main__":
    verify_bin(r"RomFS\data\person\static.bin.lz")
    verify_bin(r"RomFS\data\GameData.bin.lz")
    verify_code()