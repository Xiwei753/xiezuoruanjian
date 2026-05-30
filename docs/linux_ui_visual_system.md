# Linux UI 视觉系统

Linux 客户端采用 Qt6/QML 实现，视觉底座对齐 Android Material 3。`apps/linux/qml/DesignTokens.qml` 是 Linux UI 的唯一设计令牌来源，页面和组件应优先绑定令牌，不在业务页面中重复定义颜色、圆角、字号和状态样式。

## 色彩

基础 Material 3 色槽与 Android `colors.xml` 保持一致：

- Light：`primary #006497`、`primaryContainer #CCE5FF`、`secondary #51606F`、`background/surface #FCFCFF`、`surfaceVariant #DFE3EB`
- Dark：`primary #92CCFF`、`primaryContainer #004B73`、`background/surface #1A1C1E`、`surfaceVariant #42474E`
- 语义状态统一使用 `successContainer`、`warningContainer`、`dangerContainer`、`infoContainer` 和对应 `on*Container`

页面背景使用 `background/bg`，卡片使用 `card/surfaceContainerLow`，悬停使用 `surfaceContainer` 或 `surfaceVariant`，选中状态使用 `primaryContainer/onPrimaryContainer`。

## 尺寸

圆角统一使用令牌：`radiusMd` 用于输入框和按钮，`radiusLg` 用于卡片，`radiusXl` 用于弹窗和面板，`radiusPill` 用于导航项、工具栏按钮和状态胶囊。

间距统一使用 `sp4` 到 `sp64`。页面边距使用 `pageMarginWide/pageMarginNarrow`，不要在页面中新增魔法 margin。

## 字体

UI 字体使用 `fontFamily`、`display/title/subtitle/body/label/caption`。写作正文不使用全局 UI 字号，仍由 `backend.setting_font_size` 和编辑器设置控制，避免视觉系统污染正文排版。

## 组件

新增或改造页面时优先使用共用组件：

- `AppButton`：支持 `primary`、`secondary`、`text`、`danger`
- `AppTextField`：统一输入框、标签、焦点边框和回车信号
- `AppSwitch` / `ModernSwitch`：统一开关状态，不覆盖 Qt 基类 `enabled`
- `AppSlider`：统一滑块轨道和手柄
- `AppCard`：统一卡片背景、边框、圆角和 padding
- `StatusPill`：统一成功、警告、错误、信息状态展示
- `SidebarItem` / `ToolbarButton`：统一胶囊形导航和工具栏交互

QML 页面只做展示和状态绑定，不新增 Core/API/同步/StarMap 语义分支。
