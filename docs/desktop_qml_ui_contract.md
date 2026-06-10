# Desktop QML UI 组件契约

本文档约束 Desktop/QML 页面和可复用组件的尺寸、布局和后端调用边界，避免设置页、同步页等弹窗反复出现控件重叠、indicator 裁切和递归布局问题。

## 后端调用边界

- QML 页面只调用 Desktop backend 暴露的 view model / command，不直接实现工作区、项目、章节、同步或设置业务分支。
- 新功能优先按领域使用后端边界：workspace、project、editor、settings、sync、starmap。旧的 `backend.xxx` 调用可作为兼容转发保留，但不应继续扩张。
- `*_json` 返回值 schema 由后端适配层维护，QML 不新增分散的错误包装逻辑。

## Qt Quick Controls 使用规则

- 首选 Qt Quick Controls 内置控件承载基础交互语义，例如 Button、Switch、Slider、ComboBox、TextField、ScrollView。
- 仅当现有控件无法满足 DesignTokens 视觉或尺寸契约时允许自定义控件。
- 自定义控件必须保留标准交互状态：enabled、hovered、pressed、focused、checked 或 currentIndex 等等。

## Layout 规则

- `RowLayout`、`ColumnLayout`、`GridLayout` 的直接子项禁止使用 `anchors.fill`、`anchors.left/right/top/bottom` 混合布局。
- Layout 子项必须使用 `Layout.*` 附加属性表达尺寸策略。
- 页面只能有一个主滚动根。弹窗内容使用一个 `ScrollView`，内部不要再嵌套第二个主滚动层。
- 禁止用 magic number 修错位。间距、圆角、控件高度必须来自 `DesignTokens`，例如 `dt.sp12`、`dt.sp16`、`dt.settingsControlHeight`。

## 可复用组件尺寸

- 所有 reusable component 必须提供稳定的 `implicitWidth` 和 `implicitHeight`。
- 组件内部可用 `contentItem`、`background` 和 `indicator`，但不能依赖父级固定高度才能完整显示。
- indicator、popup、handle 等视觉元素必须留出内边距，不能被组件默认 clip 裁切。

## SettingsRow 契约

- `SettingsRow` 负责一行标题、说明和控件的排版，不负责保存设置。
- 行高度至少为控件高度、标题说明高度和垂直 padding 的最大值。
- 右侧控件必须有明确 `Layout.preferredWidth` 或自身 `implicitWidth`。
- 窄屏时优先换行，不允许标题文字压住控件。

## AppSlider 契约

- `AppSlider` 的 `implicitHeight` 必须大于 handle 直径和上下 padding 总和。
- value label 不应覆盖 groove 或 handle。
- 页面只在用户提交或 `onMoved` 中写 backend，禁止在 binding 中频繁调用后端方法。

## AppComboBox 契约

- `AppComboBox` 的 `implicitHeight` 不低于 `dt.settingsControlHeight`，并确保 indicator 完整显示。
- popup 宽度至少等于 control 宽度。
- 文本区域必须预留 indicator 宽度，不能与箭头重叠。

## AppCard 契约

- `AppCard` 只负责容器视觉、padding 和边框，不在内部创建额外滚动根。
- 卡片内容高度由子项 implicit size 和 Layout 共同决定。

## 禁止事项

- 禁止在单个页面用硬编码 margin 逐个修错位。
- 禁止在 Layout 子项中混用 anchors 导致 `Qt Quick Layouts: Detected recursive rearrange`。
- 禁止通过 Timer 轮询后端状态来绕过状态边界。
- 禁止为了视觉缩进修改正文纯文本内容。
