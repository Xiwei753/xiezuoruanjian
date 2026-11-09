# Windows 客户端

Windows 正式入口唯一指向 [`qt/`](qt/)：Rust + Qt6/QML。

## 职责

- `qt/` 提供 Windows 窗口、TSF/IME 输入、Qt Quick Scene Graph 渲染、部署与系统集成。
- 复用 `apps/Linux_qt` 中平台无关的 QML、编辑器、星图和后端实现；不复制业务规则。
- 直接链接 Rust `writer_core` / 平台能力接口，不再经过 C# / WinRT 桥。
- 正文保持纯文本，视觉状态不进入磁盘数据。

## 构建

见 [`qt/README.md`](qt/README.md)。旧 WinUI / Windows App SDK 工程与
`bindings/windows` 桥已移除。

全局架构见 [技术路线](../../docs/TECHNICAL_ROUTE.md)。
