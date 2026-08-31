//! LWW transfer — remote tree、download、upload、delete 辅助。
//!
//! 从 mod.rs 抽出的传输相关函数：远端 tree/manifest 拉取、并行下载、串行上传/删除、
//! trash 移动、冲突副本保存。

use crate::sync::github_api_client::{
    github_delete_content_serial, github_get_content, github_put_content_serial,
};
use crate::sync::types::SyncManifest;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use writer_platform_api::{HttpRequest, SyncTransport};

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

/// 从 GitHub API 拉取远端 Git tree。
///
/// 返回以本地相对路径为 key、远端 blob SHA 为 value 的 map。
///
/// 404 诊断逻辑：
/// - tree 404 + ref 200 → 空仓库，返回空 map
/// - tree 404 + ref 404 + repo 200 → 分支不存在
/// - tree 404 + ref 404 + repo 401/403 → 权限不足
#[allow(clippy::too_many_lines)]
pub(super) fn fetch_remote_tree(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
    remote_prefix: &str,
) -> crate::Result<HashMap<String, String>> {
    let tree_url = format!("{}/git/trees/{}?recursive=1", api_base, branch);
    let tree_request = HttpRequest {
        method: "GET".to_string(),
        url: tree_url,
        headers: vec![
            ("Authorization".to_string(), format!("Bearer {}", token)),
            ("User-Agent".to_string(), "WriterApp/1.0".to_string()),
            (
                "Accept".to_string(),
                "application/vnd.github+json".to_string(),
            ),
        ],
        body: None,
    };
    let tree_resp =
        transport
            .execute(tree_request)
            .map_err(|e| crate::Error::SyncNetworkUnavailable {
                reason: format!("{}: {}", e.category, e.message),
            })?;

    let mut remote_tree_files = HashMap::new();
    let tree_status = tree_resp.status;
    let tree_body = String::from_utf8(tree_resp.body).unwrap_or_default();
    if tree_status == 200 {
        let json: serde_json::Value =
            serde_json::from_str(&tree_body).map_err(|e| crate::Error::SyncGithubApiError {
                category: "api_error".to_string(),
                context: format!("invalid tree json: {}", e),
                status: 0,
                body_preview: String::new(),
            })?;
        if json["truncated"].as_bool().unwrap_or(false) {
            return Err(crate::Error::SyncGithubApiError {
                category: "api_error".to_string(),
                context: "GitHub tree response truncated, repository is too large".to_string(),
                status: 0,
                body_preview: String::new(),
            });
        }
        if let Some(tree) = json["tree"].as_array() {
            let prefix_with_slash = format!("{}/", remote_prefix);
            for item in tree {
                if item["type"].as_str() == Some("blob") {
                    if let (Some(path), Some(sha)) = (item["path"].as_str(), item["sha"].as_str()) {
                        // 只保留以 remote_prefix/ 开头的远端路径，剥掉前缀后作为本地 relative path。
                        if let Some(local_path) = path.strip_prefix(&prefix_with_slash) {
                            remote_tree_files.insert(local_path.to_string(), sha.to_string());
                        }
                    }
                }
            }
        }
    } else if tree_status == 404 {
        let ref_url = format!("{}/git/ref/heads/{}", api_base, branch);
        let ref_request = HttpRequest {
            method: "GET".to_string(),
            url: ref_url,
            headers: vec![
                ("Authorization".to_string(), format!("Bearer {}", token)),
                ("User-Agent".to_string(), "WriterApp/1.0".to_string()),
                (
                    "Accept".to_string(),
                    "application/vnd.github+json".to_string(),
                ),
            ],
            body: None,
        };
        let ref_resp =
            transport
                .execute(ref_request)
                .map_err(|e| crate::Error::SyncNetworkUnavailable {
                    reason: format!("{}: {}", e.category, e.message),
                })?;
        let ref_status = ref_resp.status;
        if ref_status == 200 {
            // 仓库和分支都存在，tree 404 说明是空仓库，remote_tree_files 保持为空
            log::debug!(
                "[sync] tree 404 but ref 200: branch {} exists with empty tree, treating as empty remote",
                branch
            );
        } else if ref_status == 404 {
            let repo_request = HttpRequest {
                method: "GET".to_string(),
                url: api_base.to_string(),
                headers: vec![
                    ("Authorization".to_string(), format!("Bearer {}", token)),
                    ("User-Agent".to_string(), "WriterApp/1.0".to_string()),
                    (
                        "Accept".to_string(),
                        "application/vnd.github+json".to_string(),
                    ),
                ],
                body: None,
            };
            let repo_resp = transport.execute(repo_request).map_err(|e| {
                crate::Error::SyncNetworkUnavailable {
                    reason: format!("{}: {}", e.category, e.message),
                }
            })?;
            let repo_status = repo_resp.status;
            if repo_status == 200 {
                // 仓库可访问但分支不存在
                return Err(crate::Error::SyncRemoteBranchNotFound {
                    detail: format!("branch '{}' not found in repository", branch),
                });
            } else if repo_status == 401 || repo_status == 403 {
                return Err(crate::Error::SyncGithubApiError {
                    category: "repo_not_found_or_no_permission".to_string(),
                    context: "token lacks access to repository".to_string(),
                    status: repo_status,
                    body_preview: String::new(),
                });
            } else {
                return Err(crate::Error::SyncGithubApiError {
                    category: "repo_not_found_or_no_permission".to_string(),
                    context: "repository not found or inaccessible".to_string(),
                    status: repo_status,
                    body_preview: String::new(),
                });
            }
        } else if ref_status == 401 || ref_status == 403 {
            return Err(crate::Error::SyncGithubApiError {
                category: "repo_not_found_or_no_permission".to_string(),
                context: "authentication failed or token lacks permission".to_string(),
                status: ref_status,
                body_preview: String::new(),
            });
        } else {
            return Err(crate::Error::SyncGithubApiError {
                category: "repo_not_found_or_no_permission".to_string(),
                context: format!("unexpected HTTP {} when checking ref", ref_status),
                status: ref_status,
                body_preview: String::new(),
            });
        }
    } else {
        return Err(crate::sync::github_api_client::github_api_error(
            "get recursive tree",
            tree_status,
            tree_body,
        ));
    }

    Ok(remote_tree_files)
}

