//! LWW transfer — remote tree、download、upload、delete 辅助。
//!
//! 从 mod.rs 抽出的传输相关函数：远端 tree/manifest 拉取、并行下载、串行上传/删除、
//! trash 移动、冲突副本保存。
//!
//! 所有远端操作通过 [`crate::sync::provider::SyncProvider`] trait 执行，
//! 不直接依赖 GitHub API 或 `SyncTransport`。ProviderError 通过 `From` 自动转为 `crate::Error`。

use std::collections::HashMap;
use std::path::Path;

use rayon::prelude::*;

use crate::sync::provider::model::{DeletePrecondition, RemoteVersion, WritePrecondition};
use crate::sync::provider::SyncProvider;
use crate::sync::types::SyncManifest;

/// 并行下载最大线程数。4 线程在远端 API 速率限制和本地磁盘 I/O 之间取得平衡。
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

/// 通过 Provider 拉取远端 tree，返回以本地相对路径为 key、远端版本为 value 的 map。
///
/// `provider.list(remote_prefix)` 返回已剥前缀的 `RemoteEntry` 列表，
/// 此函数转为 `HashMap<path, version_string>` 供 LWW 比较使用。
pub(super) fn fetch_remote_tree(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
) -> crate::Result<HashMap<String, String>> {
    let entries = provider.list(remote_prefix)?;
    let mut map = HashMap::with_capacity(entries.len());
    for entry in entries {
        map.insert(entry.path, entry.version.0);
    }
    Ok(map)
}

/// 通过 Provider 拉取远端 manifest。
///
/// 如果远端 tree 中不存在 manifest 文件，返回空的默认 manifest。
pub(super) fn fetch_remote_manifest(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    remote_tree_files: &HashMap<String, String>,
) -> crate::Result<SyncManifest> {
    let sync_manifest_path = super::manifest::SYNC_MANIFEST_PATH;
    let remote_manifest_path = format!("{}/{}", remote_prefix, sync_manifest_path);
    let mut remote_manifest = SyncManifest::default();
    if remote_tree_files.contains_key(sync_manifest_path) {
        if let Some(obj) = provider.read(&remote_manifest_path)? {
            remote_manifest =
                serde_json::from_slice::<SyncManifest>(&obj.content).map_err(|e| {
                    crate::Error::SyncRemoteApiError {
                        category: "api_error".to_string(),
                        context: format!("invalid remote manifest: {}", e),
                        status: 0,
                        body_preview: String::new(),
                    }
                })?;
        }
    }
    Ok(remote_manifest)
}

/// 并行下载 pending_take_remote 列表中的文件。
///
/// 返回 `Vec<(path, Option<content>)>`，`None` 表示远端文件缺失。
#[allow(clippy::excessive_nesting, clippy::type_complexity)]
pub(super) fn download_pending_take_remote(
    sync_root: &Path,
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    pending_paths: &[String],
) -> crate::Result<Vec<(String, Option<Vec<u8>>)>> {
    let download_pool = sync_download_pool(pending_paths.len())?;
    download_pool.install(|| {
        pending_paths
            .par_iter()
            .map(|path| {
                let remote_path = format!("{}/{}", remote_prefix, path);
                let remote = provider.read(&remote_path)?;
                let Some(obj) = remote else {
                    return Ok((path.clone(), None));
                };
                let content = obj.content;

                let full_path = sync_root.join(path);
                if let Some(parent) = full_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "create pending_take_remote dir {}: {}",
                            path, e
                        )))
                    })?;
                }
                let tmp_path = full_path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
                std::fs::write(&tmp_path, &content).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "write pending_take_remote {}: {}",
                        path, e
                    )))
                })?;
                std::fs::rename(&tmp_path, &full_path).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "rename pending_take_remote {}: {}",
                        path, e
                    )))
                })?;
                Ok((path.clone(), Some(content)))
            })
            .collect()
    })
}

/// 将远端已删除的文件移至 trash 目录而非直接删除。
///
/// trash 文件名格式：`{timestamp}_{uuid}_{original_filename}`。
/// rename 失败时静默忽略：文件可能被其他进程占用或权限不足。
pub(super) fn move_to_trash(sync_root: &Path, paths: &[String]) {
    for path in paths {
        let full_path = sync_root.join(path);
        if full_path.exists() {
            let filename = full_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let trash_dir = sync_root.join("app-meta/sync/trash");
            let _ = std::fs::create_dir_all(&trash_dir);
            let trash_path = trash_dir.join(format!(
                "{}_{}_{}",
                chrono::Utc::now().timestamp_millis(),
                uuid::Uuid::new_v4(),
                filename
            ));
            // rename 失败时静默忽略：文件可能被其他进程占用或权限不足。
            // 后果是本地文件残留，但 manifest 已记录远端删除，下次同步时
            // 该文件会被视为本地新增（local-only），不会静默丢失用户数据。
            let _ = std::fs::rename(&full_path, &trash_path);
        }
    }
}

