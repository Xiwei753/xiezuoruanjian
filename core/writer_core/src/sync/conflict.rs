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
/// 只有文件不存在才是空冲突；读取/解析失败返回 Err，绝不把损坏记录当成
/// "没有冲突"（Issue #1066 评论 #7237527352）。
pub(crate) fn load_conflicts_json(sync_root: &Path) -> crate::Result<Vec<SyncConflict>> {
    let conflicts_path = sync_root.join("app-meta/sync/conflicts.json");
    load_conflicts_json_at_path(&conflicts_path)
}

pub(crate) fn load_conflicts_json_for_group(
    sync_root: &Path,
    group: &crate::sync::types::AppDataGroup,
) -> crate::Result<Vec<SyncConflict>> {
    let group_path = sync_root.join(group.local_conflicts_path());
    if group_path.exists() {
        return Ok(load_conflicts_json_at_path(&group_path)?
            .into_iter()
            .filter(|conflict| group.owns_path(&conflict.local_path))
            .collect());
    }

    // The old flat conflict file is migration input only. Once a group's
    // canonical file exists, it is the sole source for that group's conflicts.
    Ok(load_conflicts_json(sync_root)?
        .into_iter()
        .filter(|conflict| group.owns_path(&conflict.local_path))
        .collect())
}

fn load_conflicts_json_at_path(conflicts_path: &Path) -> crate::Result<Vec<SyncConflict>> {
    if !conflicts_path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&conflicts_path)?;
    serde_json::from_str(&content).map_err(|error| {
        crate::Error::Other(format!(
            "conflicts file {} is unreadable: {error}",
            conflicts_path.display()
        ))
    })
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
    persist_conflict_state_at_paths(
        sync_root,
        "app-meta/sync/state.local.json",
        state,
        "app-meta/sync/conflicts.json",
        conflicts,
    )
}

fn persist_conflict_state_for_group(
    sync_root: &Path,
    group: &crate::sync::types::AppDataGroup,
    state: &crate::sync::types::SyncState,
    conflicts: &[SyncConflict],
) -> crate::Result<()> {
    persist_conflict_state_at_paths(
        sync_root,
        &group.local_state_path(),
        state,
        &group.local_conflicts_path(),
        conflicts,
    )
}

fn persist_conflict_state_at_paths(
    sync_root: &Path,
    state_path: &str,
    state: &crate::sync::types::SyncState,
    conflicts_path: &str,
    conflicts: &[SyncConflict],
) -> crate::Result<()> {
    let state_json = serde_json::to_string_pretty(state)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
    let conflicts_json = serde_json::to_string_pretty(conflicts)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;

    let mut tx = crate::storage::transaction::SaveTransaction::new(sync_root);
    tx.add_bytes(state_path, state_json.as_bytes())?;
    tx.add_bytes(conflicts_path, conflicts_json.as_bytes())?;
    tx.commit()?;
    Ok(())
}

fn load_conflict_state(
    sync_root: &Path,
    group: Option<&crate::sync::types::AppDataGroup>,
) -> crate::Result<crate::sync::types::SyncState> {
    match group {
        Some(group) => crate::sync::SyncService::load_sync_state_for_target(
            sync_root,
            &crate::sync::types::SyncTarget::app_data_group(group.clone()),
        ),
        None => crate::sync::SyncService::load_sync_state(sync_root),
    }
}

fn load_conflicts_for_target(
    sync_root: &Path,
    group: Option<&crate::sync::types::AppDataGroup>,
) -> crate::Result<Vec<SyncConflict>> {
    match group {
        Some(group) => load_conflicts_json_for_group(sync_root, group),
        None => load_conflicts_json(sync_root),
    }
}

fn persist_conflict_state_for_target(
    sync_root: &Path,
    group: Option<&crate::sync::types::AppDataGroup>,
    state: &crate::sync::types::SyncState,
    conflicts: &[SyncConflict],
) -> crate::Result<()> {
    match group {
        Some(group) => persist_conflict_state_for_group(sync_root, group, state, conflicts),
        None => persist_conflict_state(sync_root, state, conflicts),
    }
}

