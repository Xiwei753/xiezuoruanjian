# API_CONTRACTS.md - 接口边界与交互契约

Status: active
Last verified: 2026-06-19
Truth source: code / product decision / protocol
Supersedes:

---

## 1. Bridge Contract（桥接层契约）

桥接层核心原则：

- Rust Core 是唯一业务真相来源，Android/Linux Bridge 只做薄层转发
- 所有 Bridge 必须通过 Rust 侧 Core API 层调用（即 `api::WriterCoreApi` 或 `WriterAppService`/薄 adapter 层），不得绕过 Core 自行管理 workspace
- UI/ViewModel/QML 不直接解析 JSON，所有业务数据必须通过 Bridge 转换为强类型
- 对 `*_json` / `NativeCoreBridge` JSON 接口，仅作为 Android legacy fallback；Repository/UI/ViewModel/Controller 不得直接使用
- 所有跨平台共享的 Bridge，统一使用 JSON `String`、`Boolean`、`null` 三种基本类型传输
- Stats / StarMap 暂用 JSON 字符串传输，但必须封闭在领域 Bridge 内，后续迁移到 `BridgeResult<T>`；`Error` 和 `NotLoaded` 必须区分
- Bridge 错误必须包含 `code` 和 `message`，不允许吞错误
- 所有新增 API 不允许 `Boolean` 返回值表示成功失败，必须返回结构化结果；旧接口逐步迁移到 typed error / `BridgeResult.Error` 处理

### 1.1 各端 Bridge 架构

- **Android 主链路**：`ViewModel/UI → Repository/Controller → 领域 Bridge (WorkspaceBridge, WritingBridge, SettingsBridge, SyncBridge, StatsBridge, StarMapBridge 等) → AppServiceBridge → UniFFI typed DTO/error → WriterAppService adapter → WriterCoreApi → Rust Core`
- **Android legacy 链路**：`NativeStatusBridge` 和 `ActionBridge` 通过 `NativeCoreBridge` 调用 native 方法；仅作为 fallback，不得扩展
- **Linux 主链路**：`QML UI → AppBackend (QObject 暴露) → 领域 Bridge (writing_bridge 等) → Rust Core`
  - Linux `writing_bridge` 直接返回 Core Error 和 DTOs（如 `LinuxChapterOpenData`, `ChapterSaveReceipt`）。
  - `main.rs` 中 QObject 暴露的方法，通过 QJsonObject 传递，不做额外封装
  - QML 侧消费 QJsonObject（`success`, `data`, `code` 等），不直接做 JSON 字符串解析（`JSON.parse`）
  - Legacy `JSON over JNI` 仅作为 Android legacy fallback，不作为 Linux 主链路；Linux `BridgeProvider` 由 UniFFI 或 Bridge 层统一
- **跨平台一致性**（不要求接口签名完全一致，但语义必须一致），各端 Bridge 返回值类型可以不同（如 Android 用 `BridgeResult`，Linux 用 `QJsonObject`），但错误处理不得返回裸 `bool`

### 1.2 领域 Bridge 列表

- **Workspace**：工作区创建、打开、验证、状态查询
- **Writing**：`openChapter`、`saveChapterContent`、`clearChapterContent`、`calculateWordCount`、`processWritingEvent`
- **Stats**：统计查询和聚合；暂用 JSON 字符串，`StatsBridge` 内部转换
- **StarMap**：星图查询和快照；暂用 JSON 字符串，StarMapBridge 内部转换
- **MindMap (LEGACY)**：旧图谱查询和快照；暂用 JSON 字符串，MindMapBridge 内部转换，后续删除
- **Settings**：`getLocalSettings`、`saveLocalSettings`、`getSyncableSettings`、`saveSyncableSettings`
- **Sync**：`loadSyncState`、`loadSyncConfig`、`saveSyncConfig`、`loadSyncSecrets`、`saveSyncSecrets`、`performSyncDiagnostics`、`performSyncDryRun`、`performSync`
- **NativeStatus**：仅用于 native 加载状态和少量 AI 相关状态；不得作为业务主链路，后续逐步迁移

