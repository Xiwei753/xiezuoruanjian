//! LWW 同步单次尝试编排。
//!
//! 本文件实现 [`execute_lww_sync_attempt`]：拉取远端清单 → 扫描本地 →
//! 处理 pending_take_remote → 逐路径三路/LWW 比较 → 上传/下载/删除 →
//! 写 manifest 与 sync state。重试与错误分类由 [`super::engine::perform_lww_sync`] 负责。

use crate::sync::content_class::is_document_content_path;
use crate::sync::provider::SyncProvider;
use crate::sync::scanner::scan_for_sync;
use crate::sync::types::{
    FirstSyncMode, ManifestFileRecord, SyncConflict, SyncKind, SyncManifest, SyncResult, SyncState,
    SyncStatus,
};
use crate::sync::SyncService;
use std::path::Path;

use super::compare::{resolve_path_decision, PathDecision};
use super::manifest::{
    build_local_records, build_remote_records, lww_record_time, SYNC_MANIFEST_PATH,
};
use super::transfer::{
    delete_remote_files, download_pending_take_remote, download_remote_files,
    fetch_remote_manifest, fetch_remote_tree, move_to_trash, save_conflict_copy,
    upload_local_files, upload_manifest,
};

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
pub(crate) fn execute_lww_sync_attempt(
    sync_root: &Path,
    provider: &dyn SyncProvider,
    target: &crate::sync::types::SyncTarget,
    state: &mut SyncState,
    result: &mut SyncResult,
) -> crate::Result<SyncResult> {
    let remote_prefix = &target.remote_prefix;
    let scope = target.scope;
    log::debug!(
        "[sync] lww step=正在拉取远端清单 remote_prefix={}",
        remote_prefix
    );
    let remote_tree_files = fetch_remote_tree(provider, remote_prefix)?;

    let remote_manifest_path = format!("{}/{}", remote_prefix, SYNC_MANIFEST_PATH);
    let remote_manifest = fetch_remote_manifest(provider, remote_prefix, &remote_tree_files)?;

    log::debug!("[sync] lww step=正在比较本地和远端");
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
        let pending_results =
            download_pending_take_remote(sync_root, provider, remote_prefix, &pending_paths)?;

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
                            if let Some(remote_obj) = provider.read(&remote_path)? {
                                let remote_content = remote_obj.content;
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

    log::debug!("[sync] lww step=download newer remote files");
    download_remote_files(sync_root, provider, remote_prefix, &to_download)?;

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

    log::debug!("[sync] lww step=正在上传本地较新文件");
    upload_local_files(
        sync_root,
        provider,
        remote_prefix,
        &to_upload,
        &remote_tree_files,
    )?;

    delete_remote_files(
        provider,
        remote_prefix,
        &local_deletes_count,
        &remote_tree_files,
    )?;

    upload_manifest(
        provider,
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

    log::debug!("[sync] lww step=同步完成");
    Ok(result.clone())
}
