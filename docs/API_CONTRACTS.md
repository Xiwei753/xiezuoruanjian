# API_CONTRACTS.md — 接口边界与交互契约

Status: active
Last verified: 2026-06-11
Truth source: code / product decision / protocol
Supersedes: docs/bridge_contract.md, docs/desktop_backend_contract.md, docs/desktop_qml_ui_contract.md

---

## 1. Bridge Contract (跨端桥接契约)

跨端调用必须遵守以下边界：

- Rust Core 是唯一业务事实来源，Android/Linux Bridge 只做类型转换和错误传播。
- 平台 Bridge 只能调用 Rust 稳定 Core API 暴露层（当前为 `api::WriterCoreApi` 经 `WriterAppService`/平台 adapter 暴露），不得绕回 Core 内部业务模块直接读写 workspace。
- UI/ViewModel/QML 不直接解析内部业务 JSON，不自行判断工作区、章节保存、写作事件分类或字数规则。
- 旧 `*_json` / `NativeCoreBridge` JSON 包装仍然存在，但只能作为 Android legacy fallback；Repository/UI/ViewModel/Controller 不允许直接依赖它。
- 新调用必须进入既有或新增领域 Bridge，不得把裸 JSON `String`、`Boolean`、`null` 当作正常上层接口继续扩散。
- Stats / StarMap 仍有 JSON 字符串残留时，解析必须封闭在对应领域 Bridge 内，向上返回 `BridgeResult<T>`；`Error` 和 `NotLoaded` 必须原样传播。
- Bridge 错误必须包含稳定 `code` 和可展示 `message`，不能只依赖字符串匹配。
- 写入、写作统计、保存和同步类 API 的 `Boolean` 只能表示业务成功值，不能表示错误状态；失败必须通过 typed error / `BridgeResult.Error` 向上传递。

### 1.1 领域 Bridge 架构

- **Android 三层架构**：`ViewModel/UI → Repository/Controller → 领域 Bridge (WorkspaceBridge, WritingBridge, SettingsBridge, SyncBridge, StatsBridge, StarMapBridge 等) → AppServiceBridge → UniFFI typed DTO/error → WriterAppService adapter → WriterCoreApi → Rust Core`
- **Android legacy 例外**：`NativeStatusBridge` 和少量旧 `ActionBridge` 可通过 `NativeCoreBridge` 读取 native 加载状态或旧动作注册；该路径不是主业务入口。
- **Linux 三层架构**：`QML UI → AppBackend (QObject 适配层) → 领域 Bridge (writing_bridge 等) → Rust Core`
  - Linux `writing_bridge` 已从字符串错误改为稳定 Core Error 与 DTOs（如 `LinuxChapterOpenData`, `ChapterSaveReceipt`）。
  - `main.rs` 只是 QObject 适配层，仅做 QJsonObject 转换，不处理具体业务逻辑或控制流。
  - QML 只读取 QJsonObject 字段（`success`, `data`, `code` 等），不处理 JSON 字符串解析（`JSON.parse`）。
  - Legacy `JSON over JNI` 目前只在 Android legacy fallback 内保留，并通过 `BridgeProvider` 与主 UniFFI 领域 Bridge 隔离。
- 在关键业务路径（如保存章节、写作统计事件记录），错误必须向上传递为明确的类型（如 Android 中的 `BridgeResult`，Linux 中的 `QJsonObject`），不允许退化成无上下文的 `bool`。

### 1.2 关键领域接口说明

- **Workspace**：作品、卷、章节列表与创建、删除、重排序等。
- **Writing**：`openChapter`、`saveChapterContent`、`clearChapterContent`、`calculateWordCount`、`processWritingEvent`。
- **Stats**：项目统计和写作统计刷新/查询；残留 JSON 只允许在 `StatsBridge` 内解析。
- **StarMap**：星图列表、快照、读取图、基础节点/边和布局操作；残留 JSON 只允许在 StarMapBridge 内解析。
- **MindMap (LEGACY)**：旧导图列表、快照、读取图操作；残留 JSON 只允许在 MindMapBridge 内解析。禁止新增功能。
- **Settings**：`getLocalSettings`、`saveLocalSettings`、`getSyncableSettings`、`saveSyncableSettings`。
- **Sync**：`loadSyncState`、`loadSyncConfig`、`saveSyncConfig`、`loadSyncSecrets`、`saveSyncSecrets`、`performSyncDiagnostics`、`performSyncDryRun`、`performSync`。
- **NativeStatus**：只暴露 native 加载状态、工作区路径、工作区校验、AI 可用性等最小状态；不得透传设置、同步、写作、星图业务方法。

