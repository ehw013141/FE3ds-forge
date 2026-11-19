import struct
from pathlib import Path
from typing import Dict, Optional


# 预设经验值指令生成器（FE13：在 0x1862E0 处用 8 字节替换原 CMP + MOV）
FE13_EXP_AOB = bytes.fromhex("64 00 51 E3 01 40 A0 E1 8D 60 D5 E5 68 00 95 E5")

FE13_MULTIPLIER_OPCODES: Dict[str, int] = {
    "0.5x": 0xE1A040A1,  # MOV R4, R1, LSR #1
    "1.5x": 0xE08140A1,  # ADD R4, R1, R1, LSR #1
    "2x":   0xE1A04081,  # MOV R4, R1, LSL #1
    "3x":   0xE0814081,  # ADD R4, R1, R1, LSL #1
    "4x":   0xE1A04101,  # MOV R4, R1, LSL #2
    "5x":   0xE0814101,  # ADD R4, R1, R1, LSL #2
}


def find_aob(code_bytes: bytes, pattern: bytes) -> Optional[int]:
    idx = code_bytes.find(pattern)
    return None if idx == -1 else idx


def build_fe13_exp_patch(mode: str, value: str | int) -> bytes:
    """
    生成 FE13 经验修改的 8 字节 ARM 机器码。
    mode="multiplier", value in ("0.5x", "1.5x", "2x", "3x", "4x", "5x")
    mode="fixed", value in 1..100
    """
    if mode == "multiplier":
        key = str(value)
        if key not in FE13_MULTIPLIER_OPCODES:
            raise ValueError(f"不支持的倍率: {key}")
        insn1 = FE13_MULTIPLIER_OPCODES[key]
    elif mode == "fixed":
        v = int(value)
        if not (1 <= v <= 100):
            raise ValueError("固定经验值必须在 1 到 100 之间")
        insn1 = 0xE3A04000 | v  # MOV R4, #v
    else:
        raise ValueError(f"未知模式: {mode}")

    insn2 = 0xE3540064  # CMP R4, #0x64（保留原版 100 封顶校验）
    return struct.pack("<2I", insn1, insn2)


def export_ips(records: Dict[int, bytes], out_path: Path) -> None:
    """导出标准 3DS IPS 补丁（可直接作为 mods/{TitleID}/exefs/code.ips 使用）"""
    buf = bytearray(b"PATCH")
    for offset, payload in sorted(records.items()):
        if offset > 0xFFFFFF:
            raise ValueError("标准 IPS 不支持大于 16MB 的偏移")
        buf.extend(offset.to_bytes(3, "big"))
        buf.extend(len(payload).to_bytes(2, "big"))
        buf.extend(payload)
    buf.extend(b"EOF")
    out_path.write_bytes(buf)


def export_azahar_cheat(
    title: str, file_offset: int, payload: bytes, out_path: Path
) -> None:
    """导出 Azahar / Citra 原生 .txt 金手指（带 *citra_enabled）"""
    lines = [f"[{title}]", "*citra_enabled"]
    base_vaddr = file_offset + 0x00100000
    for i in range(0, len(payload), 4):
        word = struct.unpack_from("<I", payload, i)[0]
        lines.append(f"{base_vaddr + i:08X} {word:08X}")
    out_path.write_text("\n".join(lines) + "\n", encoding="utf-8")