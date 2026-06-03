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
