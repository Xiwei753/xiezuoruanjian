# Linux Qt 客户端

本目录是素笺写作的 Linux 原生客户端，使用 Rust、Qt6 和 QML。

## 职责

- 提供 Linux 界面、输入法、排版、渲染、动画和系统集成。
- 通过 `writer_core` 使用工作区、作品、章节、设置、同步和统计能力。
- 不在 QML 或平台适配层复制业务规则和磁盘格式。
- Qt 对象只在所属线程访问，后台任务通过消息或 queued callback 返回结果。

## 环境

需要：

- Rust stable；
- Qt 6 开发环境；
- 支持 C++17 的编译器。

运行：

```bash
cargo run -p sujian-linux-qt
```

平台相关构建参数以 `Cargo.toml`、`build.rs` 和工作流为准，不在本文档维护发行版路径清单。

全局架构见 [技术路线](../../docs/TECHNICAL_ROUTE.md)。
