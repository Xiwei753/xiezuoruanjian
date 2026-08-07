//! # 回收站管理（Core 层）
//!
//! 本模块提供回收站相关功能。
//!
//! ## 实现说明
//!
//! 删除操作已通过 `project::delete_project`、`volume::delete_volume`、`chapter::delete_chapter` 实现，
//! 所有删除都会将目标移动到 `app-meta/sync/trash/` 目录并记录 tombstone。
//!
//! `move_chapter_to_trash` 是一个便捷入口，内部委托给 `chapter::delete_chapter`。

use crate::error::Result;
use std::path::Path;

/// 将章节移动到回收站。
///
/// 委托给 `chapter::delete_chapter`，将章节目录移动到 `app-meta/sync/trash/`。
pub fn move_chapter_to_trash(projects_root: &Path, chapter_id: &str, app_data_root: &Path) -> Result<()> {
    // 查找 chapter 所属的 project_id 和 volume_id
    // 遍历 projects 目录查找包含该 chapter 的 volume
    if !projects_root.exists() {
        return Err(crate::error::Error::ChapterNotFound);
    }

    for project_entry in std::fs::read_dir(projects_root)?.filter_map(|e| e.ok()) {
        let volumes_dir = project_entry.path().join("volumes");

        let volumes = match std::fs::read_dir(&volumes_dir) {
            Ok(iter) => iter,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };

        for volume_entry in volumes.filter_map(|e| e.ok()) {
            let chapters_dir = volume_entry.path().join("chapters").join(chapter_id);
            if std::fs::metadata(&chapters_dir)
                .map(|m| m.is_dir())
                .unwrap_or(false)
            {
                let project_id = project_entry.file_name().to_string_lossy().into_owned();
                let volume_id = volume_entry.file_name().to_string_lossy().into_owned();
                let project_root = projects_root.join(&project_id);
                return crate::chapter::delete_chapter(
                    &project_root,
                    &volume_id,
                    chapter_id,
                    app_data_root,
                );
            }
        }
    }

    Err(crate::error::Error::ChapterNotFound)
}

/// 生成由于删除产生的墓碑记录。
///
/// 遍历 trash 目录下所有文件，为每个文件生成一条 `Tombstone`：
/// - `original_path`：文件在工作区中的原始相对路径（正斜杠，Git/远端约定）
/// - `trash_path`：文件在 trash 目录中的相对路径
/// - `purge_after`：30 天后可清理（`scanner::build_sync_plan_from_workspace` 据此清理）
/// - `original_hash`：从 `known_files` 中查找，缺失则为空字符串
pub(crate) fn generate_tombstones(
    state: &mut crate::sync::SyncState,
    trash_path: &Path,
    rel_original_dir: &str,
    rel_trash_path: &str,
) {
    for entry in walkdir::WalkDir::new(trash_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let rel_file_path = entry
            .path()
            .strip_prefix(trash_path)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace("\\", "/");
        // Tombstone paths use forward-slash (remote/Git convention).
        // Using [..] slice join to avoid format!("/{}/{}") which could
        // produce double-slash if rel_original_dir already ends with '/'.
        let original_file_path = if rel_original_dir.ends_with('/') {
            format!("{}{}", rel_original_dir, rel_file_path)
        } else {
            format!("{}/{}", rel_original_dir, rel_file_path)
        };
        let new_trash_path = if rel_trash_path.ends_with('/') {
            format!("{}{}", rel_trash_path, rel_file_path)
        } else {
            format!("{}/{}", rel_trash_path, rel_file_path)
        };

        let tombstone = crate::sync::Tombstone {
            original_path: original_file_path.clone(),
            trash_path: new_trash_path,
            deleted_at: chrono::Utc::now().timestamp(),
            purge_after: chrono::Utc::now().timestamp() + 30 * 24 * 3600,
            deleted_by: "local".to_string(),
            original_hash: state
                .known_files
                .get(&original_file_path)
                .cloned()
                .unwrap_or_default(),
            kind: "local_delete".to_string(),
        };
        state.tombstones.push(tombstone);
    }
}
