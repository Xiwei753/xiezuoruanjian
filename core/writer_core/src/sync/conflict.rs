//! 同步冲突检测与解决。
//!
//! 冲突检测由 `lww.rs` 中的三路比较完成，本模块负责冲突记录的持久化与合并。
//!
//! 冲突解决策略：
//! - `resolve_conflict_keep_local`：保留本地版本，丢弃远端变更
//! - `resolve_conflict_take_remote`：接受远端版本，丢弃本地变更
//! - `resolve_conflict_mark_merged`：标记为已合并（用户手动解决后调用）

use crate::sync::types::{SyncConflict, SyncConflictKind};
use std::path::Path;

/// 读取 `app-meta/sync/conflicts.json`。
///
/// 文件不存在或内容损坏（半写/无效 JSON）时回退为空列表——丢失冲突记录比
/// 阻塞后续同步更可接受。
pub(crate) fn load_conflicts_json(sync_root: &Path) -> crate::Result<Vec<SyncConflict>> {
    let conflicts_path = sync_root.join("app-meta/sync/conflicts.json");
    if !conflicts_path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&conflicts_path)?;
    let conflicts: Vec<SyncConflict> = serde_json::from_str(&content).unwrap_or_default();
    Ok(conflicts)
}

/// 一次事务写入 `state.local.json` `conflicts.json`。
///
/// 用 [`crate::storage::transaction::SaveTransaction`] 保证两个文件原子提交，
/// 不会出现"state 写了但 conflicts.json 没写"的中间不一致状态。
/// `record_staging_conflicts` 和 [`crate::sync::SyncService::record_sync_conflict`]
/// 共用本函数，不要两套写法。
fn persist_conflict_state(
    sync_root: &Path,
    state: &crate::sync::types::SyncState,
    conflicts: &[SyncConflict],
) -> crate::Result<()> {
    let state_json = serde_json::to_string_pretty(state)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
    let conflicts_json = serde_json::to_string_pretty(conflicts)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;

    let mut tx = crate::storage::transaction::SaveTransaction::new(sync_root);
    tx.add_bytes("app-meta/sync/state.local.json", state_json.as_bytes())?;
    tx.add_bytes("app-meta/sync/conflicts.json", conflicts_json.as_bytes())?;
    tx.commit()?;
    Ok(())
}

/// 一次事务写入 `manifest.sync.json` + `state.local.json` + `conflicts.json`。
///
/// 用于 LWW merge 完成后原子提交三个文件，避免分多次独立写入中间崩溃
/// 导致 manifest / state / conflicts 不一致。manifest_json 由调用方序列化好传入。
pub(crate) fn persist_sync_merge_result(
    sync_root: &Path,
    manifest_path: &str,
    manifest_json: &str,
    state: &crate::sync::types::SyncState,
    conflicts: &[SyncConflict],
) -> crate::Result<()> {
    let state_json = serde_json::to_string_pretty(state)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
    let conflicts_json = serde_json::to_string_pretty(conflicts)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;

    let mut tx = crate::storage::transaction::SaveTransaction::new(sync_root);
    tx.add_bytes(manifest_path, manifest_json.as_bytes())?;
    tx.add_bytes("app-meta/sync/state.local.json", state_json.as_bytes())?;
    tx.add_bytes("app-meta/sync/conflicts.json", conflicts_json.as_bytes())?;
    tx.commit()?;
    Ok(())
}

/// 按 `local_path` 去重/替换加入冲突。
///
/// 同一路径重复写入时替换已有记录，不无限 append。`state_conflicts` 和
/// `conflicts_json` 都做同样的去重，保持两者一致。
pub(crate) fn upsert_conflict(
    conflicts_json: &mut Vec<SyncConflict>,
    state_conflicts: &mut Vec<SyncConflict>,
    conflicted_files: &mut std::collections::HashSet<String>,
    conflict: SyncConflict,
) {
    let path = conflict.local_path.clone();
    conflicted_files.insert(path.clone());
    if let Some(existing) = conflicts_json.iter_mut().find(|c| c.local_path == path) {
        *existing = conflict.clone();
    } else {
        conflicts_json.push(conflict.clone());
    }
    if let Some(existing) = state_conflicts.iter_mut().find(|c| c.local_path == path) {
        *existing = conflict.clone();
    } else {
        state_conflicts.push(conflict);
    }
}

