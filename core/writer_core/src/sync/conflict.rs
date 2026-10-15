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

/// 对齐 `state.conflicts` / `conflicted_files` 与 `conflicts.json`，使后者成为
/// 唯一事实源（canonical record），前者完全由后者重建。
///
/// `conflicts.json` 是用户可见/可解决的 canonical record（`list_conflicts` /
/// `load_conflict_preview` 读它）。`state.conflicts` + `conflicted_files` 是
/// 同步引擎 mirror，每次 merge 入口直接用 canonical record **重建 mirror**，
/// 不做 state -> json 回填：
/// - `conflicts.json` 有、`state.conflicts` 缺 → 自然补回 state；
/// - `conflicts.json` 和 `state.conflicts` 同 path 内容不同 → JSON 覆盖 state；
/// - `state.conflicts` 有、`conflicts.json` 缺 → 丢掉 stale mirror，不复活；
/// - 只有 `conflicted_files` 的孤儿 path → 丢掉，不再永久 skip。
///
/// 如果 JSON 曾经真的丢了一条未解决冲突，也不从旧 mirror 猜着复活；下一轮正常
/// 三路比较会重新根据 local/remote/base 检出真实冲突并重新生成完整 `SyncConflict`。
pub(crate) fn align_conflict_state_mirror(
    state: &mut crate::sync::types::SyncState,
    conflicts_json: &mut Vec<SyncConflict>,
) {
    // 1. 先按 local_path 给 conflicts.json 去重（保留最后一条）。
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    conflicts_json.retain(|c| seen.insert(c.local_path.clone()));

    // 2. state.conflicts 直接用 canonical record 重建。
    state.conflicts = conflicts_json.clone();

    // 3. state.conflicted_files 直接用 canonical record 重建。
    state.conflicted_files = conflicts_json
        .iter()
        .map(|c| c.local_path.clone())
        .collect();
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
    /// 从 `conflicts.json` 加载 canonical conflict 并修复 `state` mirror。
    ///
    /// 三个 resolve 函数（`keep_local` / `take_remote` / `mark_merged`）共用：
    /// - 如果 `conflicts.json` 没有该 path → 返回 `Err`（canonical record 不存在，
    ///   不再回退到读本地文件算 MD5 的 fallback）。
    /// - 如果 `conflicts.json` 有记录但 `state.conflicts` / `conflicted_files` 缺
    ///   → 用 canonical conflict 调用 [`upsert_conflict`] 补齐 mirror
    ///   （同时更新 `conflicts_json` / `state.conflicts` / `conflicted_files` 三件套）。
    ///
    /// 返回找到的 canonical conflict，供调用方执行 resolve 逻辑。
    fn load_canonical_conflict_and_repair_mirror(
        path: &str,
        state: &mut crate::sync::types::SyncState,
        conflicts_json: &mut Vec<SyncConflict>,
        resolve_fn_name: &str,
    ) -> crate::Result<SyncConflict> {
        let conflict = conflicts_json
            .iter()
            .find(|c| c.local_path == path)
            .cloned()
            .ok_or_else(|| {
                crate::Error::Other(format!(
                    "{}: no conflict record in conflicts.json for path '{}'",
                    resolve_fn_name, path
                ))
            })?;
        // 修复 state mirror：如果 state.conflicts 缺这条或内容旧，
        // 用 canonical conflict 补齐（upsert_conflict 同时更新三件套）。
        upsert_conflict(
            conflicts_json,
            &mut state.conflicts,
            &mut state.conflicted_files,
            conflict.clone(),
        );
        Ok(conflict)
    }

    /// 计算写入 `known_files` 的规范化内容哈希（MD5）。
    ///
    /// `resolve_conflict_keep_local` / `mark_merged` / `decide_take_remote` 的
    /// BothChanged 分支都把 `conflict.remote_hash` 写回 `state.known_files`。
    /// 旧冲突记录的 `remote_hash` 可能是 40 位 Git blob SHA（旧版本同步系统误写入），
    /// 直接写进 `known_files` 会再次污染基线，导致下一轮三路比较把空内容误判为
    /// `BothChanged` 冲突。本 helper 在写入前做 MD5 规范化：
    ///
    /// 1. **`conflict.remote_hash` 已是 MD5**：直接用（保持原行为，不破坏
    ///    `resolve_conflict_keep_local` 的不变量：known_files 必须设为 remote_hash
    ///    而非 local_hash，下次同步才会上传本地版本而非下载远端覆盖）。
    /// 2. **`conflict.remote_hash` 不是 MD5**（旧 40 位 Git blob SHA），用
    ///    `remote_snapshot_path` 找一个 MD5 替代：读 `sync_root.join(snapshot_rel)`
    ///    算 MD5。BothChanged 冲突保存了远端副本快照，这是远端内容的事实来源，
    ///    算出来的 MD5 等价于 remote_hash 应有的值。
    /// 3. **都不行 → `None`**：不伪造内容哈希，调用方跳过 `known_files` 写入。
    ///
    /// **只返回可证明的远端内容 MD5，不读本地文件冒充远端 hash。** 旧版本曾把
    /// "读本地文件算 MD5" 作为最后 fallback，但在 `RemoteDeleted` 场景下会把本地
    /// 正文 MD5 写进 `known_files`，下一轮三路比较变成 local_hash==base_hash、
    /// 远端 delete 的 remote_hash!=base_hash → 误判为 `RemoteChanged` → 走
    /// `DeleteLocal`，用户明明点了"保留本地"却把本地正文移进 trash。删除本地
    /// fallback 后，`RemoteDeleted` 由调用方按 kind 直接 `known_files.remove(path)`，
    /// 不再走到本 helper。
    ///
    /// 读 snapshot 失败时返回 `None`（不向上传播 IO 错误，让调用方决定是否跳过
    /// `known_files` 写入）。本 helper 只读不写，无副作用。
    fn canonical_known_hash(
        sync_root: &Path,
        _path: &str,
        conflict: &SyncConflict,
    ) -> Option<String> {
        // 1. conflict.remote_hash 已是 MD5 → 直接用，保持原行为。
        if crate::sync::hash::is_md5_content_hash(&conflict.remote_hash) {
            return Some(conflict.remote_hash.clone());
        }
        // 2. conflict.remote_hash 不是 MD5（旧 40 位 Git blob SHA）：
        //    用 remote_snapshot_path 算 MD5（远端副本是远端内容的事实来源）。
        if let Some(snapshot_rel) = &conflict.remote_snapshot_path {
            let snapshot_path = sync_root.join(snapshot_rel);
            if let Ok(content) = std::fs::read(&snapshot_path) {
                return Some(crate::sync::hash::content_md5(&content));
            }
        }
        // 3. 都不行 → None，不伪造内容哈希（不读本地文件冒充远端 hash）。
        None
    }

    /// BothChanged 分支共用：把可证明的远端 MD5 写入 `known_files` /
    /// `known_files_updated_at`。`canonical_known_hash` 返回 `None` 时不写入
    /// （不拿本地 hash 冒充远端 hash）。`known_files_updated_at` 沿用远端
    /// `remote_path` 对应的时间戳。
    ///
    /// 抽成 helper 是为了降低 `resolve_conflict_keep_local` / `mark_merged` 里
    /// `if let Some(conflict) → match kind → BothChanged → if let Some(hash)` 的
    /// 嵌套层级，满足 clippy `excessive_nesting` 守卫。
    fn apply_both_changed_known_base(
        sync_root: &Path,
        path: &str,
        state: &mut crate::sync::types::SyncState,
        conflict: &SyncConflict,
    ) {
        // 写 known_files 前做 MD5 规范化：旧冲突记录的 remote_hash 可能是
        // 40 位 Git blob SHA，直接写进 known_files 会再次污染基线。
        // canonical_known_hash 只返回可证明的远端 MD5（remote_hash 或 snapshot），
        // 返回 None 时跳过 known_files 写入（不 insert 污染值）。
        if let Some(hash) = Self::canonical_known_hash(sync_root, path, conflict) {
            state.known_files.insert(path.to_string(), hash);
        }
        if let Some(t) = state
            .known_files_updated_at
            .get(&conflict.remote_path)
            .cloned()
        {
            state.known_files_updated_at.insert(path.to_string(), t);
        }
    }

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
    ///
    /// 按 `conflict.kind` 区分：
    /// - **`RemoteDeleted`**：远端已删除，直接移除 known base（`known_files` /
    ///   `known_files_updated_at`）。下一轮三路比较 base 默认为空，和远端 delete
    ///   的空 hash 对齐，得到 `LocalChanged` → 正确上传用户保留的本地正文。
    ///   不能把本地 MD5 写进 `known_files` 冒充远端 hash——那会让下一轮误判为
    ///   `RemoteChanged` → `DeleteLocal`，把用户保留的本地正文移进 trash。
    /// - **`BothChanged`**：只有 `canonical_known_hash()` 返回可证明的远端 MD5
    ///   时才覆盖 `known_files`。返回 `None` 时不拿本地 hash 冒充远端 hash。
    pub fn resolve_conflict_keep_local(sync_root: &Path, path: &str) -> crate::Result<()> {
        let mut state = Self::load_sync_state(sync_root)?;
        let mut conflicts_json = load_conflicts_json(sync_root)?;

        // 以 conflicts.json 为 canonical record：找不到直接返回 Err，
        // 不再回退到读本地文件算 MD5 的 fallback（那会用本地 MD5 冒充远端基线）。
        // 如果 state mirror 缺这条，用 canonical conflict 补齐。
        let conflict = Self::load_canonical_conflict_and_repair_mirror(
            path,
            &mut state,
            &mut conflicts_json,
            "resolve_conflict_keep_local",
        )?;

        // BothChanged + canonical_known_hash == None 时返回 Err，保持冲突原样。
        // 不调用 remove_conflict_in_memory，不返回"解决成功"。
        if conflict.kind == SyncConflictKind::BothChanged
            && Self::canonical_known_hash(sync_root, path, &conflict).is_none()
        {
            return Err(crate::Error::Other(format!(
                "resolve_conflict_keep_local: path '{}' has BothChanged conflict with \
                 non-MD5 remote_hash and no remote_snapshot_path — cannot resolve \
                 without a proven remote content hash; run a sync to normalize the \
                 conflict record first",
                path
            )));
        }

        // canonical conflict 已找到并修复 mirror，conflicted_files 必含该 path。
        // remove 同时校验存在性：如果仍不在则属于内部不一致，返回 Err。
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
        match conflict.kind {
            SyncConflictKind::RemoteDeleted => {
                // RemoteDeleted：远端已删除，直接移除 known base。
                // 下一轮三路比较 base 默认为空，和远端 delete 的空 hash 对齐，
                // 得到 LocalChanged → 正确上传用户保留的本地正文。
                // 不能把本地 MD5 写进 known_files 冒充远端 hash——那会让下一轮
                // 误判为 RemoteChanged → DeleteLocal，把用户保留的本地正文移进 trash。
                state.known_files.remove(path);
                state.known_files_updated_at.remove(path);
            }
            SyncConflictKind::BothChanged => {
                // BothChanged：只有 canonical_known_hash() 返回可证明的远端 MD5 时
                // 才覆盖 known_files。返回 None 时不拿本地 hash 冒充远端 hash。
                // （None 的情况已在上面提前返回 Err，这里 canonical_known_hash 必返回 Some。）
                Self::apply_both_changed_known_base(sync_root, path, &mut state, &conflict);
            }
        }
        // Remove the conflict record from state.conflicts and conflicts.json
        // via the shared in-memory helper.
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
        let mut conflicts_json = load_conflicts_json(sync_root)?;

        // 以 conflicts.json 为 canonical record：找不到直接返回 Err，
        // 不再回退到读本地文件算 MD5 的 fallback。如果 state mirror 缺这条，用 canonical conflict 补齐。
        let conflict = Self::load_canonical_conflict_and_repair_mirror(
            path,
            &mut state,
            &mut conflicts_json,
            "resolve_conflict_take_remote",
        )?;

        // canonical conflict 已找到并修复 mirror，conflicted_files 必含该 path。
        if !state.conflicted_files.remove(path) {
            return Err(crate::Error::Other(format!(
                "resolve_conflict_take_remote: path '{}' is not in conflicted_files",
                path
            )));
        }

        // 用 canonical conflict 传给 decide_take_remote。
        // decide_take_remote 的 None 分支（返回 Ok(true) 即 pending fallback）现在不应再被触发
        // （因为 conflict 必为 Some），但保留它作为防御性代码不删（签名不变，仍接受 Option）。
        let use_pending_fallback =
            Self::decide_take_remote(sync_root, path, &mut state, Some(conflict))?;

        if use_pending_fallback {
            // 兼容路径：标记为 pending_take_remote，下次 perform_sync 强制下载远端内容。
            state.pending_take_remote.insert(path.to_string());
        }

        // Remove the conflict record from state.conflicts and conflicts.json
        // via the shared in-memory helper.
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
                    // 星图对象文件冲突解决后恢复 graph 成员表（issue #902 第十二轮复核）。
                    // 本地删除、远端修改 Y 时，local Y 不存在，合并后 graph.node_ids 不含 Y。
                    // 用户点"采用远端"后 apply_remote_snapshot 只写回 Y.json，不会将 Y 重新
                    // 登记到 graph.json。此处补记：best-effort，失败只 warn 不阻断。
                    Self::restore_starmap_object_membership(sync_root, path);
                    // 基线更新为被选择的远端 snapshot 的 hash（MD5）。
                    // 写 known_files 前做 MD5 规范化：旧冲突记录的 remote_hash 可能是
                    // 40 位 Git blob SHA，直接写进 known_files 会再次污染基线。
                    // canonical_known_hash 优先用 remote_snapshot_path（刚写入的远端副本）
                    // 算 MD5，其次本地文件，都不行时跳过 known_files 写入。
                    if let Some(hash) = Self::canonical_known_hash(sync_root, path, &conflict) {
                        state.known_files.insert(path.to_string(), hash);
                        let now_ts = chrono::Utc::now().timestamp_millis();
                        state
                            .known_files_updated_at
                            .insert(path.to_string(), now_ts);
                    }
                    Ok(false)
                }
                None => {
                    // 老数据兼容：BothChanged 但无 snapshot path → 回退 pending_take_remote。
                    Ok(true)
                }
            },
        }
    }

    /// 星图对象文件冲突解决后恢复 graph 成员表（issue #902 第十二轮复核）。
    ///
    /// `sync_root` 是项目数据根目录（live_root）。`path` 是相对 sync_root 的
    /// 星图对象文件路径（`starmaps/<id>/<type>/<bucket>/<obj_id>.json`）。
    ///
    /// 本地删除、远端修改 Y 时，local Y 不存在，合并后 `graph.node_ids` 不含 Y。
    /// 用户点"采用远端"后 `apply_remote_snapshot` 只写回 Y.json，不会将 Y 重新
    /// 登记到 graph.json。此处补记：读取 graph.json，将 object_id 添加到成员表，
    /// 更新 revision，写回 graph.json。
    ///
    /// best-effort：graph.json 读取或更新失败时只 `warn`，不阻断冲突解决
    /// （对象文件已写回，graph 更新是 best-effort）。
    fn restore_starmap_object_membership(sync_root: &Path, path: &str) {
        use crate::starmap::store::meta::GraphMeta;
        use crate::sync::staging::starmap_merge::{is_starmap_object_file_path, parse_object_path};

        if !is_starmap_object_file_path(path) {
            return;
        }

        // 解析路径：starmaps/<id>/<type>/<bucket>/<obj_id>.json
        let normalized = path.replace('\\', "/");
        let parts: Vec<&str> = normalized.split('/').collect();
        // is_starmap_object_file_path 已验证 parts.len() == 5
        let starmap_id = parts[1];
        let Some((object_type, object_id)) = parse_object_path(std::path::Path::new(path)) else {
            log::warn!(
                "restore_starmap_object_membership: failed to parse object path '{}'",
                path
            );
            return;
        };

        let graph_rel = format!("starmaps/{}/graph.json", starmap_id);
        let graph_path = sync_root.join(&graph_rel);

        // 读取 graph.json
        let graph_bytes = match std::fs::read(&graph_path) {
            Ok(b) => b,
            Err(e) => {
                log::warn!(
                    "restore_starmap_object_membership: failed to read graph.json for \
                     starmap '{}': {} (object '{}' not re-registered, best-effort)",
                    starmap_id,
                    e,
                    object_id
                );
                return;
            }
        };

        let mut graph: GraphMeta = match serde_json::from_slice(&graph_bytes) {
            Ok(g) => g,
            Err(e) => {
                log::warn!(
                    "restore_starmap_object_membership: failed to parse graph.json for \
                     starmap '{}': {} (object '{}' not re-registered, best-effort)",
                    starmap_id,
                    e,
                    object_id
                );
                return;
            }
        };

        // 将 object_id 添加到对应成员表和 revision
        if !Self::add_object_to_graph_members(&mut graph, &object_type, &object_id) {
            return;
        }

        // 更新 updated_at
        let now_ms = u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        )
        .unwrap_or(u64::MAX);
        graph.updated_at = std::cmp::max(graph.updated_at, now_ms);

        // 序列化写回 graph.json
        match serde_json::to_vec_pretty(&graph) {
            Ok(bytes) => {
                if let Err(e) = crate::storage::transaction::atomic_write_bytes(&graph_path, &bytes)
                {
                    log::warn!(
                        "restore_starmap_object_membership: failed to write graph.json for \
                         starmap '{}': {} (object '{}' not re-registered, best-effort)",
                        starmap_id,
                        e,
                        object_id
                    );
                }
            }
            Err(e) => {
                log::warn!(
                    "restore_starmap_object_membership: failed to serialize graph.json for \
                     starmap '{}': {} (object '{}' not re-registered, best-effort)",
                    starmap_id,
                    e,
                    object_id
                );
            }
        }
    }

    /// 将对象 ID 添加到 graph 成员表和 revision（issue #902 第十二轮复核）。
    ///
    /// 返回 true 表示已添加/更新，false 表示未知对象类型。
    fn add_object_to_graph_members(
        graph: &mut crate::starmap::store::meta::GraphMeta,
        object_type: &str,
        object_id: &str,
    ) -> bool {
        let new_pr = graph.package_revision.wrapping_add(1);
        let added = match object_type {
            "node" => {
                if !graph.node_ids.iter().any(|x| x == object_id) {
                    graph.node_ids.push(object_id.to_string());
                }
                graph.node_revisions.insert(object_id.to_string(), new_pr);
                true
            }
            "edge" => {
                if !graph.edge_ids.iter().any(|x| x == object_id) {
                    graph.edge_ids.push(object_id.to_string());
                }
                graph.edge_revisions.insert(object_id.to_string(), new_pr);
                true
            }
            "embed" => {
                if !graph.embed_instance_ids.iter().any(|x| x == object_id) {
                    graph.embed_instance_ids.push(object_id.to_string());
                }
                graph.embed_revisions.insert(object_id.to_string(), new_pr);
                true
            }
            "link" => {
                if !graph.link_ids.iter().any(|x| x == object_id) {
                    graph.link_ids.push(object_id.to_string());
                }
                graph.link_revisions.insert(object_id.to_string(), new_pr);
                true
            }
            "hyperlink" => {
                if !graph.hyperlink_ids.iter().any(|x| x == object_id) {
                    graph.hyperlink_ids.push(object_id.to_string());
                }
                graph
                    .hyperlink_revisions
                    .insert(object_id.to_string(), new_pr);
                true
            }
            _ => false,
        };
        if added {
            graph.package_revision = new_pr;
        }
        added
    }

    /// Resolve a conflict by marking it as manually merged.
    ///
    /// Removes the path from `conflicted_files` and `conflicts`, and updates
    /// `known_files` to the remote hash so the next sync sees
    /// base=remote_hash, local≠base, remote=base → LocalChanged → uploads
    /// the merged version.
    ///
    /// 按 `conflict.kind` 区分（与 `resolve_conflict_keep_local` 一致）：
    /// - **`RemoteDeleted`**：远端已删除，直接移除 known base（`known_files` /
    ///   `known_files_updated_at`）。下一轮三路比较 base 默认为空，和远端 delete
    ///   的空 hash 对齐，得到 `LocalChanged` → 正确上传用户合并后的本地正文。
    /// - **`BothChanged`**：只有 `canonical_known_hash()` 返回可证明的远端 MD5
    ///   时才覆盖 `known_files`。返回 `None` 时不拿本地 hash 冒充远端 hash。
    pub fn resolve_conflict_mark_merged(sync_root: &Path, path: &str) -> crate::Result<()> {
        let mut state = Self::load_sync_state(sync_root)?;
        let mut conflicts_json = load_conflicts_json(sync_root)?;

        // 以 conflicts.json 为 canonical record：找不到直接返回 Err，
        // 不再回退到读本地文件算 MD5 的 fallback（那会用本地 MD5 冒充远端基线）。
        // 如果 state mirror 缺这条，用 canonical conflict 补齐。
        let conflict = Self::load_canonical_conflict_and_repair_mirror(
            path,
            &mut state,
            &mut conflicts_json,
            "resolve_conflict_mark_merged",
        )?;

        // BothChanged + canonical_known_hash == None 时返回 Err，保持冲突原样。
        // 不调用 remove_conflict_in_memory，不返回"解决成功"。
        if conflict.kind == SyncConflictKind::BothChanged
            && Self::canonical_known_hash(sync_root, path, &conflict).is_none()
        {
            return Err(crate::Error::Other(format!(
                "resolve_conflict_mark_merged: path '{}' has BothChanged conflict with \
                 non-MD5 remote_hash and no remote_snapshot_path — cannot resolve \
                 without a proven remote content hash; run a sync to normalize the \
                 conflict record first",
                path
            )));
        }

        // canonical conflict 已找到并修复 mirror，conflicted_files 必含该 path。
        if !state.conflicted_files.remove(path) {
            return Err(crate::Error::Other(format!(
                "resolve_conflict_mark_merged: path '{}' is not in conflicted_files",
                path
            )));
        }
        // Set known_files to the remote_hash so that three-way comparison on the
        // next sync sees: base=remote_hash, local≠base, remote=base → LocalChanged → upload.
        // This ensures the merged local version gets uploaded to the remote.
        match conflict.kind {
            SyncConflictKind::RemoteDeleted => {
                // RemoteDeleted：远端已删除，直接移除 known base。
                // 下一轮三路比较 base 默认为空，和远端 delete 的空 hash 对齐，
                // 得到 LocalChanged → 正确上传用户合并后的本地正文。
                // 不能把本地 MD5 写进 known_files 冒充远端 hash——那会让下一轮
                // 误判为 RemoteChanged → DeleteLocal，把用户合并后的本地正文移进 trash。
                state.known_files.remove(path);
                state.known_files_updated_at.remove(path);
            }
            SyncConflictKind::BothChanged => {
                // BothChanged：只有 canonical_known_hash() 返回可证明的远端 MD5 时
                // 才覆盖 known_files。返回 None 时不拿本地 hash 冒充远端 hash。
                // （None 的情况已在上面提前返回 Err，这里 canonical_known_hash 必返回 Some。）
                Self::apply_both_changed_known_base(sync_root, path, &mut state, &conflict);
            }
        }
        // Remove the conflict record from state.conflicts and conflicts.json
        // via the shared in-memory helper.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::starmap::store::meta::GraphMeta;
    use crate::sync::SyncService;
    use tempfile::TempDir;

    /// 创建一个最小 GraphMeta JSON 用于测试。
    fn make_graph_json(starmap_id: &str) -> String {
        let graph = GraphMeta {
            schema_version: "4".to_string(),
            starmap_id: starmap_id.to_string(),
            ..Default::default()
        };
        serde_json::to_string_pretty(&graph).unwrap()
    }

    /// 读取并解析 graph.json。
    fn read_graph(sync_root: &Path, starmap_id: &str) -> GraphMeta {
        let graph_path = sync_root.join(format!("starmaps/{}/graph.json", starmap_id));
        let content = std::fs::read_to_string(&graph_path).unwrap();
        serde_json::from_str(&content).unwrap()
    }

    /// 在 sync_root 下创建 starmaps/<id>/graph.json。
    fn write_graph(sync_root: &Path, starmap_id: &str, json: &str) {
        let graph_dir = sync_root.join(format!("starmaps/{}", starmap_id));
        std::fs::create_dir_all(&graph_dir).unwrap();
        std::fs::write(graph_dir.join("graph.json"), json).unwrap();
    }

    // =========================================================================
    // 正常恢复：graph.json 不含对象 X → 调用后含 X
    // =========================================================================

    #[test]
    fn test_restore_starmap_node_membership() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_graph(root, "sm_1", &make_graph_json("sm_1"));

        // 路径：starmaps/sm_1/nodes/00/node_abc.json
        let path = "starmaps/sm_1/nodes/00/node_abc.json";
        SyncService::restore_starmap_object_membership(root, path);

        let graph = read_graph(root, "sm_1");
        assert!(
            graph.node_ids.iter().any(|x| x == "node_abc"),
            "node_abc should be in node_ids after restore"
        );
        assert_eq!(
            graph.node_revisions.get("node_abc"),
            Some(&graph.package_revision),
            "node_revisions should match package_revision"
        );
    }

    #[test]
    fn test_restore_starmap_edge_membership() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_graph(root, "sm_1", &make_graph_json("sm_1"));

        let path = "starmaps/sm_1/edges/00/edge_xyz.json";
        SyncService::restore_starmap_object_membership(root, path);

        let graph = read_graph(root, "sm_1");
        assert!(
            graph.edge_ids.iter().any(|x| x == "edge_xyz"),
            "edge_xyz should be in edge_ids after restore"
        );
    }

    #[test]
    fn test_restore_starmap_embed_membership() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_graph(root, "sm_1", &make_graph_json("sm_1"));

        let path = "starmaps/sm_1/embeds/00/emb_001.json";
        SyncService::restore_starmap_object_membership(root, path);

        let graph = read_graph(root, "sm_1");
        assert!(
            graph.embed_instance_ids.iter().any(|x| x == "emb_001"),
            "emb_001 should be in embed_instance_ids after restore"
        );
    }

    #[test]
    fn test_restore_starmap_link_membership() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_graph(root, "sm_1", &make_graph_json("sm_1"));

        let path = "starmaps/sm_1/links/00/link_001.json";
        SyncService::restore_starmap_object_membership(root, path);

        let graph = read_graph(root, "sm_1");
        assert!(
            graph.link_ids.iter().any(|x| x == "link_001"),
            "link_001 should be in link_ids after restore"
        );
    }

    #[test]
    fn test_restore_starmap_hyperlink_membership() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_graph(root, "sm_1", &make_graph_json("sm_1"));

        let path = "starmaps/sm_1/hyperlinks/00/hl_001.json";
        SyncService::restore_starmap_object_membership(root, path);

        let graph = read_graph(root, "sm_1");
        assert!(
            graph.hyperlink_ids.iter().any(|x| x == "hl_001"),
            "hl_001 should be in hyperlink_ids after restore"
        );
    }

    // =========================================================================
    // 已存在不重复添加
    // =========================================================================

    #[test]
    fn test_restore_starmap_membership_no_duplicate() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // graph.json 已含 node_abc
        let mut graph = GraphMeta {
            schema_version: "4".to_string(),
            starmap_id: "sm_1".to_string(),
            ..Default::default()
        };
        graph.node_ids.push("node_abc".to_string());
        graph.package_revision = 5;
        let json = serde_json::to_string_pretty(&graph).unwrap();
        write_graph(root, "sm_1", &json);

        let path = "starmaps/sm_1/nodes/00/node_abc.json";
        SyncService::restore_starmap_object_membership(root, path);

        let updated = read_graph(root, "sm_1");
        let count = updated.node_ids.iter().filter(|x| x == &"node_abc").count();
        assert_eq!(count, 1, "node_abc should not be duplicated in node_ids");
    }

    // =========================================================================
    // 非星图路径跳过
    // =========================================================================

    #[test]
    fn test_restore_starmap_membership_skip_non_starmap_path() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_graph(root, "sm_1", &make_graph_json("sm_1"));

        // 普通正文路径，不是星图对象文件
        let path = "chapters/ch_001/text.txt";
        SyncService::restore_starmap_object_membership(root, path);

        let graph = read_graph(root, "sm_1");
        assert!(
            graph.node_ids.is_empty(),
            "non-starmap path should not modify graph"
        );
    }

    // =========================================================================
    // graph.json 不存在：best-effort，不 panic
    // =========================================================================

    #[test]
    fn test_restore_starmap_membership_no_graph_json() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // 不创建 graph.json，函数应 best-effort 不 panic
        let path = "starmaps/sm_1/nodes/00/node_abc.json";
        SyncService::restore_starmap_object_membership(root, path);

        // graph.json 不应被创建
        assert!(
            !root.join("starmaps/sm_1/graph.json").exists(),
            "graph.json should not be created when it didn't exist"
        );
    }

    // =========================================================================
    // graph.json 内容损坏：best-effort，不 panic
    // =========================================================================

    #[test]
    fn test_restore_starmap_membership_corrupt_graph_json() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_graph(root, "sm_1", "not valid json {{{");

        let path = "starmaps/sm_1/nodes/00/node_abc.json";
        // 不应 panic
        SyncService::restore_starmap_object_membership(root, path);
    }

    // =========================================================================
    // package_revision 递增验证
    // =========================================================================

    #[test]
    fn test_restore_starmap_membership_increments_revision() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let graph = GraphMeta {
            schema_version: "4".to_string(),
            starmap_id: "sm_1".to_string(),
            package_revision: 10,
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&graph).unwrap();
        write_graph(root, "sm_1", &json);

        let path = "starmaps/sm_1/nodes/00/node_new.json";
        SyncService::restore_starmap_object_membership(root, path);

        let updated = read_graph(root, "sm_1");
        assert_eq!(
            updated.package_revision, 11,
            "package_revision should increment by 1"
        );
        assert_eq!(
            updated.node_revisions.get("node_new"),
            Some(&11u64),
            "node_revisions should be set to new package_revision"
        );
    }
}
