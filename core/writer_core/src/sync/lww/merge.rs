//! #645 评论 5504296097 问题1 修复：唯一的文件级 LWW merge 核心。
//!
//! 把 `attempt.rs` 里真正的文件级合并抽成 [`merge_remote_into_local_snapshot`]，
//! 供 `execute_lww_sync_attempt`（普通 LWW）和 `full_sync.rs` LiveProject 两条路径
//! 复用，不再维护第二套"存在就本地赢"的同步算法。
//!
//! ## 职责
//!
//! [`merge_remote_into_local_snapshot`] 统一处理：
//! - remote manifest + remote tree 拉取
//! - `snapshot_local_records_read_only`（per-file 真实 LWW）
//! - pending_take_remote（强制下载远端覆盖本地）
//! - conflicted_files guard（跳过未解决冲突）
//! - `resolve_path_decision`（三路 / LWW 决策）
//! - remote delete tombstone（远端删除 → 本地移 trash）
//! - BothChanged / RemoteDeleted conflict（正文冲突副本）
//! - staging download / trash（本地 IO）
//! - merged manifest（本地写）
//! - known_files / known_files_updated_at 重建
//! - SyncState 保存
//!
//! **不**做远端写（upload / delete / upload_manifest）— 由调用方根据
//! [`LwwMergeOutcome`] 决定。普通 LWW 直接对 `remote_prefix` 写；LiveProject
//! 先 merge 到 staging，没有未解决冲突才 `publish_generation` 到新 generation prefix。
//!
//! ## 不变量
//!
//! - 只有一份 LWW 判断逻辑（本模块）。调用方不再自己比较时间戳/做三路决策。
//! - 远端 Delete tombstone 通过 remote manifest 看到（不只看 provider.list 物理对象），
//!   修复"远端删除被本地复活"。
//! - 远端同路径更新通过 LWW 时间戳比较进入合并，修复"远端更新丢失"。

use crate::sync::content_class::is_document_content_path;
use crate::sync::provider::SyncProvider;
use crate::sync::scanner::scan_for_sync;
use crate::sync::types::{
    ManifestFileRecord, SyncConflict, SyncKind, SyncManifest, SyncScope, SyncState,
};
use crate::sync::SyncService;
use std::collections::HashMap;
use std::path::Path;

use super::compare::{resolve_path_decision, PathDecision};
use super::manifest::{
    build_remote_records, lww_record_time, snapshot_local_records_read_only, SYNC_MANIFEST_PATH,
};
use super::transfer::{
    download_pending_take_remote, download_remote_files, fetch_remote_manifest, fetch_remote_tree,
    move_to_trash, save_conflict_copy,
};

/// #645 评论 5504296097 问题1 修复：LWW merge 核心产出。
///
/// [`merge_remote_into_local_snapshot`] 返回此结构，调用方根据字段做远端写。
#[derive(Debug, Clone)]
pub(crate) struct LwwMergeOutcome {
    /// 正文冲突列表（BothChanged / RemoteDeleted）。非空时调用方不应发布新 generation。
    pub conflicts: Vec<SyncConflict>,
    /// 已下载到本地的远端较新文件路径（含 pending_take_remote 成功下载的）。
    pub downloaded_files: Vec<String>,
    /// 本地已删除（移到 trash）的文件路径。
    pub local_deletes: Vec<String>,
    /// 调用方应上传到远端的本地文件路径。
    pub remote_upload_paths: Vec<String>,
    /// 调用方应从远端删除的文件路径。
    pub remote_delete_paths: Vec<String>,
    /// 被覆盖的文件路径（LWW 决胜中被覆盖的一方，仅 Metadata/GeneratedCache）。
    pub overwritten_files: Vec<String>,
    /// 被跳过的文件路径（NoOp / unresolved conflict / pending_take_remote）。
    pub ignored_files: Vec<String>,
    /// pending_take_remote 中下载失败（远端缺失）的路径，保留在 pending 中待下轮重试。
    pub pending_take_remote_failed: Vec<String>,
    /// 远端 tree 文件（供调用方做远端写的前置条件）。
    pub remote_tree_files: HashMap<String, String>,
    /// 远端 manifest 路径（供调用方上传 manifest）。
    pub remote_manifest_path: String,
    /// manifest JSON（供调用方上传 manifest）。
    pub manifest_json: String,
    /// #645 评论 5504296097 问题1 修复：合并后的完整强类型 manifest 快照。
    ///
    /// generation publisher 必须用此字段发布完整快照，不能只上传
    /// `remote_upload_paths`（delta 动作）。`remote_upload_paths` 只给普通
    /// "原地 LWW"写远端用，不要拿它发布 generation。
    pub merged_manifest: SyncManifest,
}