/// 合并两批冲突，按 `local_path` 去重。
///
/// `incoming` 中的记录覆盖 `existing` 中同路径的旧记录（外层同路径覆盖旧记录即可）。
/// 返回合并后的完整列表。
pub fn merge_sync_conflicts(
    existing: &[SyncConflict],
    incoming: &[SyncConflict],
) -> Vec<SyncConflict> {
    if incoming.is_empty() {
        return existing.to_vec();
    }
    if existing.is_empty() {
        return incoming.to_vec();
    }
    let mut merged: Vec<SyncConflict> = existing.to_vec();
    for inc in incoming {
        if let Some(pos) = merged.iter().position(|c| c.local_path == inc.local_path) {
            merged[pos] = inc.clone();
        } else {
            merged.push(inc.clone());
        }
    }
    merged
}

/// 只操作内存三件套的冲突移除 helper。
///
/// 同时从 `state.conflicted_files`、`state.conflicts` 和 `conflicts_json` 删除
/// 指定路径的冲突记录，保持三者一致。不持久化（调用方负责后续 persist）。
///
/// 用于：
/// - `resolve_conflict_keep_local` / `take_remote` / `mark_merged` 解决冲突后清理；
/// - `merge_remote_into_local_snapshot` 旧基线归一化时清理历史哈希污染制造的假冲突。
///
/// 用 `local_path` 和 `remote_path` 双重匹配，避免同路径不同 remote_path 的边缘情况。
pub(crate) fn remove_conflict_in_memory(
    state: &mut crate::sync::types::SyncState,
    conflicts_json: &mut Vec<SyncConflict>,
    path: &str,
) {
    state.conflicted_files.remove(path);
    state
        .conflicts
        .retain(|c| c.local_path != path && c.remote_path != path);
    conflicts_json.retain(|c| c.local_path != path && c.remote_path != path);
}

/// staging 三方冲突 → `SyncConflict` 映射 持久化。
///
/// 改成完整事务——先在内存里构造新的 `SyncState` 和
/// 完整 `Vec<SyncConflict>`，用 [`persist_conflict_state`] 一次提交
/// `app-meta/sync/state.local.json` + `app-meta/sync/conflicts.json`。
/// 不再循环调用 `record_sync_conflict` 一条一条落盘，中间写失败不会留下不一致。
/// 同一路径重复写入时按 `local_path` 去重/替换。
///
/// `existing_conflicts` 参数接收 Transfer 阶段已有的
/// 冲突（如 GitHub LWW 发现的正文冲突），与新 staging 冲突合并后一起持久化。
/// 返回合并后的完整 `Vec<SyncConflict>`（Transfer + staging），供调用方填入
/// `SyncResult.conflicts`。
///
/// 持久化失败必须返回 Err（不能只打日志），让对应 target 进入错误状态。
pub fn record_staging_conflicts(
    sync_root: &Path,
    remote_prefix: &str,
    staging_conflicts: &[crate::sync::staging::StagingConflict],
    existing_conflicts: &[SyncConflict],
) -> crate::Result<Vec<SyncConflict>> {
    if staging_conflicts.is_empty() && existing_conflicts.is_empty() {
        return Ok(Vec::new());
    }

    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0);

    // 先在内存里构造完整的新状态。
    let mut state = crate::sync::SyncService::load_sync_state(sync_root)?;
    let mut conflicts_json = load_conflicts_json(sync_root)?;

    // 先把 existing_conflicts（Transfer 冲突）合并进来，
    // 确保持久化状态包含两层冲突。
    for ec in existing_conflicts {
        upsert_conflict(
            &mut conflicts_json,
            &mut state.conflicts,
            &mut state.conflicted_files,
            ec.clone(),
        );
    }

    let mut new_staging_conflicts = Vec::with_capacity(staging_conflicts.len());
    for sc in staging_conflicts {
        let rel_str = sc.rel_path.to_string_lossy().to_string();
        let rel_unix = rel_str.replace('\\', "/");
        let description = match sc.kind {
            crate::sync::types::SyncConflictKind::BothChanged => format!(
                "three-way conflict: both local and remote changed {}",
                sc.rel_path.display()
            ),
            crate::sync::types::SyncConflictKind::RemoteDeleted => format!(
                "conflict: remote deleted {} but local modified",
                sc.rel_path.display()
            ),
        };
        let sync_conflict = SyncConflict {
            local_path: rel_str.clone(),
            remote_path: format!("{}/{}", remote_prefix, rel_unix),
            local_hash: sc.local_hash.clone(),
            remote_hash: sc.incoming_hash.clone(),
            base_hash: sc.base_hash.clone(),
            created_at: now_ts,
            description,
            kind: sc.kind,
            // RemoteDeleted 的 remote_snapshot_path 必须是 None。
            remote_snapshot_path: sc.remote_snapshot_path.clone(),
        };

        upsert_conflict(
            &mut conflicts_json,
            &mut state.conflicts,
            &mut state.conflicted_files,
            sync_conflict.clone(),
        );
        new_staging_conflicts.push(sync_conflict);
    }

    // 一次事务写 state + conflicts.json。
    persist_conflict_state(sync_root, &state, &conflicts_json)?;

    // 返回合并后的完整冲突列表（existing new staging）。
    Ok(merge_sync_conflicts(
        existing_conflicts,
        &new_staging_conflicts,
    ))
}

