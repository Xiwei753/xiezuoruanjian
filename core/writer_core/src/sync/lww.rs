//! LWW (Last-Writer-Wins) 同步策略实现。
//!
//! 本模块实现基于 GitHub API 的文件级同步，不依赖 Git 本地仓库。
//! 与 `service.rs` 中的 Git 同步路径（依赖 git2 crate，需 `git-https` feature）并行存在，
//! 两者目的相同但传输和冲突检测方式不同：
//!
//! | 维度         | LWW 路径（本模块）                    | Git 路径（service.rs）          |
//! |-------------|--------------------------------------|-------------------------------|
//! | 传输方式     | GitHub REST API 直接读写文件           | git2 clone/pull/push          |
//! | 冲突检测     | 三路比较（UserTextDocument）+ LWW 时间戳（Metadata/GeneratedCache） | dry-run checkout + index diff |
//! | 清单文件     | `app-meta/sync/manifest.sync.json`    | Git index                     |
//! | feature 门控 | 无（始终可用）                         | `git-https`                   |
//!
//! ## 核心不变量
//!
//! - `manifest.sync.json` 是本地文件状态的唯一事实来源，记录每个路径的 content_hash、op、updated_at_ms。
//! - 三路比较仅用于 `UserTextDocument`（正文、大纲等）；`Metadata`/`GeneratedCache` 走 LWW 时间戳决胜。
//! - LWW 决胜规则：时间戳较大方获胜；时间戳相同时按 device_id 字典序决胜（保证双方独立计算结果一致）。
//! - 远端删除的文件移至 `app-meta/sync/trash/` 而非直接删除，防止同步异常导致数据丢失。
//! - 下载使用 atomic rename（先写 .tmp 再 rename），保证中断不会留下半写入文件。

use crate::sync::github_api_client::{
    github_delete_content_serial, github_get_content, github_put_content_serial,
};
use crate::sync::scanner::scan_for_sync;
use crate::sync::types::{
    FirstSyncMode, ManifestFileRecord, SyncConfig, SyncConflict, SyncKind, SyncManifest,
    SyncResult, SyncSecrets, SyncState, SyncStatus,
};
use crate::sync::SyncService;
use rayon::prelude::*;
use std::path::Path;
use writer_platform_api::{HttpRequest, SyncTransport};

/// 同步清单文件路径——记录本地所有文件的哈希、操作类型和时间戳。
/// 这是 LWW 同步的唯一事实来源：三路比较的 base_hash 即从此文件读取。
const SYNC_MANIFEST_PATH: &str = "app-meta/sync/manifest.sync.json";
/// 并行下载最大线程数。4 线程在 GitHub API 速率限制和本地磁盘 I/O 之间取得平衡。
const MAX_PARALLEL_DOWNLOADS: usize = 4;