### 1.3 Android BridgeProvider 架构

- `BridgeProvider` 负责创建和管理 `AppServiceBridge` 实例，所有 Bridge 通过 `AppServiceBridge` 访问
- `AppServiceBridge` 通过 UniFFI 绑定到 `WriterAppService` 实例，后者委托 Core 业务逻辑
- `BridgeProvider` 同时管理 `NativeCoreBridge` 实例，仅用于 legacy status/action 查询
- `getNativeStatusBridge` 返回 `NativeStatusBridge`，后者委托 `NativeCoreBridge`
- `NativeCoreBridge` 是 `internal` legacy adapter；不得暴露给 Repository、Activity、ViewModel 或 Controller 使用
- `NativeResult` 是 legacy adapter 内部 Bridge 返回值；新代码必须使用 `BridgeResult<T>`

### 1.4 保存契约

- 保存操作必须通过 Core 事务层，不得绕过 Core 直接写文件
- 空内容覆盖必须返回 `EMPTY_OVERWRITE_BLOCKED` 错误码
- 清空章节内容必须走 clear 接口，不得通过保存空字符串绕过，返回 `ChapterSaveReceipt`
- 保存失败时，调用方（如 `WorkspaceRepository` 或 `EditorController.qml`）必须显示错误提示，不得静默忽略
- 保存成功后，Bridge 返回结构化收据，不得返回 HTML 内容

---

## 2. Desktop Backend Contract（桌面后端契约）

`AppBackend` 是 Desktop QML 端的唯一后端入口，负责所有业务调用。Desktop 采用 composition root 模式 `BackendRuntime`：各领域 backend 作为独立 QObject 暴露给 QML。

各领域 backend 实例：`workspaceBackend`、`projectBackend`、`editorBackend`、`settingsBackend`、`syncBackend`、`starmapBackend`。QML 通过 `appBackend.xxx` / `backend.xxx` 访问对应领域方法；不得直接调用非公开 API；新增 API 必须挂载到对应 `AppBackend`。

### 2.1 领域 Backend

- **WorkspaceBackend**：工作区创建、打开、验证、状态查询、GitHub 仓库管理
- **ProjectBackend**：项目创建、删除、重命名、列表查询、项目树查询
- **EditorBackend**：章节打开、保存、清空、字数统计、格式化、编辑器 Action 管理
- **SettingsBackend**：设置读写、Monet 取色、AI 设置、同步设置
- **SyncBackend**：同步 Token 管理、同步配置、同步执行、诊断报告
- **StarMapBackend**：StarMap 图谱查询和快照、节点管理，不返回 mind map JSON 字符串

### 2.2 调用规则

- 所有业务调用必须通过对应领域的 `AppBackend`
- 不得在 QML 中直接解析 JSON 字符串
- 不得在 UI 层定义 JSON schema，所有 schema 由 Rust Core 定义
- 不得绕过 backend 直接调用 Rust Core

### 2.3 生命周期管理

- `main.rs` 创建 Qt/QML 的 `QmlEngine`，在引擎初始化时创建 `BackendRuntime`；`BackendRuntime` 持有 `AppBackend` 及各领域 backend 实例，通过 `QmlEngine::set_object_property` 设置为 `backend` / `appBackend` 等上下文属性
- 各领域 backend 必须是 `QObject`，通过 `qt_base_class!`、`qt_property!` 和 `qt_method!` 暴露，QML 才能访问
- `AppBackend` 的 Qt 生命周期由 Rust 侧 owner 管理；owner 必须存活到 `engine.load_file(...)` 和 `engine.exec()` event loop 结束
- `QmlEngine::set_object_property` 将 QObject 设置到 QML root context；不转移所有权，需要 `QObjectBox` / `QObjectPinned` 防止 QML 提前释放 owner
- `BackendRuntime` 持有 `app_backend`、`workspace_backend`、`project_backend`、`editor_backend`、`settings_backend`、`sync_backend`、`starmap_backend` 的 `QObjectBox` owner，确保 QML load 过程中，到 event loop 结束前不被释放
- 任何 QML 侧 QObject 引用必须在 Rust 侧有 owner，否则会导致悬垂指针
- QML 侧访问可能为 null 的 backend 时，必须做 null guard，可以用 `Timer` 或 fallback 避免 TypeError 导致 Rust 侧崩溃
- context property 通过 qmetaobject 设置到 root context property；如果需要模块化，可以考虑 QML module / singleton