impl crate::sync::SyncService {
    /// 记录同步冲突——将冲突元数据追加到 `app-meta/sync/conflicts.json`，
    /// 并将本地内容备份为 `{path}.conflict.{timestamp}` 文件。
    ///
    /// 改成完整事务——先在内存里构造新的 `SyncState`
    /// 和完整 `Vec<SyncConflict>`，用 [`persist_conflict_state`] 一次提交
    /// `state.local.json` + `conflicts.json`。同时更新 `conflicted_files` 和
    /// `state.conflicts`，修复原来"调用前路径已加入 conflicted_files"的注释违反。
    /// 同一路径重复写入时按 `local_path` 去重/替换，不无限 append。
    ///
    /// 备份文件（`{path}.conflict.{timestamp}`）是辅助文件，不进事务（事务只保证
    /// state + conflicts.json 一致，备份文件丢失不影响同步语义）。
    pub fn record_sync_conflict(
        sync_root: &Path,
        conflict: SyncConflict,
        local_content: Option<&str>,
    ) -> crate::Result<()> {
        // 备份本地内容（事务外，辅助文件）。
        if let Some(content) = local_content {
            let conflict_file_path = sync_root.join(format!(
                "{}.conflict.{}",
                conflict.local_path, conflict.created_at
            ));
            crate::storage::transaction::atomic_write_bytes(
                &conflict_file_path,
                content.as_bytes(),
            )?;
        }

        // 先在内存里构造完整的新状态。
        let mut state = Self::load_sync_state(sync_root)?;
        let mut conflicts_json = load_conflicts_json(sync_root)?;

        upsert_conflict(
            &mut conflicts_json,
            &mut state.conflicts,
            &mut state.conflicted_files,
            conflict,
        );

        // 一次事务写 state + conflicts.json。
        persist_conflict_state(sync_root, &state, &conflicts_json)?;
        Ok(())
    }

