# Core API

`writer_core` Rust 库的跨平台暴露边界已经拆成四层：

- Core domain：`workspace`、`project`、`volume`、`chapter`、`settings`、`sync_service`、`mind_map`、`starmap`、`writing_stats` 等模块负责真实业务和文件 I/O。
- Facade：`facade::WriterCore` 聚合内部业务能力，是 **Core 内部协调层**；它不承诺作为平台稳定 DTO 边界，也不应该被外部直接调用。
- Core API：`api::WriterCoreApi`、`api::types`、`api::error`、`api::envelope` 是**平台稳定入口**，面向 UniFFI、Linux binding、Android JNI 和未来前端。Android JNI 只能调用 `WriterCoreApi`。
- UniFFI adapter：`app_service::WriterAppService` 只保留 Android 兼容的对象名和方法签名，负责接收 UniFFI 参数并委托 `WriterCoreApi`。

> 注意：`sync_service` 的解耦拆分是后续阶段的任务，不在本次重构范围内。

`api.udl` 只是 UniFFI 绑定声明，用于生成 Kotlin/外部语言桥接代码；业务 API 的事实来源是 Rust `api/` 模块及其文档边界，不是 UDL 文件本身。图谱写入类 UniFFI 方法必须接收 DTO，不允许在 `WriterAppService` 或 `WriterCoreApi` 接收 JSON 字符串后再解析。

## Core API 模块

- `core/writer_core/src/api/types.rs`：平台稳定 DTO，如 `ProjectDto`、`VolumeDto`、`ChapterMetaDto`、`ChapterContentDto`、`ChapterSaveReceiptDto`、`RecentEditDto`、`LocalSettingsDto`、`SyncConfigDto`、`SyncStateDto`、`SyncResultDto` 等。
- `core/writer_core/src/api/error.rs`：平台稳定错误 `WriterError`，集中从 `crate::error::Error` 映射，保留 `Io`、`Json`、`InvalidWorkspace`、`ProjectNotFound`、`VolumeNotFound`、`ChapterNotFound`、`EmptyOverwriteBlocked`、`NotImplemented`、`RefuseToDeleteWorkspaceRoot`、`InvalidDeleteTarget`、`Other` 语义。
- `core/writer_core/src/api/envelope.rs`：跨平台标准 `ResultEnvelope<T>`，统一序列化 `success`、`data`、`errorCode`、`userMessage`、`rawError`、`warnings`、`changedPaths`、`changedEntities`。平台端只能根据 `success` / `errorCode` / `userMessage` 分支，不能解析 `rawError` 猜错误。
- `core/writer_core/src/api/service.rs`：`WriterCoreApi` 持有 workspace path，统一封装 `facade::WriterCore` 调用，返回 API DTO / `WriterError`，不依赖 Android、Linux、QML 或 UniFFI。

`writer_core` 当前稳定能力包括：

