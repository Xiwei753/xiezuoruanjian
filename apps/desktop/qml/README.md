# QML 界面组件层

本目录包含了 Linux 原生桌面端的所有 QML 页面与视觉组件。

## 页面与核心控制器

- **main.qml**：应用主窗口，控制各路由页的切换及全局初始化诊断。
- **WritingWorkspace.qml**：主写作页。
  - **核心重构**：左侧树型目录、中间编辑排版区、右侧抽屉式星图/设置，全部统一整合在唯一的顶层 `SplitView` 之下。去除了容易冲突的局部拖拽逻辑。
  - **阅读排版**：写作纸张（`paperBg`）居中，且宽度通过 `Math.min(parent.width, 820)` 进行 clamp，保证宽屏下的高级排版阅读体验。
- **EditorController.qml**：编辑器的主要逻辑控制器，将 `QTextDocument` 与 `DocumentHandler` 绑定。
- **SyncPage.qml**：同步设置与诊断页面。
  - 按钮在 clicked 时获取返回的 `operation_id` 并将其缓存为 `activeOperationId`。
  - 执行同步和运行诊断在同步中时会被互斥禁用。
  - 在 Connections 内解析 `sync_operation_state` JSON，校验其 `operation_id`，杜绝过期异步打印竞态覆盖。

## UI 组件规则

1. **绝对禁止裸写底层视觉基础**：如 `Text` 必须使用 `AppText`，以确保深浅色模式的动态切换。
2. **不允许在 QML 中硬编码数字 margin/padding**：必须全部统一使用 DesignToken，如 `dt.sp16`、`dt.sp20`、`dt.settingsControlHeight`。