/// 从 GitHub API 拉取远端 manifest。
///
/// 如果远端 tree 中不存在 manifest 文件，返回空的默认 manifest。
pub(super) fn fetch_remote_manifest(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
    remote_prefix: &str,
    remote_tree_files: &HashMap<String, String>,
) -> crate::Result<SyncManifest> {
    let sync_manifest_path = super::manifest::SYNC_MANIFEST_PATH;
    let remote_manifest_path = format!("{}/{}", remote_prefix, sync_manifest_path);
    let mut remote_manifest = SyncManifest::default();
    if remote_tree_files.contains_key(sync_manifest_path) {
        if let Some((content_bytes, _)) =
            github_get_content(transport, api_base, token, branch, &remote_manifest_path)?
        {
            remote_manifest =
                serde_json::from_slice::<SyncManifest>(&content_bytes).map_err(|e| {
                    crate::Error::SyncGithubApiError {
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
pub(super) fn download_pending_take_remote(
    sync_root: &Path,
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
    remote_prefix: &str,
    pending_paths: &[String],
) -> crate::Result<Vec<(String, Option<Vec<u8>>)>> {
    let download_pool = sync_download_pool(pending_paths.len())?;
    download_pool.install(|| {
        pending_paths
            .par_iter()
            .map(|path| {
                let remote_path = format!("{}/{}", remote_prefix, path);
                let remote = github_get_content(transport, api_base, token, branch, &remote_path)?;
                let Some((content, _sha)) = remote else {
                    return Ok((path.clone(), None));
                };

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
pub(super) fn download_remote_files(
    sync_root: &Path,
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
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
            let Some((content, _sha)) =
                github_get_content(transport, api_base, token, branch, &remote_path)?
            else {
                return Err(crate::Error::SyncGithubApiError {
                    category: "api_error".to_string(),
                    context: format!("remote file missing while downloading {}", path),
                    status: 0,
                    body_preview: String::new(),
                });
            };
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
/// GitHub API 要求 serial PUT 以避免 SHA 冲突，每个 PUT 需要携带远端文件的当前 SHA（若存在），
/// 实现幂等的 create-or-update。
pub(super) fn upload_local_files(
    sync_root: &Path,
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
    remote_prefix: &str,
    to_upload: &[String],
    remote_tree_files: &HashMap<String, String>,
) -> crate::Result<()> {
    for path in to_upload {
        let full_path = sync_root.join(path);
        if !full_path.exists() {
            continue;
        }
        let content = std::fs::read(&full_path).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!("read {}: {}", path, e)))
        })?;
        let remote_path = format!("{}/{}", remote_prefix, path);
        github_put_content_serial(
            transport,
            api_base,
            token,
            branch,
            &remote_path,
            &content,
            remote_tree_files.get(path.as_str()).cloned(),
        )?;
    }
    Ok(())
}

/// 串行删除远端文件。
pub(super) fn delete_remote_files(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
    remote_prefix: &str,
    paths: &[String],
    remote_tree_files: &HashMap<String, String>,
) -> crate::Result<()> {
    for path in paths {
        let remote_path = format!("{}/{}", remote_prefix, path);
        github_delete_content_serial(
            transport,
            api_base,
            token,
            branch,
            &remote_path,
            remote_tree_files.get(path.as_str()).cloned(),
        )?;
    }
    Ok(())
}

/// 上传合并后的 manifest 到远端。
pub(super) fn upload_manifest(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
    remote_manifest_path: &str,
    manifest_json: &str,
    remote_tree_files: &HashMap<String, String>,
) -> crate::Result<()> {
    let sync_manifest_path = super::manifest::SYNC_MANIFEST_PATH;
    github_put_content_serial(
        transport,
        api_base,
        token,
        branch,
        remote_manifest_path,
        manifest_json.as_bytes(),
        remote_tree_files.get(sync_manifest_path).cloned(),
    )
}
