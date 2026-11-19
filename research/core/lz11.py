import struct
from typing import Tuple


def decompress_lz11(data: bytes) -> Tuple[bytes, bool]:
    """
    解压 3DS 火焰纹章的 .bin.lz 文件。
    返回: (解压后的字节流, 是否带有 FE14/FE15 的 0x13 外层封装头)
    """
    if len(data) < 4:
        raise ValueError("数据长度不足，不是有效的 LZ11 文件")

    has_lz13_wrapper = False
    pos = 0

    # FE14 / FE15 常见 4 字节 0x13 外层封装头，紧跟 0x11 正式流
    if data[0] == 0x13 and len(data) >= 8 and data[4] == 0x11:
        has_lz13_wrapper = True
        pos = 4

    if data[pos] != 0x11:
        raise ValueError(f"不支持的压缩魔数: 0x{data[pos]:02X}（期望 0x11）")

    decomp_size = data[pos + 1] | (data[pos + 2] << 8) | (data[pos + 3] << 16)
    pos += 4
    if decomp_size == 0:
        decomp_size = struct.unpack_from("<I", data, pos)[0]
        pos += 4

    out = bytearray()
    data_len = len(data)

    while len(out) < decomp_size and pos < data_len:
        flags = data[pos]
        pos += 1

        for bit in range(7, -1, -1):
            if len(out) >= decomp_size or pos >= data_len:
                break

            if (flags & (1 << bit)) == 0:
                out.append(data[pos])
                pos += 1
            else:
                b1 = data[pos]
                b2 = data[pos + 1]
                indicator = b1 >> 4

                if indicator == 0:
                    b3 = data[pos + 2]
                    length = (((b1 & 0x0F) << 4) | (b2 >> 4)) + 0x11
                    disp = (((b2 & 0x0F) << 8) | b3) + 1
                    pos += 3
                elif indicator == 1:
                    b3 = data[pos + 2]
                    b4 = data[pos + 3]
                    length = (((b1 & 0x0F) << 12) | (b2 << 4) | (b3 >> 4)) + 0x111
                    disp = (((b3 & 0x0F) << 8) | b4) + 1
                    pos += 4
                else:
                    length = indicator + 1
                    disp = (((b1 & 0x0F) << 8) | b2) + 1
                    pos += 2

                # 必须逐字节复制，支持 disp < length 的重叠 RLE
                for _ in range(length):
                    out.append(out[-disp])

    return bytes(out[:decomp_size]), has_lz13_wrapper


def compress_lz11(raw: bytes, add_lz13_wrapper: bool = False) -> bytes:
    """
    将原始二进制数据重新压缩为标准 3DS LZ11 格式（带哈希链加速）。
    """
    n = len(raw)
    out = bytearray()

    if add_lz13_wrapper:
        out.append(0x13)
        out.extend((n & 0xFF, (n >> 8) & 0xFF, (n >> 16) & 0xFF))

    out.append(0x11)
    if n < 0x1000000:
        out.extend((n & 0xFF, (n >> 8) & 0xFF, (n >> 16) & 0xFF))
    else:
        out.extend(b"\x00\x00\x00")
        out.extend(struct.pack("<I", n))

    # 3 字节前缀哈希表，限制每个桶的回溯深度以兼顾速度与压缩率
    buckets: dict[bytes, list[int]] = {}
    pos = 0
    max_window = 4096
    max_match = 0x10110

    while pos < n:
        flag_pos = len(out)
        out.append(0)
        flags = 0

        for bit in range(7, -1, -1):
            if pos >= n:
                break

            best_len = 0
            best_disp = 0

            if pos + 3 <= n:
                key = raw[pos : pos + 3]
                candidates = buckets.get(key)
                if candidates:
                    limit_start = max(0, pos - max_window)
                    # 从最近的候选位置向前搜索（最多检查 32 个候选）
                    for cand in reversed(candidates[-32:]):
                        if cand < limit_start:
                            break
                        match_len = 3
                        max_possible = min(max_match, n - pos)
                        while match_len < max_possible and raw[cand + match_len] == raw[pos + match_len]:
                            match_len += 1
                        if match_len > best_len:
                            best_len = match_len
                            best_disp = pos - cand
                            if best_len >= 64:
                                break

            if best_len >= 3:
                flags |= (1 << bit)
                d = best_disp - 1
                if best_len <= 16:
                    out.append(((best_len - 1) << 4) | ((d >> 8) & 0x0F))
                    out.append(d & 0xFF)
                elif best_len <= 272:
                    l = best_len - 0x11
                    out.append(l >> 4)
                    out.append(((l & 0x0F) << 4) | ((d >> 8) & 0x0F))
                    out.append(d & 0xFF)
                else:
                    l = best_len - 0x111
                    out.append(0x10 | ((l >> 12) & 0x0F))
                    out.append((l >> 4) & 0xFF)
                    out.append(((l & 0x0F) << 4) | ((d >> 8) & 0x0F))
                    out.append(d & 0xFF)

                # 为跳过区间补录哈希索引
                end_pos = min(pos + best_len, n - 2)
                for p in range(pos, end_pos):
                    k = raw[p : p + 3]
                    buckets.setdefault(k, []).append(p)
                pos += best_len
            else:
                if pos + 3 <= n:
                    k = raw[pos : pos + 3]
                    buckets.setdefault(k, []).append(pos)
                out.append(raw[pos])
                pos += 1

        out[flag_pos] = flags

    return bytes(out)