- `create_workspace(path: &Path) -> Result<()>`
- `validate_workspace(path: &Path) -> Result<bool>`
- `get_workspace_diagnostics(path: &Path, has_workspace: bool, tree_count: u64) -> Result<WorkspaceDiagnosticsDto>`：工作区结构、可写性、最近工作区与新建作品可用性诊断由 Core 计算；Linux/QML 只展示 `ResultEnvelope<WorkspaceDiagnosticsDto>`。
- `list_projects(path: &Path) -> Result<Vec<Project>>`
- `create_project(path: &Path, title: &str) -> Result<Project>`
- `get_project_stats(path: &Path, project_id: &str) -> Result<ProjectStats>`
- `list_volumes(path: &Path, project_id: &str) -> Result<Vec<Volume>>`
- `create_volume(path: &Path, project_id: &str, title: &str) -> Result<Volume>`
- `list_chapters(path: &Path, project_id: &str, volume_id: &str) -> Result<Vec<Chapter>>`
- `create_chapter(path: &Path, project_id: &str, volume_id: &str, title: &str) -> Result<Chapter>`
- `read_chapter(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<ChapterContent>`
- `open_chapter(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<ChapterOpenResult>`
- `save_chapter(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str, content: &str) -> Result<()>`
- `save_chapter_verified(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str, content: &str) -> Result<ChapterSaveReceipt>`
- `save_chapter_verified_with_allow_empty_overwrite(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str, content: &str, allow_empty_overwrite: bool) -> Result<ChapterSaveReceipt>`
- `clear_chapter_content(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<()>`
- `clear_chapter_content_verified(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<ChapterSaveReceipt>`
- `load_local_settings(path: &Path) -> Result<LocalSettings>`
- `save_local_settings(path: &Path, settings: &LocalSettings) -> Result<()>`
- `load_syncable_settings(workspace_path: &Path) -> Result<SyncableSettings>`
- `save_syncable_settings(workspace_path: &Path, settings: &SyncableSettings) -> Result<()>`
- `backup_project(path: &Path, project_id: &str) -> Result<()>`
- `move_chapter_to_trash(path: &Path, chapter_id: &str) -> Result<()>`
- `process_writing_event(...) -> Result<bool, WriterError>`
- `record_writing_event(...) -> Result<bool, WriterError>`
- `record_writing_event_for_platform(...) -> Result<bool, WriterError>`：Linux/其他平台显式传入 `platform`，Android 兼容入口仍由 `record_writing_event` 固定为 `android`。
- `flush_writing_stats() -> Result<bool, WriterError>`
- `flush_recent_edits() -> Result<bool, WriterError>`
- `save_mindmap_graph(project_id: &str, graph: MindMapGraphDto) -> Result<bool, WriterError>`：MindMap 整图保存接收强类型 DTO，Core API 不再提供 `graph_json` 写入入口。
- `add_starmap_node(starmap_id: &str, node: StarMapNodeDto, x: f32, y: f32) -> Result<StarMapNodeDto, WriterError>`：StarMap 节点写入接收强类型 DTO。
- `save_starmap_layout(starmap_id: &str, layout: &StarMapLayoutDto) -> Result<bool, WriterError>`：StarMap 布局保存接收强类型 DTO。
- `list_starmaps_for_project(project_id: &str) -> Result<Vec<StarMapMetaDto>, WriterError>`
- `get_starmap(starmap_id: &str) -> Result<StarMapMetaDto, WriterError>`
- `add_starmap_embed(starmap_id: &str, embed: StarMapEmbedDto) -> Result<StarMapEmbedDto, WriterError>`
- `update_starmap_embed(starmap_id: &str, instance_id: &str, patch: StarMapEmbedPatchDto) -> Result<StarMapEmbedDto, WriterError>`
- `delete_starmap_embed(...) -> Result<()>`
- `add_starmap_link(starmap_id: &str, link: StarMapLinkDto) -> Result<StarMapLinkDto, WriterError>`
- `update_starmap_link(starmap_id: &str, link_id: &str, patch: StarMapLinkPatchDto) -> Result<StarMapLinkDto, WriterError>`
- `delete_starmap_link(...) -> Result<()>`
- `find_starmap_references(...) -> Result<Vec<StarMapReference>>`

### 文件操作
所有写操作（`save_chapter`、`save_*_settings`）必须使用 core 的原子替换写入路径：写入临时文件、flush、`fsync` 临时文件，然后 `rename` 替换目标文件。该机制避免目标文件半写入；目录项持久化仍受平台和文件系统语义影响，不宣称跨设备断电的绝对耐久性。

章节写入有防止静默数据丢失的保护机制。在写入章节之前，Core 会读取现有的 `chapter.md`。如果现有内容非空，而普通保存尝试写入空或仅空白的内容，Core 会以 `Error::EmptyOverwriteBlocked` / `EMPTY_OVERWRITE_BLOCKED` 拒绝写入，并保持原文件不变。如需 intentional 清空，必须使用 `clear_chapter_content`、`clear_chapter_content_verified`，或在平台层已确认“用户主动清空”时调用 `save_chapter_content_with_options(..., allow_empty_overwrite=true)` / `save_chapter_verified_with_allow_empty_overwrite(..., true)`；自动保存和普通写入路径不得在未确认用户主动清空时打开该开关。

在用不同内容覆盖现有非空章节文本之前，Core 会在 `backups/chapters/` 下写入一个轻量级恢复副本。备份文件名包含 `project_id`、`volume_id`、`chapter_id` 和时间戳。Core 仅保留每个章节最近的备份，以避免无限增长。

### 错误处理
Core domain 定义统一 `Error` 枚举（如 `writer_core::error::Error`）来处理内部失败模式。跨平台 `api::error::WriterError` 是平台暴露层稳定错误类型，由 `api/error.rs` 集中映射。跨端 Bridge 必须传播稳定错误码和错误语义，不得吞错或只依赖字符串匹配。当前稳定错误码包括 `IO_ERROR`、`JSON_ERROR`、`INVALID_WORKSPACE`、`PROJECT_NOT_FOUND`、`VOLUME_NOT_FOUND`、`CHAPTER_NOT_FOUND`、`EMPTY_OVERWRITE_BLOCKED`、`NOT_IMPLEMENTED`、`REFUSE_DELETE_WORKSPACE_ROOT`、`INVALID_DELETE_TARGET`、`OTHER`。