/// 并行下载远端较新文件到本地。
///
/// 使用 rayon 并行线程池，每个文件先写入临时文件（带随机后缀），再 rename 替换目标文件，
/// 保证下载中断不会留下半写入文件。
#[allow(clippy::excessive_nesting)]
pub(super) fn download_remote_files(
    sync_root: &Path,
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    to_download: &[String],
) -> crate::Result<()> {
    if to_download.is_empty() {
        return Ok(());
    }
    let download_pool = sync_download_pool(to_download.len())?;
    let download_result: crate::Result<()> = download_pool.install(|| {
        to_download.par_iter().try_for_each(|path| {
            let remote_path = format!("{}/{}", remote_prefix, path);
            let Some(obj) = provider.read(&remote_path)? else {
                return Err(crate::Error::SyncRemoteApiError {
                    category: "api_error".to_string(),
                    context: format!("remote file missing while downloading {}", path),
                    status: 0,
                    body_preview: String::new(),
                });
            };
            let content = obj.content;
            let full_path = sync_root.join(path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!("{}: {}", path, e)))
                })?;
            }
            let tmp_path = full_path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
            std::fs::write(&tmp_path, content)
                .map_err(|e| crate::Error::Io(std::io::Error::other(format!("{}: {}", path, e))))?;
            std::fs::rename(tmp_path, &full_path)
                .map_err(|e| crate::Error::Io(std::io::Error::other(format!("{}: {}", path, e))))?;
            Ok(())
        })
    });
    download_result
}

/// 串行上传本地较新文件到远端。
///
/// 通过 `provider.write` 实现 create-or-update：
/// - `conditional_write = true`：使用乐观并发前置条件（IfMatch / CreateNew）。
/// - `conditional_write = false`：使用 Unconditional 写入，由 engine 在写前读取远端版本。
pub(super) fn upload_local_files(
    sync_root: &Path,
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    to_upload: &[String],
    remote_tree_files: &HashMap<String, String>,
) -> crate::Result<()> {
    let caps = provider.capabilities();
    for path in to_upload {
        let full_path = sync_root.join(path);
        if !full_path.exists() {
            continue;
        }
        let content = std::fs::read(&full_path).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!("read {}: {}", path, e)))
        })?;
        let remote_path = format!("{}/{}", remote_prefix, path);
        let precondition = if caps.conditional_write {
            match remote_tree_files.get(path.as_str()) {
                Some(sha) => WritePrecondition::IfMatch(RemoteVersion(sha.clone())),
                None => WritePrecondition::CreateNew,
            }
        } else {
            WritePrecondition::Unconditional
        };
        provider.write(&remote_path, &content, precondition)?;
    }
    Ok(())
}

/// 串行删除远端文件。
///
/// - `conditional_write = true`：使用 IfMatch 前置条件（乐观并发）。
/// - `conditional_write = false`：使用 Unconditional 删除。
pub(super) fn delete_remote_files(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    paths: &[String],
    remote_tree_files: &HashMap<String, String>,
) -> crate::Result<()> {
    let caps = provider.capabilities();
    for path in paths {
        let remote_path = format!("{}/{}", remote_prefix, path);
        let precondition = if caps.conditional_write {
            match remote_tree_files.get(path.as_str()) {
                Some(sha) => DeletePrecondition::IfMatch(RemoteVersion(sha.clone())),
                None => DeletePrecondition::Unconditional,
            }
        } else {
            DeletePrecondition::Unconditional
        };
        provider.delete(&remote_path, precondition)?;
    }
    Ok(())
}

/// 上传合并后的 manifest 到远端。
///
/// - `conditional_write = true`：使用乐观并发前置条件。
/// - `conditional_write = false`：使用 Unconditional 写入。
pub(super) fn upload_manifest(
    provider: &dyn SyncProvider,
    remote_manifest_path: &str,
    manifest_json: &str,
    remote_tree_files: &HashMap<String, String>,
) -> crate::Result<()> {
    let sync_manifest_path = super::manifest::SYNC_MANIFEST_PATH;
    let caps = provider.capabilities();
    let precondition = if caps.conditional_write {
        match remote_tree_files.get(sync_manifest_path) {
            Some(sha) => WritePrecondition::IfMatch(RemoteVersion(sha.clone())),
            None => WritePrecondition::CreateNew,
        }
    } else {
        WritePrecondition::Unconditional
    };
    provider.write(remote_manifest_path, manifest_json.as_bytes(), precondition)?;
    Ok(())
}