### 1.3 Android BridgeProvider 收口

- `BridgeProvider` 内部必须优先持有 `AppServiceBridge` 单例，领域 Bridge 默认依赖 `AppServiceBridge`。
- `AppServiceBridge` 只消费 UniFFI 生成的 `WriterAppService` 稳定方法，不应绕过它直接绑定 Core 内部模块。
- `BridgeProvider` 可以额外持有 `NativeCoreBridge` 单例，但只能给 legacy status/action 路径使用。
- `getNativeStatusBridge` 返回 `NativeStatusBridge`，不是 `NativeCoreBridge`。
- `NativeCoreBridge` 是 `internal` legacy adapter；新代码不得把它作为 Repository、Activity、ViewModel 或 Controller 的依赖。
- `NativeResult` 仅允许 legacy adapter 和领域 Bridge 内部使用；上层应处理 `BridgeResult<T>`。

### 1.4 章节保存语义

- 普通保存必须走 Core 的验证保存，误传空字符串覆盖非空正文会返回 `EMPTY_OVERWRITE_BLOCKED` 错误码。
- 明确清空必须走专用 clear 接口，并返回保存回执 `ChapterSaveReceipt`。
- 无论成功或失败，调用方（如 `WorkspaceRepository` 或 `EditorController.qml`）均应能够提取错误码，并对特殊拦截做出反馈。
- 正文始终为纯文本，Bridge 不得引入 HTML 保存路径。

---

## 2. Desktop Backend Contract (桌面后端适配契约)

`AppBackend` 是 Desktop QML 层的共享状态入口，不是新的业务上帝类。Desktop 启动入口的 composition root 是 `BackendRuntime`：它负责创建、持有并注册暴露给 QML 的所有 QObject。

新页面必须按领域使用稳定 backend 名称：`workspaceBackend`、`projectBackend`、`editorBackend`、`settingsBackend`、`syncBackend`、`starmapBackend`。旧 `appBackend.xxx` / `backend.xxx` 调用只允许用于应用级调试、系统主题检测、全局状态兼容；不得把领域 API 重新挂回 `AppBackend`。

### 2.1 职责边界

- **WorkspaceBackend**：工作区恢复、打开、切换、关闭、GitHub 初始化、工作区路径与诊断。
- **ProjectBackend**：作品、卷、章节树结构的创建、删除、重命名、排序、选择和应用状态刷新。
- **EditorBackend**：章节打开、正文保存、编辑器状态、字数统计、写作统计和 Action 执行。
- **SettingsBackend**：字号、行距、主题、Monet 色、动画、自动保存、自动缩进、AI 开关和本地设置持久化。
- **SyncBackend**：同步配置、Token 状态、诊断、手动同步、自动同步触发、同步日志和同步状态。
- **StarMapBackend**：StarMap 列表、图加载、节点/边操作、布局保存，以及旧 mind map JSON 入口。

### 2.2 禁止事项

- 禁止把所有新方法重新堆回 `AppBackend`。
- 禁止在 QML 中新增复杂 JSON 解析或业务分支。
- 禁止为了 UI 迁移改变 JSON schema、同步协议或 Rust Core 同步算法。
- 禁止让子 backend 文件退化成只含注释的空壳。

### 2.3 当前兼容策略

