//! #644 评论 5462823517 第3节：LWW transfer — remote tree、download、upload、delete 辅助。
//!
//! 从 lww.rs 抽出的传输相关：sync_download_pool、save_conflict_copy。

use std::path::Path;

/// 并行下载最大线程数。4 线程在 GitHub API 速率限制和本地磁盘 I/O 之间取得平衡。
pub(super) const MAX_PARALLEL_DOWNLOADS: usize = 4;

/// 创建并行下载线程池。线程数取 `task_count` 和 `MAX_PARALLEL_DOWNLOADS` 的较小值，
/// 至少 1 线程。使用 rayon 的 work-stealing 调度器。
pub(super) fn sync_download_pool(task_count: usize) -> crate::Result<rayon::ThreadPool> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(task_count.clamp(1, MAX_PARALLEL_DOWNLOADS))
        .build()
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "sync_parallel_pool_error: {}",
                e
            )))
        })
}

/// 将远端冲突内容保存为本地备份文件。
///
/// 文件名格式：`{原文件名}.remote-conflict-{时间戳}`，保存在原文件同目录下。
/// 此备份供用户手动对比本地与远端内容，不参与自动合并逻辑。
pub(super) fn save_conflict_copy(
    sync_root: &Path,
    path: &str,
    remote_content: &[u8],
) -> crate::Result<String> {
    let full_path = sync_root.join(path);
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

    if let Some(parent) = conflict_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    std::fs::write(&conflict_path, remote_content).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "write conflict copy {}: {}",
            path, e
        )))
    })?;

    Ok(conflict_filename)
}
