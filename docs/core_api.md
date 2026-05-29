# Core API

`writer_core` Rust 库为客户端提供以下高层 API 函数：

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
- `clear_chapter_content(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<()>`
- `clear_chapter_content_verified(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<ChapterSaveReceipt>`
- `load_local_settings(path: &Path) -> Result<LocalSettings>`
- `save_local_settings(path: &Path, settings: &LocalSettings) -> Result<()>`
- `load_syncable_settings(workspace_path: &Path) -> Result<SyncableSettings>`
- `save_syncable_settings(workspace_path: &Path, settings: &SyncableSettings) -> Result<()>`
- `backup_project(path: &Path, project_id: &str) -> Result<()>`
- `move_chapter_to_trash(path: &Path, chapter_id: &str) -> Result<()>`
- `flush_writing_stats() -> Result<()>`

### 文件操作
所有写操作（`save_chapter`、`save_*_settings`）必须使用原子写入（写入临时文件、`fsync/flush`，然后原子 `rename`）。

章节写入有防止静默数据丢失的保护机制。在写入章节之前，Core 会读取现有的 `chapter.md`。如果现有内容非空，而普通保存尝试写入空或仅空白的内容，Core 会以 `Error::EmptyOverwriteBlocked` / `EMPTY_OVERWRITE_BLOCKED` 拒绝写入，并保持原文件不变。如需 intentional 清空，必须使用 `clear_chapter_content` 或 `clear_chapter_content_verified`；自动保存和普通写入路径不应调用该 API。

在用不同内容覆盖现有非空章节文本之前，Core 会在 `backups/chapters/` 下写入一个轻量级恢复副本。备份文件名包含 `project_id`、`volume_id`、`chapter_id` 和时间戳。Core 仅保留每个章节最近的备份，以避免无限增长。

### 错误处理
核心定义了统一的 `Error` 枚举（如 `writer_core::error::Error`）来处理所有失败模式，而不是依赖基于字符串的错误。跨端 Bridge 必须使用 `Error::code()` 暴露稳定错误码，并可通过 `BridgeResult<T>` 兼容旧 JSON 包装。当前稳定错误码包括 `IO_ERROR`、`JSON_ERROR`、`INVALID_WORKSPACE`、`PROJECT_NOT_FOUND`、`VOLUME_NOT_FOUND`、`CHAPTER_NOT_FOUND`、`EMPTY_OVERWRITE_BLOCKED`、`NOT_IMPLEMENTED`、`REFUSE_DELETE_WORKSPACE_ROOT`、`INVALID_DELETE_TARGET`、`OTHER`。

### 跨端 DTO

- `ChapterOpenResult { meta, content }`：打开章节的唯一权威返回，客户端不应再自行用列表结果拼接标题、备注或正文。
- `ChapterSaveReceipt { chapter_relative_path, content_len, content_hash, meta_hash, updated_at, word_count }`：保存或明确清空后的回执，供客户端确认 Core 已完成写入和校验。
- `ProjectStats { total_word_count, volume_count, chapter_count }`：作品统计由 Core 计算，Android 通过 UniFFI `ProjectStatsDto` 获取，不在客户端自行遍历汇总。

### Android Bridge 入口

Android 主业务入口是 `AppServiceBridge + UniFFI`，`api.udl` 暴露 `WriterAppService` typed DTO/error。上层必须使用领域 Bridge：`WorkspaceBridge`、`WritingBridge`、`StatsBridge`、`StarMapBridge`、`MindMapBridge`、`SettingsBridge`、`SyncBridge`。`NativeCoreBridge` 仅作为 legacy JSON/JNI fallback 和 native 状态/旧动作路径；Repository/UI/ViewModel/Controller 不应直接处理 `NativeResult` 或 `NativeCoreBridge`。