- `main.rs` 初始化 Qt/QML、创建 `QmlEngine`，然后创建一个长期存活的 `BackendRuntime`。`BackendRuntime` 创建一个 `AppBackend` 共享状态对象，以及六个领域 backend QObject，并通过 `QmlEngine::set_object_property` 注册 `backend` / `appBackend` 以及六个领域 backend context property。
- 六个领域 backend 都是真实 `QObject`，在各自文件中声明 `qt_base_class!`、`qt_property!` 和 `qt_method!`，QML 直接访问领域对象。
- `AppBackend` 的 Qt 宏外观只保留应用级属性/方法与共享信号；领域业务方法继续作为 Rust 内部实现，供领域 QObject 委托调用。
- `apps/desktop/src/backend/app_backend/` 下旧 symlink 已删除；该目录只保留普通空 shim 文件，用于绕过 `rust-cpp` 解析器不识别 `#[path = "..."]` 的限制。真实模块仍通过 `#[path = "..."]` 指向 `apps/desktop/src/backend/*.rs` 同级领域文件，禁止恢复 symlink 或在 shim 中写业务逻辑。
- 设置弹窗和同步弹窗必须通过 `settingsBackend` / `syncBackend` 名称调用，不再新增全能 backend 路径。

### 2.4 QObject 生命周期规则

- 暴露给 QML 的 QObject 必须由 Rust 侧稳定 owner 持有，owner 生命周期必须覆盖 `engine.load_file(...)` 和整个 `engine.exec()` event loop。
- `QmlEngine::set_object_property` 只把 QObject 指针放入 QML root context；不得把函数局部临时 `QObjectBox` / `QObjectPinned` 注册给 QML 后让 owner 离开作用域。
- `BackendRuntime` 必须持有 `app_backend`、`workspace_backend`、`project_backend`、`editor_backend`、`settings_backend`、`sync_backend`、`starmap_backend` 的 `QObjectBox` owner，并且必须在 QML load 前完成注册，在 event loop 结束后才释放。
- 已注册给 QML 的 QObject 不得再移动到新的临时容器中，不得只保存裸指针或借用指针。
- QML 中可以保留启动期 null 诊断和极小安全 guard，但禁止用 `Timer`、大面积 fallback 或吞掉 TypeError 的方式掩盖 Rust 侧生命周期错误。
- context property 是当前 qmetaobject 过渡方案。新能力不要无限增加 root context property；后续如条件允许，应优先评估正式 QML module / singleton 路线。

---

## 3. Desktop QML UI Component Contract (桌面 QML UI 组件契约)

本文档约束 Desktop/QML 页面和可复用组件的尺寸、布局和后端调用边界，避免设置页、同步页等弹窗反复出现控件重叠、indicator 裁切和递归布局问题。

### 3.1 后端调用边界

- QML 页面只调用 Desktop backend 暴露的 view model / command，不直接实现工作区、项目、章节、同步或设置业务分支。
- 新功能优先按领域使用后端边界：workspace、project、editor、settings、sync、starmap。旧的 `backend.xxx` 调用可作为兼容转发保留，但不应继续扩张。
- `*_json` 返回值 schema 由后端适配层维护，QML 不新增分散的错误包装逻辑。

### 3.2 Qt Quick Controls 使用规则

- 首选 Qt Quick Controls 内置控件承载基础交互语义，例如 Button、Switch、Slider、ComboBox、TextField、ScrollView。
- 仅当现有控件无法满足 DesignTokens 视觉或尺寸契约时允许自定义控件。
- 自定义控件必须保留标准交互状态：enabled、hovered、pressed、focused、checked 或 currentIndex 等等。

### 3.3 Layout 规则

- `RowLayout`、`ColumnLayout`、`GridLayout` 的直接子项禁止使用 `anchors.fill`、`anchors.left/right/top/bottom` 混合布局。
- Layout 子项必须使用 `Layout.*` 附加属性表达尺寸策略。
- 页面只能有一个主滚动根。弹窗内容使用一个 `ScrollView`，内部不要再嵌套第二个主滚动层。
- 禁止用 magic number 修错位。间距、圆角、控件高度必须来自 `DesignTokens`，例如 `dt.sp12`、`dt.sp16`、`dt.settingsControlHeight`。

### 3.4 可复用组件尺寸

- 所有 reusable component 必须提供稳定的 `implicitWidth` 和 `implicitHeight`。
- 组件内部可用 `contentItem`、`background` 和 `indicator`，但不能依赖父级固定高度才能完整显示。
- indicator、popup、handle 等视觉元素必须留出内边距，不能被组件默认 clip 裁切。

### 3.5 SettingsRow 契约

- `SettingsRow` 负责一行标题、说明和控件的排版，不负责保存设置。
- 行高度至少为控件高度、标题说明高度和垂直 padding 的最大值。
- 右侧控件必须有明确 `Layout.preferredWidth` 或自身 `implicitWidth`。
- 窄屏时优先换行，不允许标题文字压住控件。

