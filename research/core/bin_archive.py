import struct
from dataclasses import dataclass
from typing import Dict, List


@dataclass
class BinHeader:
    file_size: int
    data_size: int
    ptr1_count: int
    ptr2_count: int


class ISBinArchive:
    """Intelligent Systems 3DS 通用 .bin 容器（适用于 FE13 / FE14 / FE15）"""

    HEADER_SIZE = 0x20

    def __init__(self, raw: bytes):
        if len(raw) < self.HEADER_SIZE:
            raise ValueError("不是有效的 IS .bin 文件")

        self.raw = bytearray(raw)
        file_size, data_size, ptr1_count, ptr2_count = struct.unpack_from("<4I", raw, 0)
        self.header = BinHeader(file_size, data_size, ptr1_count, ptr2_count)

        self.ptr1_offset = self.HEADER_SIZE + data_size
        self.ptr2_offset = self.ptr1_offset + ptr1_count * 4
        self.label_region_offset = self.ptr2_offset + ptr2_count * 8

        self.ptr1: List[int] = list(
            struct.unpack_from(f"<{ptr1_count}I", raw, self.ptr1_offset)
        )
        self.sections: Dict[str, int] = {}
        self._parse_ptr2()

    def _read_cstr(self, offset: int) -> str:
        end = self.raw.find(b"\x00", offset)
        if end == -1:
            end = len(self.raw)
        return self.raw[offset:end].decode("shift_jis", errors="replace")

    def _parse_ptr2(self) -> None:
        for i in range(self.header.ptr2_count):
            entry_off = self.ptr2_offset + i * 8
            data_rel_off, label_rel_off = struct.unpack_from("<2I", self.raw, entry_off)
            label = self._read_cstr(self.label_region_offset + label_rel_off)
            # 保存为在数据区（不含 0x20 头）内的相对偏移
            self.sections[label] = data_rel_off

    def read_ptr_str(self, data_rel_off: int) -> str:
        """从数据区指定偏移处读取一个内部指针指向的字符串"""
        target_rel = struct.unpack_from("<I", self.raw, self.HEADER_SIZE + data_rel_off)[0]
        if target_rel == 0 or self.HEADER_SIZE + target_rel >= self.ptr1_offset:
            return ""
        return self._read_cstr(self.HEADER_SIZE + target_rel)

    def read_bytes(self, data_rel_off: int, length: int) -> bytes:
        start = self.HEADER_SIZE + data_rel_off
        return bytes(self.raw[start : start + length])

    def write_bytes(self, data_rel_off: int, payload: bytes) -> None:
        start = self.HEADER_SIZE + data_rel_off
        self.raw[start : start + len(payload)] = payload

    def to_bytes(self) -> bytes:
        return bytes(self.raw)