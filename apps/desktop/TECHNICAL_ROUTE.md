# Linux 技术路线与实现边界

**本目录最高优先级规则**
- 本文档是 apps/desktop 目录的技术路线约束。
- 后续任何修改本目录的提示词、AI 任务、人工 PR，必须先读取本文档。
- 如果提示词和本文档冲突，以本文档为准。
- 如果确实需要改变路线，必须先提交本文档变更。

## 当前事实
- Linux 当前路线是 Qt/QML。
- 遵循 Qt/KDE 桌面应用路线。
- **遵守跨平台契约：** 所有的业务适配与状态展示必须无条件符合 [《跨平台能力契约与 Core-first 架构约束》](../../docs/CROSS_PLATFORM_CAPABILITY_CONTRACT.md)。
- **纯适配器层：** Linux backend / `qmetaobject` 只能作为调用 Core Capability API 的适配器（Adapter）。**只负责状态和指令的转发，绝不能独立实现任何与 Android 侧分叉的业务语义或状态机。**
- QML 负责展示和交互，不负责业务真相。
- Rust/Linux backend 负责调用 Core、组织 AppState、同步状态。

## Linux UI 路线
- 使用 Qt Quick Controls / QML。
- 主题跟随 Qt/KDE/SystemPalette。
- KDE 下优先 org.kde.desktop / qqc2-desktop-style 思路。
- 非 KDE 可使用 Qt Quick Controls 默认/Fusion/SystemPalette。
- 不手搓整套深色主题。
- 不用硬编码颜色覆盖所有控件。
- 不在 QML 里写业务状态机。
- 写作编辑器最终路线是 `SujianEditorItem` 自绘，平台只负责输入适配、`QTextLayout` 布局、绘制、滚动和命中测试。
- 在 `SujianEditorItem` 落地前，Linux 文字吐字动画关闭；不得恢复通过 `QTextDocument` 字符格式隐藏真实文字的方案。

## Linux 禁止事项
- 不随意切 Qt5 / Qt6 / GTK / Flutter / Electron / WebView。
- 不用 QML Timer 轮询同步结果。
- 不在 QML 里做重 IO。
- 不在 QML 里拼业务树、猜路径、猜 ID。
- 不在 QML 里吞 backend/core 错误。
- 不用临时本地文件绕过 Core。
- 不在 QML `TextArea` 上继续堆文字动画 diff、overlay 和 hidden-range 补丁。

## Linux 状态路线
- backend 维护 AppState / WorkspaceTreeState / SyncState。
- QML 只绑定状态和触发命令。
- 同步用后端异步任务和 Qt 主线程回调/信号。
- 成功、失败、冲突、无关历史等状态都必须闭环。

## Linux 路线变更规则
- 若要换 Qt 版本或 UI 栈，必须先改本文档并说明原因。
- 若要引入新渲染技术，必须说明和 Qt/QML 的边界。
- 不允许直接在功能 PR 里偷换栈。
