<div align="center">
  <img src="src-tauri/icons/128x128@2x.png" alt="FE3DS Forge Logo" width="120" height="120" />
  <h1>⚔️ FE3DS Forge</h1>
  <p><strong>面向 3DS《火焰纹章》三部曲的轻量级、单文件绿色通用修改器</strong></p>

  <p>
    <a href="https://github.com/ehw013141/FE3ds-forge/releases/latest">
      <img src="https://img.shields.io/github/v/release/ehw013141/FE3ds-forge?style=flat-square&color=d97706&label=Release" alt="Latest Release" />
    </a>
    <img src="https://img.shields.io/badge/Platform-Windows-0078D6?style=flat-square&logo=windows" alt="Platform" />
    <img src="https://img.shields.io/badge/Rust-fe3ds--core-DEA584?style=flat-square&logo=rust&logoColor=white" alt="Rust" />
    <img src="https://img.shields.io/badge/Tauri-v2-FFC131?style=flat-square&logo=tauri&logoColor=black" alt="Tauri v2" />
    <img src="https://img.shields.io/badge/Vue-3-4FC08D?style=flat-square&logo=vuedotjs&logoColor=white" alt="Vue 3" />
  </p>

  <p>
    <a href="#-支持作品与特性">支持作品</a> •
    <a href="#-快速上手">快速上手</a> •
    <a href="#-补丁加载指南">补丁加载</a> •
    <a href="#️-开发与构建">开发构建</a>
  </p>
</div>

---

> [!TIP]
> **零配置开箱即用**：无需安装任何运行库或外部解包工具，前往 [Releases 页面](https://github.com/ehw013141/FE3ds-forge/releases/latest) 下载 `FE3DS-Forge.exe` 即可双击运行。

<!-- 
💡 建议后续在这里放 1~2 张软件主界面截图，观感会直接拉满：
把截图拖进 GitHub Issue 输入框获取图片链接，或存到 docs/preview.png 后取消下面注释：
<div align="center">
  <img src="docs/preview.png" alt="FE3DS Forge 主界面预览" width="85%" />
</div>
-->

## ✨ 核心亮点

- 🚀 **秒开镜像，免手动解包**：内置 `NCCH / NCSD` 虚拟文件系统与 `LZ11 / LZ13 / BLZ` 编解码器，直接读取未加密 `.cci` / `.3ds` ROM 或已解包目录。
- 🎨 **原汁原味可视化编辑**：自动加载游戏内汉化/日英译名，原生解码 `CTPK` 与 `BCH`（ETC1A4）角色立绘头像。
- ⚡ **批量法则与无损写回**：支持单角色精调、全表成长率批量偏移（`+10 / +20 / +30 / +50` 或自定义）与一键还原，精准保留原始二进制结构。
- 📦 **一站式双端补丁导出**：一次导出同时生成 `romfs/` 数据补丁、`exefs/code.ips` 经验补丁与 `cheats.txt`（Azahar / Citra 金手指）。
- 🧩 **Schema 驱动热扩展**：在程序同级目录放置 `schemas/<game>/*.json` 即可热覆盖或扩展自定义数据表（如武器、技能表）。

---

## 🎮 支持作品与特性

| 作品 | 数据表支持 | 文本与头像解码 | 战后经验规则 (`ExeFS`) |
| :--- | :--- | :--- | :--- |
| **FE13《觉醒》** | 57 角色 + 83 职业<br>*(自动 `CipherData` 加解密)* | 汉化/日英文本<br>`CTPK` (ETC1A4) 头像 | 单点 `0x1862E0`<br>*(0.5×~5× / 固定 1~100)* |
| **FE14《if》** | 255 角色 + 129 职业<br>*(明文多表合并写回)* | 汉化/日英文本<br>`BCH` (ETC1A4) 头像 | 单点 `0x42D7DC`<br>*(0.5×~5× / 固定 1~100)* |
| **FE15《回声》** | 303 角色 + 73 职业<br>*(7 维属性 / 移动力 / 成长修正)* | 合并 `Name` + `Job` 译名<br>`BCH` (ETC1A4) 头像 | 双点 `0x350B6C` + `0x353770`<br>*(战斗与白魔法同步生效)* |

---

## 📖 快速上手

1. **下载运行**：从 [Releases](https://github.com/ehw013141/FE3ds-forge/releases/latest) 下载 `FE3DS-Forge.exe` 并双击启动。
2. **加载游戏**：点击右上角 **打开镜像**（支持已解密 `.cci` / `.3ds`）或 **打开目录**（包含 `RomFS` / `ExeFS` 的解包目录），程序会自动识别作品。
3. **编辑数值**：
   - 在左侧切换 **角色成长率** / **职业成长率**，手动微调或使用顶部「批量法则」一键加减。
   - 在 **战后经验规则** 中选择倍率（如 `2×`）或固定经验值。
4. **导出补丁**：点击右上角 **导出补丁**，默认输出至程序同级 `output/` 目录。

---

## 🕹️ 补丁加载指南

### Azahar / Citra 模拟器
1. 在模拟器游戏列表中右键对应游戏 → **打开 Mod 位置**。
2. 将 `output/` 中的 `romfs` 与 `exefs` 文件夹直接复制到该 Mod 目录下（例如 `...\load\mods\<TitleID>\romfs\...` 与 `...\exefs\code.ips`）。
3. **冷重启游戏**即可生效。

> [!IMPORTANT]
> 修改完 `romfs` 补丁后务必**冷重启游戏**并读取游戏内正常存档，切勿直接读取修改前的模拟器即时存档（Save State），否则内存中仍会残留原版成长率表。

<details>
<summary><strong>📱 3DS 实机（Luma3DS）加载方法</strong></summary>

1. 将 `output/` 中的 `romfs` 与 `exefs` 文件夹复制到 SD 卡的 `/luma/titles/<TitleID>/` 目录。
2. 开机按住 `Select` 进入 Luma3DS 配置菜单，勾选 **Enable game patching** 并按 `Start` 保存重启即可。

</details>

---

## 🛠️ 开发与构建

### 环境要求
- [Rust](https://www.rust-lang.org/) `1.78+`
- [Node.js](https://nodejs.org/) `20+`
- Windows 10/11（默认自带 WebView2）

<details>
<summary><strong>📂 查看项目目录结构与常用构建命令</strong></summary>

```text
fe3ds-forge/
├── crates/fe3ds-core/   # 纯 Rust 核心库（VFS / LZ11 / ISBin / Cipher / CTPK & BCH / ExeFS Patcher / Schema）
├── schemas/             # FE13 / FE14 / FE15 内置数据表 JSON Schema
├── src/                 # Vue 3 + TypeScript 前端界面
└── src-tauri/           # Tauri v2 桌面端宿主与 Workspace 状态机


⚖️ 免责声明
本项目仅用于游戏二进制数据结构研究与个人正版备份的本地化修改，不包含任何受版权保护的游戏镜像、密钥或官方资源文件。