fn validate_group_conflict_path(
    group: Option<&crate::sync::types::AppDataGroup>,
    path: &str,
    operation: &str,
) -> crate::Result<()> {
    if let Some(group) = group {
        if !group.owns_path(path) {
            return Err(crate::Error::Other(format!(
                "{operation}: path '{}' is outside App data group '{}'",
                path,
                group.id()
            )));
        }
    }
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
    record_staging_conflicts_for_group(
        sync_root,
        remote_prefix,
        staging_conflicts,
        existing_conflicts,
        None,
    )
}

/// Group-aware App variant. A `None` group preserves the flat Project/legacy path;
/// an App group reads and writes only its own state and canonical conflicts file.
pub fn record_staging_conflicts_for_group(
    sync_root: &Path,
    remote_prefix: &str,
    staging_conflicts: &[crate::sync::staging::StagingConflict],
    existing_conflicts: &[SyncConflict],
    app_group: Option<&crate::sync::types::AppDataGroup>,
) -> crate::Result<Vec<SyncConflict>> {
    let group_conflicts: Vec<SyncConflict> = existing_conflicts
        .iter()
        .filter(|conflict| app_group.map_or(true, |group| group.owns_path(&conflict.local_path)))
        .cloned()
        .collect();

    if let Some(group) = app_group {
        for conflict in staging_conflicts {
            let path = conflict.rel_path.to_string_lossy();
            if !group.owns_path(&path) {
                return Err(crate::Error::Other(format!(
                    "record_staging_conflicts: path '{}' is outside App data group '{}'",
                    path,
                    group.id()
                )));
            }
        }
    }

    if staging_conflicts.is_empty() && group_conflicts.is_empty() {
        return Ok(Vec::new());
    }

    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0);

    // 先在内存里构造完整的新状态。
    let mut state = load_conflict_state(sync_root, app_group)?;
    let mut conflicts_json = load_conflicts_for_target(sync_root, app_group)?;

    // 先把 existing_conflicts（Transfer 冲突）合并进来，
    // 确保持久化状态包含两层冲突。
    for ec in &group_conflicts {
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
            // graph 级 BothChanged 时保存的远端整图快照目录；
            // 非 graph 级冲突或 RemoteDeleted 时为 None。
            remote_graph_snapshot_dir: sc.remote_graph_snapshot_dir.clone(),
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
    persist_conflict_state_for_target(sync_root, app_group, &state, &conflicts_json)?;

    // 返回合并后的完整冲突列表（existing new staging）。
    Ok(merge_sync_conflicts(
        &group_conflicts,
        &new_staging_conflicts,
    ))
}

/// 远端星图 index 身份事实（issue #902 第十五轮复核）。
///
/// 由 `save_graph_conflict_snapshot` 写入快照目录下的 `index_facts.json`，
/// `apply_starmap_graph_remote_modify` 读取后恢复 `root_starmap_ids` 和
/// `main_starmap_by_project`。与
/// `crate::sync::commit_helpers::StarmapIdentityFacts` 字段一致，
/// 此处仅需反序列化。
#[derive(serde::Deserialize)]
struct StarmapIdentityFacts {
    is_root: bool,
    main_for_projects: Vec<String>,
}

