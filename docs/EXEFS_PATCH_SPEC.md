# ExeFS 汇编补丁与特征码开发规范

`crates/fe3ds-core/src/exefs_patcher.rs` 负责扫描解压后的 `ExeFS/code.bin`，定位逻辑注入点并同步生成标准 `code.ips` 与 Azahar `.txt` 金手指。

## 1. 内存虚拟地址与文件偏移换算

3DS 加载未压缩的 `code.bin` 时，固定将其映射至虚拟地址 `0x00100000` 起始位置：

```text
file_offset = virtual_address - 0x00100000