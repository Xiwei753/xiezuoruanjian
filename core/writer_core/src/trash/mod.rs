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
pub fn move_chapter_to_trash(
    projects_root: &Path,
    chapter_id: &str,
    app_data_root: &Path,
) -> Result<()> {
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
/// - `original_path`：文件在作品目录中的原始相对路径（正斜杠，Git/远端约定）
/// - `trash_path`：文件在 trash 目录中的相对路径
/// - `purge_after`：30 天后可清理（`scanner::build_sync_plan` 据此清理）
/// - `original_hash`：从 `known_files` 中查找，缺失则为空字符串
///
/// #645 评论 5504296097 问题3（缺口3修复）：幂等 upsert/skip。
///
/// 对同一 trash 目录多次调用（崩溃恢复重放）不会重复追加 tombstone。
/// 按 `(original_path, trash_path, kind)` 三元组判定已有 tombstone，
/// 已存在则跳过，不存在才追加。这样即使死在"tombstone 已写、phase 尚未
/// 持久化"的窗口，重放也不改变语义。
pub(crate) fn generate_tombstones(
    state: &mut crate::sync::SyncState,
    trash_path: &Path,
    rel_original_dir: &str,
    rel_trash_path: &str,
) {
    // 预收集已有 tombstone 的键集合，避免 O(n²) 线性扫描。
    let mut existing_keys: std::collections::HashSet<(String, String, String)> =
        std::collections::HashSet::with_capacity(state.tombstones.len());
    for ts in &state.tombstones {
        existing_keys.insert((
            ts.original_path.clone(),
            ts.trash_path.clone(),
            ts.kind.clone(),
        ));
    }

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
        // #645 评论 5504296097 缺口3：跳过 sync 引擎内部状态文件
        // （app-meta/ 下）。这些是 save_sync_state 写到 trash 里的引擎状态，
        // 不是被删除的用户内容，不应生成 tombstone 通知远端。
        // 同时保证 write_tombstone 重放幂等——save_sync_state 的副作用不会
        // 让第二次扫描多出"新文件"破坏幂等性。
        if rel_file_path.starts_with("app-meta/") {
            continue;
        }
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

        let kind = "local_delete".to_string();

        // 幂等 upsert/skip：按 (original_path, trash_path, kind) 判定已有则跳过。
        let key = (
            original_file_path.clone(),
            new_trash_path.clone(),
            kind.clone(),
        );
        if existing_keys.contains(&key) {
            continue;
        }
        existing_keys.insert(key);

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
            kind,
        };
        state.tombstones.push(tombstone);
    }
}

#[cfg(test)]
mod tests;
