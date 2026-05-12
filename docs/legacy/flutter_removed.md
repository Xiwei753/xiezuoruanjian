# 遗留 Flutter 客户端移除说明

原先的 `apps/flutter_legacy` 已被正式移除。不再保留可编译旧工程。
所有跨平台能力收敛至 `core/writer_core`。Android 和 Linux 客户端现在仅作为特定平台的容器。

由于以下原因移除了 Flutter 遗留工程：
1. 双重数据源冲突：Flutter 在自身侧和 Rust 核心库中都具有全套读写、同步规则等，导致不一致且难以维护。
2. 遗留包袱：包含了诸如底层文本渲染动画、Wayland 兼容等过于深层的 UI 细节尝试。
3. 架构转向：向“Rust 统一内核，各端独立薄壳 UI”方向演进。

后续开发不再支持或维护 Flutter 版本。
