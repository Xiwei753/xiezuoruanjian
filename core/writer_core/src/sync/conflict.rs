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

/// 成员对象内容映射：(object_type, object_id) → 对象 JSON bytes。
type ObjectContentMap = std::collections::HashMap<(String, String), Vec<u8>>;

/// 读取 `app-meta/sync/conflicts.json`。
///
/// 文件不存在或内容损坏（半写/无效 JSON）时回退为空列表——丢失冲突记录比
/// 阻塞后续同步更可接受。
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

/// 一次事务写入权威清单 + `state.local.json` + `conflicts.json`。
///
/// 用于 LWW merge 完成后原子提交三个决议，避免分多次独立写入中间崩溃
/// 导致 manifest / state / conflicts 不一致。清单已迁移到 v2 权威存储
/// （版本化根 + 内容寻址对象 + 两级索引）：这里按对象级存储差生成计划，
/// 只写受影响的 objects/index/root，陈旧对象与索引由计划 `deletes` 回收
/// （#1062 评论 #7219676521 第 2 项）。
pub(crate) fn persist_sync_merge_result(
    sync_root: &Path,
    manifest_path: &str,
    manifest: &crate::sync::types::SyncManifest,
    state: &crate::sync::types::SyncState,
    conflicts: &[SyncConflict],
) -> crate::Result<()> {
    persist_sync_merge_result_at_paths(
        sync_root,
        manifest_path,
        manifest,
        "app-meta/sync/state.local.json",
        state,
        "app-meta/sync/conflicts.json",
        conflicts,
    )
}

/// Group-aware variant that keeps each App group's local sync metadata isolated.
pub(crate) fn persist_sync_merge_result_at_paths(
    sync_root: &Path,
    manifest_path: &str,
    manifest: &crate::sync::types::SyncManifest,
    state_path: &str,
    state: &crate::sync::types::SyncState,
    conflicts_path: &str,
    conflicts: &[SyncConflict],
) -> crate::Result<()> {
    let state_json = serde_json::to_string_pretty(state)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
    let conflicts_json = serde_json::to_string_pretty(conflicts)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;

    // known_files 是基线索，不是存储差；用完整目标清单与当前权威 root
    // 求"存储差"，只重写变化的 objects/index/root。清单语义未变化时计划为空，
    // 只提交 state/conflicts。generation 由内容派生，调用方不再生成版本串。
    let store = crate::sync::lww::ManifestStore::new(sync_root, manifest_path);
    let plan = store.plan_sync_manifest(manifest, "")?;
    let mut tx = crate::storage::transaction::SaveTransaction::new(sync_root);
    // 对象 → 索引 → root（plan.writes 最后一项是 root）；deletes 排在
    // root 之后，只回收已不被新根引用的旧对象/索引。
    for (rel, bytes) in &plan.writes {
        tx.add_bytes(rel, bytes)?;
    }
    for rel in &plan.deletes {
        tx.add_delete(rel)?;
    }
    tx.add_bytes(state_path, state_json.as_bytes())?;
    tx.add_bytes(conflicts_path, conflicts_json.as_bytes())?;
    tx.commit()?;
    Ok(())
}

/// 对象级增量的原子持久化入口。
///
/// 稀疏合并只产出 affected 范围内的 upsert/delete：这里直接调用
/// [`crate::sync::lww::ManifestStore::apply_delta`]，只写受影响的
/// object/index/root，未涉及的存储对象保持原字节；state + conflicts 与清单
/// 在同一个 [`crate::storage::transaction::SaveTransaction`] 中提交。
///
/// 非稀疏路径仍走 [`persist_sync_merge_result`]（完整清单一次迁移/重建）。
pub(crate) fn persist_sync_merge_delta(
    sync_root: &Path,
    manifest_path: &str,
    delta: &crate::sync::manifest_delta::ManifestDelta,
    state: &crate::sync::types::SyncState,
    conflicts: &[SyncConflict],
) -> crate::Result<()> {
    let state_json = serde_json::to_string_pretty(state)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
    let conflicts_json = serde_json::to_string_pretty(conflicts)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;

    let store = crate::sync::lww::ManifestStore::new(sync_root, manifest_path);
    // 根身份由最终 namespace 映射内容派生；空 delta / 语义相同 delta 不写根。
    let plan = store.apply_delta(delta, "")?;

    let mut tx = crate::storage::transaction::SaveTransaction::new(sync_root);
    // 对象 → 索引 → root；deletes 排在 root 之后，只回收已不被新根引用的
    // 旧对象/索引。未进入 delta 的 object/index 不重写。
    for (rel, bytes) in &plan.writes {
        tx.add_bytes(rel, bytes)?;
    }
    for rel in &plan.deletes {
        tx.add_delete(rel)?;
    }
    tx.add_bytes("app-meta/sync/state.local.json", state_json.as_bytes())?;
    tx.add_bytes("app-meta/sync/conflicts.json", conflicts_json.as_bytes())?;
    tx.commit()?;
    Ok(())
}