### 3.6 AppSlider 契约

- `AppSlider` 的 `implicitHeight` 必须大于 handle 直径和上下 padding 总和。
- value label 不应覆盖 groove 或 handle。
- 页面只在用户提交或 `onMoved` 中写 backend，禁止在 binding 中频繁调用后端方法。

### 3.7 AppComboBox 契约

- `AppComboBox` 的 `implicitHeight` 不低于 `dt.settingsControlHeight`，并确保 indicator 完整显示。
- popup 宽度至少等于 control 宽度。
- 文本区域必须预留 indicator 宽度，不能与箭头重叠。

### 3.8 AppCard 契约

- `AppCard` 只负责容器视觉、padding 和边框，不在内部创建额外滚动根。
- 卡片内容高度由子项 implicit size 和 Layout 共同决定。

### 3.9 禁止事项

- 禁止在单个页面用硬编码 margin 逐个修错位。
- 禁止在 Layout 子项中混用 anchors 导致 `Qt Quick Layouts: Detected recursive rearrange`。
- 禁止通过 Timer 轮询后端状态来绕过状态边界。
- 禁止为了视觉缩进修改正文纯文本内容。

---

## 4. Capability API Boundaries (核心能力 API 边界)

以下为 Rust Core 定义的统一 Capability API，多端绑定层在对接 Rust Core 时必须对照此接口清单进行封装：

### 4.1 WorkspaceCapability
- `createWorkspace(path: String, name: String) -> ResultEnvelope`
- `openWorkspace(path: String) -> ResultEnvelope`
- `validateWorkspace(path: String) -> ResultEnvelope` (工作区结构与合法性校验)
- `getWorkspaceState() -> WorkspaceState` (锁状态、同步配置、最近使用等)

### 4.2 ProjectCapability
- `createProject(workspacePath: String, name: String) -> ResultEnvelope`
- `renameProject(workspacePath: String, projectId: String, newName: String) -> ResultEnvelope`
- `deleteProject(workspacePath: String, projectId: String) -> ResultEnvelope`
- `listProjects(workspacePath: String) -> ResultEnvelope<List<Project>>`
- `getProjectTree(workspacePath: String, projectId: String) -> ResultEnvelope<ProjectTree>`

### 4.3 ChapterCapability
- `createVolume(workspacePath: String, projectId: String, volumeName: String) -> ResultEnvelope`
- `createChapter(workspacePath: String, projectId: String, volumeId: String, chapterName: String) -> ResultEnvelope`
- `renameChapter(workspacePath: String, projectId: String, chapterId: String, newName: String) -> ResultEnvelope`
- `deleteChapter(workspacePath: String, projectId: String, chapterId: String) -> ResultEnvelope`
- `loadChapter(workspacePath: String, projectId: String, chapterId: String) -> ResultEnvelope<ChapterData>`
- `saveChapter(workspacePath: String, projectId: String, chapterId: String, content: String) -> ResultEnvelope`
- `getStats(workspacePath: String, projectId: String, chapterId: String) -> ResultEnvelope<ChapterStats>`

### 4.4 SettingsCapability
- `getLocalSettings(workspacePath: String) -> ResultEnvelope<LocalSettings>`
- `saveLocalSettings(workspacePath: String, settings: LocalSettings) -> ResultEnvelope`
- `getSyncableSettings(workspacePath: String) -> ResultEnvelope<SyncableSettings>`
- `saveSyncableSettings(workspacePath: String, settings: SyncableSettings) -> ResultEnvelope`
- `getEffectiveSettings(workspacePath: String) -> ResultEnvelope<EffectiveSettings>` (合并本地与同步配置的最终计算设置)
- **触发事件**：在写入成功时强制发出 `SettingsSaved` 广播事件。