/// 创建并行下载线程池。线程数取 `task_count` 和 `MAX_PARALLEL_DOWNLOADS` 的较小值，
/// 至少 1 线程。使用 rayon 的 work-stealing 调度器。
fn sync_download_pool(task_count: usize) -> crate::Result<rayon::ThreadPool> {
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
fn save_conflict_copy(
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

/// 获取 LWW 比较时间戳。
///
/// 对于 delete 操作，优先使用 `deleted_at_ms`（精确的删除时间），
/// 回退到 `updated_at_ms`（删除操作记录的更新时间）。
/// 对于 upsert 操作，直接使用 `updated_at_ms`。
fn lww_record_time(record: &ManifestFileRecord) -> i64 {
    if record.op == "delete" {
        record.deleted_at_ms.unwrap_or(record.updated_at_ms)
    } else {
        record.updated_at_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::types::ManifestFileRecord;

    #[test]
    fn test_lww_record_time_non_delete() {
        let record = ManifestFileRecord {
            path: "test.md".to_string(),
            content_hash: "hash".to_string(),
            op: "upsert".to_string(),
            updated_at_ms: 1000,
            deleted_at_ms: Some(2000), // Should be ignored
            device_id: "dev1".to_string(),
            schema_version: 1,
        };
        assert_eq!(lww_record_time(&record), 1000);
    }

    #[test]
    fn test_lww_record_time_delete_with_deleted_at_ms() {
        let record = ManifestFileRecord {
            path: "test.md".to_string(),
            content_hash: "hash".to_string(),
            op: "delete".to_string(),
            updated_at_ms: 1000,
            deleted_at_ms: Some(2000),
            device_id: "dev1".to_string(),
            schema_version: 1,
        };
        assert_eq!(lww_record_time(&record), 2000);
    }

    #[test]
    fn test_lww_record_time_delete_without_deleted_at_ms() {
        let record = ManifestFileRecord {
            path: "test.md".to_string(),
            content_hash: "hash".to_string(),
            op: "delete".to_string(),
            updated_at_ms: 1000,
            deleted_at_ms: None,
            device_id: "dev1".to_string(),
            schema_version: 1,
        };
        assert_eq!(lww_record_time(&record), 1000); // Fallback to updated_at_ms
    }

    #[test]
    fn test_lww_record_time_tie_breaker_with_deleted_at_ms() {
        // Local is a newer edit based on updated_at_ms
        let local_rec = ManifestFileRecord {
            path: "a.txt".to_string(),
            content_hash: "hash1".to_string(),
            op: "upsert".to_string(),
            updated_at_ms: 1500,
            deleted_at_ms: None,
            device_id: "dev1".to_string(),
            schema_version: 1,
        };
        // Remote is a delete. updated_at_ms is older, but deleted_at_ms is newer
        let remote_rec = ManifestFileRecord {
            path: "a.txt".to_string(),
            content_hash: "hash1".to_string(),
            op: "delete".to_string(),
            updated_at_ms: 1000,
            deleted_at_ms: Some(2000),
            device_id: "dev2".to_string(),
            schema_version: 1,
        };

        let local_time = lww_record_time(&local_rec);
        let remote_time = lww_record_time(&remote_rec);

        // Remote time should be 2000 (from deleted_at_ms) and win against local's 1500
        assert!(remote_time > local_time);
        assert_eq!(remote_time, 2000);
    }
}

/// 内容分类 — 决定同步策略。
///
/// - UserTextDocument：用户创作的文本（章节正文、笔记等），走三路比较，
///   BothChanged 时记录冲突，不静默覆盖。
/// - Metadata：项目/卷/章元数据 JSON，走 LWW 或逐键语义合并。
/// - LocalOnly：本地专用数据（备份、app-meta 内部文件），不同步。
/// - GeneratedCache：生成/缓存数据，LWW 可接受。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentClass {
    /// User-authored text: chapter.md, note.md, outline.md, scene.md, etc.
    /// Three-way merge on sync; never silently overwritten by LWW.
    UserTextDocument,
    /// Project/volume/chapter metadata JSON. LWW or semantic merge.
    Metadata,
    /// Local-only data (backups, app-meta internals). Never synced.
    LocalOnly,
    /// Generated or cache data. LWW is acceptable.
    GeneratedCache,
}

/// Classify a workspace-relative path into a content category.
///
/// Uses suffix-based rules so it works for any project/volume/chapter ID.
/// The path is normalized to forward slashes before matching to ensure
/// correct behavior on Windows where local paths may contain backslashes.
pub(crate) fn classify_content_path(raw_path: &str) -> ContentClass {
    // Normalize backslashes to forward slashes for consistent matching on Windows
    let path = if raw_path.contains('\\') {
        std::borrow::Cow::Owned(raw_path.replace('\\', "/"))
    } else {
        std::borrow::Cow::Borrowed(raw_path)
    };

    // Local-only directories
    if path.starts_with("backups/") || path.starts_with("app-meta/") {
        return ContentClass::LocalOnly;
    }

    // User text documents: any .md file under /chapters/, plus
    // note.md, outline.md, scene.md, character_notes.md, timeline_notes.md
    // anywhere in the workspace
    if path.ends_with(".md") {
        if path.contains("/chapters/") {
            return ContentClass::UserTextDocument;
        }
        let filename = path.rsplit('/').next().unwrap_or(&path);
        if matches!(
            filename,
            "note.md"
                | "outline.md"
                | "scene.md"
                | "character_notes.md"
                | "timeline_notes.md"
                | "draft.md"
        ) {
            return ContentClass::UserTextDocument;
        }
        return ContentClass::GeneratedCache;
    }

    // Metadata JSON files
    if path.ends_with(".json") {
        let filename = path.rsplit('/').next().unwrap_or(&path);
        if matches!(
            filename,
            "project.json"
                | "volume.json"
                | "chapter.meta.json"
                | "settings.sync.json"
                | "starmap.json"
                | "writing_stats.json"
        ) {
            return ContentClass::Metadata;
        }
    }

    ContentClass::GeneratedCache
}

/// `classify_content_path == UserTextDocument` 的快捷判断。
/// 用于在同步流程中快速识别需要走三路比较的正文类文件。
pub(crate) fn is_document_content_path(path: &str) -> bool {
    classify_content_path(path) == ContentClass::UserTextDocument
}

/// 基于内容哈希的三路比较。
///
/// 以 `base_hash` 作为双方上次同步后的共识版本，比较 local 和 remote
/// 各自是否相对 base 发生了变化。用于 UserTextDocument 类型的冲突检测：
/// 仅一方修改时直接取修改方；双方都修改时返回 BothChanged，需走冲突解决流程。
///
/// 不变量：
/// - local_hash == remote_hash 时一定返回 NoConflict（即使两者都 != base），
///   因为内容相同无需选择。
/// - 三路比较仅用于 UserTextDocument；Metadata/GeneratedCache 走 LWW 时间戳决胜。
/// - LWW 决胜不变量：时间戳较大方获胜；时间戳相同时按 device_id 字典序决胜
///   （字典序较大的 device_id 获胜），保证双方独立计算结果一致。
fn three_way_resolve(base_hash: &str, local_hash: &str, remote_hash: &str) -> ThreeWayResult {
    if local_hash == remote_hash {
        return ThreeWayResult::NoConflict;
    }
    if local_hash == base_hash && remote_hash != base_hash {
        return ThreeWayResult::RemoteChanged;
    }
    if local_hash != base_hash && remote_hash == base_hash {
        return ThreeWayResult::LocalChanged;
    }
    if local_hash != base_hash && remote_hash != base_hash {
        return ThreeWayResult::BothChanged;
    }
    ThreeWayResult::NoConflict
}

/// 基于内容哈希的三路比较结果。
///
/// base 是上次同步后双方共识的文件版本哈希。
/// 通过比较 local/remote 与 base 的差异判断冲突情况。
enum ThreeWayResult {
    /// 双方相同，或双方均未修改
    NoConflict,
    /// 仅本地修改，远端未变 → 上传本地版本
    LocalChanged,
    /// 仅远端修改，本地未变 → 下载远端版本
    RemoteChanged,
    /// 双方均修改 → 需要冲突解决策略
    BothChanged,
}

/// 执行 LWW 同步 — 入口函数。
///
/// 整体流程：前置检查 → debounce → 重试循环（最多 2 次）→ 错误分类。
///
/// 重试策略：最多重试 2 次，间隔 500ms。仅对可恢复错误（网络/限流）重试；
/// 认证/权限等不可恢复错误直接返回，不重试。
///
/// 错误分类（`SyncErrorCategory`）：
/// - `LocalIoError` → Error（不可恢复）
/// - `TokenMissing/TokenInvalid/TokenPermissionDenied/AuthError` → Error（不可恢复）
/// - `ApiRateLimited` → RecoverableError（可恢复，下次同步自动重试）
/// - `GithubNetworkFailed/DnsFailed/TlsFailed/NetworkProbeFailed` → RecoverableError
/// - 其他 → RecoverableError（保守处理，避免误报不可恢复）
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub(crate) fn perform_lww_sync(
    sync_root: &Path,
    config: &SyncConfig,
    secrets: &SyncSecrets,
    force_sync: bool,
    transport: &dyn SyncTransport,
) -> crate::Result<SyncResult> {
    log::debug!(
        "[sync] backend_type=github_api sync_mode=lww_manifest entry=perform_lww_sync workspace={}",
        sync_root.display()
    );
    let mut result = SyncResult::success();
    result.status = SyncStatus::Idle;

    if !config.enabled {
        result.status = SyncStatus::Success;
        return Ok(result);
    }

    if config.remote_url.is_empty() {
        return Ok(SyncResult::error(
            SyncStatus::Error("Remote URL is empty".to_string()),
            FirstSyncMode::NotAttempted,
            "Remote URL is empty".to_string(),
            Some("empty_url".to_string()),
        ));
    }

    let token = secrets.token.clone().unwrap_or_default();
    if token.is_empty() {
        return Ok(SyncResult::error(
            SyncStatus::Error("No token provided".to_string()),
            FirstSyncMode::NotAttempted,
            "No token provided".to_string(),
            Some("token_missing".to_string()),
        ));
    }

    let mut state = crate::sync::SyncService::load_sync_state(sync_root)?;
    if state.device_id.is_empty() {
        state.device_id = uuid::Uuid::new_v4().to_string();
        crate::sync::SyncService::save_sync_state(sync_root, &state)?;
    }

    // P1-4: Core-level debounce. Even if clients call sync too often,
    // the core enforces a minimum interval to prevent network I/O flood.
    // This is a safety net; clients should also debounce.
    // However, force_sync=true bypasses this debounce for manual sync,
    // conflict resolution, and first configuration.
    if !force_sync {
        let min_interval = i64::from(config.sync_interval_seconds.max(60));
        if let Some(last_sync) = state.last_sync_time {
            let now = chrono::Utc::now().timestamp();
            let elapsed = now - last_sync;
            if elapsed >= 0 && elapsed < min_interval {
                // 冲突解决后（pending_take_remote 非空）必须绕过 debounce，
                // 否则用户解决冲突后可能要等 60 秒才能同步到远端内容
                if !state.pending_take_remote.is_empty() {
                    log::debug!(
                        "[sync] debounce bypassed: pending_take_remote has {} entries",
                        state.pending_take_remote.len()
                    );
                } else {
                    log::debug!(
                        "[sync] debounce: last_sync={}s ago, min_interval={}s, skipping",
                        elapsed,
                        min_interval
                    );
                    result.status = SyncStatus::Success;
                    return Ok(result);
                }
            }
        }
    }

    let api_base = crate::sync::github_backend::GitHubApiBackend::api_base_url(&config.remote_url);

    let max_retries = 2;
    let mut attempt = 0;
    loop {
        match execute_lww_sync_attempt(
            sync_root,
            config,
            &token,
            &api_base,
            transport,
            &mut state,
            &mut result,
        ) {
            Ok(res) => return Ok(res),
            Err(e) => {
                attempt += 1;
                if attempt >= max_retries {
                    let err = e.to_string();
                    let category =
                        crate::sync::types::SyncErrorCategory::from_code(e.sync_category(), &err);
                    result.status = match category {
                        crate::sync::types::SyncErrorCategory::LocalIoError => {
                            SyncStatus::Error("local_io_error".to_string())
                        }
                        crate::sync::types::SyncErrorCategory::TokenMissing
                        | crate::sync::types::SyncErrorCategory::TokenInvalid
                        | crate::sync::types::SyncErrorCategory::TokenPermissionDenied
                        | crate::sync::types::SyncErrorCategory::AuthError => {
                            SyncStatus::Error(category.to_ui_status().to_string())
                        }
                        crate::sync::types::SyncErrorCategory::ApiRateLimited => {
                            SyncStatus::RecoverableError("api_rate_limited".to_string())
                        }
                        crate::sync::types::SyncErrorCategory::GithubNetworkFailed
                        | crate::sync::types::SyncErrorCategory::DnsFailed
                        | crate::sync::types::SyncErrorCategory::TlsFailed
                        | crate::sync::types::SyncErrorCategory::NetworkProbeFailed => {
                            SyncStatus::RecoverableError("network_error".to_string())
                        }
                        _ => SyncStatus::RecoverableError("api_error".to_string()),
                    };
                    result.error = Some(err.clone());
                    return Ok(result);
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
}

/// 执行一次 LWW 同步尝试。
///
/// 整体流程：
/// 1. 拉取远端 Git tree 和 manifest，诊断 404（空仓库 vs 权限不足 vs 分支不存在）
/// 2. 扫描本地工作区，构建 local_records（含 upsert 和 delete 墓碑）
/// 3. 处理 pending_take_remote：强制下载远端内容覆盖本地，不进入三路比较
/// 4. 逐路径三路/LWW 比较：
///    - UserTextDocument 走三路比较，BothChanged 时记录冲突
///    - Metadata/GeneratedCache 走 LWW 时间戳决胜，平局时按 device_id 字典序
///    - unresolved_conflict_paths 跳过，等待用户解决
/// 5. 下载远端较新文件、上传本地较新文件、删除本地文件（移至 trash）
/// 6. 写入合并后的 manifest、持久化 sync state
///
/// 调用方 `perform_lww_sync` 负责重试（最多 2 次）和错误分类。
#[allow(clippy::cast_possible_truncation)]
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
fn execute_lww_sync_attempt(
    sync_root: &Path,
    config: &SyncConfig,
    token: &str,
    api_base: &str,
    transport: &dyn SyncTransport,
    state: &mut SyncState,
    result: &mut SyncResult,
) -> crate::Result<SyncResult> {
    log::debug!("[sync] github_api step=正在拉取远端清单");
    let tree_url = format!("{}/git/trees/{}?recursive=1", api_base, config.branch);
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

    let mut remote_tree_files = std::collections::HashMap::new();
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
            for item in tree {
                if item["type"].as_str() == Some("blob") {
                    if let (Some(path), Some(sha)) = (item["path"].as_str(), item["sha"].as_str()) {
                        remote_tree_files.insert(path.to_string(), sha.to_string());
                    }
                }
            }
        }
    } else if tree_status == 404 {
        let ref_url = format!("{}/git/ref/heads/{}", api_base, config.branch);
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
                config.branch
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
                    detail: format!("branch '{}' not found in repository", config.branch),
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

    let mut remote_manifest = SyncManifest::default();
    if remote_tree_files.contains_key(SYNC_MANIFEST_PATH) {
        if let Some((content_bytes, _)) = github_get_content(
            transport,
            api_base,
            token,
            &config.branch,
            SYNC_MANIFEST_PATH,
        )? {
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

    log::debug!("[sync] github_api step=正在比较本地和远端");
    let local_entries = scan_for_sync(sync_root)?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut local_records = std::collections::HashMap::new();

    // ── 构建本地文件记录 ──
    // updated_at_ms 的确定策略：
    //   1. 已知文件且哈希未变 → 使用上次同步记录的时间戳（避免文件系统 mtime 精度丢失）
    //   2. 已知文件但哈希已变 → 使用文件系统 mtime（反映实际修改时间）
    //   3. 新文件（不在 known_files 中）→ 使用文件系统 mtime
    //   4. mtime 读取失败 → 退回 now_ms（保守取当前时间，确保不被远端旧版本覆盖）
    for entry in &local_entries {
        if entry.sync_kind == SyncKind::Upload && entry.relative_path != SYNC_MANIFEST_PATH {
            let path = entry.relative_path.clone();
            let local_hash = entry.file_hash.clone();

            let updated_at_ms;
            let op = "upsert".to_string();

            if let Some(known_hash) = state.known_files.get(&path) {
                if *known_hash == local_hash {
                    // 哈希未变，沿用已知时间戳
                    updated_at_ms = state
                        .known_files_updated_at
                        .get(&path)
                        .cloned()
                        .unwrap_or(0);
                } else {
                    // 哈希已变，取文件系统修改时间
                    let modified_ms = std::fs::metadata(sync_root.join(&path))
                        .and_then(|m| m.modified())
                        .and_then(|t| {
                            t.duration_since(std::time::SystemTime::UNIX_EPOCH)
                                .map_err(std::io::Error::other)
                        })
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(now_ms);
                    updated_at_ms = modified_ms;
                }
            } else {
                // 新文件，取文件系统修改时间
                let modified_ms = std::fs::metadata(sync_root.join(&path))
                    .and_then(|m| m.modified())
                    .and_then(|t| {
                        t.duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .map_err(std::io::Error::other)
                    })
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(now_ms);
                updated_at_ms = modified_ms;
            }

            local_records.insert(
                path.clone(),
                ManifestFileRecord {
                    path,
                    content_hash: local_hash,
                    updated_at_ms,
                    deleted_at_ms: None,
                    device_id: state.device_id.clone(),
                    op,
                    schema_version: 1,
                },
            );
        }
    }

    // ── 构建本地删除墓碑记录 ──
    // known_files 中存在但本地文件已不存在的路径，生成 delete 墓碑。
    // 墓碑的 updated_at_ms 优先取 tombstone 记录的 deleted_at（秒级→毫秒级），
    // 否则使用当前时间。content_hash 为空，因为文件已不存在。
    for path in state.known_files.keys() {
        if !local_records.contains_key(path) {
            if !SyncService::is_whitelisted_path(path) || SyncService::is_blacklisted_path(path) {
                continue;
            }
            if !sync_root.join(path).exists() {
                let mut updated_at_ms = now_ms;
                if let Some(tombstone) = state.tombstones.iter().find(|t| t.original_path == *path)
                {
                    updated_at_ms = tombstone.deleted_at * 1000;
                }

                local_records.insert(
                    path.clone(),
                    ManifestFileRecord {
                        path: path.clone(),
                        content_hash: String::new(),
                        updated_at_ms,
                        deleted_at_ms: Some(updated_at_ms),
                        device_id: state.device_id.clone(),
                        op: "delete".to_string(),
                        schema_version: 1,
                    },
                );
            }
        }
    }

    let mut remote_records = std::collections::HashMap::new();
    for rec in remote_manifest.files {
        if rec.path != SYNC_MANIFEST_PATH {
            remote_records.insert(rec.path.clone(), rec);
        }
    }

    // 远端 tree 中存在但 manifest 中无记录的文件（首次同步或 manifest 损失），
    // 用 tree SHA 作为 content_hash 补充记录，时间戳设为 0（最旧），
    // device_id 设为 "remote" 以避免与本地 device_id 冲突。
    for (path, sha) in &remote_tree_files {
        if path != SYNC_MANIFEST_PATH && !remote_records.contains_key(path) {
            if !SyncService::is_whitelisted_path(path) || SyncService::is_blacklisted_path(path) {
                continue;
            }
            remote_records.insert(
                path.clone(),
                ManifestFileRecord {
                    path: path.clone(),
                    content_hash: sha.clone(),
                    updated_at_ms: 0,
                    deleted_at_ms: None,
                    device_id: "remote".to_string(),
                    op: "upsert".to_string(),
                    schema_version: 1,
                },
            );
        }
    }

    // Build a quick-lookup set of unresolved conflict paths from the persisted state.
    // While a path remains in this set, the sync engine must not auto-upload,
    // auto-download, or apply LWW/three-way resolution to it.
    let unresolved_conflict_paths: std::collections::HashSet<String> =
        state.conflicted_files.clone();

    // ── Process pending_take_remote ──
    // For each path in pending_take_remote, force-download the remote content
    // to the local file, then update known_files to the new local hash.
    // This must happen BEFORE the three-way comparison loop so that the
    // downloaded content becomes the local version for the sync plan.
    //
    // CRITICAL: Regardless of whether the download succeeds or fails, the path
    // must NOT enter the normal three-way/LWW comparison loop. If it did, a
    // "local has, remote missing" scenario could cause the old local content to
    // be uploaded back, violating the "take remote" intent.
    let pending_take_remote_all_set: std::collections::HashSet<String> =
        state.pending_take_remote.clone();
    let mut pending_take_remote_downloaded: Vec<String> = Vec::new();
    let mut pending_take_remote_failed: Vec<String> = Vec::new();
    if !state.pending_take_remote.is_empty() {
        log::debug!(
            "[sync] processing pending_take_remote count={}",
            state.pending_take_remote.len()
        );
        let pending_paths: Vec<String> = state.pending_take_remote.iter().cloned().collect();
        let download_pool = sync_download_pool(pending_paths.len())?;
        let pending_results: crate::Result<Vec<_>> = download_pool.install(|| {
            pending_paths
                .par_iter()
                .map(|path| {
                    let remote =
                        github_get_content(transport, api_base, token, &config.branch, path)?;
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
                    let tmp_path =
                        full_path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
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
        });

        for (path, content) in pending_results? {
            if let Some(content) = content {
                let hash = format!("{:x}", md5::compute(&content));
                state.known_files.insert(path.clone(), hash);
                let now_ts = chrono::Utc::now().timestamp_millis();
                state.known_files_updated_at.insert(path.clone(), now_ts);
                pending_take_remote_downloaded.push(path.clone());
                log::debug!("[sync] pending_take_remote downloaded path={}", path);
            } else {
                log::debug!(
                    "[sync] pending_take_remote: remote file missing for path={}, keeping in pending",
                    path
                );
                pending_take_remote_failed.push(path);
            }
        }
        // Only clear paths that were successfully downloaded;
        // failed/missing paths remain in pending_take_remote so the user
        // is not silently left with stale local content.
        //
        // retain 语义：保留仍在 failed 集合中的路径（即下载失败/远端缺失的），
        // 成功下载的路径从 pending_take_remote 中移除，后续走正常合并流程。
        state
            .pending_take_remote
            .retain(|p| pending_take_remote_failed.contains(p));
    }

    let mut merged_manifest_files = std::collections::HashMap::new();
    let mut to_download = Vec::new();
    let mut to_upload = Vec::new();
    let mut to_delete_local = Vec::new();
    let mut local_deletes_count = Vec::new();
    let mut remote_deletes_count = Vec::new();
    let mut overwritten_files = Vec::new();
    let mut doc_conflicts: Vec<SyncConflict> = Vec::new();

    let all_paths: std::collections::HashSet<String> = local_records
        .keys()
        .cloned()
        .chain(remote_records.keys().cloned())
        .collect();

    for path in all_paths {
        // Skip ALL paths that were in pending_take_remote — both successfully
        // downloaded and failed/missing ones. A failed download must NOT fall
        // through to the normal three-way/LWW logic, which could upload the
        // old local content back (violating "take remote" intent).
        if pending_take_remote_all_set.contains(&path) {
            if pending_take_remote_downloaded.contains(&path) {
                // Successfully downloaded: use remote_rec in merged manifest
                if let Some(remote_rec) = remote_records.get(&path) {
                    merged_manifest_files.insert(path.clone(), remote_rec.clone());
                }
                result.ignored_files.push(path);
            } else {
                // Failed/missing: keep whichever record exists, but do NOT
                // schedule any upload/download/delete. The path remains in
                // pending_take_remote for the next sync attempt.
                if let Some(remote_rec) = remote_records.get(&path) {
                    merged_manifest_files.insert(path.clone(), remote_rec.clone());
                } else if let Some(local_rec) = local_records.get(&path) {
                    merged_manifest_files.insert(path.clone(), local_rec.clone());
                }
            }
            continue;
        }
        // Skip paths that have unresolved conflicts — do not auto-upload,
        // auto-download, or apply LWW/three-way resolution.
        if unresolved_conflict_paths.contains(&path) {
            log::debug!(
                "[sync] skipping unresolved_conflict path={} (awaiting user resolution)",
                path
            );
            // Keep the remote record in the merged manifest so the remote side
            // stays consistent, but do NOT schedule any upload/download/delete.
            if let Some(remote_rec) = remote_records.get(&path) {
                merged_manifest_files.insert(path.clone(), remote_rec.clone());
            } else if let Some(local_rec) = local_records.get(&path) {
                merged_manifest_files.insert(path.clone(), local_rec.clone());
            }
            continue;
        }

        let local_opt = local_records.get(&path);
        let remote_opt = remote_records.get(&path);

        match (local_opt, remote_opt) {
            (Some(local_rec), None) => {
                merged_manifest_files.insert(path.clone(), local_rec.clone());
                if local_rec.op == "upsert" {
                    to_upload.push(path);
                }
            }
            (None, Some(remote_rec)) => {
                merged_manifest_files.insert(path.clone(), remote_rec.clone());
                if remote_rec.op == "upsert" {
                    to_download.push(path);
                } else if remote_rec.op == "delete" {
                    to_delete_local.push(path.clone());
                    remote_deletes_count.push(path);
                }
            }
            (Some(local_rec), Some(remote_rec)) => {
                if is_document_content_path(&path) {
                    let base_hash = state
                        .known_files
                        .get(&path)
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    let local_hash = &local_rec.content_hash;
                    let remote_hash = &remote_rec.content_hash;

                    match three_way_resolve(base_hash, local_hash, remote_hash) {
                        ThreeWayResult::NoConflict => {
                            merged_manifest_files.insert(path.clone(), local_rec.clone());
                            result.ignored_files.push(path);
                        }
                        ThreeWayResult::LocalChanged => {
                            merged_manifest_files.insert(path.clone(), local_rec.clone());
                            to_upload.push(path);
                        }
                        ThreeWayResult::RemoteChanged => {
                            merged_manifest_files.insert(path.clone(), remote_rec.clone());
                            if remote_rec.op == "upsert" {
                                to_download.push(path);
                            } else if remote_rec.op == "delete" {
                                to_delete_local.push(path.clone());
                                remote_deletes_count.push(path);
                            }
                        }
                        ThreeWayResult::BothChanged => {
                            log::warn!(
                                "[sync] document_conflict path={} local_hash={} remote_hash={} base_hash={}",
                                path, local_hash, remote_hash, base_hash
                            );

                            let conflict = if remote_rec.op == "upsert" {
                                if let Some((remote_content, _)) = github_get_content(
                                    transport,
                                    api_base,
                                    token,
                                    &config.branch,
                                    &path,
                                )? {
                                    let conflict_filename =
                                        save_conflict_copy(sync_root, &path, &remote_content)?;

                                    Some(SyncConflict {
                                        local_path: path.clone(),
                                        remote_path: path.clone(),
                                        local_hash: local_hash.clone(),
                                        remote_hash: remote_hash.clone(),
                                        base_hash: base_hash.to_string(),
                                        created_at: chrono::Utc::now().timestamp(),
                                        description: format!(
                                            "正文文件双端修改冲突。本地修改和远端修改均保留。远端副本: {}",
                                            conflict_filename
                                        ),
                                    })
                                } else {
                                    None
                                }
                            } else if remote_rec.op == "delete" {
                                Some(SyncConflict {
                                    local_path: path.clone(),
                                    remote_path: path.clone(),
                                    local_hash: local_hash.clone(),
                                    remote_hash: remote_hash.clone(),
                                    base_hash: base_hash.to_string(),
                                    created_at: chrono::Utc::now().timestamp(),
                                    description:
                                        "正文文件冲突：本地已修改，远端已删除。保留本地文件。"
                                            .to_string(),
                                })
                            } else {
                                None
                            };

                            if let Some(conflict) = &conflict {
                                doc_conflicts.push(conflict.clone());
                                // Record the path as having an unresolved conflict so that
                                // subsequent syncs skip it until the user explicitly resolves.
                                state.conflicted_files.insert(path.clone());
                                // Also persist the conflict record in state.conflicts so that
                                // resolve_conflict_keep_local / take_remote / mark_merged can
                                // look up the remote_hash without needing a separate query.
                                state.conflicts.push(conflict.clone());
                            }
                            // Keep remote_rec in manifest so the remote side stays consistent.
                            // Do NOT update known_files[path] — it must remain at base_hash
                            // so that three-way comparison on the next sync still sees
                            // base=base, local≠base, remote≠base → BothChanged (or the
                            // unresolved_conflict_paths guard catches it first).
                            merged_manifest_files.insert(path.clone(), remote_rec.clone());
                        }
                    }
                } else {
                    // ── LWW 时间戳决胜（Metadata/GeneratedCache） ──
                    // 时间戳较大者获胜。时间戳相同时：
                    //   - 哈希和操作均相同 → 无实际冲突，忽略
                    //   - 否则按 device_id 字典序决胜（确定性，无需用户干预）
                    //
                    // 不变量：LWW 决胜仅适用于 Metadata 和 GeneratedCache，
                    // UserTextDocument 必须走三路比较，不得静默覆盖。
                    let local_time = lww_record_time(local_rec);
                    let remote_time = lww_record_time(remote_rec);
                    let mut remote_wins = false;
                    if remote_time > local_time {
                        remote_wins = true;
                    } else if remote_time == local_time {
                        if remote_rec.content_hash == local_rec.content_hash
                            && remote_rec.op == local_rec.op
                        {
                            merged_manifest_files.insert(path.clone(), local_rec.clone());
                            result.ignored_files.push(path);
                            continue;
                        }
                        remote_wins = remote_rec.device_id > local_rec.device_id;
                        log::debug!(
                            "[sync] lww_tie_breaker path={} winner={} local_device={} remote_device={}",
                            path,
                            if remote_wins { "remote" } else { "local" },
                            local_rec.device_id,
                            remote_rec.device_id
                        );
                    }

                    if remote_wins {
                        merged_manifest_files.insert(path.clone(), remote_rec.clone());
                        if remote_rec.op == "upsert" {
                            if local_rec.op == "delete"
                                || local_rec.content_hash != remote_rec.content_hash
                            {
                                overwritten_files.push(path.clone());
                                to_download.push(path);
                            }
                        } else if remote_rec.op == "delete" {
                            if local_rec.op == "upsert" {
                                overwritten_files.push(path.clone());
                            }
                            to_delete_local.push(path.clone());
                            remote_deletes_count.push(path);
                        }
                    } else {
                        merged_manifest_files.insert(path.clone(), local_rec.clone());
                        if local_rec.op == "upsert" {
                            if remote_rec.op == "delete"
                                || remote_rec.content_hash != local_rec.content_hash
                            {
                                overwritten_files.push(path.clone());
                                to_upload.push(path);
                            }
                        } else if local_rec.op == "delete" {
                            if remote_rec.op == "upsert" {
                                overwritten_files.push(path.clone());
                            }
                            local_deletes_count.push(path);
                        }
                    }
                }
            }
            (None, None) => {}
        }
    }

    for path in &to_delete_local {
        // 远端已删除的文件移至 trash 目录而非直接删除，
        // 防止同步异常时用户数据丢失。trash 文件名格式：
        // {timestamp}_{uuid}_{original_filename}
        //
        // 不变量：远端删除操作只在本地文件与远端记录一致（三路 NoConflict/RemoteChanged，
        // 或 LWW 远端获胜）时执行。冲突路径不会进入 to_delete_local。
        let full_path = sync_root.join(path);
        if full_path.exists() {
            let filename = full_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let trash_dir = sync_root.join("sync/trash");
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

    log::debug!("[sync] github_api step=download newer remote files");
    // 下载使用 rayon 并行线程池，线程数取 min(文件数, MAX_PARALLEL_DOWNLOADS)。
    // 每个文件先写入临时文件（带随机后缀），再 rename 替换目标文件，
    // 保证下载中断不会留下半写入文件。
    if !to_download.is_empty() {
        let download_pool = sync_download_pool(to_download.len())?;
        let download_result: crate::Result<()> = download_pool.install(|| {
            to_download.par_iter().try_for_each(|path| {
                let Some((content, _sha)) =
                    github_get_content(transport, api_base, token, &config.branch, path)?
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
                std::fs::write(&tmp_path, content).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!("{}: {}", path, e)))
                })?;
                std::fs::rename(tmp_path, &full_path).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!("{}: {}", path, e)))
                })?;
                Ok(())
            })
        });
        download_result?;
    }

    // 清除超过 30 天的 delete 墓碑记录，避免 manifest 无限膨胀。
    // 墓碑保留 30 天是为了让远端设备有足够时间拉取删除信息。
    let purge_time = now_ms - 30 * 24 * 3600 * 1000;
    let mut manifest_files_vec: Vec<ManifestFileRecord> =
        merged_manifest_files.values().cloned().collect();
    manifest_files_vec.retain(|rec| rec.op != "delete" || lww_record_time(rec) > purge_time);
    manifest_files_vec.sort_by(|a, b| a.path.cmp(&b.path));

    let sync_manifest = SyncManifest {
        files: manifest_files_vec,
    };

    let manifest_json = serde_json::to_string_pretty(&sync_manifest).unwrap_or_default();
    let full_manifest_path = sync_root.join(SYNC_MANIFEST_PATH);
    if let Some(parent) = full_manifest_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| crate::Error::Io(std::io::Error::other(format!("manifest dir: {}", e))))?;
    }
    std::fs::write(&full_manifest_path, &manifest_json)
        .map_err(|e| crate::Error::Io(std::io::Error::other(format!("write manifest: {}", e))))?;

    log::debug!("[sync] github_api step=正在上传本地较新文件");
    // 上传串行执行（GitHub API 要求 serial PUT 以避免 SHA 冲突），
    // 每个 PUT 需要携带远端文件的当前 SHA（若存在），实现幂等的 create-or-update。
    for path in &to_upload {
        let full_path = sync_root.join(path);
        if !full_path.exists() {
            continue;
        }
        let content = std::fs::read(&full_path).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!("read {}: {}", path, e)))
        })?;
        github_put_content_serial(
            transport,
            api_base,
            token,
            &config.branch,
            path,
            &content,
            remote_tree_files.get(path).cloned(),
        )?;
    }

    for path in &local_deletes_count {
        github_delete_content_serial(
            transport,
            api_base,
            token,
            &config.branch,
            path,
            remote_tree_files.get(path).cloned(),
        )?;
    }

    github_put_content_serial(
        transport,
        api_base,
        token,
        &config.branch,
        SYNC_MANIFEST_PATH,
        manifest_json.as_bytes(),
        remote_tree_files.get(SYNC_MANIFEST_PATH).cloned(),
    )?;

    for conflict in &doc_conflicts {
        let _ = SyncService::record_sync_conflict(sync_root, conflict.clone(), None);
    }

    state.last_sync_time = Some(chrono::Utc::now().timestamp());
    state.last_synced_commit = None;
    state.last_error = None;

    let post_local_entries = scan_for_sync(sync_root)?;

    // ── 同步后重建 known_files ──
    // 同步完成后重新扫描本地文件，用当前文件哈希更新 known_files。
    // 关键不变量：冲突路径的 known_files 必须保留在 base_hash（三路比较基准），
    // 否则下次同步时 known_files 会变成当前本地哈希，导致三路比较误判为 NoConflict
    // 或 LocalChanged，而非 BothChanged。
    // 因此：先保存冲突路径的 base_hash → 清空 known_files → 重建 → 恢复冲突路径。
    //
    // Before clearing known_files, save the base_hash values for conflicted
    // paths so we can restore them after the scan. The scan would otherwise
    // overwrite them with the current local file hash, which would break the
    // three-way comparison on the next sync.
    let conflicted_known_files: std::collections::HashMap<String, String> = state
        .conflicted_files
        .iter()
        .filter_map(|p| state.known_files.get(p).map(|v| (p.clone(), v.clone())))
        .collect();
    let conflicted_known_files_updated_at: std::collections::HashMap<String, i64> = state
        .conflicted_files
        .iter()
        .filter_map(|p| state.known_files_updated_at.get(p).map(|v| (p.clone(), *v)))
        .collect();

    state.known_files.clear();
    state.known_files_updated_at.clear();
    for entry in post_local_entries {
        if entry.sync_kind == SyncKind::Upload && entry.relative_path != SYNC_MANIFEST_PATH {
            // Do not let post-sync scan overwrite known_files for paths that
            // have unresolved conflicts — their known_files must stay at the
            // base_hash so three-way comparison keeps detecting BothChanged.
            if state.conflicted_files.contains(&entry.relative_path) {
                continue;
            }

            state
                .known_files
                .insert(entry.relative_path.clone(), entry.file_hash.clone());

            let matched_rec = merged_manifest_files.get(&entry.relative_path);
            let t = matched_rec.map(|r| r.updated_at_ms).unwrap_or_else(|| {
                std::fs::metadata(sync_root.join(&entry.relative_path))
                    .and_then(|m| m.modified())
                    .and_then(|time| {
                        time.duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .map_err(std::io::Error::other)
                    })
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(now_ms)
            });
            state.known_files_updated_at.insert(entry.relative_path, t);
        }
    }

    // Restore the base_hash values for conflicted paths.
    for (path, hash) in conflicted_known_files {
        state.known_files.insert(path, hash);
    }
    for (path, t) in conflicted_known_files_updated_at {
        state.known_files_updated_at.insert(path, t);
    }

    // NOTE: The old logic that set known_files[path] = remote_hash for
    // conflicted files has been removed. Conflicted paths keep their
    // known_files at base_hash, and the unresolved_conflict_paths guard
    // at the top of the sync loop prevents any auto-resolution.

    // 同步后清理过期墓碑（purge_after 已过期的条目）
    state
        .tombstones
        .retain(|t| t.purge_after > chrono::Utc::now().timestamp());

    crate::sync::SyncService::save_sync_state(sync_root, state)?;

    let has_doc_conflicts = !doc_conflicts.is_empty();
    let has_changes = !to_upload.is_empty()
        || !to_download.is_empty()
        || !pending_take_remote_downloaded.is_empty()
        || !local_deletes_count.is_empty()
        || !remote_deletes_count.is_empty();

    if has_doc_conflicts {
        result.status = SyncStatus::PartialConflict;
        result.conflicts = doc_conflicts;
    } else if !pending_take_remote_failed.is_empty() {
        result.status = SyncStatus::RecoverableError(format!(
            "pending_take_remote_failed: {}",
            pending_take_remote_failed.join(", ")
        ));
        result.error = Some(format!(
            "pending_take_remote: remote file missing for paths: {}",
            pending_take_remote_failed.join(", ")
        ));
    } else if has_changes {
        result.status = SyncStatus::LatestWinsApplied;
    } else {
        result.status = SyncStatus::NoChanges;
    }

    result.uploaded_files = to_upload;
    // Merge pending_take_remote downloads with regular downloads
    let mut all_downloaded = pending_take_remote_downloaded;
    all_downloaded.extend(to_download);
    result.downloaded_files = all_downloaded;
    result.local_deletes = local_deletes_count;
    result.remote_deletes = remote_deletes_count;
    result.overwritten_files = overwritten_files;
    result.commit_hash = None;
    result.first_sync_mode = FirstSyncMode::AlreadyGitRepo;

    log::debug!("[sync] github_api step=同步完成");
    Ok(result.clone())
}
