# Apps 客户端应用层

本目录包含了不同操作系统平台上的写作客户端应用。

## 目录结构说明

- [linux/](file:///home/xiwei/xiezuoruanjian/apps/linux/)：Qt/QML Linux 桌面薄客户端。使用 Rust 进行底层 QObject 绑定，QML 负责高还原度的主题、写作排版渲染和交互。
- [android/](file:///home/xiwei/xiezuoruanjian/apps/android/)：Kotlin Android 移动薄客户端。使用 XML/View 渲染 Material 现代组件，通过 UniFFI 绑定与共享 Rust Core 交互。

## 核心设计契约：薄客户端原则 (Thin Client Principle)

1. **零业务计算**：客户端只负责 UI 展现、导航、主题、输入法交互和手势反馈。
2. **状态绑定而非管理**：客户端不应该私自维护工作区结构、计算同步优先级或实现任何业务分支。所有的动作都委托给 Rust Core，且数据读取只消费后端暴露的 view model。
3. **数据格式中立**：正文编辑区内容必须为纯文本。UI 上的富文本、首行缩进等均属于视觉呈现（由 QTextDocument 或系统渲染层动态处理），在保存时必须剥离 HTML 标签，保证物理存储始终为纯文本。