---

## 3. Desktop QML UI Component Contract（桌面 QML UI 组件契约）

本节定义 Desktop/QML 端 UI 组件的设计规范和交互契约，确保 UI 一致性和可维护性。indicator 等可复用组件必须遵循以下规范。

### 3.1 数据绑定规则

- QML 组件只绑定 Desktop backend 暴露的 view model / command，不得在组件内部维护业务数据
- 所有业务数据必须通过领域 backend 获取：workspace、project、editor、settings、sync、starmap，通过 `backend.xxx` 访问，不得绕过
- `*_json` 返回值的 schema 由 Core 定义，QML 不得自行解析或假设字段

### 3.2 Qt Quick Controls 规范

- 优先使用 Qt Quick Controls 原生组件，如 Button、Switch、Slider、ComboBox、TextField、ScrollView
- 所有样式必须通过 DesignTokens 统一管理，不得硬编码颜色和尺寸
- 组件状态必须通过标准属性管理：enabled、hovered、pressed、focused、checked 和 currentIndex 等

### 3.3 Layout 规范

- `RowLayout`、`ColumnLayout`、`GridLayout` 优先使用，避免 `anchors.fill`、`anchors.left/right/top/bottom` 混用
- Layout 子项必须使用 `Layout.*` 附加属性设置尺寸
- 滚动区域必须使用 `ScrollView`，不得嵌套多个滚动层
- 禁止 magic number，所有间距和尺寸必须使用 `DesignTokens`，如 `dt.sp12`、`dt.sp16`、`dt.settingsControlHeight`

### 3.4 可复用组件规范

- 所有 reusable component 必须设置 `implicitWidth` 和 `implicitHeight`
- 组件内部必须通过 `contentItem`、`background` 和 `indicator` 自定义外观，不得覆盖外部样式
- indicator、popup、handle 等辅助元素必须正确 clip，不得溢出

### 3.5 SettingsRow 规范

- `SettingsRow` 是设置页面的基础行组件，所有设置项必须使用
- 行内元素必须通过 padding 对齐，不得硬编码 margin
- 行宽必须通过 `Layout.preferredWidth` 或 `implicitWidth` 设置
- 行内不得嵌套其他布局容器，避免高度计算错误

### 3.6 AppSlider 规范

- `AppSlider` 的 `implicitHeight` 必须包含 handle 和 padding 空间
- value label 必须定位在 groove 和 handle 之上
- 值变更必须通过 `onMoved` 通知 backend，不得使用 binding 循环

### 3.7 AppComboBox 规范

- `AppComboBox` 的 `implicitHeight` 必须等于 `dt.settingsControlHeight`，包含 indicator 空间
- popup 宽度必须等于 control 宽度
- 选中项变更必须通过 indicator 反馈，不得使用 binding 循环

### 3.8 AppCard 规范

- `AppCard` 必须通过内部 padding 控制内容间距，不得硬编码 margin
- 卡片尺寸必须通过 implicit size 和 Layout 属性设置

### 3.9 禁止事项

- 禁止在 QML 中硬编码 margin 和间距
- 禁止在 Layout 内使用 anchors，避免 `Qt Quick Layouts: Detected recursive rearrange`
- 禁止用 Timer 模拟动画，必须使用 Qt Animation
- 禁止在 QML 中实现业务逻辑