    /// Resolve a conflict by keeping the local version.
    ///
    /// Removes the path from `conflicted_files` and `conflicts`, and updates
    /// `known_files` to the remote hash so the next sync sees
    /// base=remote_hash, local≠base, remote=base → LocalChanged → uploads
    /// the local version.
    ///
    /// 不变量：known_files 必须设为 remote_hash（而非 local_hash）。
    /// 若设为 local_hash，三路比较会看到 base=local_hash, remote≠base → RemoteChanged，
    /// 导致下次同步下载远端版本覆盖本地——与"保留本地"的意图相反。
    /// 设为 remote_hash 后，三路比较看到 base=remote_hash, local≠base, remote=base
    /// → LocalChanged → 上传本地版本，符合预期。
    pub fn resolve_conflict_keep_local(sync_root: &Path, path: &str) -> crate::Result<()> {
        let mut state = Self::load_sync_state(sync_root)?;
        if !state.conflicted_files.remove(path) {
            return Err(crate::Error::Other(format!(
                "resolve_conflict_keep_local: path '{}' is not in conflicted_files",
                path
            )));
        }
        // Set known_files to the remote_hash so that three-way comparison on the
        // next sync sees: base=remote_hash, local≠base, remote=base → LocalChanged → upload.
        // If we set known_files to local_hash instead, three-way would see
        // RemoteChanged and download the remote version over local — the opposite of
        // what "keep local" means.
        if let Some(conflict) = state.conflicts.iter().find(|c| c.local_path == path) {
            state
                .known_files
                .insert(path.to_string(), conflict.remote_hash.clone());
            if let Some(t) = state
                .known_files_updated_at
                .get(&conflict.remote_path)
                .cloned()
            {
                state.known_files_updated_at.insert(path.to_string(), t);
            }
        } else {
            // Fallback: if no conflict record, use the current local file hash.
            let full_path = sync_root.join(path);
            if full_path.exists() {
                let content = std::fs::read(&full_path)?;
                let hash = crate::sync::hash::content_md5(&content);
                state.known_files.insert(path.to_string(), hash);
            }
        }
        // Remove the conflict record from state.conflicts and conflicts.json
        // via the shared in-memory helper.
        let mut conflicts_json = load_conflicts_json(sync_root)?;
        remove_conflict_in_memory(&mut state, &mut conflicts_json, path);
        persist_conflict_state(sync_root, &state, &conflicts_json)?;
        Ok(())
    }

