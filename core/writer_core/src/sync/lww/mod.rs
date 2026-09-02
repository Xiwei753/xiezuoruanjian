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

use crate::sync::github_api_client::github_get_content;
use crate::sync::scanner::scan_for_sync;
use crate::sync::types::{
    FirstSyncMode, ManifestFileRecord, SyncConfig, SyncConflict, SyncKind, SyncManifest,
    SyncResult, SyncSecrets, SyncState, SyncStatus,
};
use crate::sync::SyncService;
use std::path::Path;
use writer_platform_api::SyncTransport;

mod compare;
mod manifest;
mod transfer;

// #644 评论 5462823517 第3节：从 lww.rs 抽出的子模块，保持 pub/pub(crate) 接口不变。
// #644 评论 5473789298 第3节：纯分类/比较提升为 sync::content_class（始终可用），
// 这里 re-export 保持原 lww.rs 的 pub(crate) 接口，让旧测试 `crate::sync::lww::*` 仍可用。
#[allow(unused_imports)]
pub(crate) use crate::sync::content_class::{
    classify_content_path, is_document_content_path, ContentClass,
};
use compare::{resolve_path_decision, PathDecision};
use manifest::{build_local_records, build_remote_records, lww_record_time, SYNC_MANIFEST_PATH};
use transfer::{
    delete_remote_files, download_pending_take_remote, download_remote_files,
    fetch_remote_manifest, fetch_remote_tree, move_to_trash, save_conflict_copy,
    upload_local_files, upload_manifest,
};

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
    target: &crate::sync::types::SyncTarget,
    force_sync: bool,
    transport: &dyn SyncTransport,
) -> crate::Result<SyncResult> {
    let remote_prefix = &target.remote_prefix;
    log::debug!(
        "[sync] backend_type=github_api sync_mode=lww_manifest entry=perform_lww_sync sync_root={} remote_prefix={}",
        sync_root.display(),
        remote_prefix
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
            target,
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
/// 2. 扫描本地作品目录，构建 local_records（含 upsert 和 delete 墓碑）
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
    target: &crate::sync::types::SyncTarget,
    transport: &dyn SyncTransport,
    state: &mut SyncState,
    result: &mut SyncResult,
) -> crate::Result<SyncResult> {
    let remote_prefix = &target.remote_prefix;
    let scope = target.scope;
    log::debug!(
        "[sync] github_api step=正在拉取远端清单 remote_prefix={}",
        remote_prefix
    );
    let remote_tree_files =
        fetch_remote_tree(transport, api_base, token, &config.branch, remote_prefix)?;

    let remote_manifest_path = format!("{}/{}", remote_prefix, SYNC_MANIFEST_PATH);
    let remote_manifest = fetch_remote_manifest(
        transport,
        api_base,
        token,
        &config.branch,
        remote_prefix,
        &remote_tree_files,
    )?;

    log::debug!("[sync] github_api step=正在比较本地和远端");
    let local_entries = scan_for_sync(sync_root, scope)?;
    let now_ms = chrono::Utc::now().timestamp_millis();

    // #644 评论 5473105049 第5节：local/remote record 构造委托给 manifest.rs。
    let local_records = build_local_records(sync_root, &local_entries, state, scope, now_ms);
    let remote_records = build_remote_records(remote_manifest, &remote_tree_files, scope);

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
        let pending_results = download_pending_take_remote(
            sync_root,
            transport,
            api_base,
            token,
            &config.branch,
            remote_prefix,
            &pending_paths,
        )?;

        for (path, content) in pending_results {
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
                // #644 评论 5473105049 第5节：三方/LWW 决策委托给 compare.rs。
                let base_hash = state
                    .known_files
                    .get(&path)
                    .map(|s| s.as_str())
                    .unwrap_or("");
                let is_document = is_document_content_path(&path);
                let (decision, overwritten) =
                    resolve_path_decision(local_rec, remote_rec, base_hash, is_document);

                if overwritten {
                    overwritten_files.push(path.clone());
                }

                match decision {
                    PathDecision::NoOp => {
                        merged_manifest_files.insert(path.clone(), local_rec.clone());
                        result.ignored_files.push(path);
                    }
                    PathDecision::UploadLocal => {
                        merged_manifest_files.insert(path.clone(), local_rec.clone());
                        to_upload.push(path);
                    }
                    PathDecision::DownloadRemote => {
                        merged_manifest_files.insert(path.clone(), remote_rec.clone());
                        to_download.push(path);
                    }
                    PathDecision::DeleteLocal => {
                        merged_manifest_files.insert(path.clone(), remote_rec.clone());
                        to_delete_local.push(path.clone());
                        remote_deletes_count.push(path);
                    }
                    PathDecision::LwwRemoteWinsDownload => {
                        merged_manifest_files.insert(path.clone(), remote_rec.clone());
                        to_download.push(path);
                    }
                    PathDecision::LwwRemoteWinsDelete => {
                        merged_manifest_files.insert(path.clone(), remote_rec.clone());
                        to_delete_local.push(path.clone());
                        remote_deletes_count.push(path);
                    }
                    PathDecision::LwwLocalWinsUpload => {
                        merged_manifest_files.insert(path.clone(), local_rec.clone());
                        to_upload.push(path);
                    }
                    PathDecision::LwwLocalWinsDeleteRecord => {
                        merged_manifest_files.insert(path.clone(), local_rec.clone());
                        local_deletes_count.push(path);
                    }
                    PathDecision::DocumentConflictRemoteDeleted => {
                        let local_hash = &local_rec.content_hash;
                        let conflict = SyncConflict {
                            local_path: path.clone(),
                            remote_path: path.clone(),
                            local_hash: local_hash.clone(),
                            remote_hash: remote_rec.content_hash.clone(),
                            base_hash: base_hash.to_string(),
                            created_at: chrono::Utc::now().timestamp(),
                            description: "正文文件冲突：本地已修改，远端已删除。保留本地文件。"
                                .to_string(),
                        };
                        doc_conflicts.push(conflict.clone());
                        state.conflicted_files.insert(path.clone());
                        state.conflicts.push(conflict);
                        merged_manifest_files.insert(path.clone(), remote_rec.clone());
                    }
                    PathDecision::DocumentConflictBothChanged => {
                        let local_hash = &local_rec.content_hash;
                        let remote_hash = &remote_rec.content_hash;
                        log::warn!(
                            "[sync] document_conflict path={} local_hash={} remote_hash={} base_hash={}",
                            path, local_hash, remote_hash, base_hash
                        );

                        let conflict = {
                            let remote_path = format!("{}/{}", remote_prefix, path);
                            if let Some((remote_content, _)) = github_get_content(
                                transport,
                                api_base,
                                token,
                                &config.branch,
                                &remote_path,
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
                        };

                        if let Some(conflict) = &conflict {
                            doc_conflicts.push(conflict.clone());
                            state.conflicted_files.insert(path.clone());
                            state.conflicts.push(conflict.clone());
                        }
                        merged_manifest_files.insert(path.clone(), remote_rec.clone());
                    }
                }
            }
            (None, None) => {}
        }
    }

    // 远端已删除的文件移至 trash 目录而非直接删除，
    // 防止同步异常时用户数据丢失。
    //
    // 不变量：远端删除操作只在本地文件与远端记录一致（三路 NoConflict/RemoteChanged，
    // 或 LWW 远端获胜）时执行。冲突路径不会进入 to_delete_local。
    move_to_trash(sync_root, &to_delete_local);

    log::debug!("[sync] github_api step=download newer remote files");
    download_remote_files(
        sync_root,
        transport,
        api_base,
        token,
        &config.branch,
        remote_prefix,
        &to_download,
    )?;

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
    upload_local_files(
        sync_root,
        transport,
        api_base,
        token,
        &config.branch,
        remote_prefix,
        &to_upload,
        &remote_tree_files,
    )?;

    delete_remote_files(
        transport,
        api_base,
        token,
        &config.branch,
        remote_prefix,
        &local_deletes_count,
        &remote_tree_files,
    )?;

    upload_manifest(
        transport,
        api_base,
        token,
        &config.branch,
        &remote_manifest_path,
        &manifest_json,
        &remote_tree_files,
    )?;

    for conflict in &doc_conflicts {
        let _ = SyncService::record_sync_conflict(sync_root, conflict.clone(), None);
    }

    state.last_sync_time = Some(chrono::Utc::now().timestamp());
    state.last_synced_commit = None;
    state.last_error = None;

    let post_local_entries = scan_for_sync(sync_root, scope)?;

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
