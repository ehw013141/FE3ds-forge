# Schema 声明式数据表扩展规范

任何贡献者无需修改 Rust 引擎或前端基础代码，只需在 `schemas/<game>/` 下新增一份 JSON 文件，即可为修改器扩展新的可视化编辑模块（如武器、道具、技能、地形、章节配置）。

## 顶层字段契约

| 字段 | 类型 | 必填 | 说明 |
| :--- | :--- | :--- | :--- |
| `id` | `string` | 是 | 全局唯一表标识，如 `fe13_weapons` |
| `game` | `string` | 是 | 作品代号：`fe13` / `fe14` / `fe15` |
| `title` | `string` | 是 | 界面导航栏与标题显示的中文名称 |
| `target_file` | `string` | 是 | 相对游戏根目录的目标 `.bin.lz` 路径 |
| `entry_prefix` | `string` | 是 | `ptr2` 标签匹配前缀（如 `PID_`、`JID_`、`IID_`、`RSID_`） |
| `entry_size` | `number` | 是 | 单条结构体步长（字节），引擎据此强制执行写越界检查 |
| `cipher_profile` | `object` | 否 | 含加密成长率字段时必填（`c1`, `c5`, `idx_mul`, `stat_step`, `xor_mask`） |
| `fields` | `FieldDef[]` | 是 | 可视化字段定义数组 |

## 支持的字段类型 (`FieldKind`)

* `u8` / `i8` / `u16` / `i16` / `u32`：小端序整型（自动执行类型范围校验）。
* `u8_array` / `i8_array`：需指定 `len`，适用于 8 维基础值、上限或武器熟练度。
* `ciphered_u8_array`：需 `len: 8` 且顶层配置 `cipher_profile`，由内核自动经 `CipherData` 加解密。

## 示例：零代码扩展 FE13 武器/道具表 (`schemas/fe13/items.json`)

```json
{
  "id": "fe13_items",
  "game": "fe13",
  "title": "武器与道具",
  "target_file": "RomFS/data/GameData.bin.lz",
  "entry_prefix": "IID_",
  "entry_size": 56,
  "fields": [
    { "key": "might", "label": "威力", "offset": 24, "kind": { "type": "u8" }, "min": 0, "max": 255 },
    { "key": "hit", "label": "命中", "offset": 25, "kind": { "type": "u8" }, "min": 0, "max": 255 },
    { "key": "crit", "label": "必杀", "offset": 26, "kind": { "type": "u8" }, "min": 0, "max": 255 }
  ]
}