    /// 用保存的远端 snapshot 原子替换本地正文。
    ///
    /// 读取 `sync_root/snapshot_rel` 的内容，atomic_write 到 `sync_root/path`。
    /// 返回 `Ok(true)` 表示成功替换；`Ok(false)` 表示无 snapshot path（老数据兼容）。
    fn apply_remote_snapshot(
        sync_root: &Path,
        path: &str,
        snapshot_rel: &str,
    ) -> crate::Result<()> {
        let snapshot_path = sync_root.join(snapshot_rel);
        let remote_content = std::fs::read(&snapshot_path).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "resolve_conflict_take_remote: read remote snapshot {}: {}",
                snapshot_rel, e
            )))
        })?;
        let local_full_path = sync_root.join(path);
        crate::storage::transaction::atomic_write_bytes(&local_full_path, &remote_content)
            .map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "resolve_conflict_take_remote: write local {}: {}",
                    path, e
                )))
            })?;
        Ok(())
    }

    /// Resolve a conflict by taking the remote version.
    ///
    /// 根据 [`SyncConflictKind`] 区分两种语义：
    ///
    /// - **BothChanged**：使用冲突记录对应的 `remote_snapshot_path` 原子替换本地正文
    ///   （读 snapshot 文件内容，atomic_write 到本地正文路径），然后把 base/known hash
    ///   更新到 `conflict.remote_hash`。**不再放进 `pending_take_remote`**，不再等
    ///   下一次联网后重新下载一个可能已经变掉的"最新远端"。
    /// - **RemoteDeleted**：接受删除，复用同步引擎 trash 语义把本地文件移入回收区，
    ///   并从 known_files 移除该路径（manifest 里的远端 delete record 作为同步基线保留）。
    ///   **不放进 `pending_take_remote`**。
    /// - **兼容老数据**：旧冲突记录没有 `kind` 字段（反序列化默认 `BothChanged`）时，
    ///   如果有 `remote_snapshot_path` 就走 BothChanged snapshot 替换；没有 snapshot path
    ///   则回退到旧的 `pending_take_remote` 行为（保持兼容）。
    ///
    /// 返回值 `applied_live`：
    /// - `Ok(true)`：已立即修改 live 正文（BothChanged snapshot 替换 / RemoteDeleted 移入 trash）。
    ///   平台层据此触发编辑器重载（`sync_content_applied`）。
    /// - `Ok(false)`：仅排队 `pending_take_remote`（老数据兼容，live 正文未变），
    ///   平台层不应触发编辑器重载。
    pub fn resolve_conflict_take_remote(sync_root: &Path, path: &str) -> crate::Result<bool> {
        let mut state = Self::load_sync_state(sync_root)?;
        if !state.conflicted_files.remove(path) {
            return Err(crate::Error::Other(format!(
                "resolve_conflict_take_remote: path '{}' is not in conflicted_files",
                path
            )));
        }

        // 取出该路径的冲突记录，按 kind 决策。
        let conflict_opt = state
            .conflicts
            .iter()
            .find(|c| c.local_path == path)
            .cloned();

        let use_pending_fallback =
            Self::decide_take_remote(sync_root, path, &mut state, conflict_opt)?;

        if use_pending_fallback {
            // 兼容路径：标记为 pending_take_remote，下次 perform_sync 强制下载远端内容。
            state.pending_take_remote.insert(path.to_string());
        }

        // Remove the conflict record from state.conflicts and conflicts.json
        // via the shared in-memory helper.
        let mut conflicts_json = load_conflicts_json(sync_root)?;
        remove_conflict_in_memory(&mut state, &mut conflicts_json, path);
        persist_conflict_state(sync_root, &state, &conflicts_json)?;
        // applied_live = !use_pending_fallback：
        // - use_pending_fallback=false → 已立即应用 live 正文（snapshot 替换/移入 trash）。
        // - use_pending_fallback=true → 仅排队 pending_take_remote，live 正文未变。
        Ok(!use_pending_fallback)
    }

    /// 根据 conflict kind 决定 take_remote 的具体动作，返回是否需要回退到 pending_take_remote。
    fn decide_take_remote(
        sync_root: &Path,
        path: &str,
        state: &mut crate::sync::types::SyncState,
        conflict_opt: Option<SyncConflict>,
    ) -> crate::Result<bool> {
        let Some(conflict) = conflict_opt else {
            // 无冲突记录：回退 pending_take_remote 行为。
            return Ok(true);
        };
        match conflict.kind {
            SyncConflictKind::RemoteDeleted => {
                // RemoteDeleted：把本地文件移入 trash，并从 known_files 移除。
                // 不能继续把已删除路径留在 known_files —— 否则下一轮
                // snapshot_local_records_read_only 会看到"known file missing
                // without tombstone"直接返回 Err，把已删除文件当成损坏。
                // manifest 里的远端 delete record 作为同步基线保留，不在这里动。
                // conflict / conflicted_files 的清理由外层 resolve_conflict_take_remote
                // 统一完成（conflicted_files.remove + conflicts.retain）。
                crate::sync::lww::move_to_trash(
                    sync_root,
                    std::slice::from_ref(&path.to_string()),
                )?;
                state.known_files.remove(path);
                state.known_files_updated_at.remove(path);
                Ok(false)
            }
            SyncConflictKind::BothChanged => match &conflict.remote_snapshot_path {
                Some(snapshot_rel) => {
                    // BothChanged + snapshot：用保存的远端副本原子替换本地正文。
                    Self::apply_remote_snapshot(sync_root, path, snapshot_rel)?;
                    // 基线更新为被选择的远端 snapshot 的 hash（即 conflict.remote_hash）。
                    state
                        .known_files
                        .insert(path.to_string(), conflict.remote_hash.clone());
                    let now_ts = chrono::Utc::now().timestamp_millis();
                    state
                        .known_files_updated_at
                        .insert(path.to_string(), now_ts);
                    Ok(false)
                }
                None => {
                    // 老数据兼容：BothChanged 但无 snapshot path → 回退 pending_take_remote。
                    Ok(true)
                }
            },
        }
    }

    /// Resolve a conflict by marking it as manually merged.
    ///
    /// Removes the path from `conflicted_files` and `conflicts`, and updates
    /// `known_files` to the remote hash so the next sync sees
    /// base=remote_hash, local≠base, remote=base → LocalChanged → uploads
    /// the merged version.
    pub fn resolve_conflict_mark_merged(sync_root: &Path, path: &str) -> crate::Result<()> {
        let mut state = Self::load_sync_state(sync_root)?;
        if !state.conflicted_files.remove(path) {
            return Err(crate::Error::Other(format!(
                "resolve_conflict_mark_merged: path '{}' is not in conflicted_files",
                path
            )));
        }
        // Set known_files to the remote_hash so that three-way comparison on the
        // next sync sees: base=remote_hash, local≠base, remote=base → LocalChanged → upload.
        // This ensures the merged local version gets uploaded to the remote.
        if let Some(conflict) = state.conflicts.iter().find(|c| c.local_path == path) {
            state
                .known_files
                .insert(path.to_string(), conflict.remote_hash.clone());
            if let Some(t) = state
                .known_files_updated_at
                .get(&conflict.remote_path)
                .cloned()
            {
                state.known_files_updated_at.insert(path.to_string(), t);
            }
        } else {
            // Fallback: if no conflict record, use the current local file hash.
            let full_path = sync_root.join(path);
            if full_path.exists() {
                let content = std::fs::read(&full_path)?;
                let hash = crate::sync::hash::content_md5(&content);
                state.known_files.insert(path.to_string(), hash);
            }
        }
        // Remove the conflict record from state.conflicts and conflicts.json
        // via the shared in-memory helper.
        let mut conflicts_json = load_conflicts_json(sync_root)?;
        remove_conflict_in_memory(&mut state, &mut conflicts_json, path);
        persist_conflict_state(sync_root, &state, &conflicts_json)?;
        Ok(())
    }

    /// 加载冲突预览 — 返回本地/远端内容供平台层展示。
    ///
    /// 从 `conflicts.json` 找到对应 path 的冲突记录，读取本地正文和远端 snapshot
    /// （`BothChanged` 时）。平台层只拿返回的 [`SyncConflictPreview`]，不直接读
    /// `conflicts.json` 或拼磁盘路径。
    pub fn load_conflict_preview(
        sync_root: &Path,
        path: &str,
    ) -> crate::Result<crate::sync::types::SyncConflictPreview> {
        let conflicts = load_conflicts_json(sync_root)?;
        let conflict = conflicts
            .iter()
            .find(|c| c.local_path == path)
            .ok_or_else(|| {
                crate::Error::Other(format!(
                    "load_conflict_preview: no conflict record for path '{}'",
                    path
                ))
            })?;

        // 读取本地正文。
        let local_full_path = sync_root.join(path);
        let local_content = if local_full_path.exists() {
            std::fs::read_to_string(&local_full_path).map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "load_conflict_preview: read local {}: {}",
                    path, e
                )))
            })?
        } else {
            String::new()
        };

        let (remote_content, remote_deleted) = match conflict.kind {
            SyncConflictKind::RemoteDeleted => (None, true),
            SyncConflictKind::BothChanged => match &conflict.remote_snapshot_path {
                Some(snapshot_rel) => {
                    let snapshot_path = sync_root.join(snapshot_rel);
                    let content = std::fs::read_to_string(&snapshot_path).map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "load_conflict_preview: read remote snapshot {}: {}",
                            snapshot_rel, e
                        )))
                    })?;
                    (Some(content), false)
                }
                None => (None, false),
            },
        };

        Ok(crate::sync::types::SyncConflictPreview {
            path: path.to_string(),
            kind: conflict.kind,
            created_at: conflict.created_at,
            local_content,
            remote_content,
            remote_deleted,
        })
    }

    /// 列出当前项目的所有冲突记录。
    ///
    /// 从 `conflicts.json` 读取，供平台层展示冲突列表。
    pub fn list_conflicts(sync_root: &Path) -> crate::Result<Vec<SyncConflict>> {
        load_conflicts_json(sync_root)
    }
}
