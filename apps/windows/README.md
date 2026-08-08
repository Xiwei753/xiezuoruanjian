# Windows 客户端

本目录是素笺写作的 Windows 原生客户端，独立于 Linux Qt 客户端。

## 职责

- 提供 Windows 界面、输入法、排版、渲染、动画和系统集成。
- 通过 Windows Bridge 调用 `writer_core`。
- 不复制作品、保存、设置、同步和统计规则。
- 正文保持纯文本，视觉状态不进入磁盘数据。

## 开发

使用 Visual Studio 和 Windows App SDK 打开并构建本目录中的项目。

Rust 动态库与跨语言调用由 `bindings/windows` 管理。跨语言边界只做类型转换、错误传递和资源释放，不承载业务逻辑。

全局架构见 [技术路线](../../docs/TECHNICAL_ROUTE.md)。
