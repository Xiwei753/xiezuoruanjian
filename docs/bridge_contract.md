# Bridge Contract

跨端调用必须遵守以下边界：

- Rust Core 是唯一业务事实来源，Android/Linux Bridge 只做类型转换和错误传播。
- UI/ViewModel/QML 不直接解析内部业务 JSON，不自行判断工作区、章节保存、写作事件分类或字数规则。
- 旧 `*_json` / `NativeCoreBridge` JSON 包装仍然存在，但只能作为 Android legacy fallback；Repository/UI/ViewModel/Controller 不允许直接依赖它。
- 新调用必须进入既有或新增领域 Bridge，不得把裸 JSON `String`、`Boolean`、`null` 当作正常上层接口继续扩散。
- Bridge 错误必须包含稳定 `code` 和可展示 `message`，不能只依赖字符串匹配。

## 领域 Bridge 架构

- **Android 三层架构**：`ViewModel/UI → Repository/Controller → 领域 Bridge (WorkspaceBridge, WritingBridge, SettingsBridge, SyncBridge, StatsBridge, StarMapBridge, MindMapBridge 等) → AppServiceBridge → UniFFI typed DTO/error → Rust Core`
- **Android legacy 例外**：`NativeStatusBridge` 和少量旧 `ActionBridge` 可通过 `NativeCoreBridge` 读取 native 加载状态或旧动作注册；该路径不是主业务入口。
- **Linux 三层架构**：`QML UI → AppBackend (QObject 适配层) → 领域 Bridge (writing_bridge 等) → Rust Core`
  - Linux `writing_bridge` 已从字符串错误改为稳定 Core Error 与 DTOs（如 `LinuxChapterOpenData`, `ChapterSaveReceipt`）。
  - `main.rs` 只是 QObject 适配层，仅做 QJsonObject 转换，不处理具体业务逻辑或控制流。
  - QML 只读取 QJsonObject 字段（`success`, `data`, `code` 等），不处理 JSON 字符串解析（`JSON.parse`）。
  - Legacy `JSON over JNI` 目前只在 Android legacy fallback 内保留，并通过 `BridgeProvider` 与主 UniFFI 领域 Bridge 隔离。
在关键业务路径（如保存章节），错误必须向上传递为明确的类型（如 Android 中的 `BridgeResult`，Linux 中的 `QJsonObject`），不允许退化成无上下文的 `bool`。

## 关键领域接口：

- Workspace：作品、卷、章节列表与创建、删除、重排序等。
- Writing：`openChapter`、`saveChapterContent`、`clearChapterContent`、`calculateWordCount`、`processWritingEvent`。
- Stats：项目统计和写作统计刷新/查询。
- StarMap：星图列表、创建、读取图、基础节点/边和布局操作。
- Settings：`getLocalSettings`、`saveLocalSettings`、`getSyncableSettings`、`saveSyncableSettings`。
- Sync：`loadSyncState`、`loadSyncConfig`、`saveSyncConfig`、`loadSyncSecrets`、`saveSyncSecrets`、`performSyncDiagnostics`、`performSyncDryRun`、`performSync`。
- NativeStatus：只暴露 native 加载状态、工作区路径、工作区校验、AI 可用性等最小状态；不得透传设置、同步、写作、星图业务方法。

## Android BridgeProvider 收口

- `BridgeProvider` 内部必须优先持有 `AppServiceBridge` 单例，领域 Bridge 默认依赖 `AppServiceBridge`。
- `BridgeProvider` 可以额外持有 `NativeCoreBridge` 单例，但只能给 legacy status/action 路径使用。
- `getNativeStatusBridge` 返回 `NativeStatusBridge`，不是 `NativeCoreBridge`。
- `NativeCoreBridge` 是 `internal` legacy adapter；新代码不得把它作为 Repository、Activity、ViewModel 或 Controller 的依赖。
- `NativeResult` 仅允许 legacy adapter 和领域 Bridge 内部使用；上层应处理 `BridgeResult<T>`。

## 章节保存语义：

- 普通保存必须走 Core 的验证保存，误传空字符串覆盖非空正文会返回 `EMPTY_OVERWRITE_BLOCKED` 错误码。
- 明确清空必须走专用 clear 接口，并返回保存回执 `ChapterSaveReceipt`。
- 无论成功或失败，调用方（如 `WorkspaceRepository` 或 `EditorController.qml`）均应能够提取错误码，并对特殊拦截做出反馈。
- 正文始终为纯文本，Bridge 不得引入 HTML 保存路径。