---

## 4. Capability API Boundaries（能力 API 边界）

以下定义 Rust Core 各能力模块的 Capability API，所有跨平台业务调用必须通过 Rust Core 的 Capability API：

### 4.1 WorkspaceCapability
- `createWorkspace(path: String, name: String) -> ResultEnvelope`
- `openWorkspace(path: String) -> ResultEnvelope`
- `validateWorkspace(path: String) -> ResultEnvelope` (仅验证工作区格式)
- `getWorkspaceState() -> WorkspaceState` (返回当前工作区状态和元信息)

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
- `getEffectiveSettings(workspacePath: String) -> ResultEnvelope<EffectiveSettings>` (合并本地和同步设置)
- **注意**：设置保存后必须触发 `SettingsSaved` 事件通知

### 4.5 SyncCapability
- `loadSyncConfig(workspacePath: String) -> ResultEnvelope<SyncConfig>`
- `saveSyncConfig(workspacePath: String, config: SyncConfig) -> ResultEnvelope`
- `dryRun(workspacePath: String) -> ResultEnvelope<SyncReport>` (模拟同步，不写磁盘)
- `diagnostics(workspacePath: String) -> ResultEnvelope<SyncDiagnostics>` (诊断同步状态)
- `sync(workspacePath: String) -> ResultEnvelope<SyncResult>` (执行同步，写磁盘)
- **注意**：同步底层使用 `libgit2` 实现，Core 不直接暴露 `SyncStatus`，因为 libgit2 可能产生不可恢复错误
- **注意**：合并冲突时，Core 返回冲突详情（文件级/段落级/行级），UI 负责展示和解决

### 4.6 MindMapCapability (REMOVED)

> MindMap API 已废弃，所有图谱功能迁移到 `starmap::legacy_migration` 模块
> 新功能必须使用 `starmap`（星图），不得使用 MindMapCapability

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
- `getSnapshot(workspacePath: String, projectId: String, graphId: String) -> ResultEnvelope<MindMapSnapshot>` (仅用于迁移)

### 4.6b StarMapCapability（星图能力，正式路线）
- StarMap 是正式图谱能力，详细语义见 `starmap` 和 `starmap_semantics.md`
- 支持节点和边的 CRUD，以及 viewport 查询和 embed/link 等跨图谱引用能力

### 4.7 EditorModelCapability
- `loadChapterText(workspacePath: String, projectId: String, chapterId: String) -> ResultEnvelope<EditorTextState>`
- `saveChapterText(workspacePath: String, projectId: String, chapterId: String, transaction: TextTransaction) -> ResultEnvelope`
- `computeWordStats(text: String) -> WordStats`
- `trackSessionStats(workspacePath: String, charsAdded: Int, durationSeconds: Int) -> ResultEnvelope`
- **注意**：自动缩进等编辑行为由 Core 的 Settings 驱动，不得在平台端硬编码

### 4.8 预留 AI Capability（AI 能力预留）
- **注意**：AI 能力（包括对话和操作建议）暂不纳入 Capability API，后续独立设计
- **设计原则**：AI 返回结构化操作建议，通过 `Core Action Proposal` 统一执行；不得绕过 Core Command 直接操作 workspace
- **扩展方向**：AI 操作建议可以与 `StarMapCapability` 联动，实现图谱自动扩展

---

## 5. EditorInteractionContract（编辑器交互契约）

EditorInteractionContract 属于平台编辑器交互层，不是 workspace 业务能力。

统一命令 ID：
- `copy`
- `cut`
- `paste`
- `paste_plain_text`
- `select_all`
- `delete_selection`
- `share`
- `format_text`
- `link_to_starmap`
- `ai_assist`

平台必须根据 `EditorSelectionSnapshot` 判断菜单项 enabled/visible。

会修改正文的命令必须生成 `EditorTransaction`。
不会修改正文的命令不得触发保存状态。

Core 只管最终正文事务，不管"菜单怎么弹"。