/// 用户在冲突列表里的三种选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConflictChoice {
    KeepLocal,
    TakeRemote,
    MarkMerged,
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
        Self::record_sync_conflict_at_target(sync_root, conflict, local_content, None)
    }

    pub fn record_sync_conflict_for_group(
        sync_root: &Path,
        conflict: SyncConflict,
        local_content: Option<&str>,
        group: &crate::sync::types::AppDataGroup,
    ) -> crate::Result<()> {
        if !group.owns_path(&conflict.local_path) {
            return Err(crate::Error::Other(format!(
                "record_sync_conflict: path '{}' is outside App data group '{}'",
                conflict.local_path,
                group.id()
            )));
        }
        Self::record_sync_conflict_at_target(sync_root, conflict, local_content, Some(group))
    }

    fn record_sync_conflict_at_target(
        sync_root: &Path,
        conflict: SyncConflict,
        local_content: Option<&str>,
        group: Option<&crate::sync::types::AppDataGroup>,
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
        let mut state = load_conflict_state(sync_root, group)?;
        let mut conflicts_json = load_conflicts_for_target(sync_root, group)?;

        upsert_conflict(
            &mut conflicts_json,
            &mut state.conflicts,
            &mut state.conflicted_files,
            conflict,
        );

        // 一次事务写 state + conflicts.json。
        persist_conflict_state_for_target(sync_root, group, &state, &conflicts_json)?;
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
    /// 用户在冲突列表里的三种选择 — 都只应用当前对象的决议。
    ///
    /// 选择结果通过一次正常对象事务写回：更新本地已确认版本 / 删除事实，
    /// 不在同步时另加"用户选过了"的状态。
    fn resolve_object_conflict(
        sync_root: &Path,
        path: &str,
        group: Option<&crate::sync::types::AppDataGroup>,
        guard: Option<&crate::storage::WorkspaceWriteLock>,
        app_data_root: Option<&Path>,
        choice: ConflictChoice,
    ) -> crate::Result<bool> {
        let operation = match choice {
            ConflictChoice::KeepLocal => "resolve_conflict_keep_local",
            ConflictChoice::TakeRemote => "resolve_conflict_take_remote",
            ConflictChoice::MarkMerged => "resolve_conflict_mark_merged",
        };
        validate_group_conflict_path(group, path, operation)?;
        let mut state = load_conflict_state(sync_root, group)?;
        let mut conflicts_json = load_conflicts_for_target(sync_root, group)?;
        let conflict = Self::load_canonical_conflict_and_repair_mirror(
            path,
            &mut state,
            &mut conflicts_json,
            operation,
        )?;
        if !state.conflicted_files.contains(path) {
            return Err(crate::Error::Other(format!(
                "{operation}: path '{path}' is not in conflicted_files"
            )));
        }

        let applied_live = match choice {
            ConflictChoice::KeepLocal => {
                Self::apply_keep_local_choice(sync_root, path, &conflict, &mut state);
                false
            }
            ConflictChoice::MarkMerged => {
                Self::apply_mark_merged_choice(path, &conflict, &mut state);
                false
            }
            ConflictChoice::TakeRemote => {
                let queued = Self::apply_take_remote_choice(
                    sync_root,
                    path,
                    &conflict,
                    &mut state,
                    guard,
                    app_data_root,
                )?;
                if queued {
                    // 还没有真实采用远端内容：冲突记录与队列都保留，
                    // 不能告诉 UI 已经应用。
                    persist_conflict_state_for_target(sync_root, group, &state, &conflicts_json)?;
                    return Ok(false);
                }
                true
            }
        };
        if !applied_live {
            Self::register_chosen_local_mutation(sync_root, path, &state)?;
        }
        state.conflicted_files.remove(path);
        remove_conflict_in_memory(&mut state, &mut conflicts_json, path);
        persist_conflict_state_for_target(sync_root, group, &state, &conflicts_json)?;
        Ok(applied_live)
    }

    /// 保留本地版本：把共同基线推进到"用户选择后的版本事实"，
    /// 下一轮按正常对象比较传播本地版本，不再回到同一个冲突。
    fn apply_keep_local_choice(
        sync_root: &Path,
        path: &str,
        conflict: &SyncConflict,
        state: &mut crate::sync::types::SyncState,
    ) {
        match conflict.kind {
            // 远端删除、本地修改：基线变成"已删除"，下一轮本地内容按新增上传。
            SyncConflictKind::RemoteDeleted => {
                state.known_files.remove(path);
                state.known_files_updated_at.remove(path);
                state.deleted_files.insert(path.to_string());
                state.pending_take_remote.remove(path);
            }
            // 双端修改 / 本地删除远端修改：基线取远端版本，本地删除或本地内容
            // 都在下一轮相对该基线传播。
            SyncConflictKind::BothChanged => {
                if let Some(hash) = Self::canonical_known_hash(sync_root, path, conflict) {
                    state.known_files.insert(path.to_string(), hash);
                    state
                        .known_files_updated_at
                        .insert(path.to_string(), chrono::Utc::now().timestamp_millis());
                }
                state.deleted_files.remove(path);
                state.pending_take_remote.remove(path);
            }
        }
    }

    fn apply_mark_merged_choice(
        path: &str,
        conflict: &SyncConflict,
        state: &mut crate::sync::types::SyncState,
    ) {
        match conflict.kind {
            SyncConflictKind::RemoteDeleted => {
                state.known_files.remove(path);
                state.known_files_updated_at.remove(path);
                state.deleted_files.insert(path.to_string());
            }
            SyncConflictKind::BothChanged => {
                let remote_hash = conflict.remote_hash.clone();
                if !remote_hash.is_empty() {
                    state.known_files.insert(path.to_string(), remote_hash);
                    state
                        .known_files_updated_at
                        .insert(path.to_string(), chrono::Utc::now().timestamp_millis());
                }
                state.deleted_files.remove(path);
            }
        }
        state.pending_take_remote.remove(path);
    }

    fn apply_take_remote_choice(
        sync_root: &Path,
        path: &str,
        conflict: &SyncConflict,
        state: &mut crate::sync::types::SyncState,
        guard: Option<&crate::storage::WorkspaceWriteLock>,
        app_data_root: Option<&Path>,
    ) -> crate::Result<bool> {
        match conflict.kind {
            SyncConflictKind::RemoteDeleted => {
                if let (Some((volume_id, chapter_id)), Some(app_data_root)) =
                    (Self::chapter_sync_path_parts(path), app_data_root)
                {
                    crate::chapter::delete_chapter(
                        sync_root,
                        &volume_id,
                        &chapter_id,
                        app_data_root,
                    )?;
                } else {
                    let paths = [path.to_string()];
                    crate::sync::lww::move_to_trash(sync_root, &paths)?;
                }
                state.known_files.remove(path);
                state.known_files_updated_at.remove(path);
                state.deleted_files.insert(path.to_string());
                state.pending_take_remote.remove(path);
                Self::settle_replaced_ledger(sync_root, path, state)?;
            }
            SyncConflictKind::BothChanged => {
                let Some(snapshot_rel) = conflict.remote_snapshot_path.as_deref() else {
                    // 旧记录没有快照：本轮无法落盘远端内容，排队下一轮重新下载。
                    state.pending_take_remote.insert(path.to_string());
                    return Ok(true);
                };
                Self::apply_remote_snapshot(sync_root, path, snapshot_rel, guard)?;
                let hash = Self::canonical_known_hash(sync_root, path, conflict);
                if let Some(hash) = hash {
                    state.known_files.insert(path.to_string(), hash);
                    state
                        .known_files_updated_at
                        .insert(path.to_string(), chrono::Utc::now().timestamp_millis());
                }
                state.deleted_files.remove(path);
                state.tombstones.retain(|tombstone| tombstone.original_path != path);
                state.pending_take_remote.remove(path);
                Self::settle_replaced_ledger(sync_root, path, state)?;
            }
        }
        Ok(false)
    }

    /// 远端内容已覆盖本地对象：同一条持久状态里结算被覆盖的账本序号，
    /// 下一轮不会再用旧 delete/upsert 事实把远端版本删掉。
    fn settle_replaced_ledger(
        sync_root: &Path,
        path: &str,
        state: &mut crate::sync::types::SyncState,
    ) -> crate::Result<()> {
        let seq = crate::sync::local_mutations::latest_ledger_seq(sync_root, path)?;
        if seq > 0 {
            state
                .confirmed_mutation_seq
                .insert(path.to_string(), seq);
        }
        Ok(())
    }

    /// 保留本地/手工合并后，把当前实际内容或删除登记为一条新的普通 mutation。
    fn register_chosen_local_mutation(
        sync_root: &Path,
        path: &str,
        state: &crate::sync::types::SyncState,
    ) -> crate::Result<()> {
        let exists = sync_root.join(path).is_file();
        let (upserted, deleted) = if exists {
            (vec![path.to_string()], Vec::new())
        } else {
            (Vec::new(), vec![path.to_string()])
        };
        let records = crate::sync::local_mutations::records_for_committed_paths(
            sync_root,
            &format!("resolve_conflict:{path}"),
            &state.device_id,
            &upserted,
            &deleted,
            chrono::Utc::now().timestamp_millis(),
        )?;
        crate::sync::local_mutations::append_batch(sync_root, &records)
    }

    /// 解析 `volumes/<volume>/chapters/<chapter>/chapter.md` 路径。
    fn chapter_sync_path_parts(path: &str) -> Option<(String, String)> {
        let components: Vec<&str> = path.split('/').collect();
        if components.len() == 5
            && components[0] == "volumes"
            && components[2] == "chapters"
            && components[4] == "chapter.md"
        {
            Some((components[1].to_string(), components[3].to_string()))
        } else {
            None
        }
    }

    fn apply_remote_snapshot(
        sync_root: &Path,
        path: &str,
        snapshot_rel: &str,
        guard: Option<&crate::storage::WorkspaceWriteLock>,
    ) -> crate::Result<()> {
        let snapshot_path = sync_root.join(snapshot_rel);
        let remote_content = std::fs::read(&snapshot_path).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "resolve_conflict_take_remote: read remote snapshot {}: {}",
                snapshot_rel, e
            )))
        })?;
        let local_full_path = sync_root.join(path);
        if let Some((volume_id, chapter_id)) = Self::chapter_sync_path_parts(path) {
            let project_id = sync_root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string();
            if project_id.is_empty() {
                return Err(crate::Error::Other(
                    "resolve_conflict_take_remote: cannot derive project id".to_string(),
                ));
            }
            let remote_text = String::from_utf8(remote_content.clone()).map_err(|e| {
                crate::Error::Other(format!(
                    "resolve_conflict_take_remote: remote chapter is not UTF-8 {}: {}",
                    path, e
                ))
            })?;
            let Some(guard) = guard else {
                return Err(crate::Error::Other(
                    "resolve_conflict_take_remote: chapter restore requires workspace guard"
                        .to_string(),
                ));
            };
            let (_, local_content, expected) =
                crate::chapter::commit::read_chapter_state_recovering(
                    sync_root,
                    guard,
                    &volume_id,
                    &chapter_id,
                )?;
            let mut request = crate::chapter::ChapterCommitRequest::new(
                crate::chapter::ChapterKey::new(project_id, volume_id, chapter_id),
                remote_text,
            );
            request.expected = Some(expected);
            request.base_content = Some(local_content);
            request.mutation = crate::chapter::ChapterMutationKind::Restore;
            request.source = crate::chapter::ChapterCommitSource::Restore;
            request.allow_empty_overwrite = request.content.is_empty();
            match crate::chapter::commit::commit_chapter_with_guard(
                sync_root,
                Some(guard),
                request,
            )? {
                crate::chapter::ChapterCommitOutcome::Committed(_) => return Ok(()),
                crate::chapter::ChapterCommitOutcome::Conflict(conflict) => {
                    return Err(crate::Error::Other(format!(
                        "resolve_conflict_take_remote: chapter version advanced during restore: {}",
                        conflict.key.chapter_id
                    )));
                }
            }
        }
        crate::storage::transaction::atomic_write_bytes(&local_full_path, &remote_content)
            .map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "resolve_conflict_take_remote: write local {}: {}",
                    path, e
                )))
            })?;
        Ok(())
    }

    pub fn resolve_conflict_keep_local(sync_root: &Path, path: &str) -> crate::Result<()> {
        Self::resolve_object_conflict(
            sync_root,
            path,
            None,
            None,
            None,
            ConflictChoice::KeepLocal,
        )
        .map(|_| ())
    }

    pub fn resolve_conflict_keep_local_for_group(
        sync_root: &Path,
        path: &str,
        group: &crate::sync::types::AppDataGroup,
    ) -> crate::Result<()> {
        Self::resolve_object_conflict(
            sync_root,
            path,
            Some(group),
            None,
            None,
            ConflictChoice::KeepLocal,
        )
        .map(|_| ())
    }

    pub fn resolve_conflict_take_remote(sync_root: &Path, path: &str) -> crate::Result<bool> {
        let guard = crate::storage::WorkspaceWriteLock::acquire(sync_root).ok();
        Self::resolve_object_conflict(
            sync_root,
            path,
            None,
            guard.as_ref(),
            None,
            ConflictChoice::TakeRemote,
        )
    }

    pub fn resolve_conflict_take_remote_with_guard(
        sync_root: &Path,
        guard: &crate::storage::WorkspaceWriteLock,
        app_data_root: Option<&Path>,
        path: &str,
    ) -> crate::Result<bool> {
        Self::resolve_object_conflict(
            sync_root,
            path,
            None,
            Some(guard),
            app_data_root,
            ConflictChoice::TakeRemote,
        )
    }

    pub fn resolve_conflict_take_remote_for_group(
        sync_root: &Path,
        path: &str,
        group: &crate::sync::types::AppDataGroup,
    ) -> crate::Result<bool> {
        let guard = crate::storage::WorkspaceWriteLock::acquire(sync_root).ok();
        Self::resolve_object_conflict(
            sync_root,
            path,
            Some(group),
            guard.as_ref(),
            None,
            ConflictChoice::TakeRemote,
        )
    }

    pub fn resolve_conflict_take_remote_for_group_with_guard(
        sync_root: &Path,
        guard: &crate::storage::WorkspaceWriteLock,
        app_data_root: Option<&Path>,
        path: &str,
        group: &crate::sync::types::AppDataGroup,
    ) -> crate::Result<bool> {
        Self::resolve_object_conflict(
            sync_root,
            path,
            Some(group),
            Some(guard),
            app_data_root,
            ConflictChoice::TakeRemote,
        )
    }

    pub fn resolve_conflict_mark_merged(sync_root: &Path, path: &str) -> crate::Result<()> {
        Self::resolve_object_conflict(
            sync_root,
            path,
            None,
            None,
            None,
            ConflictChoice::MarkMerged,
        )
        .map(|_| ())
    }

    pub fn resolve_conflict_mark_merged_for_group(
        sync_root: &Path,
        path: &str,
        group: &crate::sync::types::AppDataGroup,
    ) -> crate::Result<()> {
        Self::resolve_object_conflict(
            sync_root,
            path,
            Some(group),
            None,
            None,
            ConflictChoice::MarkMerged,
        )
        .map(|_| ())
    }

    pub fn load_conflict_preview(
        sync_root: &Path,
        path: &str,
    ) -> crate::Result<crate::sync::types::SyncConflictPreview> {
        Self::load_conflict_preview_at_target(sync_root, path, None)
    }

    pub fn load_conflict_preview_for_group(
        sync_root: &Path,
        path: &str,
        group: &crate::sync::types::AppDataGroup,
    ) -> crate::Result<crate::sync::types::SyncConflictPreview> {
        Self::load_conflict_preview_at_target(sync_root, path, Some(group))
    }

    fn load_conflict_preview_at_target(
        sync_root: &Path,
        path: &str,
        group: Option<&crate::sync::types::AppDataGroup>,
    ) -> crate::Result<crate::sync::types::SyncConflictPreview> {
        validate_group_conflict_path(group, path, "load_conflict_preview")?;
        let conflicts = load_conflicts_for_target(sync_root, group)?;
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

    pub fn list_conflicts_for_group(
        sync_root: &Path,
        group: &crate::sync::types::AppDataGroup,
    ) -> crate::Result<Vec<SyncConflict>> {
        load_conflicts_json_for_group(sync_root, group)
    }
}