跨端 JSON 入口必须返回标准 `ResultEnvelope`。JNI fallback 已通过 `ResultEnvelope::from_api_result` 序列化，Android legacy parser 兼容旧字段但优先读取 `errorCode` / `userMessage`。设置保存类 envelope 会把 `SettingsSaved` 写入 `changedEntities`；Android `SettingsBridge` 已改为消费该 Core 标记，不再通过平台端 `SettingsChangeBus.notifyChanged()` 自行发保存事件。

Linux 工作区诊断入口已迁移到 `WriterCoreApi::get_workspace_diagnostics_envelope_json`。平台层只传递当前是否已加载工作区和缓存树节点数，不再自行探测 `workspace_manifest.json`、`projects/`、`app-meta/` 或创建 `.writer_write_test` 判断可写性。

写入、保存、同步和写作统计事件类 API 不允许用裸 `false` 代替错误。`bool` 只能表示业务成功值；Core API 失败必须返回 `WriterError`，Android Bridge 必须转换为 `BridgeResult.Error`。

### 跨端 DTO

- `ChapterOpenResult { meta, content }`：打开章节的唯一权威返回，客户端不应再自行用列表结果拼接标题、备注或正文。
- `ChapterSaveReceipt { chapter_relative_path, content_len, content_hash, meta_hash, updated_at, word_count }`：保存或明确清空后的回执，供客户端确认 Core 已完成写入和校验。
- `ProjectStats { total_word_count, volume_count, chapter_count }`：作品统计由 Core 计算，Android 通过 UniFFI `ProjectStatsDto` 获取，不在客户端自行遍历汇总。

### 同步 API

- 当前运行期同步后端仅暴露 `git` 和 `github_api`。WebDAV、S3、本地文件夹等后端不在 `BackendType` 配置枚举中，也不是客户端可选择的同步路径。

- `SyncConfigDto.backend_type = "github_api"` 使用 Rust Core 的 GitHub REST API 同步路径，不进入 libgit2/Git 工作树 reset 流程；`backend_type = "git"` 才使用传统 Git 后端。
- GitHub API 同步采用 `app-meta/sync/manifest.sync.json` 作为 LWW 清单。清单文件由 Core 管理，不作为普通用户文件计入 `downloaded_files` / `uploaded_files`。
- `ManifestFileRecord` 字段包括 `path`、`content_hash`、`updated_at_ms`、`deleted_at_ms`、`device_id`、`op`、`schema_version`。`deleted_at_ms` 为可选 tombstone 时间戳；旧清单缺少该字段时按 `updated_at_ms` 兼容读取。
- GitHub API 写入使用 Contents API 串行执行：更新文件时带当前远端 blob `sha`，删除文件时带当前远端 blob `sha`；遇到 `409` 会重新读取 `sha` 并重试一次。此路径不调用 Git Database API 写提交，也不调用本地 `fetch/reset`。
- `SyncResultDto` 的 `uploaded_files`、`downloaded_files`、`local_deletes`、`remote_deletes`、`overwritten_files`、`ignored_files` 为 UI 展示的权威计数来源；`error_category` 为同步错误分类的权威来源，客户端仅在该字段为空时退回消息文本分类；`error` 非空时客户端不得显示同步成功，即使状态字符串异常地表示成功。
- `perform_sync_diagnostics` 在 `github_api` 后端返回 `backend_type = "github_api"`，避免 UI 将 GitHub API 模式误判为传统 Git。

### Android Bridge 入口

Android 主业务入口是 `AppServiceBridge + UniFFI`，UniFFI 暴露的 `WriterAppService` 只适配到 `WriterCoreApi`。上层必须使用领域 Bridge：`WorkspaceBridge`、`WritingBridge`、`StatsBridge`、`StarMapBridge`、`MindMapBridge`、`SettingsBridge`、`SyncBridge`。`NativeCoreBridge` 仅作为 legacy JSON/JNI fallback 和 native 状态/旧动作路径；Repository/UI/ViewModel/Controller 不应直接处理 `NativeResult` 或 `NativeCoreBridge`。


## 平台边界原则

Android 和 Linux backend 都必须通过 `WriterCoreApi` 作为主入口，不能直接以 `facade::WriterCore` 作为平台稳定边界。
