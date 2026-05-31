# Linux/src 源码目录

本目录是 Linux Qt 客户端的原生 C++ 与 Rust 桥接和事件绑定模块。

## 目录结构与职责

- [backend/](file:///home/xiwei/xiezuoruanjian/apps/linux/src/backend/)：包含将底层 Rust Core 业务拆分为独立领域的 QObject 适配对象（如 WorkspaceBackend、SyncBackend 等），它们在主引擎运行前会被注册到 Qt Quick 的上下文属性中。
- `main.rs`：桌面程序的统一启动入口。负责初始化系统事件循环、实例化 QmlEngine，并在此处确保在加载 `main.qml` 之前将所有的 backend QObject 实例完整注册，防止 QML 运行阶段的 null 空指针崩溃。
- `document_handler.rs`：**唯一允许的 QTextDocument 排版操作模块**。它纯粹以物理无害的 QTextBlockFormat 处理字号、行距、以及首行缩进的视觉呈现，确保在修改显示样式时，物理落盘的正文内容永远是干净的纯文本。
- `sync_bridge.rs`：网络同步任务的多线程桥接模块，定义了携带 `operation_id` 保证线程并发隔离的 `SyncTaskOutcome` 传输包。
- `starmap_bridge.rs` / `starmap_bridge.rs`：星图与其它辅助特性的 Rust-Qt 桥接。
