# Core API

`writer_core` Rust 库的跨平台暴露边界已经拆成四层：

- Core domain：`workspace`、`project`、`volume`、`chapter`、`settings`、`sync_service`、`mind_map`、`starmap`、`writing_stats` 等模块负责真实业务和文件 I/O。
- Facade：`facade::WriterCore` 聚合内部业务能力，是 **Core 内部协调层**；它不承诺作为平台稳定 DTO 边界，也不应该被外部直接调用。
- Core API：`api::WriterCoreApi`、`api::types`、`api::error` 是**平台稳定入口**，面向 UniFFI、Linux binding、Android JNI 和未来前端。Android JNI 只能调用 `WriterCoreApi`。
- UniFFI adapter：`app_service::WriterAppService` 只保留 Android 兼容的对象名和方法签名，负责接收 UniFFI 参数并委托 `WriterCoreApi`。

> 注意：`sync_service` 的解耦拆分是后续阶段的任务，不在本次重构范围内。

`api.udl` 只是 UniFFI 绑定声明，用于生成 Kotlin/外部语言桥接代码；业务 API 的事实来源是 Rust `api/` 模块及其文档边界，不是 UDL 文件本身。

## Core API 模块

- `core/writer_core/src/api/types.rs`：平台稳定 DTO，如 `ProjectDto`、`VolumeDto`、`ChapterMetaDto`、`ChapterContentDto`、`ChapterSaveReceiptDto`、`RecentEditDto`、`LocalSettingsDto`、`SyncConfigDto`、`SyncStateDto`、`SyncResultDto` 等。
- `core/writer_core/src/api/error.rs`：平台稳定错误 `WriterError`，集中从 `crate::error::Error` 映射，保留 `Io`、`Json`、`InvalidWorkspace`、`ProjectNotFound`、`VolumeNotFound`、`ChapterNotFound`、`EmptyOverwriteBlocked`、`NotImplemented`、`RefuseToDeleteWorkspaceRoot`、`InvalidDeleteTarget`、`Other` 语义。
- `core/writer_core/src/api/service.rs`：`WriterCoreApi` 持有 workspace path，统一封装 `facade::WriterCore` 调用，返回 API DTO / `WriterError`，不依赖 Android、Linux、QML 或 UniFFI。

`writer_core` 当前稳定能力包括：

- `create_workspace(path: &Path) -> Result<()>`
- `validate_workspace(path: &Path) -> Result<bool>`
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
- `list_starmaps_for_project(project_id: &str) -> Result<Vec<StarMapMetaDto>, WriterError>`
- `get_starmap(starmap_id: &str) -> Result<StarMapMetaDto, WriterError>`
- `add_starmap_embed(...) -> Result<StarMapEmbed, ...>`
- `update_starmap_embed(...) -> Result<StarMapEmbed, ...>`
- `delete_starmap_embed(...) -> Result<()>`
- `add_starmap_link(...) -> Result<StarMapLink, ...>`
- `update_starmap_link(...) -> Result<StarMapLink, ...>`
- `delete_starmap_link(...) -> Result<()>`
- `find_starmap_references(...) -> Result<Vec<StarMapReference>>`

### 文件操作
所有写操作（`save_chapter`、`save_*_settings`）必须使用原子写入（写入临时文件、`fsync/flush`，然后原子 `rename`）。

章节写入有防止静默数据丢失的保护机制。在写入章节之前，Core 会读取现有的 `chapter.md`。如果现有内容非空，而普通保存尝试写入空或仅空白的内容，Core 会以 `Error::EmptyOverwriteBlocked` / `EMPTY_OVERWRITE_BLOCKED` 拒绝写入，并保持原文件不变。如需 intentional 清空，必须使用 `clear_chapter_content`、`clear_chapter_content_verified`，或在平台层已确认“用户主动清空”时调用 `save_chapter_content_with_options(..., allow_empty_overwrite=true)` / `save_chapter_verified_with_allow_empty_overwrite(..., true)`；自动保存和普通写入路径不得在未确认用户主动清空时打开该开关。

在用不同内容覆盖现有非空章节文本之前，Core 会在 `backups/chapters/` 下写入一个轻量级恢复副本。备份文件名包含 `project_id`、`volume_id`、`chapter_id` 和时间戳。Core 仅保留每个章节最近的备份，以避免无限增长。

### 错误处理
Core domain 定义统一 `Error` 枚举（如 `writer_core::error::Error`）来处理内部失败模式。跨平台 `api::error::WriterError` 是平台暴露层稳定错误类型，由 `api/error.rs` 集中映射。跨端 Bridge 必须传播稳定错误码和错误语义，不得吞错或只依赖字符串匹配。当前稳定错误码包括 `IO_ERROR`、`JSON_ERROR`、`INVALID_WORKSPACE`、`PROJECT_NOT_FOUND`、`VOLUME_NOT_FOUND`、`CHAPTER_NOT_FOUND`、`EMPTY_OVERWRITE_BLOCKED`、`NOT_IMPLEMENTED`、`REFUSE_DELETE_WORKSPACE_ROOT`、`INVALID_DELETE_TARGET`、`OTHER`。

写入、保存、同步和写作统计事件类 API 不允许用裸 `false` 代替错误。`bool` 只能表示业务成功值；Core API 失败必须返回 `WriterError`，Android Bridge 必须转换为 `BridgeResult.Error`。

### 跨端 DTO

- `ChapterOpenResult { meta, content }`：打开章节的唯一权威返回，客户端不应再自行用列表结果拼接标题、备注或正文。
- `ChapterSaveReceipt { chapter_relative_path, content_len, content_hash, meta_hash, updated_at, word_count }`：保存或明确清空后的回执，供客户端确认 Core 已完成写入和校验。
- `ProjectStats { total_word_count, volume_count, chapter_count }`：作品统计由 Core 计算，Android 通过 UniFFI `ProjectStatsDto` 获取，不在客户端自行遍历汇总。

### Android Bridge 入口

Android 主业务入口是 `AppServiceBridge + UniFFI`，UniFFI 暴露的 `WriterAppService` 只适配到 `WriterCoreApi`。上层必须使用领域 Bridge：`WorkspaceBridge`、`WritingBridge`、`StatsBridge`、`StarMapBridge`、`MindMapBridge`、`SettingsBridge`、`SyncBridge`。`NativeCoreBridge` 仅作为 legacy JSON/JNI fallback 和 native 状态/旧动作路径；Repository/UI/ViewModel/Controller 不应直接处理 `NativeResult` 或 `NativeCoreBridge`。


## 平台边界原则

Android 和 Linux backend 都必须通过 `WriterCoreApi` 作为主入口，不能直接以 `facade::WriterCore` 作为平台稳定边界。