### 4.5 SyncCapability
- `loadSyncConfig(workspacePath: String) -> ResultEnvelope<SyncConfig>`
- `saveSyncConfig(workspacePath: String, config: SyncConfig) -> ResultEnvelope`
- `dryRun(workspacePath: String) -> ResultEnvelope<SyncReport>` (演练，返回受影响文件)
- `diagnostics(workspacePath: String) -> ResultEnvelope<SyncDiagnostics>` (诊断本地库状态)
- `sync(workspacePath: String) -> ResultEnvelope<SyncResult>` (执行推送与拉取)
- **映射规则**：底层调用如 `libgit2` 产生的错误，必须在 Core 中全部映射为 `SyncStatus`，严禁直接把 libgit2 裸字符串传递给平台层展示。
- **冲突处理**：当发生 Merge 冲突时，冲突判定、备选方案选择（保留本地/覆盖云端/人工合并）必须由 Core 定义的冲突处理算法进行，平台只提供选择 UI。

### 4.6 MindMapCapability (REMOVED)

> MindMap API 已从运行时移除。旧数据迁移请使用 `starmap::legacy_migration` 模块。
> 正式图谱路线为 `starmap`（星图）。所有图谱能力必须走 StarMapCapability。

- `createGraph(workspacePath: String, projectId: String, graphName: String) -> ResultEnvelope<GraphId>`
- `listGraphs(workspacePath: String, projectId: String) -> ResultEnvelope<List<GraphMeta>>`
- `setDefaultGraph(workspacePath: String, projectId: String, graphId: String) -> ResultEnvelope`
- `createNode(workspacePath: String, projectId: String, graphId: String, parentId: String, nodeText: String) -> ResultEnvelope<NodeId>`
- `updateNode(workspacePath: String, projectId: String, graphId: String, nodeId: String, newText: String) -> ResultEnvelope`
- `deleteNode(workspacePath: String, projectId: String, graphId: String, nodeId: String) -> ResultEnvelope`
- `createEdge(workspacePath: String, projectId: String, graphId: String, sourceId: String, targetId: String) -> ResultEnvelope<EdgeId>`
- `updateEdge(workspacePath: String, projectId: String, graphId: String, edgeId: String, properties: EdgeProperties) -> ResultEnvelope`
- `deleteEdge(workspacePath: String, projectId: String, graphId: String, edgeId: String) -> ResultEnvelope`
- `createAnchor(workspacePath: String, projectId: String, graphId: String, nodeId: String, chapterId: String, anchorOffset: Int, anchorText: String) -> ResultEnvelope<AnchorId>`
- `resolveAnchor(workspacePath: String, projectId: String, graphId: String, anchorId: String) -> ResultEnvelope<ResolvedAnchor>`
- `bindAnchor(workspacePath: String, projectId: String, graphId: String, anchorId: String, nodeId: String) -> ResultEnvelope`
- `saveLayout(workspacePath: String, projectId: String, graphId: String, layoutData: LayoutData) -> ResultEnvelope`
- `getSnapshot(workspacePath: String, projectId: String, graphId: String) -> ResultEnvelope<MindMapSnapshot>` (只读渲染视图快照)

### 4.6b StarMapCapability (正式图谱路线)
- StarMap 是唯一推荐的图谱能力入口，详见 `starmap` 模块和 `starmap_semantics.md`。
- 提供星图元数据管理、图数据 CRUD、布局持久化、viewport 状态、embed/link 语义边、连线渲染和命中测试等能力。

### 4.7 EditorModelCapability
- `loadChapterText(workspacePath: String, projectId: String, chapterId: String) -> ResultEnvelope<EditorTextState>`
- `saveChapterText(workspacePath: String, projectId: String, chapterId: String, transaction: TextTransaction) -> ResultEnvelope`
- `computeWordStats(text: String) -> WordStats`
- `trackSessionStats(workspacePath: String, charsAdded: Int, durationSeconds: Int) -> ResultEnvelope`
- **排版控制**：自动缩进（autoIndent）等格式化行为必须读取 Core 中 Settings 对应的配置，平台端排版和编辑器渲染器只消费状态。

### 4.8 未来 AI Capability (AI 业务功能)
- **安全隔离**：AI 辅助功能（如扩写、续写、提示节点）绝对不允许直接操纵或写入平台侧的局部状态。
- **动作化**：AI 生成的内容在被用户采纳前，只作为 `Core Action Proposal` 传输给平台；用户点击接受后，必须作为 Core Command 写入 workspace 历史中。
- **图谱输入**：AI 自动抽取的节点、大纲、逻辑锚点，必须走 `StarMapCapability` 提供的标准接口导入。
