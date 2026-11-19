# FE3DS Forge

一款面向 3DS《火焰纹章》三部曲的轻量级、单文件绿色通用修改器，基于 **Rust + Tauri v2 + Vue 3 + TypeScript** 构建。支持直接读取未加密 `.cci` / `.3ds` ROM 镜像或已解包目录，一站式导出兼容 **Azahar / Citra 模拟器** 与 **实机 Luma3DS LayeredFS** 的补丁包。

---

## 支持作品与核心功能

| 作品 | 数据表支持 | 文本与头像 | 战后经验规则 (`ExeFS`) |
| :--- | :--- | :--- | :--- |
| **FE13《觉醒》** | 57 角色 + 83 职业（自动 `CipherData` 加解密） | 汉化/日英文本 + `CTPK` (ETC1A4) 头像 | 单点 `0x1862E0`（倍率 0.5×~5× / 固定 1~100） |
| **FE14《if》** | 255 角色 + 129 职业（明文，多表合并写回） | 汉化/日英文本 + `BCH` (ETC1A4) 头像 | 单点 `0x42D7DC`（倍率 0.5×~5× / 固定 1~100） |
| **FE15《回声》** | 303 角色 + 73 职业（7 维属性，移动力与成长修正） | 合并 `Name`/`Job` 译名 + `BCH` (ETC1A4) 头像 | 双点 `0x350B6C` + `0x353770`（战斗 + 白魔法同步生效） |

### 特性亮点
- **秒开镜像，零外置依赖**：内置 `NCCH / NCSD` 虚拟文件系统、`LZ11 / LZ13 / BLZ` 编解码与 JSON Schema，打包后为单个便携 `.exe`。
- **无损修改与批量法则**：支持单角色精调、全表成长率批量偏移（`+10 / +20 / +30 / +50` 或自定义）与一键还原。
- **双端补丁导出**：一次导出同时生成 `romfs/` 补丁、`exefs/code.ips` 以及 `cheats.txt`（Azahar/Citra 金手指）。
- **Schema 驱动扩展**：在程序同级放置 `schemas/<game>/*.json` 即可热扩展或覆盖自定义数据表（如武器、技能表）。

---

## 使用说明

### 1. 加载游戏
启动 `FE3DS-Forge.exe`，点击右上角 **打开镜像**（支持已解密的 `.cci` / `.3ds`）或 **打开目录**（支持包含 `RomFS` / `ExeFS` 的解包目录），程序将自动识别作品并加载汉化译名与角色头像。

### 2. 编辑与导出
1. 在左侧选择 **角色成长率** / **职业成长率**，调整数值或使用顶部「批量法则」一键修改。
2. 在左侧 **战后经验规则** 中选择经验倍率（如 `2×`）或固定经验值。
3. 点击右上角 **导出补丁**，默认输出到当前目录的 `output/` 文件夹。

### 3. 在 Azahar / Citra 模拟器中加载
1. 在模拟器游戏列表中右键对应游戏 → **打开 Mod 位置**。
2. 将 `output/` 中的 `romfs` 与 `exefs` 文件夹直接复制到该 Mod 目录下（例如 `...\load\mods\<TitleID>\romfs\...` 与 `...\exefs\code.ips`）。
3. **冷重启游戏**（切勿直接读取旧的即时存档）即可生效。

> 实机 Luma3DS 用户请将 `romfs` 与 `exefs` 复制到 SD 卡的 `/luma/titles/<TitleID>/` 目录，并在 Luma 设置中开启 **Enable game patching**。

---

## 开发与构建

### 环境要求
- [Rust](https://www.rust-lang.org/) (1.78+)
- [Node.js](https://nodejs.org/) (20+)
- Windows 平台需安装 WebView2（Windows 10/11 默认自带）

### 目录结构
```text
fe3ds-forge/
├── crates/fe3ds-core/   # 纯 Rust 核心库（VFS / LZ11 / ISBin / Cipher / CTPK & BCH / ExeFS Patcher / Schema）
├── schemas/             # FE13 / FE14 / FE15 内置数据表 JSON Schema
├── src/                 # Vue 3 + TypeScript 前端界面
└── src-tauri/           # Tauri v2 桌面端宿主与 Workspace 状态机