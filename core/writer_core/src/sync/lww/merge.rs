//!   ：唯一的文件级 LWW merge 核心。
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
    ManifestFileRecord, SyncConflict, SyncConflictKind, SyncKind, SyncManifest, SyncScope,
    SyncState,
};
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

/// LWW merge 核心产出。
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
    /// 合并后的完整强类型 manifest 快照。
    ///
    /// generation publisher 必须用此字段发布完整快照，不能只上传
    /// `remote_upload_paths`（delta 动作）。`remote_upload_paths` 只给普通
    /// "原地 LWW"写远端用，不要拿它发布 generation。
    pub merged_manifest: SyncManifest,
}

/// 唯一的只读 merge 核心。
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
    let remote_records = build_remote_records(remote_manifest, &remote_tree_files, scope)?;

    // 提前加载 conflicts_json，让它一直带到最后的 persist_sync_merge_result，
    // 不要末尾再重新读一份。旧基线归一化和主循环都操作这一份 conflicts_json，
    // 保证归一化清除的假冲突不会在末尾 persist 时被重新加载的旧 conflicts.json 覆盖。
    let mut conflicts_json = crate::sync::conflict::load_conflicts_json(sync_root)?;

    // ── 旧基线归一化 ──
    // 旧版本同步系统可能把 Git blob OID（40位 hex）误写入 state.known_files，
    // 而当前同步系统使用 MD5（32位 hex）。三路比较里 local/remote/base 不是同一种
    // 内容哈希会导致空内容被误判为 BothChanged 冲突。
    //
    // 对 state.known_files 中每个值是 40 位 hex（is_legacy_git_blob_oid）的 path：
    // 1. 能证明 remote blob oid == base → 把 known_files[path] 改成 remote_rec.content_hash（MD5）。
    // 2. 否则能证明 local blob oid == base → 改成 local_rec.content_hash（MD5）。
    // 3. 如果该 path 已经在 conflicted_files，用归一化后的 base 重新跑正文三路决策。
    // 4. 如果不再是冲突，说明是历史哈希污染制造的假冲突，用 remove_conflict_in_memory 清除。
    // 5. 无法证明时不自动清冲突，保留真实选择权。
    let legacy_paths: Vec<String> = state
        .known_files
        .iter()
        .filter(|(_, h)| crate::sync::hash::is_legacy_git_blob_oid(h))
        .map(|(p, _)| p.clone())
        .collect();
    for path in legacy_paths {
        let old_base = match state.known_files.get(&path) {
            Some(h) => h.clone(),
            None => continue,
        };
        let remote_content_hash = remote_records
            .get(&path)
            .map(|r| r.content_hash.as_str())
            .unwrap_or("");
        let local_content_opt = local_records.get(&path).map(|r| r.content_hash.as_str());
        let new_base = crate::sync::hash::normalize_legacy_base_hash(
            &old_base,
            remote_content_hash,
            local_content_opt,
            &remote_tree_files,
            &path,
            sync_root,
        );
        if new_base == old_base || !crate::sync::hash::is_md5_content_hash(&new_base) {
            // 无法证明旧 base 对应哪一侧，或归一化结果不是 MD5（数据异常），
            // 保留原值，不自动清冲突。
            continue;
        }
        state.known_files.insert(path.clone(), new_base.clone());
        log::info!(
            "[sync] legacy base hash normalization: path={} old_base={} new_base={}",
            path,
            old_base,
            new_base
        );

        // 如果该 path 已经在 conflicted_files，用归一化后的 base 重新跑正文三路决策。
        // 只对正文文件做（is_document_content_path），非正文文件不走三路比较。
        if state.conflicted_files.contains(&path) {
            if let (Some(local_rec), Some(remote_rec)) =
                (local_records.get(&path), remote_records.get(&path))
            {
                let is_document = is_document_content_path(&path);
                let (decision, _overwritten) =
                    resolve_path_decision(local_rec, remote_rec, &new_base, is_document);
                match decision {
                    PathDecision::DocumentConflictBothChanged
                    | PathDecision::DocumentConflictRemoteDeleted => {
                        // 仍是真实冲突，保留用户选择权。
                    }
                    _ => {
                        // 历史哈希污染制造的假冲突，从内存三件套清除。
                        // 后续主循环会按归一化后的 base 走正常 UploadLocal/DownloadRemote/NoOp。
                        crate::sync::conflict::remove_conflict_in_memory(
                            state,
                            &mut conflicts_json,
                            &path,
                        );
                        log::info!(
                            "[sync] legacy base hash normalization: cleared fake conflict path={}",
                            path
                        );
                    }
                }
            }
        }
    }

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
                let hash = crate::sync::hash::content_md5(&content);
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
                            kind: SyncConflictKind::RemoteDeleted,
                            remote_snapshot_path: None,
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
                                let remote_snapshot_path =
                                    save_conflict_copy(sync_root, &path, &remote_content)?;

                                Some(SyncConflict {
                                    local_path: path.clone(),
                                    remote_path: path.clone(),
                                    local_hash: local_hash.clone(),
                                    remote_hash: remote_hash.clone(),
                                    base_hash: base_hash.to_string(),
                                    created_at: chrono::Utc::now().timestamp(),
                                    description: "正文文件双端修改冲突。本地修改和远端修改均保留。"
                                        .to_string(),
                                    kind: SyncConflictKind::BothChanged,
                                    remote_snapshot_path: Some(remote_snapshot_path),
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

    move_to_trash(sync_root, &to_delete_local)?;

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

    // 在内存中收集冲突到 state.conflicts 和 conflicts_json，
    // 不再逐条调用 record_sync_conflict（每次内部写盘）。
    // 冲突记录失败必须向上返回 Err，不再 let _ = 吞错误。
    // conflicts_json 已在归一化前提前加载（旧基线归一化可能清除假冲突），
    // 这里直接复用，不再重新读一份。
    for conflict in &doc_conflicts {
        crate::sync::conflict::upsert_conflict(
            &mut conflicts_json,
            &mut state.conflicts,
            &mut state.conflicted_files,
            conflict.clone(),
        );
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

    // 一次事务原子提交 manifest + state + conflicts.json，
    // 避免分多次独立写入中间崩溃导致三者不一致。
    crate::sync::conflict::persist_sync_merge_result(
        sync_root,
        SYNC_MANIFEST_PATH,
        &manifest_json,
        state,
        &conflicts_json,
    )?;

    let mut all_downloaded = pending_take_remote_downloaded;
    all_downloaded.extend(to_download);

    Ok(LwwMergeOutcome {
        conflicts: doc_conflicts,
        downloaded_files: all_downloaded,
        //   local_deletes_count 是本地发起的 delete
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
        // 携带完整强类型 manifest 快照，
        // 供 generation publisher 上传完整快照（不只是 delta 动作）。
        merged_manifest: sync_manifest,
    })
}

/// Issue #761 评论 5829270182：远端无 visible source 时的本地-only snapshot materialization。
///
/// 真实首次同步（live project 无 manifest.sync.json、远端 catalog 空）时，
/// `transfer_live_project` 得到 `merge_outcome_opt = None`，但后续
/// `read_post_transfer_lww` / candidate 构造 / `publish_generation_batch` 都假设
/// staging 已有 manifest。`seed_from_live` 只复制 live 已存在文件，不创建 manifest，
/// 所以 staging 里也没有 manifest.sync.json，导致 `read_post_transfer_lww` 返回 None
/// → `RecoverableError("post-transfer staging manifest unreadable")`，走不到 batch 路径。
///
/// 本 helper 复用现有只读投影 + manifest 构造 + SyncState 重建 + 原子持久化，
/// 把当前本地完整快照 materialize 成 staging 的 manifest + state + conflicts，
/// 返回 `LwwMergeOutcome` 让后续 publish 走统一 batch 路径：
///
/// - `merged_manifest` = 当前本地完整快照（`snapshot_local_records_read_only` 投影）；
/// - 所有 `op=upsert` 路径进入 `remote_upload_paths`（首次同步全部需上传）；
/// - delete tombstone 只留在 manifest，不需要物理 blob；
/// - `remote_tree_files` 为空（远端无 visible source，无 blob 可复用）；
/// - `conflicts` 为空（首次同步无冲突）；
/// - `remote_manifest_path` / `manifest_json` 与 `merged_manifest` 一致。
///
/// 内部复用：
/// - `snapshot_local_records_read_only()`（与 planner / LWW attempt 同源）；
/// - manifest 排序/构造（与 `merge_remote_into_local_snapshot` 对齐）；
/// - SyncState 的 `known_files / known_files_updated_at / last_sync_time` 重建；
/// - `persist_sync_merge_result()` 一次写入 manifest + state + conflicts。
///
/// 不在 generation.rs 单独再补一套 scanner——本 helper 是唯一的本地快照 materialization 入口。
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::cast_possible_truncation
)]
pub(crate) fn materialize_local_snapshot_for_empty_remote(
    sync_root: &Path,
    scope: SyncScope,
    preferred_device_id: &str,
) -> crate::error::Result<LwwMergeOutcome> {
    log::debug!(
        "[sync] materialize_local_snapshot_for_empty_remote: sync_root={} scope={:?} \
         preferred_device_id={}",
        sync_root.display(),
        scope,
        preferred_device_id
    );

    let now_ms = chrono::Utc::now().timestamp_millis();

    // 1. 用 snapshot_local_records_read_only 取本地完整快照（与 planner / LWW attempt 同源）。
    //    manifest 不存在 → 空 HashMap（首次同步，正常）；manifest 损坏 → Err。
    let local_records = snapshot_local_records_read_only(sync_root, scope, preferred_device_id)?;

    // 2. 构造 manifest（排序、清除过期 delete tombstone），与 merge_remote_into_local_snapshot 对齐。
    let purge_time = now_ms - 30 * 24 * 3600 * 1000;
    let mut manifest_files_vec: Vec<ManifestFileRecord> = local_records.values().cloned().collect();
    manifest_files_vec.retain(|rec| rec.op != "delete" || lww_record_time(rec) > purge_time);
    manifest_files_vec.sort_by(|a, b| a.path.cmp(&b.path));

    let sync_manifest = SyncManifest {
        files: manifest_files_vec,
    };
    let manifest_json = serde_json::to_string_pretty(&sync_manifest)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;

    // 3. 重建 SyncState：known_files / known_files_updated_at / last_sync_time。
    //    与 merge_remote_into_local_snapshot 的 known_files 重建逻辑对齐：
    //    遍历本地 scan 结果，对每个 Upload 类（非 manifest）文件记录 hash + updated_at。
    // 与 snapshot_local_records_read_only 使用同一设备身份来源：
    //   preferred_device_id 非空 → Some(preferred_device_id)
    //   preferred_device_id 为空 → None（由 resolve_device_id 在真实首次同步时随机生成）
    // load_sync_state_with_preferred_device_id 内部 resolve_device_id 已覆盖
    // 「existing 非空用 existing；否则 preferred 非空用 preferred；否则随机」语义，
    // 不再在此处追加 fallback 或第二套 device_id 推断。
    let preferred_device_opt: Option<&str> = if preferred_device_id.is_empty() {
        None
    } else {
        Some(preferred_device_id)
    };
    let mut state = crate::sync::SyncService::load_sync_state_with_preferred_device_id(
        sync_root,
        preferred_device_opt,
    )?;

    // 保留冲突状态（conflicted_files / conflicts / pending_take_remote），
    // 只重建 known_files / known_files_updated_at。
    let conflicted_known_files: HashMap<String, String> = state
        .conflicted_files
        .iter()
        .filter_map(|p| state.known_files.get(p).map(|v| (p.clone(), v.clone())))
        .collect();
    let conflicted_known_files_updated_at: HashMap<String, i64> = state
        .conflicted_files
        .iter()
        .filter_map(|p| state.known_files_updated_at.get(p).map(|v| (p.clone(), *v)))
        .collect();

    state.known_files.clear();
    state.known_files_updated_at.clear();

    let post_local_entries = scan_for_sync(sync_root, scope)?;
    for entry in &post_local_entries {
        if entry.sync_kind == SyncKind::Upload && entry.relative_path != SYNC_MANIFEST_PATH {
            if state.conflicted_files.contains(&entry.relative_path) {
                continue;
            }
            state
                .known_files
                .insert(entry.relative_path.clone(), entry.file_hash.clone());

            // 用 manifest record 的 updated_at_ms（若匹配），否则读文件 mtime，最后回退 now_ms。
            let matched_rec = local_records.get(&entry.relative_path);
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
            state
                .known_files_updated_at
                .insert(entry.relative_path.clone(), t);
        }
    }

    for (path, hash) in conflicted_known_files {
        state.known_files.insert(path, hash);
    }
    for (path, t) in conflicted_known_files_updated_at {
        state.known_files_updated_at.insert(path, t);
    }

    state.last_sync_time = Some(chrono::Utc::now().timestamp());
    state.last_error = None;
    state
        .tombstones
        .retain(|t| t.purge_after > chrono::Utc::now().timestamp());

    // 4. 一次事务原子提交 manifest + state + conflicts，
    //    避免分多次独立写入中间崩溃导致三者不一致。
    let conflicts_json = crate::sync::conflict::load_conflicts_json(sync_root)?;
    crate::sync::conflict::persist_sync_merge_result(
        sync_root,
        SYNC_MANIFEST_PATH,
        &manifest_json,
        &state,
        &conflicts_json,
    )?;

    // 5. 构造 LwwMergeOutcome：所有 upsert 进 remote_upload_paths（首次同步全部需上传），
    //    remote_tree_files 为空（远端无 visible source，无 blob 可复用），
    //    conflicts 为空（首次同步无冲突）。
    let remote_upload_paths: Vec<String> = sync_manifest
        .files
        .iter()
        .filter(|rec| rec.op == "upsert")
        .map(|rec| rec.path.clone())
        .collect();

    // remote_manifest_path 用空 source prefix（远端无 visible source）。
    // 调用方（publish_generation_batch）不依赖此字段构造远端路径——
    // 它用 generation_prefix + record.path。
    let remote_manifest_path = SYNC_MANIFEST_PATH.to_string();

    Ok(LwwMergeOutcome {
        conflicts: Vec::new(),
        downloaded_files: Vec::new(),
        local_deletes: Vec::new(),
        remote_upload_paths,
        remote_delete_paths: Vec::new(),
        overwritten_files: Vec::new(),
        ignored_files: Vec::new(),
        pending_take_remote_failed: Vec::new(),
        remote_tree_files: HashMap::new(),
        remote_manifest_path,
        manifest_json,
        merged_manifest: sync_manifest,
    })
}
