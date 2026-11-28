//! 同步对象落地用的两个文件原语：冲突副本保存、远端删除移入 trash。
//!
//! 所有远端读写都通过 [`crate::sync::provider::SyncProvider`] trait 执行；
//! 本模块不依赖具体传输实现（Git HTTPS 或 GitHub REST API）。

use std::path::Path;

use crate::sync::path::ValidatedSyncPath;

/// 把远端内容保存成本地冲突副本，返回相对 `sync_root` 的完整 snapshot 路径
/// （如 `volumes/v1/chapters/c1/chapter.md.remote-conflict-20260925-012345`）。
///
/// 供 [`crate::sync::types::SyncConflict::remote_snapshot_path`] 记录，
/// 预览 API 据此读取远端内容；选择远端时按该快照落盘。
pub(crate) fn save_conflict_copy(
    sync_root: &Path,
    path: &str,
    remote_content: &[u8],
) -> crate::Result<String> {
    let validated = ValidatedSyncPath::new(path)?;
    let full_path = validated.join_under(sync_root);
    let filename = full_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let conflict_filename = format!("{}.remote-conflict-{}", filename, timestamp);

    let conflict_path = full_path
        .parent()
        .unwrap_or(&full_path)
        .join(&conflict_filename);

    crate::storage::transaction::atomic_write_bytes(&conflict_path, remote_content).map_err(
        |e| {
            crate::Error::Io(std::io::Error::other(format!(
                "write conflict copy {}: {}",
                path, e
            )))
        },
    )?;

    // 返回相对 sync_root 的路径，供 remote_snapshot_path 记录。
    let relative = conflict_path
        .strip_prefix(sync_root)
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "save_conflict_copy: conflict_path not under sync_root: {}",
                e
            )))
        })?
        .to_string_lossy()
        .to_string();
    Ok(relative)
}

/// 将远端已删除的文件移至 trash 目录而非直接删除。
///
/// trash 文件名格式：`{timestamp}_{uuid}_{original_filename}`。
/// rename 失败必须显式报错：真实文件还在时不能先把对象索引当成"已经删除"。
pub(crate) fn move_to_trash(sync_root: &Path, paths: &[String]) -> crate::Result<()> {
    for path in paths {
        let validated = ValidatedSyncPath::new(path)?;
        let full_path = validated.join_under(sync_root);
        if full_path.exists() {
            let filename = full_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let trash_dir = sync_root.join("app-meta/sync/trash");
            std::fs::create_dir_all(&trash_dir)?;
            let trash_path = trash_dir.join(format!(
                "{}_{}_{}",
                chrono::Utc::now().timestamp_millis(),
                uuid::Uuid::new_v4(),
                filename
            ));
            crate::storage::transaction::durable_rename(&full_path, &trash_path).map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "move_to_trash: failed to move {} to trash: {}",
                    path, e
                )))
            })?;
        }
    }
    Ok(())
}
