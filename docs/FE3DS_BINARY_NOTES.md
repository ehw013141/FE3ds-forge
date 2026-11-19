# 3DS 火焰纹章实锤二进制结构笔记

本文档记录经实机 ROM 样本交叉验证的底层格式与偏移结论，供核心维护者与社区贡献者查阅。

## FE13《火焰纹章：觉醒》（已实锤验证）

### 1. 容器与压缩
* **压缩格式**：`RomFS/data/*.bin.lz` 与 `RomFS/m/*.bin.lz` 为标准 3DS `LZ11`（`0x11` 魔数）；`RomFS/face/*.arc` 内部贴图为 `LZ13` 外壳（`0x13`）+ `LZ11` + `CTPK`。
* **IS `.bin` 四段结构**：`0x20` 头部（`file_size, data_size, ptr1_count, ptr2_count`）→ `Data` 数据区 → `ptr1` 指针表 → `ptr2` 命名标签表与 Shift-JIS 字符串池。

### 2. 成长率混淆算法（人物与职业同源）
* **256 字节双射表**：位于 `RomFS/data/GameData.bin` 的 `ptr2["CipherData"]`（长度 256 B）。
* **加解密公式**：
  * 解密：`plain = CipherData[(enc - k[i]) & 0xFF]`
  * 加密：`enc = (inv_CipherData[plain] + k[i]) & 0xFF`
* **人物表 (`RomFS/data/person/static.bin.lz`, `PID_`, 57 项, 步长 `0x238`)**：
  * `seed(idx) = (0x22 + 0x63 * (idx ^ 0x18)) & 0xFF`，`k[i] = ((seed - i * 0x3D) & 0xFF) ^ 0x26`
  * `+0x1C` (8B `i8`): 个人基础修正；`+0x24` (8B 加密): 个人成长率；`+0x2C` (8B 加密): 敌方/增援成长加成；`+0x34` (8B `i8`): 个人上限修正。
* **职业表 (`RomFS/data/GameData.bin.lz`, `JID_`, 83 项, 步长 `0x80`)**：
  * `seed(idx) = (0x22 + 0x23 * (idx ^ 0x39)) & 0xFF`，`k[i] = ((seed - i * 0x0D) & 0xFF) ^ 0x07`
  * `+0x18` (8B `u8`): 职业基础属性；`+0x20` (8B 加密): 我方职业成长率；`+0x28` (8B 加密): 敌方职业成长率；`+0x30` (8B `u8`): 职业能力上限。

### 3. 文本、立绘与 ExeFS 经验结算
* **汉化文本 (`RomFS/m/GameData.bin.lz`)**：`ptr2` 直接映射 `MPID_*`/`MJID_*`/`MIID_*` 至 UTF-16LE 零终止字符串（共 1,315 条）。
* **头像容器 (`RomFS/face/*_fc_*.arc`)**：IS `.bin` 归档，内含各表情 `CTPK` 压缩贴图。
* **战后经验注入点 (`ExeFS/code.bin`)**：
  * AOB：`64 00 51 E3 01 40 A0 E1 8D 60 D5 E5 68 00 95 E5`（实测偏移 `0x1862E0` / 虚址 `0x002862E0`）。
  * 覆写前 8 字节：`[运算 R1 -> R4 指令] + E3540064 (CMP R4, #0x64)`，完整保留原版 100 经验封顶。

## FE14《if》 / FE15《回声》（Phase 4 待实锤补充）
* `LZ13`（`0x13` 外层头）与 `ISBinArchive` 容器层已在 `fe3ds-core` 预先兼容，待 Phase 4 用 `research/` 探针补充精确表偏移与经验注入点。