/// App group 的稀疏增量持久化：清单对象/索引/root + group state/conflicts
/// 同一 `SaveTransaction`，未涉及对象不重写。
pub(crate) fn persist_sync_merge_delta_at_paths(
    sync_root: &Path,
    manifest_path: &str,
    delta: &crate::sync::manifest_delta::ManifestDelta,
    state_path: &str,
    state: &crate::sync::types::SyncState,
    conflicts_path: &str,
    conflicts: &[SyncConflict],
) -> crate::Result<()> {
    let state_json = serde_json::to_string_pretty(state)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
    let conflicts_json = serde_json::to_string_pretty(conflicts)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
    let plan =
        crate::sync::lww::ManifestStore::new(sync_root, manifest_path).apply_delta(delta, "")?;
    let mut tx = crate::storage::transaction::SaveTransaction::new(sync_root);
    for (rel, bytes) in &plan.writes {
        tx.add_bytes(rel, bytes)?;
    }
    for rel in &plan.deletes {
        tx.add_delete(rel)?;
    }
    tx.add_bytes(state_path, state_json.as_bytes())?;
    tx.add_bytes(conflicts_path, conflicts_json.as_bytes())?;
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
    pub fn resolve_conflict_keep_local(sync_root: &Path, path: &str) -> crate::Result<()> {
        Self::resolve_conflict_keep_local_at_target(sync_root, path, None)
    }

    pub fn resolve_conflict_keep_local_for_group(
        sync_root: &Path,
        path: &str,
        group: &crate::sync::types::AppDataGroup,
    ) -> crate::Result<()> {
        Self::resolve_conflict_keep_local_at_target(sync_root, path, Some(group))
    }

    fn resolve_conflict_keep_local_at_target(
        sync_root: &Path,
        path: &str,
        group: Option<&crate::sync::types::AppDataGroup>,
    ) -> crate::Result<()> {
        validate_group_conflict_path(group, path, "resolve_conflict_keep_local")?;
        let mut state = load_conflict_state(sync_root, group)?;
        let mut conflicts_json = load_conflicts_for_target(sync_root, group)?;

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
        persist_conflict_state_for_target(sync_root, group, &state, &conflicts_json)?;
        Ok(())
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

    /// 用保存的远端 snapshot 原子替换本地正文。
    ///
    /// 读取 `sync_root/snapshot_rel` 的内容，atomic_write 到 `sync_root/path`。
    /// 返回 `Ok(true)` 表示成功替换；`Ok(false)` 表示无 snapshot path（老数据兼容）。
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
        // 章节正文必须走版本化提交协调器：冲突解决选择远端正文时，
        // 正文、meta、提交序号和后续证明批次共享同一决议；不能用普通
        // atomic_write 绕开章节版本，否则旧的后台保存仍可能覆盖这里。
        if let Some((volume_id, chapter_id)) = Self::chapter_sync_path_parts(path) {
            {
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

    /// 星图对象文件的远端 snapshot 应用：读取远端 snapshot 内容，委托
    /// [`restore_starmap_object_membership`] 用 SaveTransaction 原子提交
    /// 对象文件 + graph.json。
    fn apply_starmap_remote_snapshot(
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
        Self::restore_starmap_object_membership(sync_root, path, &remote_content)
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
        // 兼容入口（测试/旧调用方）：以 sync_root 为锁根获取 guard。生产路径
        // 必须走 *_with_guard，复用 API 已持有的 workspace guard。
        let guard = crate::storage::WorkspaceWriteLock::acquire(sync_root).ok();
        Self::resolve_conflict_take_remote_at_target(sync_root, path, None, guard.as_ref(), None)
    }

    /// 与 [`Self::resolve_conflict_take_remote`] 相同，但复用调用方已持有的
    /// workspace guard；章节正文恢复必须在该 guard 下恢复 pending journal 并
    /// 做 generation/revision/digest CAS。
    pub fn resolve_conflict_take_remote_with_guard(
        sync_root: &Path,
        guard: &crate::storage::WorkspaceWriteLock,
        app_data_root: Option<&Path>,
        path: &str,
    ) -> crate::Result<bool> {
        Self::resolve_conflict_take_remote_at_target(
            sync_root,
            path,
            None,
            Some(guard),
            app_data_root,
        )
    }

    pub fn resolve_conflict_take_remote_for_group(
        sync_root: &Path,
        path: &str,
        group: &crate::sync::types::AppDataGroup,
    ) -> crate::Result<bool> {
        let guard = crate::storage::WorkspaceWriteLock::acquire(sync_root).ok();
        Self::resolve_conflict_take_remote_at_target(
            sync_root,
            path,
            Some(group),
            guard.as_ref(),
            None,
        )
    }

    pub fn resolve_conflict_take_remote_for_group_with_guard(
        sync_root: &Path,
        guard: &crate::storage::WorkspaceWriteLock,
        app_data_root: Option<&Path>,
        path: &str,
        group: &crate::sync::types::AppDataGroup,
    ) -> crate::Result<bool> {
        Self::resolve_conflict_take_remote_at_target(
            sync_root,
            path,
            Some(group),
            Some(guard),
            app_data_root,
        )
    }

    fn resolve_conflict_take_remote_at_target(
        sync_root: &Path,
        path: &str,
        group: Option<&crate::sync::types::AppDataGroup>,
        guard: Option<&crate::storage::WorkspaceWriteLock>,
        app_data_root: Option<&Path>,
    ) -> crate::Result<bool> {
        validate_group_conflict_path(group, path, "resolve_conflict_take_remote")?;
        let mut state = load_conflict_state(sync_root, group)?;
        let mut conflicts_json = load_conflicts_for_target(sync_root, group)?;

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
        let use_pending_fallback = Self::decide_take_remote(
            sync_root,
            path,
            &mut state,
            Some(conflict),
            guard,
            app_data_root,
        )?;

        if use_pending_fallback {
            // 兼容路径：标记为 pending_take_remote，下次 perform_sync 强制下载远端内容。
            state.pending_take_remote.insert(path.to_string());
        }

        // Remove the conflict record from state.conflicts and conflicts.json
        // via the shared in-memory helper.
        remove_conflict_in_memory(&mut state, &mut conflicts_json, path);

        // issue #902 第十四轮复核：整图 graph.json 冲突解决后，防御性清理
        // conflicts.json 中该星图路径下的子对象冲突记录。整图恢复/删除后，
        // 子对象冲突不再有意义（graph 已统一处理）。
        if crate::sync::staging::starmap_merge::is_starmap_graph_meta_path(path) {
            Self::cleanup_subobject_conflicts(&mut state, &mut conflicts_json, path);
        }

        persist_conflict_state_for_target(sync_root, group, &state, &conflicts_json)?;
        // applied_live = !use_pending_fallback：
        // - use_pending_fallback=false → 已立即应用 live 正文（snapshot 替换/移入 trash）。
        // - use_pending_fallback=true → 仅排队 pending_take_remote，live 正文未变。
        Ok(!use_pending_fallback)
    }

    /// 根据 conflict kind 决定 take_remote 的具体动作，返回是否需要回退到 pending_take_remote。
    ///
    /// **issue #902 第十四轮复核**：在 match conflict.kind 前先判断路径类型：
    /// - `is_starmap_graph_meta_path(path)` → 整图冲突解决
    ///   （[`decide_take_remote_starmap_graph`]）
    /// - 星图对象文件 + RemoteDeleted → 领域级删除事务
    ///   （[`apply_starmap_object_remote_delete`]）
    /// - 非星图路径 → 原有 move_to_trash / apply_remote_snapshot 逻辑
    fn decide_take_remote(
        sync_root: &Path,
        path: &str,
        state: &mut crate::sync::types::SyncState,
        conflict_opt: Option<SyncConflict>,
        guard: Option<&crate::storage::WorkspaceWriteLock>,
        app_data_root: Option<&Path>,
    ) -> crate::Result<bool> {
        let Some(conflict) = conflict_opt else {
            // 无冲突记录：回退 pending_take_remote 行为。
            return Ok(true);
        };

        // 整图 graph.json 冲突：路由到整图冲突解决
        if crate::sync::staging::starmap_merge::is_starmap_graph_meta_path(path) {
            return Self::decide_take_remote_starmap_graph(sync_root, path, state, &conflict);
        }

        // meta-only 星图冲突：路由到 meta 锚点冲突解决
        // （issue #902 第十九轮复核问题1c）。
        if crate::sync::staging::starmap_merge::is_starmap_meta_path(path) {
            return Self::decide_take_remote_starmap_meta(sync_root, path, state, &conflict);
        }

        match conflict.kind {
            SyncConflictKind::RemoteDeleted => {
                // 星图对象文件：路由到领域级删除事务
                if crate::sync::staging::starmap_merge::is_starmap_object_file_path(path) {
                    // issue #902 第十五轮复核：apply_starmap_object_remote_delete 返回
                    // 实际删除的所有路径集合（含级联删除的 edge/link/embed）。
                    // 为每个路径清理 known_files / known_files_updated_at 并记录文件级
                    // tombstone，防止 snapshot_local_records_read_only 报
                    // "known file missing without tombstone"。
                    let deleted_paths = Self::apply_starmap_object_remote_delete(sync_root, path)?;
                    Self::cleanup_deleted_paths_and_record_tombstones(state, &deleted_paths);
                    return Ok(false);
                }
                if let (Some((volume_id, chapter_id)), Some(app_data_root)) =
                    (Self::chapter_sync_path_parts(path), app_data_root)
                {
                    // 章节 RemoteDeleted 走章节删除协议：delete_guard + trash +
                    // tombstone 一次完成，不能再像普通文件一样只 move_to_trash。
                    crate::chapter::delete_chapter(
                        sync_root,
                        &volume_id,
                        &chapter_id,
                        app_data_root,
                    )?;
                    state.known_files.remove(path);
                    state.known_files_updated_at.remove(path);
                    return Ok(false);
                }
                // 非星图路径：RemoteDeleted 把本地文件移入 trash，并从 known_files 移除。
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
                    if crate::sync::staging::starmap_merge::is_starmap_object_file_path(path) {
                        // 星图对象文件：领域级冲突应用，SaveTransaction 原子提交
                        // 对象文件 + graph.json（恢复成员表、清除 tombstone、重建派生索引）。
                        // 失败返回 Err，外层不会清理冲突记录、不宣告成功。
                        Self::apply_starmap_remote_snapshot(sync_root, path, snapshot_rel)?;
                    } else {
                        // 非星图路径：原子替换本地正文。
                        Self::apply_remote_snapshot(sync_root, path, snapshot_rel, guard)?;
                    }
                    // issue #902 第十六轮复核：清理已恢复文件的旧 tombstone。
                    // 文件已恢复（远端副本写入本地），旧 tombstone 不再适用。
                    state.tombstones.retain(|t| t.original_path != path);
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

    /// 星图对象文件冲突解决后恢复 graph 成员表（issue #902 第十三轮复核）。
    ///
    /// `sync_root` 是项目数据根目录（live_root）。`path` 是相对 sync_root 的
    /// 星图对象文件路径（`starmaps/<id>/<type>/<bucket>/<obj_id>.json`）。
    /// `remote_content` 是远端 snapshot 的对象文件内容（已由调用方读取）。
    ///
    /// 本地删除、远端修改 Y 时，local Y 不存在，合并后 `graph.node_ids` 不含 Y。
    /// 用户点"采用远端"后本函数将 Y 重新登记到 graph.json：读取 graph.json，
    /// 将 object_id 添加到成员表，更新 revision，清除对应 tombstone，重建全部
    /// 派生索引（edge_relation_index / embed_host_index / link_relation_index /
    /// node_kind_counts），最后用 SaveTransaction
    /// 把对象文件 + graph.json 原子提交。
    ///
    /// 任何环节失败都返回 `Err`：调用方不得清理冲突记录、不得宣告冲突解决成功。
    fn restore_starmap_object_membership(
        sync_root: &Path,
        path: &str,
        remote_content: &[u8],
    ) -> crate::Result<()> {
        use crate::starmap::store::meta::GraphMeta;
        use crate::sync::staging::starmap_merge::{is_starmap_object_file_path, parse_object_path};

        if !is_starmap_object_file_path(path) {
            // 非星图对象路径：不应进入本函数，防御性返回 Ok。
            return Ok(());
        }

        // 解析路径：starmaps/<id>/<type>/<bucket>/<obj_id>.json
        let normalized = path.replace('\\', "/");
        let parts: Vec<&str> = normalized.split('/').collect();
        // is_starmap_object_file_path 已验证 parts.len() == 5
        let starmap_id = parts[1];
        let Some((object_type, object_id)) = parse_object_path(std::path::Path::new(path)) else {
            return Err(crate::Error::Other(format!(
                "restore_starmap_object_membership: failed to parse object path '{}'",
                path
            )));
        };

        let graph_rel = format!("starmaps/{}/graph.json", starmap_id);
        let graph_path = sync_root.join(&graph_rel);

        // 读取 graph.json
        let graph_bytes = std::fs::read(&graph_path).map_err(|e| {
            crate::Error::Other(format!(
                "restore_starmap_object_membership: failed to read graph.json for \
                 starmap '{}': {} (object '{}' not re-registered)",
                starmap_id, e, object_id
            ))
        })?;

        let mut graph: GraphMeta = serde_json::from_slice(&graph_bytes).map_err(|e| {
            crate::Error::Other(format!(
                "restore_starmap_object_membership: failed to parse graph.json for \
                 starmap '{}': {} (object '{}' not re-registered)",
                starmap_id, e, object_id
            ))
        })?;

        // 将 object_id 添加到对应成员表和 revision，并清除 tombstone。
        // 未知对象类型返回 Err，不静默跳过。
        if !Self::add_object_to_graph_members(&mut graph, &object_type, &object_id) {
            return Err(crate::Error::Other(format!(
                "restore_starmap_object_membership: unknown object type '{}' for path '{}'",
                object_type, path
            )));
        }

        // 收集所有成员对象文件内容，用于重建派生索引。
        // 被恢复的对象用 remote_content 覆盖（此时还未写到磁盘）。
        let object_contents = Self::collect_member_object_contents(
            sync_root,
            starmap_id,
            &graph,
            &object_type,
            &object_id,
            remote_content,
        )?;

        // 重建全部派生索引。
        let derived = Self::rebuild_derived_indexes_for_graph(
            sync_root,
            starmap_id,
            &graph,
            &object_contents,
        )?;
        graph.edge_relation_index = derived.edge_relation_index;
        graph.embed_host_index = derived.embed_host_index;
        graph.link_relation_index = derived.link_relation_index;
        // node_kind_counts 是素材库定义的派生缓存，不从画布实例文件重建。

        // 更新 updated_at
        let now_ms = u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        )
        .unwrap_or(u64::MAX);
        graph.updated_at = std::cmp::max(graph.updated_at, now_ms);

        // 序列化 graph.json
        let new_graph_bytes = serde_json::to_vec_pretty(&graph).map_err(|e| {
            crate::Error::Other(format!(
                "restore_starmap_object_membership: failed to serialize graph.json for \
                 starmap '{}': {} (object '{}' not re-registered)",
                starmap_id, e, object_id
            ))
        })?;

        // 用 SaveTransaction 原子提交对象文件 + graph.json
        let mut txn = crate::storage::transaction::SaveTransaction::new(sync_root);
        txn.add_bytes(path, remote_content).map_err(|e| {
            crate::Error::Other(format!(
                "restore_starmap_object_membership: failed to stage object file '{}': {}",
                path, e
            ))
        })?;
        txn.add_bytes(&graph_rel, &new_graph_bytes).map_err(|e| {
            crate::Error::Other(format!(
                "restore_starmap_object_membership: failed to stage graph.json for \
                 starmap '{}': {}",
                starmap_id, e
            ))
        })?;
        txn.commit().map_err(|e| {
            crate::Error::Other(format!(
                "restore_starmap_object_membership: failed to commit transaction for \
                 starmap '{}': {}",
                starmap_id, e
            ))
        })?;

        Ok(())
    }

    /// 收集 graph 所有成员对象文件内容，用于重建派生索引。
    ///
    /// 被恢复的对象（`restore_type`/`restore_id`）用 `remote_content` 覆盖，
    /// 因为此时对象文件还未写到磁盘。其他成员对象从磁盘读取。
    ///
    /// **issue #902 第十四轮复核**：`read_member_files` 现在返回 `Result`，
    /// graph 已登记但磁盘缺失的对象会返回 `Err`，不再静默跳过。调用方必须
    /// 传播错误，中止冲突解决事务。
    fn collect_member_object_contents(
        sync_root: &Path,
        starmap_id: &str,
        graph: &crate::starmap::store::meta::GraphMeta,
        restore_type: &str,
        restore_id: &str,
        remote_content: &[u8],
    ) -> crate::Result<ObjectContentMap> {
        let mut map: ObjectContentMap = std::collections::HashMap::new();

        // 被恢复的对象直接用 remote_content
        map.insert(
            (restore_type.to_string(), restore_id.to_string()),
            remote_content.to_vec(),
        );

        // 遍历所有成员类型，读磁盘对象文件。graph 已登记但磁盘缺失的对象
        // 会返回 Err，中止事务——不写出不完整派生索引。
        Self::read_member_files(
            sync_root,
            starmap_id,
            "node",
            "nodes",
            &graph.node_ids,
            &mut map,
        )?;
        Self::read_member_files(
            sync_root,
            starmap_id,
            "edge",
            "edges",
            &graph.edge_ids,
            &mut map,
        )?;
        Self::read_member_files(
            sync_root,
            starmap_id,
            "embed",
            "embeds",
            &graph.embed_instance_ids,
            &mut map,
        )?;
        // 导航链接不在星图包内：唯一持久化所有者是 annotations 仓库，
        // 冲突解决不再读取/恢复 `starmaps/<id>/links/*.json`。

        Ok(map)
    }

    /// 收集 graph 所有成员对象文件内容（不带 restore 覆盖）。
    ///
    /// 用于对象删除 / 整图恢复场景：所有成员对象都从磁盘读取，没有
    /// "尚未写盘的 restore 对象"。graph 已登记但磁盘缺失的对象返回 `Err`。
    fn collect_all_member_object_contents(
        sync_root: &Path,
        starmap_id: &str,
        graph: &crate::starmap::store::meta::GraphMeta,
    ) -> crate::Result<ObjectContentMap> {
        let mut map: ObjectContentMap = std::collections::HashMap::new();
        Self::read_member_files(
            sync_root,
            starmap_id,
            "node",
            "nodes",
            &graph.node_ids,
            &mut map,
        )?;
        Self::read_member_files(
            sync_root,
            starmap_id,
            "edge",
            "edges",
            &graph.edge_ids,
            &mut map,
        )?;
        Self::read_member_files(
            sync_root,
            starmap_id,
            "embed",
            "embeds",
            &graph.embed_instance_ids,
            &mut map,
        )?;
        // 导航链接不在星图包内：唯一持久化所有者是 annotations 仓库，
        // 冲突解决不再读取/恢复 `starmaps/<id>/links/*.json`。
        Ok(map)
    }

    /// 读取一种成员类型的所有对象文件到 `map`。已存在的 key 跳过。
    ///
    /// **issue #902 第十四轮复核**：graph 已登记的成员对象必须存在对应文件，
    /// 缺失或无法读取时返回 `Err`，中止冲突解决事务——不再静默跳过。
    /// 此前 `if let Ok(bytes) = ...` 会跳过读失败的对象，导致
    /// `rebuild_derived_indexes_for_graph` 写出不完整派生索引，graph 成员表
    /// 与磁盘对象文件不一致。
    fn read_member_files(
        sync_root: &Path,
        starmap_id: &str,
        otype: &str,
        type_dir: &str,
        ids: &[String],
        map: &mut ObjectContentMap,
    ) -> crate::Result<()> {
        use crate::starmap::package_storage::bucket_for_id;

        for id in ids {
            let key = (otype.to_string(), id.clone());
            if map.contains_key(&key) {
                continue; // 已用 remote_content 覆盖
            }
            let bucket = bucket_for_id(id);
            let rel = format!(
                "starmaps/{}/{}/{}/{}.json",
                starmap_id, type_dir, bucket, id
            );
            let bytes = std::fs::read(sync_root.join(&rel)).map_err(|e| {
                crate::Error::Other(format!(
                    "read_member_files: 已登记的 {} '{}' 对象文件缺失或无法读取 ({}): {}",
                    otype, id, rel, e
                ))
            })?;
            map.insert(key, bytes);
        }
        Ok(())
    }

    /// 从 `object_contents` 取出并解析单个成员对象。
    ///
    /// 返回 `Ok(None)` 表示 map 中无此对象（跳过）；`Ok(Some)` 解析成功；
    /// `Err` 表示对象文件损坏（JSON 解析失败）。
    fn parse_member_object<T: serde::de::DeserializeOwned>(
        object_contents: &ObjectContentMap,
        otype: &str,
        id: &str,
    ) -> crate::Result<Option<T>> {
        let key = (otype.to_string(), id.to_string());
        let Some(bytes) = object_contents.get(&key) else {
            return Ok(None);
        };
        let obj: T = serde_json::from_slice(bytes).map_err(|e| {
            crate::Error::Other(format!(
                "restore_starmap_object_membership: failed to parse {} '{}': {}",
                otype, id, e
            ))
        })?;
        Ok(Some(obj))
    }

    /// 从成员对象文件内容重建派生索引（issue #902 第十三轮复核）。
    ///
    /// 与 [`crate::sync::staging::starmap_merge::rebuild_derived_indexes`] 逻辑等价，
    /// 但直接使用 `GraphMeta` 的公开成员列表，无需构造 `MergedMemberIds`。
    /// `object_contents` 的 key 是 `(object_type, object_id)`，value 是对象 JSON bytes。
    ///
    /// **issue #902 第十四轮复核**：`read_member_files` 现在对已登记但磁盘缺失
    /// 的对象返回 `Err`，因此 `object_contents` 应包含 graph 全部成员。
    /// `parse_member_object` 返回 `Ok(None)` 的分支理论上不再触发，保留为
    /// 防御性 `continue`；解析失败（对象文件损坏）仍返回 `Err`。
    ///
    /// **Issue #1062**：节点对象经统一迁移入口
    /// [`crate::starmap::migration::nodes::decode_node_object`] 校验，旧格式完整
    /// 节点不再被误判为损坏；身份冲突带星图、对象和路径返回 `Err`。
    fn rebuild_derived_indexes_for_graph(
        sync_root: &Path,
        starmap_id: &str,
        graph: &crate::starmap::store::meta::GraphMeta,
        object_contents: &ObjectContentMap,
    ) -> crate::Result<crate::sync::staging::starmap_merge::DerivedIndexes> {
        use crate::starmap::package_storage::bucket_for_id;
        use crate::starmap::store::relation_index::{EdgeRelationIndex, EmbedHostIndex};
        use crate::starmap::types::{StarMapEdge, StarMapEmbed};

        let mut edge_relation_index: Vec<EdgeRelationIndex> = Vec::new();
        for edge_id in &graph.edge_ids {
            let Some(edge) =
                Self::parse_member_object::<StarMapEdge>(object_contents, "edge", edge_id)?
            else {
                continue;
            };
            edge_relation_index.push(EdgeRelationIndex {
                edge_id: edge.id,
                from: edge.from,
                to: edge.to,
            });
        }

        let mut embed_host_index: Vec<EmbedHostIndex> = Vec::new();
        for instance_id in &graph.embed_instance_ids {
            let Some(embed) =
                Self::parse_member_object::<StarMapEmbed>(object_contents, "embed", instance_id)?
            else {
                continue;
            };
            embed_host_index.push(EmbedHostIndex {
                instance_id: embed.instance_id,
                host_path: embed.host_path,
            });
        }

        // 节点对象：统一迁移入口校验（旧格式完整节点 / 实例都合法）。
        let node_bindings =
            crate::starmap::migration::nodes::NodeObjectBindings::load_reference_bindings(
                sync_root,
            )
            .map_err(|error| {
                crate::Error::Other(format!(
                    "rebuild_derived_indexes_for_graph: 读取素材库实例绑定失败 \
                     (starmap_id={starmap_id}, root={}): {error}",
                    sync_root.display()
                ))
            })?;
        for node_id in &graph.node_ids {
            let key = ("node".to_string(), node_id.clone());
            let Some(bytes) = object_contents.get(&key) else {
                continue;
            };
            let rel_path = format!(
                "starmaps/{starmap_id}/nodes/{}/{node_id}.json",
                bucket_for_id(node_id)
            );
            crate::starmap::migration::nodes::decode_node_object(
                sync_root,
                starmap_id,
                node_id,
                bytes,
                Some(&node_bindings),
            )
            .map_err(|error| {
                crate::Error::Other(format!(
                    "rebuild_derived_indexes_for_graph: 节点对象迁移校验失败 \
                     (starmap_id={starmap_id}, object_id={node_id}, path={rel_path}): {error}"
                ))
            })?;
        }

        // 链接索引由 StarMapStore 的 annotations 投影实时构建，不再写入
        // graph.json 的派生索引；这里保持空 vec 兼容冲突解决输出。
        let link_relation_index = Vec::new();

        Ok(crate::sync::staging::starmap_merge::DerivedIndexes {
            edge_relation_index,
            embed_host_index,
            link_relation_index,
            // 冲突解决直接写回快照内容；实例改写/本体发布由 Store 加载时的
            // 统一迁移入口完成，这里不产生发布计划。
            node_object_plans: Vec::new(),
        })
    }

    /// 将对象 ID 添加到 graph 成员表和 revision，并清除对应 tombstone
    /// （issue #902 第十三轮复核）。
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
            // "link" 不在星图包内：链接由 annotations 仓库同步，成员表只是
            // 派生缓存，冲突解决不再增删链接成员。
            _ => false,
        };
        if added {
            graph.package_revision = new_pr;
            // 清除 tombstone：对象被恢复到成员表，不再是"已删除"。
            // 曾经删除的对象被选远端恢复后，不能同时出现在成员列表与删除 tombstone 中。
            graph
                .deleted_since_last_sync
                .remove_entry(object_type, object_id);
        }
        added
    }

    /// 将对象 ID 从 graph 成员表和 revision 移除，并添加 tombstone
    /// （issue #902 第十四轮复核，与 [`add_object_to_graph_members`] 对称）。
    ///
    /// 返回 true 表示已移除，false 表示未知对象类型或成员表中本就没有该 ID。
    /// 移除时递增 `package_revision` 并在 `deleted_since_last_sync` 添加 tombstone。
    fn remove_object_from_graph_members(
        graph: &mut crate::starmap::store::meta::GraphMeta,
        object_type: &str,
        object_id: &str,
    ) -> bool {
        let removed = match object_type {
            "node" => {
                let before = graph.node_ids.len();
                graph.node_ids.retain(|x| x != object_id);
                graph.node_revisions.remove(object_id);
                graph.node_ids.len() < before
            }
            "edge" => {
                let before = graph.edge_ids.len();
                graph.edge_ids.retain(|x| x != object_id);
                graph.edge_revisions.remove(object_id);
                graph.edge_ids.len() < before
            }
            "embed" => {
                let before = graph.embed_instance_ids.len();
                graph.embed_instance_ids.retain(|x| x != object_id);
                graph.embed_revisions.remove(object_id);
                graph.embed_instance_ids.len() < before
            }
            // "link" 不在星图包内，删除事实由 annotations tombstone 承担。
            _ => false,
        };
        if removed {
            graph.package_revision = graph.package_revision.wrapping_add(1);
            graph
                .deleted_since_last_sync
                .add_entry(object_type, object_id, graph.package_revision);
        }
        removed
    }

    /// 检查 `StarMapTargetPath` 是否引用给定 node_id（issue #902 第十四轮复核）。
    ///
    /// 用于级联清理：删除 node 时，检查 edge/link/embed 的路径端点
    /// 是否引用该 node。引用出现在 `target`（Node/Anchor）或 `segments`
    /// （EnterPortal）中。
    fn target_path_references_node(
        path: &crate::starmap::types::StarMapTargetPath,
        node_id: &str,
    ) -> bool {
        use crate::starmap::semantic::StarMapTargetDetail;
        use crate::starmap::types::StarMapPathSegment;

        let target_ref = match &path.target {
            StarMapTargetDetail::Node { node_id: nid } => nid == node_id,
            StarMapTargetDetail::Anchor { node_id: nid, .. } => nid == node_id,
            _ => false,
        };
        target_ref
            || path.segments.iter().any(|seg| match seg {
                StarMapPathSegment::EnterPortal { node_id: nid } => nid == node_id,
                _ => false,
            })
    }

    /// 从 graph.json 路径解析 starmap_id。
    ///
    /// `path` 格式：`starmaps/<id>/graph.json`。返回 `None` 如果格式不匹配。
    fn parse_starmap_id_from_graph_path(path: &str) -> Option<String> {
        let normalized = path.replace('\\', "/");
        let parts: Vec<&str> = normalized.split('/').collect();
        if parts.len() == 3 && parts[0] == "starmaps" && parts[2] == "graph.json" {
            Some(parts[1].to_string())
        } else {
            None
        }
    }

    /// 从 meta.json 路径解析 starmap_id（issue #902 第十九轮复核问题1c）。
    ///
    /// `path` 格式：`starmaps/<id>.meta.json`。返回 `None` 如果格式不匹配。
    fn parse_starmap_id_from_meta_path(path: &str) -> Option<String> {
        let normalized = path.replace('\\', "/");
        let parts: Vec<&str> = normalized.split('/').collect();
        if parts.len() == 2 && parts[0] == "starmaps" && parts[1].ends_with(".meta.json") {
            let id = &parts[1][..parts[1].len() - ".meta.json".len()];
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
        None
    }

    /// 构造星图对象文件的相对路径。
    fn starmap_object_rel(starmap_id: &str, type_dir: &str, object_id: &str) -> String {
        use crate::starmap::package_storage::bucket_for_id;
        let bucket = bucket_for_id(object_id);
        format!(
            "starmaps/{}/{}/{}/{}.json",
            starmap_id, type_dir, bucket, object_id
        )
    }

    /// 从 state.known_files / known_files_updated_at 移除指定星图前缀下的所有路径。
    fn cleanup_starmap_known_files(state: &mut crate::sync::types::SyncState, starmap_id: &str) {
        let prefix = format!("starmaps/{}/", starmap_id);
        state.known_files.retain(|k, _| !k.starts_with(&prefix));
        state
            .known_files_updated_at
            .retain(|k, _| !k.starts_with(&prefix));
    }

    /// 清理已恢复星图文件的旧 tombstone（issue #902 第十六轮复核）。
    ///
    /// 整图恢复（BothChanged）后调用：已恢复文件包括 graph.json、meta.json 和
    /// 所有子对象文件。`starmaps/<id>/` 前缀覆盖 graph.json 和所有子对象，
    /// `starmaps/<id>.meta.json` 单独处理。
    fn cleanup_starmap_tombstones(state: &mut crate::sync::types::SyncState, starmap_id: &str) {
        let prefix = format!("starmaps/{}/", starmap_id);
        let meta_rel = format!("starmaps/{}.meta.json", starmap_id);
        state
            .tombstones
            .retain(|t| !t.original_path.starts_with(&prefix) && t.original_path != meta_rel);
    }

    /// 为远端删除的文件记录文件级 tombstone（issue #902 第十五轮复核）。
    ///
    /// `apply_starmap_object_remote_delete` / `apply_starmap_graph_remote_delete`
    /// 删除文件后，调用方为每个被删路径调用此函数，防止
    /// `snapshot_local_records_read_only` 报"known file missing without tombstone"。
    /// 若该路径已有 tombstone 则跳过（幂等）。
    fn record_remote_delete_tombstone(state: &mut crate::sync::types::SyncState, path: &str) {
        if state.tombstones.iter().any(|t| t.original_path == path) {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        state.tombstones.push(crate::sync::types::Tombstone {
            original_path: path.to_string(),
            trash_path: String::new(),
            deleted_at: now,
            purge_after: now + 30 * 24 * 3600,
            deleted_by: state.device_id.clone(),
            original_hash: String::new(),
            kind: "remote_delete".to_string(),
        });
    }

    /// 批量清理已删除路径的 known_files / known_files_updated_at 并记录文件级
    /// tombstone（issue #902 第十五轮复核）。
    ///
    /// 供 `decide_take_remote` / `decide_take_remote_starmap_graph` 在
    /// RemoteDeleted 分支调用，减少嵌套层级。
    fn cleanup_deleted_paths_and_record_tombstones(
        state: &mut crate::sync::types::SyncState,
        deleted_paths: &[String],
    ) {
        for del_path in deleted_paths {
            state.known_files.remove(del_path);
            state.known_files_updated_at.remove(del_path);
            Self::record_remote_delete_tombstone(state, del_path);
        }
    }

    /// 读取 index.json 并移除指定星图 ID，返回序列化后的 bytes（issue #902 第十四轮复核）。
    ///
    /// 供 `apply_starmap_graph_remote_delete` 在 SaveTransaction 中原子写入。
    fn load_index_and_remove_starmap(sync_root: &Path, starmap_id: &str) -> crate::Result<Vec<u8>> {
        let mut idx = crate::starmap::load_index(sync_root)?;
        idx.starmap_ids.retain(|x| x != starmap_id);
        idx.root_starmap_ids.retain(|x| x != starmap_id);
        idx.main_starmap_by_project.retain(|_, v| v != starmap_id);
        idx.updated_at = crate::starmap::now_epoch();
        serde_json::to_vec_pretty(&idx).map_err(|e| {
            crate::Error::Other(format!("load_index_and_remove_starmap: 序列化失败: {}", e))
        })
    }

    /// 读取 index.json 并确保包含指定星图 ID，返回序列化后的 bytes。
    ///
    /// 供 `apply_starmap_graph_remote_modify` 在 SaveTransaction 中原子写入。
    ///
    /// **issue #902 第十五轮复核**：当 `identity_facts` 为 `Some(facts)` 时，
    /// 额外恢复 `root_starmap_ids` 和 `main_starmap_by_project`：
    /// - 若 `facts.is_root` 且 `starmap_id` 不在 `root_starmap_ids` 中，添加它。
    /// - 对每个 `facts.main_for_projects` 中的 project_id，设置
    ///   `main_starmap_by_project[project_id] = starmap_id`（远端恢复优先，
    ///   覆盖本地映射）。
    ///   为 `None` 时只恢复 `starmap_ids`（保持向后兼容）。
    fn load_index_and_ensure_starmap(
        sync_root: &Path,
        starmap_id: &str,
        identity_facts: Option<&StarmapIdentityFacts>,
    ) -> crate::Result<Vec<u8>> {
        let mut idx = crate::starmap::load_index(sync_root)?;
        if !idx.starmap_ids.iter().any(|x| x == starmap_id) {
            idx.starmap_ids.push(starmap_id.to_string());
        }
        if let Some(facts) = identity_facts {
            if facts.is_root && !idx.root_starmap_ids.iter().any(|x| x == starmap_id) {
                idx.root_starmap_ids.push(starmap_id.to_string());
            }
            for project_id in &facts.main_for_projects {
                idx.main_starmap_by_project
                    .insert(project_id.clone(), starmap_id.to_string());
            }
        }
        idx.updated_at = crate::starmap::now_epoch();
        serde_json::to_vec_pretty(&idx).map_err(|e| {
            crate::Error::Other(format!("load_index_and_ensure_starmap: 序列化失败: {}", e))
        })
    }

    /// 从快照目录读取远端星图身份事实（issue #902 第十五轮复核）。
    ///
    /// 读取 `{snapshot_dir_rel}/index_facts.json`。issue #902 第十九轮复核问题3：
    /// 文件不存在、读取失败或 JSON 解析失败时返回 `Err`（不再静默返回 `None`）。
    /// 远端整图恢复要求 index_facts.json 存在；旧冲突快照缺少身份事实，无法完整
    /// 确定整图恢复，应明确拒绝而非静默跳过 root/main 身份恢复。
    fn load_starmap_identity_facts(
        sync_root: &Path,
        snapshot_dir_rel: &str,
    ) -> crate::Result<StarmapIdentityFacts> {
        let facts_rel = format!("{}/index_facts.json", snapshot_dir_rel);
        let facts_path = sync_root.join(&facts_rel);
        if !facts_path.exists() {
            return Err(crate::Error::Other(format!(
                "load_starmap_identity_facts: 远端整图恢复要求 index_facts.json 存在；\
                 旧冲突快照缺少身份事实，无法完整确定整图恢复 (path='{}')",
                facts_rel
            )));
        }
        let bytes = std::fs::read(&facts_path).map_err(|e| {
            crate::Error::Other(format!(
                "load_starmap_identity_facts: 读取 index_facts.json 失败 (path='{}'): {}",
                facts_rel, e
            ))
        })?;
        serde_json::from_slice(&bytes).map_err(|e| {
            crate::Error::Other(format!(
                "load_starmap_identity_facts: 解析 index_facts.json 失败 (path='{}'): {}",
                facts_rel, e
            ))
        })
    }

    /// 星图对象文件的远端删除事务（issue #902 第十四轮复核，问题 3）。
    ///
    /// `sync_root` 是项目数据根目录。`path` 是相对 sync_root 的星图对象文件路径。
    ///
    /// RemoteDeleted 冲突（远端删除了对象，本地修改了对象），用户选"采用远端"=
    /// 接受删除。本函数执行领域级删除：
    /// 1. 解析路径得到 starmap_id、object_type、object_id
    /// 2. 读取 graph.json
    /// 3. 从成员表 / revisions 移除该 ID，添加 tombstone
    /// 4. 级联清理引用该对象的 edge/embed/link（不允许悬挂成员）
    /// 5. 重建派生索引
    /// 6. SaveTransaction 原子提交：删除对象文件 + 更新 graph.json
    ///
    /// 任何环节失败返回 `Err`，不部分应用。
    fn apply_starmap_object_remote_delete(
        sync_root: &Path,
        path: &str,
    ) -> crate::Result<Vec<String>> {
        use crate::starmap::store::meta::GraphMeta;
        use crate::sync::staging::starmap_merge::{is_starmap_object_file_path, parse_object_path};

        if !is_starmap_object_file_path(path) {
            return Err(crate::Error::Other(format!(
                "apply_starmap_object_remote_delete: 非星图对象路径 '{}'",
                path
            )));
        }

        let normalized = path.replace('\\', "/");
        let parts: Vec<&str> = normalized.split('/').collect();
        let starmap_id = parts[1];
        let Some((object_type, object_id)) = parse_object_path(std::path::Path::new(path)) else {
            return Err(crate::Error::Other(format!(
                "apply_starmap_object_remote_delete: 解析对象路径失败 '{}'",
                path
            )));
        };

        let graph_rel = format!("starmaps/{}/graph.json", starmap_id);
        let graph_path = sync_root.join(&graph_rel);

        // 读取 graph.json
        let graph_bytes = std::fs::read(&graph_path).map_err(|e| {
            crate::Error::Other(format!(
                "apply_starmap_object_remote_delete: 读取 graph.json 失败 ({}): {}",
                starmap_id, e
            ))
        })?;
        let mut graph: GraphMeta = serde_json::from_slice(&graph_bytes).map_err(|e| {
            crate::Error::Other(format!(
                "apply_starmap_object_remote_delete: 解析 graph.json 失败 ({}): {}",
                starmap_id, e
            ))
        })?;

        // 收集所有成员对象内容（含被删对象），用于级联引用检查。
        // 被删对象文件存在（RemoteDeleted 冲突 = 本地修改了对象）。
        let object_contents =
            Self::collect_all_member_object_contents(sync_root, starmap_id, &graph)?;

        // 从成员表移除主对象
        if !Self::remove_object_from_graph_members(&mut graph, &object_type, &object_id) {
            return Err(crate::Error::Other(format!(
                "apply_starmap_object_remote_delete: 对象 '{}' 不在 graph 成员表中 (type '{}')",
                object_id, object_type
            )));
        }

        // 记算需要删除的对象文件路径列表（含级联删除的）
        let mut delete_rels: Vec<String> = vec![path.to_string()];

        // 级联清理：删除 node 时，移除引用该 node 的 edge/embed/link
        if object_type == "node" {
            Self::cascade_remove_references(
                &mut graph,
                &object_id,
                &object_contents,
                starmap_id,
                &mut delete_rels,
            )?;
        }

        // 收集剩余成员对象内容（从已读的 object_contents 中取，跳过被移除的）
        let remaining_contents: ObjectContentMap = object_contents
            .into_iter()
            .filter(|((otype, oid), _)| Self::is_object_in_graph_members(&graph, otype, oid))
            .collect();

        // 重建派生索引
        let derived = Self::rebuild_derived_indexes_for_graph(
            sync_root,
            starmap_id,
            &graph,
            &remaining_contents,
        )?;
        graph.edge_relation_index = derived.edge_relation_index;
        graph.embed_host_index = derived.embed_host_index;
        graph.link_relation_index = derived.link_relation_index;
        // node_kind_counts 是素材库定义的派生缓存，不从画布实例文件重建。

        // 更新 updated_at
        let now_ms = u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        )
        .unwrap_or(u64::MAX);
        graph.updated_at = std::cmp::max(graph.updated_at, now_ms);

        let new_graph_bytes = serde_json::to_vec_pretty(&graph).map_err(|e| {
            crate::Error::Other(format!(
                "apply_starmap_object_remote_delete: 序列化 graph.json 失败 ({}): {}",
                starmap_id, e
            ))
        })?;

        // SaveTransaction 原子提交：删除对象文件 + 更新 graph.json
        let mut txn = crate::storage::transaction::SaveTransaction::new(sync_root);
        for del_rel in &delete_rels {
            txn.add_delete(del_rel)?;
        }
        txn.add_bytes(&graph_rel, &new_graph_bytes).map_err(|e| {
            crate::Error::Other(format!(
                "apply_starmap_object_remote_delete: 暂存 graph.json 失败 ({}): {}",
                starmap_id, e
            ))
        })?;
        txn.commit().map_err(|e| {
            crate::Error::Other(format!(
                "apply_starmap_object_remote_delete: 提交事务失败 ({}): {}",
                starmap_id, e
            ))
        })?;

        // issue #902 第十五轮复核：返回实际删除的所有路径集合，
        // 供调用方同步维护 known_files / known_files_updated_at / tombstones。
        Ok(delete_rels)
    }

    /// 级联移除引用指定 node 的 edge/embed/link（issue #902 第十四轮复核）。
    ///
    /// 不允许留下悬挂成员：删除 node 后，引用该 node 的 edge（from/to）、
    /// link（source/target）、embed（host_path）也必须移除。
    fn cascade_remove_references(
        graph: &mut crate::starmap::store::meta::GraphMeta,
        node_id: &str,
        object_contents: &ObjectContentMap,
        starmap_id: &str,
        delete_rels: &mut Vec<String>,
    ) -> crate::Result<()> {
        use crate::starmap::types::{StarMapEdge, StarMapEmbed};

        // edge：from/to 引用 node
        // issue #902 第十五轮复核：用显式 for 循环 + `?` 传播 parse_member_object
        // 的反序列化错误，不再用 `.ok().flatten()` 吞掉对象文件损坏导致的解析失败。
        let mut edges_to_remove: Vec<String> = Vec::new();
        for eid in &graph.edge_ids {
            let Some(edge) =
                Self::parse_member_object::<StarMapEdge>(object_contents, "edge", eid)?
            else {
                continue;
            };
            if Self::target_path_references_node(&edge.from, node_id)
                || Self::target_path_references_node(&edge.to, node_id)
            {
                edges_to_remove.push(eid.clone());
            }
        }
        for eid in &edges_to_remove {
            Self::remove_object_from_graph_members(graph, "edge", eid);
            delete_rels.push(Self::starmap_object_rel(starmap_id, "edges", eid));
        }

        // 链接不在星图包内：node 删除后的链接 tombstone 由作者侧
        // StarMapStore::delete_link 写入 annotations 仓库，冲突解决不处理链接文件。

        // embed：host_path 引用 node
        let mut embeds_to_remove: Vec<String> = Vec::new();
        for eid in &graph.embed_instance_ids {
            let Some(embed) =
                Self::parse_member_object::<StarMapEmbed>(object_contents, "embed", eid)?
            else {
                continue;
            };
            if Self::target_path_references_node(&embed.host_path, node_id) {
                embeds_to_remove.push(eid.clone());
            }
        }
        for eid in &embeds_to_remove {
            Self::remove_object_from_graph_members(graph, "embed", eid);
            delete_rels.push(Self::starmap_object_rel(starmap_id, "embeds", eid));
        }

        Ok(())
    }

    /// 检查对象是否在 graph 成员表中。
    fn is_object_in_graph_members(
        graph: &crate::starmap::store::meta::GraphMeta,
        object_type: &str,
        object_id: &str,
    ) -> bool {
        match object_type {
            "node" => graph.node_ids.iter().any(|x| x == object_id),
            "edge" => graph.edge_ids.iter().any(|x| x == object_id),
            "embed" => graph.embed_instance_ids.iter().any(|x| x == object_id),
            "link" => graph.link_ids.iter().any(|x| x == object_id),
            _ => false,
        }
    }

    /// 整图远端删除事务（issue #902 第十四轮复核，问题 2a）。
    ///
    /// graph.json 的 RemoteDeleted 冲突（远端删除了整图，用户选远端 = 接受删除）。
    /// 1. 解析路径得到 starmap_id
    /// 2. 读取 live graph.json 得到成员表
    /// 3. 收集所有子对象文件路径
    /// 4. SaveTransaction 原子提交：删除 graph.json + 删除所有子对象文件
    /// 5. 从 starmaps/index.json 移除该星图 ID
    fn apply_starmap_graph_remote_delete(
        sync_root: &Path,
        path: &str,
    ) -> crate::Result<Vec<String>> {
        use crate::starmap::store::meta::GraphMeta;

        let Some(starmap_id) = Self::parse_starmap_id_from_graph_path(path) else {
            return Err(crate::Error::Other(format!(
                "apply_starmap_graph_remote_delete: 非法 graph.json 路径 '{}'",
                path
            )));
        };

        let graph_rel = format!("starmaps/{}/graph.json", starmap_id);
        let graph_path = sync_root.join(&graph_rel);

        // 读取 live graph.json 得到成员表（如果 graph.json 不存在，说明本地已删除，
        // 只需清理 index.json）
        let member_rels: Vec<String> = if graph_path.exists() {
            let graph_bytes = std::fs::read(&graph_path).map_err(|e| {
                crate::Error::Other(format!(
                    "apply_starmap_graph_remote_delete: 读取 graph.json 失败 ({}): {}",
                    starmap_id, e
                ))
            })?;
            let graph: GraphMeta = serde_json::from_slice(&graph_bytes).map_err(|e| {
                crate::Error::Other(format!(
                    "apply_starmap_graph_remote_delete: 解析 graph.json 失败 ({}): {}",
                    starmap_id, e
                ))
            })?;
            Self::collect_all_member_file_rels(&starmap_id, &graph)
        } else {
            Vec::new()
        };

        // 准备 index.json（移除该星图 ID）
        let index_bytes = Self::load_index_and_remove_starmap(sync_root, &starmap_id)?;

        // SaveTransaction 原子提交：删除 graph.json + 删除所有子对象文件 + 更新 index.json
        let mut txn = crate::storage::transaction::SaveTransaction::new(sync_root);
        txn.add_delete(&graph_rel)?;
        // issue #902 第十六轮复核：删除 meta.json 作为整图生命周期的一部分
        let meta_rel = format!("starmaps/{}.meta.json", starmap_id);
        txn.add_delete(&meta_rel)?;
        for obj_rel in &member_rels {
            txn.add_delete(obj_rel)?;
        }
        txn.add_bytes("starmaps/index.json", &index_bytes)
            .map_err(|e| {
                crate::Error::Other(format!(
                    "apply_starmap_graph_remote_delete: 暂存 index.json 失败: {}",
                    e
                ))
            })?;
        txn.commit().map_err(|e| {
            crate::Error::Other(format!(
                "apply_starmap_graph_remote_delete: 提交事务失败 ({}): {}",
                starmap_id, e
            ))
        })?;

        // issue #902 第十五轮复核：返回实际删除的所有路径集合
        // （graph.json + meta.json + 全部子对象文件），供调用方同步维护
        // known_files / known_files_updated_at / tombstones。
        let mut deleted_paths: Vec<String> = Vec::with_capacity(member_rels.len() + 2);
        deleted_paths.push(graph_rel);
        deleted_paths.push(meta_rel);
        deleted_paths.extend(member_rels);
        Ok(deleted_paths)
    }

    /// 整图远端修改恢复事务（issue #902 第十四轮复核，问题 2b）。
    ///
    /// graph.json 的 BothChanged 冲突（本地删除远端修改，用户选远端 = 恢复远端整图）。
    /// 1. 从 conflict.remote_snapshot_path 读取远端 graph.json
    /// 2. 从 conflict.remote_graph_snapshot_dir 读取所有远端子对象文件
    /// 3. 解析远端 graph.json 得到成员表
    /// 4. SaveTransaction 原子提交：写入 graph.json + 写入所有远端子对象文件
    /// 5. 恢复 starmaps/index.json 条目
    /// 6. 重建派生索引
    fn apply_starmap_graph_remote_modify(
        sync_root: &Path,
        path: &str,
        conflict: &SyncConflict,
    ) -> crate::Result<()> {
        use crate::starmap::store::meta::GraphMeta;

        let Some(starmap_id) = Self::parse_starmap_id_from_graph_path(path) else {
            return Err(crate::Error::Other(format!(
                "apply_starmap_graph_remote_modify: 非法 graph.json 路径 '{}'",
                path
            )));
        };

        let graph_rel = format!("starmaps/{}/graph.json", starmap_id);

        // 从 remote_snapshot_path 读取远端 graph.json
        let snapshot_rel = conflict.remote_snapshot_path.as_ref().ok_or_else(|| {
            crate::Error::Other(format!(
                "apply_starmap_graph_remote_modify: 缺少 remote_snapshot_path (graph '{}')",
                starmap_id
            ))
        })?;
        let remote_graph_bytes = std::fs::read(sync_root.join(snapshot_rel)).map_err(|e| {
            crate::Error::Other(format!(
                "apply_starmap_graph_remote_modify: 读取远端 graph snapshot 失败 ({}): {}",
                starmap_id, e
            ))
        })?;

        let mut remote_graph: GraphMeta =
            serde_json::from_slice(&remote_graph_bytes).map_err(|e| {
                crate::Error::Other(format!(
                    "apply_starmap_graph_remote_modify: 解析远端 graph.json 失败 ({}): {}",
                    starmap_id, e
                ))
            })?;

        // 从 remote_graph_snapshot_dir 读取所有远端子对象文件
        let snapshot_dir_rel = conflict.remote_graph_snapshot_dir.as_ref().ok_or_else(|| {
            crate::Error::Other(format!(
                "apply_starmap_graph_remote_modify: 缺少 remote_graph_snapshot_dir (graph '{}')",
                starmap_id
            ))
        })?;

        let member_rels = Self::collect_all_member_file_rels(&starmap_id, &remote_graph);
        // 一次读取所有子对象快照：返回 (rel_path, bytes) 列表和 (otype,oid)→bytes 映射
        let (obj_files, object_contents) =
            Self::read_remote_graph_snapshot_objects(sync_root, &member_rels, snapshot_dir_rel)?;

        // issue #902 第十六轮复核：从快照目录读取远端 meta.json
        // issue #902 第十七轮复核问题4：meta 快照必须存在，缺少时返回错误。
        let (meta_rel, meta_bytes) =
            Self::read_remote_meta_snapshot(sync_root, &starmap_id, snapshot_dir_rel)?;

        // 重建派生索引
        let derived = Self::rebuild_derived_indexes_for_graph(
            sync_root,
            &starmap_id,
            &remote_graph,
            &object_contents,
        )?;
        remote_graph.edge_relation_index = derived.edge_relation_index;
        remote_graph.embed_host_index = derived.embed_host_index;
        remote_graph.link_relation_index = derived.link_relation_index;

        let now_ms = u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        )
        .unwrap_or(u64::MAX);
        remote_graph.updated_at = std::cmp::max(remote_graph.updated_at, now_ms);

        let new_graph_bytes = serde_json::to_vec_pretty(&remote_graph).map_err(|e| {
            crate::Error::Other(format!(
                "apply_starmap_graph_remote_modify: 序列化 graph.json 失败 ({}): {}",
                starmap_id, e
            ))
        })?;

        // 准备 index.json（确保包含该星图 ID）
        // issue #902 第十五轮复核：读取快照目录下的 index_facts.json，
        // 恢复 root_starmap_ids 和 main_starmap_by_project 身份事实。
        // issue #902 第十九轮复核问题3：index_facts.json 必须存在且可解析，
        // 缺失/损坏时返回错误（不再传 None 静默跳过）。
        let identity_facts = Self::load_starmap_identity_facts(sync_root, snapshot_dir_rel)?;
        let index_bytes =
            Self::load_index_and_ensure_starmap(sync_root, &starmap_id, Some(&identity_facts))?;

        // SaveTransaction 原子提交：写入 graph.json + 写入所有远端子对象文件 + 更新 index.json
        let mut txn = crate::storage::transaction::SaveTransaction::new(sync_root);
        txn.add_bytes(&graph_rel, &new_graph_bytes).map_err(|e| {
            crate::Error::Other(format!(
                "apply_starmap_graph_remote_modify: 暂存 graph.json 失败 ({}): {}",
                starmap_id, e
            ))
        })?;
        for (obj_rel, bytes) in &obj_files {
            txn.add_bytes(obj_rel, bytes).map_err(|e| {
                crate::Error::Other(format!(
                    "apply_starmap_graph_remote_modify: 暂存子对象 '{}' 失败: {}",
                    obj_rel, e
                ))
            })?;
        }
        // issue #902 第十六轮复核：恢复 meta.json
        // issue #902 第十七轮复核问题4：meta 必须存在（read_remote_meta_snapshot 已校验），
        // 直接写入，不再用 if let Some 跳过。
        txn.add_bytes(&meta_rel, &meta_bytes).map_err(|e| {
            crate::Error::Other(format!(
                "apply_starmap_graph_remote_modify: 暂存 meta.json 失败 ({}): {}",
                starmap_id, e
            ))
        })?;
        txn.add_bytes("starmaps/index.json", &index_bytes)
            .map_err(|e| {
                crate::Error::Other(format!(
                    "apply_starmap_graph_remote_modify: 暂存 index.json 失败: {}",
                    e
                ))
            })?;
        txn.commit().map_err(|e| {
            crate::Error::Other(format!(
                "apply_starmap_graph_remote_modify: 提交事务失败 ({}): {}",
                starmap_id, e
            ))
        })?;

        Ok(())
    }

    /// meta-only 星图远端删除事务（issue #902 第十九轮复核问题1c）。
    ///
    /// meta.json 的 RemoteDeleted 冲突（远端删除本地修改，用户选远端 = 接受删除）。
    /// 删除 meta.json + 更新 index.json（移除该星图 ID）。不删 graph（graph 不存在）。
    fn apply_starmap_meta_remote_delete(
        sync_root: &Path,
        path: &str,
    ) -> crate::Result<Vec<String>> {
        let Some(starmap_id) = Self::parse_starmap_id_from_meta_path(path) else {
            return Err(crate::Error::Other(format!(
                "apply_starmap_meta_remote_delete: 非法 meta.json 路径 '{}'",
                path
            )));
        };

        let meta_rel = format!("starmaps/{}.meta.json", starmap_id);
        let index_bytes = Self::load_index_and_remove_starmap(sync_root, &starmap_id)?;

        let mut txn = crate::storage::transaction::SaveTransaction::new(sync_root);
        txn.add_delete(&meta_rel)?;
        txn.add_bytes("starmaps/index.json", &index_bytes)
            .map_err(|e| {
                crate::Error::Other(format!(
                    "apply_starmap_meta_remote_delete: 暂存 index.json 失败: {}",
                    e
                ))
            })?;
        txn.commit().map_err(|e| {
            crate::Error::Other(format!(
                "apply_starmap_meta_remote_delete: 提交事务失败 ({}): {}",
                starmap_id, e
            ))
        })?;

        Ok(vec![meta_rel])
    }

    /// meta-only 星图远端修改恢复事务（issue #902 第十九轮复核问题1c）。
    ///
    /// meta.json 的 BothChanged 冲突（本地删除远端修改，用户选远端 = 恢复远端 meta）。
    /// 1. 从 conflict.remote_snapshot_path 读取远端 meta.json
    /// 2. 从 conflict.remote_graph_snapshot_dir 读取 index 身份事实
    /// 3. SaveTransaction 原子提交：写入 meta.json + 更新 index.json
    ///
    /// 不恢复 graph（graph 不存在）。
    fn apply_starmap_meta_remote_modify(
        sync_root: &Path,
        path: &str,
        conflict: &SyncConflict,
    ) -> crate::Result<()> {
        let Some(starmap_id) = Self::parse_starmap_id_from_meta_path(path) else {
            return Err(crate::Error::Other(format!(
                "apply_starmap_meta_remote_modify: 非法 meta.json 路径 '{}'",
                path
            )));
        };

        let meta_rel = format!("starmaps/{}.meta.json", starmap_id);

        // 从 remote_snapshot_path 读取远端 meta.json
        let snapshot_rel = conflict.remote_snapshot_path.as_ref().ok_or_else(|| {
            crate::Error::Other(format!(
                "apply_starmap_meta_remote_modify: 缺少 remote_snapshot_path (meta '{}')",
                starmap_id
            ))
        })?;
        let meta_bytes = std::fs::read(sync_root.join(snapshot_rel)).map_err(|e| {
            crate::Error::Other(format!(
                "apply_starmap_meta_remote_modify: 读取远端 meta snapshot 失败 ({}): {}",
                starmap_id, e
            ))
        })?;

        // 读取 index 身份事实
        let snapshot_dir_rel = conflict.remote_graph_snapshot_dir.as_ref().ok_or_else(|| {
            crate::Error::Other(format!(
                "apply_starmap_meta_remote_modify: 缺少 remote_graph_snapshot_dir (meta '{}')",
                starmap_id
            ))
        })?;
        // issue #902 第十九轮复核问题3：index_facts.json 必须存在且可解析，
        // 缺失/损坏时返回错误（不再传 None 静默跳过）。
        let identity_facts = Self::load_starmap_identity_facts(sync_root, snapshot_dir_rel)?;
        let index_bytes =
            Self::load_index_and_ensure_starmap(sync_root, &starmap_id, Some(&identity_facts))?;

        // SaveTransaction 原子提交
        let mut txn = crate::storage::transaction::SaveTransaction::new(sync_root);
        txn.add_bytes(&meta_rel, &meta_bytes).map_err(|e| {
            crate::Error::Other(format!(
                "apply_starmap_meta_remote_modify: 暂存 meta.json 失败 ({}): {}",
                starmap_id, e
            ))
        })?;
        txn.add_bytes("starmaps/index.json", &index_bytes)
            .map_err(|e| {
                crate::Error::Other(format!(
                    "apply_starmap_meta_remote_modify: 暂存 index.json 失败: {}",
                    e
                ))
            })?;
        txn.commit().map_err(|e| {
            crate::Error::Other(format!(
                "apply_starmap_meta_remote_modify: 提交事务失败 ({}): {}",
                starmap_id, e
            ))
        })?;

        Ok(())
    }

    /// 从快照目录读取远端 meta.json（issue #902 第十六轮复核）。
    ///
    /// issue #902 第十七轮复核问题4：快照不存在时返回 `Err` 而非 `Ok(None)`。
    /// "恢复完整远端星图"的快照要求必需的 meta 与 graph 都存在且匹配相同 starmap_id；
    /// 缺少 meta 时应返回明确错误，不注册该图 ID，也不宣告恢复成功。
    fn read_remote_meta_snapshot(
        sync_root: &Path,
        starmap_id: &str,
        snapshot_dir_rel: &str,
    ) -> crate::Result<(String, Vec<u8>)> {
        let meta_rel = format!("starmaps/{}.meta.json", starmap_id);
        let snap_meta_rel = format!("{}/{}", snapshot_dir_rel, meta_rel);
        let snap_meta_path = sync_root.join(&snap_meta_rel);
        if !snap_meta_path.exists() {
            return Err(crate::Error::Other(format!(
                "read_remote_meta_snapshot: 远端整图恢复快照缺少 meta.json ({})，\
                 缺少 meta 无法完整恢复远端星图",
                snap_meta_rel
            )));
        }
        let bytes = std::fs::read(&snap_meta_path).map_err(|e| {
            crate::Error::Other(format!(
                "read_remote_meta_snapshot: 读取远端 meta snapshot 失败 ({}): {}",
                starmap_id, e
            ))
        })?;
        Ok((meta_rel, bytes))
    }

    /// 从快照目录读取远端整图子对象（issue #902 第十四轮复核）。
    ///
    /// 返回 `(obj_files, object_contents)`：
    /// - `obj_files`：`(rel_path, bytes)` 列表，供 SaveTransaction 写入
    /// - `object_contents`：`(object_type, object_id) → bytes` 映射，供派生索引重建
    #[allow(clippy::type_complexity)]
    fn read_remote_graph_snapshot_objects(
        sync_root: &Path,
        member_rels: &[String],
        snapshot_dir_rel: &str,
    ) -> crate::Result<(Vec<(String, Vec<u8>)>, ObjectContentMap)> {
        let mut obj_files: Vec<(String, Vec<u8>)> = Vec::with_capacity(member_rels.len());
        let mut object_contents: ObjectContentMap = std::collections::HashMap::new();
        for obj_rel in member_rels {
            let snap_obj_rel = format!("{}/{}", snapshot_dir_rel, obj_rel);
            let snap_obj_path = sync_root.join(&snap_obj_rel);
            let bytes = std::fs::read(&snap_obj_path).map_err(|e| {
                crate::Error::Other(format!(
                    "read_remote_graph_snapshot_objects: 读取远端子对象快照失败 ({}): {}",
                    obj_rel, e
                ))
            })?;
            if let Some((otype, oid)) = crate::sync::staging::starmap_merge::parse_object_path(
                std::path::Path::new(obj_rel),
            ) {
                object_contents.insert((otype, oid), bytes.clone());
            }
            obj_files.push((obj_rel.clone(), bytes));
        }
        Ok((obj_files, object_contents))
    }

    /// 收集 graph 所有成员对象文件的相对路径（用于事务中批量删除/写入）。
    fn collect_all_member_file_rels(
        starmap_id: &str,
        graph: &crate::starmap::store::meta::GraphMeta,
    ) -> Vec<String> {
        let mut rels = Vec::new();
        for id in &graph.node_ids {
            rels.push(Self::starmap_object_rel(starmap_id, "nodes", id));
        }
        for id in &graph.edge_ids {
            rels.push(Self::starmap_object_rel(starmap_id, "edges", id));
        }
        for id in &graph.embed_instance_ids {
            rels.push(Self::starmap_object_rel(starmap_id, "embeds", id));
        }
        for id in &graph.link_ids {
            rels.push(Self::starmap_object_rel(starmap_id, "links", id));
        }
        rels
    }

    /// 整图 graph.json 冲突的路由（issue #902 第十四轮复核，问题 2）。
    ///
    /// - RemoteDeleted：远端删除整图，用户选远端 = 接受删除 →
    ///   [`apply_starmap_graph_remote_delete`]
    /// - BothChanged：本地删除远端修改，用户选远端 = 恢复远端整图 →
    ///   [`apply_starmap_graph_remote_modify`]
    fn decide_take_remote_starmap_graph(
        sync_root: &Path,
        path: &str,
        state: &mut crate::sync::types::SyncState,
        conflict: &SyncConflict,
    ) -> crate::Result<bool> {
        match conflict.kind {
            SyncConflictKind::RemoteDeleted => {
                // issue #902 第十五轮复核：apply_starmap_graph_remote_delete 返回
                // 实际删除的所有路径集合（graph.json + 全部子对象文件）。
                // 为每个路径清理 known_files / known_files_updated_at 并记录文件级
                // tombstone，防止 snapshot_local_records_read_only 报
                // "known file missing without tombstone"。
                let deleted_paths = Self::apply_starmap_graph_remote_delete(sync_root, path)?;
                Self::cleanup_deleted_paths_and_record_tombstones(state, &deleted_paths);
                if let Some(starmap_id) = Self::parse_starmap_id_from_graph_path(path) {
                    Self::cleanup_starmap_known_files(state, &starmap_id);
                }
                Ok(false)
            }
            SyncConflictKind::BothChanged => {
                Self::apply_starmap_graph_remote_modify(sync_root, path, conflict)?;
                if let Some(starmap_id) = Self::parse_starmap_id_from_graph_path(path) {
                    Self::cleanup_starmap_known_files(state, &starmap_id);
                    // issue #902 第十六轮复核：清理已恢复文件的旧 tombstone。
                    Self::cleanup_starmap_tombstones(state, &starmap_id);
                }
                Ok(false)
            }
        }
    }

    /// meta-only 星图冲突的路由（issue #902 第十九轮复核问题1c）。
    ///
    /// meta-only 星图（无 graph.json）的冲突以 meta.json 为锚点。
    /// - RemoteDeleted：远端删除整图，用户选远端 = 接受删除 →
    ///   [`apply_starmap_meta_remote_delete`]
    /// - BothChanged：本地删除远端修改，用户选远端 = 恢复远端 meta →
    ///   [`apply_starmap_meta_remote_modify`]
    fn decide_take_remote_starmap_meta(
        sync_root: &Path,
        path: &str,
        state: &mut crate::sync::types::SyncState,
        conflict: &SyncConflict,
    ) -> crate::Result<bool> {
        match conflict.kind {
            SyncConflictKind::RemoteDeleted => {
                let deleted_paths = Self::apply_starmap_meta_remote_delete(sync_root, path)?;
                Self::cleanup_deleted_paths_and_record_tombstones(state, &deleted_paths);
                if let Some(starmap_id) = Self::parse_starmap_id_from_meta_path(path) {
                    Self::cleanup_starmap_known_files(state, &starmap_id);
                }
                Ok(false)
            }
            SyncConflictKind::BothChanged => {
                Self::apply_starmap_meta_remote_modify(sync_root, path, conflict)?;
                if let Some(starmap_id) = Self::parse_starmap_id_from_meta_path(path) {
                    Self::cleanup_starmap_known_files(state, &starmap_id);
                    Self::cleanup_starmap_tombstones(state, &starmap_id);
                }
                Ok(false)
            }
        }
    }

    /// 清理指定星图前缀下的子对象冲突记录（issue #902 第十四轮复核）。
    ///
    /// 整图 graph.json 冲突解决后调用：从 `conflicts_json` / `state.conflicts` /
    /// `state.conflicted_files` 移除该星图路径下除 `graph_path` 自身外的所有条目。
    fn cleanup_subobject_conflicts(
        state: &mut crate::sync::types::SyncState,
        conflicts_json: &mut Vec<SyncConflict>,
        graph_path: &str,
    ) {
        let Some(starmap_id) = Self::parse_starmap_id_from_graph_path(graph_path) else {
            return;
        };
        let prefix = format!("starmaps/{}/", starmap_id);
        // 只清理子对象冲突（前缀匹配且不是 graph.json 自身）
        conflicts_json
            .retain(|c| !(c.local_path.starts_with(&prefix) && c.local_path != graph_path));
        state
            .conflicts
            .retain(|c| !(c.local_path.starts_with(&prefix) && c.local_path != graph_path));
        state
            .conflicted_files
            .retain(|p| !(p.starts_with(&prefix) && p != graph_path));
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
        Self::resolve_conflict_mark_merged_at_target(sync_root, path, None)
    }

    pub fn resolve_conflict_mark_merged_for_group(
        sync_root: &Path,
        path: &str,
        group: &crate::sync::types::AppDataGroup,
    ) -> crate::Result<()> {
        Self::resolve_conflict_mark_merged_at_target(sync_root, path, Some(group))
    }

    fn resolve_conflict_mark_merged_at_target(
        sync_root: &Path,
        path: &str,
        group: Option<&crate::sync::types::AppDataGroup>,
    ) -> crate::Result<()> {
        validate_group_conflict_path(group, path, "resolve_conflict_mark_merged")?;
        let mut state = load_conflict_state(sync_root, group)?;
        let mut conflicts_json = load_conflicts_for_target(sync_root, group)?;

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
        persist_conflict_state_for_target(sync_root, group, &state, &conflicts_json)?;
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

#[cfg(test)]
#[path = "conflict/tests.rs"]
mod tests;

#[cfg(test)]
mod persist_manifest_tests {
    use super::*;
    use crate::sync::types::{ManifestFileRecord, SyncManifest};

    fn record(path: &str, hash: &str) -> ManifestFileRecord {
        ManifestFileRecord {
            path: path.to_string(),
            content_hash: hash.to_string(),
            updated_at_ms: 1,
            deleted_at_ms: None,
            device_id: "dev".to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        }
    }

    const MANIFEST: &str = "app-meta/sync/manifest.sync.json";

    fn object_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let objects = dir.join("app-meta/sync/manifest/objects");
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&objects)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        files.sort();
        files
    }

    #[test]
    fn unchanged_manifest_is_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = SyncManifest {
            files: vec![record("volumes/a.json", "h1")],
        };
        let state = crate::sync::types::SyncState::default();
        persist_sync_merge_result(dir.path(), MANIFEST, &manifest, &state, &[]).unwrap();
        let objects_before: Vec<(std::path::PathBuf, Vec<u8>)> = object_files(dir.path())
            .into_iter()
            .map(|path| {
                let bytes = std::fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();
        assert_eq!(objects_before.len(), 1);
        // 索引内容寻址：路径由 root 记录的 sha 决定。
        let root: crate::sync::lww::ManifestRoot = serde_json::from_slice(
            &std::fs::read(dir.path().join("app-meta/sync/manifest/root.json")).unwrap(),
        )
        .unwrap();
        let index_sha = root.namespaces.get("volumes").expect("volumes index");
        let index_path = dir.path().join(format!(
            "app-meta/sync/manifest/index/volumes/{index_sha}.json"
        ));
        let index_before = std::fs::read(&index_path).unwrap();
        let root_path = dir.path().join("app-meta/sync/manifest/root.json");
        let root_before = std::fs::read(&root_path).unwrap();

        persist_sync_merge_result(dir.path(), MANIFEST, &manifest, &state, &[]).unwrap();
        for (path, bytes) in objects_before {
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        assert_eq!(std::fs::read(&index_path).unwrap(), index_before);
        assert_eq!(
            std::fs::read(&root_path).unwrap(),
            root_before,
            "清单语义未变化时不得重写 root（不推进本地版本）"
        );
    }

    #[test]
    fn empty_delta_persists_state_without_touching_manifest_root() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = SyncManifest {
            files: vec![record("volumes/a.json", "h1")],
        };
        persist_sync_merge_result(
            dir.path(),
            MANIFEST,
            &manifest,
            &crate::sync::types::SyncState::default(),
            &[],
        )
        .unwrap();
        let root_path = dir.path().join("app-meta/sync/manifest/root.json");
        let root_before = std::fs::read(&root_path).unwrap();

        // 空 delta 但 state 真实变化：只提交 state/conflicts，不写清单根。
        let mut state = crate::sync::types::SyncState::default();
        state.device_id = "device-after-empty-delta".to_string();
        persist_sync_merge_delta(
            dir.path(),
            MANIFEST,
            &crate::sync::manifest_delta::ManifestDelta::default(),
            &state,
            &[],
        )
        .unwrap();

        assert_eq!(
            std::fs::read(&root_path).unwrap(),
            root_before,
            "空增量只更新 state/conflicts 时不得写 root"
        );
        let state_text =
            std::fs::read_to_string(dir.path().join("app-meta/sync/state.local.json")).unwrap();
        assert!(
            state_text.contains("device-after-empty-delta"),
            "state 必须照常提交，实际 {state_text}"
        );
    }

    #[test]
    fn changed_manifest_rewrites_only_affected_object() {
        let dir = tempfile::tempdir().unwrap();
        let old = SyncManifest {
            files: vec![record("volumes/a.json", "h1"), record("project.json", "p1")],
        };
        persist_sync_merge_result(
            dir.path(),
            MANIFEST,
            &old,
            &crate::sync::types::SyncState::default(),
            &[],
        )
        .unwrap();
        let project_object = object_files(dir.path())
            .into_iter()
            .find(|path| {
                let bytes = std::fs::read(path).unwrap();
                let text = String::from_utf8(bytes).unwrap();
                text.contains("project.json")
            })
            .expect("project object");
        let project_before = std::fs::read(&project_object).unwrap();

        let new = SyncManifest {
            files: vec![record("volumes/a.json", "h2"), record("project.json", "p1")],
        };
        persist_sync_merge_result(
            dir.path(),
            MANIFEST,
            &new,
            &crate::sync::types::SyncState::default(),
            &[],
        )
        .unwrap();
        assert_eq!(std::fs::read(&project_object).unwrap(), project_before);
        let written = crate::sync::lww::ManifestStore::new(dir.path(), MANIFEST)
            .load_all()
            .unwrap()
            .unwrap();
        let volumes = written
            .files
            .iter()
            .find(|r| r.path == "volumes/a.json")
            .unwrap();
        assert_eq!(volumes.content_hash, "h2");
    }
}
