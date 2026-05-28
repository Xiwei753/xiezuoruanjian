# Core API

The `writer_core` Rust library exposes the following high-level API functions for the clients:

- `create_workspace(path: &Path) -> Result<()>`
- `validate_workspace(path: &Path) -> Result<bool>`
- `list_projects(path: &Path) -> Result<Vec<Project>>`
- `create_project(path: &Path, title: &str) -> Result<Project>`
- `list_volumes(path: &Path, project_id: &str) -> Result<Vec<Volume>>`
- `create_volume(path: &Path, project_id: &str, title: &str) -> Result<Volume>`
- `list_chapters(path: &Path, project_id: &str, volume_id: &str) -> Result<Vec<Chapter>>`
- `create_chapter(path: &Path, project_id: &str, volume_id: &str, title: &str) -> Result<Chapter>`
- `read_chapter(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<ChapterContent>`
- `save_chapter(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str, content: &str) -> Result<()>`
- `save_chapter_verified(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str, content: &str) -> Result<ChapterSaveReceipt>`
- `clear_chapter_content(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<()>`
- `load_local_settings(path: &Path) -> Result<LocalSettings>`
- `save_local_settings(path: &Path, settings: &LocalSettings) -> Result<()>`
- `load_syncable_settings(workspace_path: &Path) -> Result<SyncableSettings>`
- `save_syncable_settings(workspace_path: &Path, settings: &SyncableSettings) -> Result<()>`
- `backup_project(path: &Path, project_id: &str) -> Result<()>`
- `move_chapter_to_trash(path: &Path, chapter_id: &str) -> Result<()>`

### File Operations
All write operations (`save_chapter`, `save_*_settings`) must use atomic writing (write to temporary file, `fsync/flush`, and atomic `rename`).

Chapter writes are guarded against silent data loss. Before writing a chapter, Core reads the existing `chapter.md`. If the existing content is non-empty and a normal save attempts to write empty or whitespace-only content, Core rejects the write with `Error::EmptyOverwriteBlocked` / `blocked_empty_overwrite` and leaves the original file unchanged. Intentional clearing must use `clear_chapter_content`; autosave and normal write paths must not call that API.

Before overwriting existing non-empty chapter text with different content, Core writes a lightweight recovery copy under `backups/chapters/`. Backup filenames include `project_id`, `volume_id`, `chapter_id`, and a timestamp. Core keeps only the most recent chapter backups per chapter to avoid unbounded growth.

### Error Handling
The core defines a unified `Error` enum (e.g., `writer_core::error::Error`) for all failure modes, instead of relying on string-based errors.