/// #645 评论 5504296097 问题1 修复：唯一的只读 merge 核心。
///
/// 把 `attempt.rs` 里真正的文件级合并抽成本函数，供
/// `execute_lww_sync_attempt`（普通 LWW）和 `full_sync.rs` LiveProject 复用。
///
/// 做本地 IO（download / trash / 写 manifest / known_files / SyncState 保存），
/// **不**做远端写（upload / delete / upload_manifest）— 由调用方根据
/// [`LwwMergeOutcome`] 决定。
///
/// # 参数
///
/// - `sync_root`：本地同步根（普通 LWW = live root；LiveProject = staging root）。
/// - `provider`：远端 provider。
/// - `source_remote_prefix`：拉取远端 manifest/tree 的 prefix（普通 LWW = target.remote_prefix；
///   LiveProject = source generation prefix）。
/// - `scope`：同步范围。
/// - `state`：可变 SyncState（pending_take_remote / known_files / conflicted_files 等）。
#[allow(
    clippy::cast_possible_truncation,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub(crate) fn merge_remote_into_local_snapshot(
    sync_root: &Path,
    provider: &dyn SyncProvider,
    source_remote_prefix: &str,
    scope: SyncScope,
    state: &mut SyncState,
) -> crate::error::Result<LwwMergeOutcome> {
    log::debug!(
        "[sync] merge_remote_into_local_snapshot: source_remote_prefix={}",
        source_remote_prefix
    );
    let remote_tree_files = fetch_remote_tree(provider, source_remote_prefix)?;
    let remote_manifest_path = format!("{}/{}", source_remote_prefix, SYNC_MANIFEST_PATH);
    let remote_manifest =
        fetch_remote_manifest(provider, source_remote_prefix, &remote_tree_files)?;

    let now_ms = chrono::Utc::now().timestamp_millis();

    let local_records = snapshot_local_records_read_only(sync_root, scope, &state.device_id)?;
    let remote_records = build_remote_records(remote_manifest, &remote_tree_files, scope);

    let unresolved_conflict_paths: std::collections::HashSet<String> =
        state.conflicted_files.clone();

    // ── Process pending_take_remote ──
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
            provider,
            source_remote_prefix,
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
    let mut ignored_files = Vec::new();

    let all_paths: std::collections::HashSet<String> = local_records
        .keys()
        .cloned()
        .chain(remote_records.keys().cloned())
        .collect();

    for path in all_paths {
        if pending_take_remote_all_set.contains(&path) {
            if pending_take_remote_downloaded.contains(&path) {
                if let Some(remote_rec) = remote_records.get(&path) {
                    merged_manifest_files.insert(path.clone(), remote_rec.clone());
                }
                ignored_files.push(path);
            } else {
                if let Some(remote_rec) = remote_records.get(&path) {
                    merged_manifest_files.insert(path.clone(), remote_rec.clone());
                } else if let Some(local_rec) = local_records.get(&path) {
                    merged_manifest_files.insert(path.clone(), local_rec.clone());
                }
            }
            continue;
        }
        if unresolved_conflict_paths.contains(&path) {
            log::debug!(
                "[sync] skipping unresolved_conflict path={} (awaiting user resolution)",
                path
            );
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
                        ignored_files.push(path);
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
                            let remote_path = format!("{}/{}", source_remote_prefix, path);
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

    move_to_trash(sync_root, &to_delete_local);

    download_remote_files(sync_root, provider, source_remote_prefix, &to_download)?;

    // 清除超过 30 天的 delete 墓碑记录。
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

    for conflict in &doc_conflicts {
        let _ = SyncService::record_sync_conflict(sync_root, conflict.clone(), None);
    }

    state.last_sync_time = Some(chrono::Utc::now().timestamp());
    state.last_error = None;

    let post_local_entries = scan_for_sync(sync_root, scope)?;

    // ── 同步后重建 known_files ──
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

    for (path, hash) in conflicted_known_files {
        state.known_files.insert(path, hash);
    }
    for (path, t) in conflicted_known_files_updated_at {
        state.known_files_updated_at.insert(path, t);
    }

    state
        .tombstones
        .retain(|t| t.purge_after > chrono::Utc::now().timestamp());

    crate::sync::SyncService::save_sync_state(sync_root, state)?;

    let mut all_downloaded = pending_take_remote_downloaded;
    all_downloaded.extend(to_download);

    Ok(LwwMergeOutcome {
        conflicts: doc_conflicts,
        downloaded_files: all_downloaded,
        // #645 评论 5504296097 问题1：local_deletes_count 是本地发起的 delete
        // （LwwLocalWinsDeleteRecord），调用方用此列表调 delete_remote_files 删远端。
        // remote_deletes_count 是远端发起的 delete（DeleteLocal / LwwRemoteWinsDelete），
        // 已在 merge 内 move_to_trash，作为 result.remote_deletes 报告。
        local_deletes: remote_deletes_count,
        remote_upload_paths: to_upload,
        remote_delete_paths: local_deletes_count,
        overwritten_files,
        ignored_files,
        pending_take_remote_failed,
        remote_tree_files,
        remote_manifest_path,
        manifest_json,
        // #645 评论 5504296097 问题1 修复：携带完整强类型 manifest 快照，
        // 供 generation publisher 上传完整快照（不只是 delta 动作）。
        merged_manifest: sync_manifest,
    })
}
