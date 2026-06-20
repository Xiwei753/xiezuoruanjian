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
pub fn move_chapter_to_trash(workspace_path: &Path, chapter_id: &str) -> Result<()> {
    // 查找 chapter 所属的 project_id 和 volume_id
    // 遍历 projects 目录查找包含该 chapter 的 volume
    let projects_dir = workspace_path.join("projects");
    if !projects_dir.exists() {
        return Err(crate::error::Error::ChapterNotFound);
    }

    for project_entry in std::fs::read_dir(&projects_dir)?.filter_map(|e| e.ok()) {
        let project_id = project_entry.file_name().to_string_lossy().to_string();
        let volumes_dir = project_entry.path().join("volumes");
        if !volumes_dir.exists() {
            continue;
        }
        for volume_entry in std::fs::read_dir(&volumes_dir)?.filter_map(|e| e.ok()) {
            let volume_id = volume_entry.file_name().to_string_lossy().to_string();
            let chapters_dir = volume_entry.path().join("chapters").join(chapter_id);
            if chapters_dir.exists() {
                return crate::chapter::delete_chapter(
                    workspace_path,
                    &project_id,
                    &volume_id,
                    chapter_id,
                );
            }
        }
    }

    Err(crate::error::Error::ChapterNotFound)
}

/// 生成由于删除产生的墓碑记录
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
        let original_file_path = format!("{}/{}", rel_original_dir, rel_file_path);
        let new_trash_path = format!("{}/{}", rel_trash_path, rel_file_path);

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
