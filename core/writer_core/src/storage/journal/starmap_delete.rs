//! # 星图删除事务（Crash-safe StarMap Delete Transaction）
//!
//! 解决 Issue #805：`delete_starmap_with_changes()` 不调用
//! `ensure_sync_tombstones_from_facts()`，删除星图后 `SyncState.tombstones`
//! 缺失，导致 `snapshot_local_records_read_only` 返回 Err。
//!
//! ## 问题背景
//!
//! 旧 `delete_starmap_with_changes()` 先调 `delete_starmap` 删文件，再组装
//! `WorkspaceChangeSet`，全程不接触 `SyncState`/tombstone。chapter/volume
//! 删除链路调用了 `ensure_tombstones_from_facts`，StarMap 删除链路漏接。
//!
//! 后果：删除星图后 `starmaps/{id}.meta.json` 与 `starmaps/{id}/nodes/...`
//! 等对象文件从磁盘消失，但 `SyncState.tombstones` 没有对应 `original_path`
//! 记录。下次同步时 `snapshot_local_records_read_only` 遍历 `known_files`，
//! 发现这些路径 known 但磁盘缺失且无 tombstone，进入 "known file missing
//! without tombstone — cannot fabricate delete record" 分支返回 Err。
//!
//! ## 解决方案
//!
//! 把 StarMap 删除统一做成 durable transaction：
//! 1. `plan_delete_starmap`：枚举 meta + graph dir 全部文件 → 生成
//!    `SyncDeleteFact` → 返回 `PlannedStarmapDelete`（不修改磁盘）。
//! 2. `apply_planned_delete_starmap`：断 index 引用 → 移动 graph 文件到
//!    trash → 删除 meta → 写 `SyncState` tombstones → 完成。
//!
//! 事务在任何物理删除前保存每个即将消失文件的 `SyncDeleteFact`。
//! 物理删除完成后，必须先确保 `SyncState.tombstones` 持久化成功，API 才能
//! 返回成功。崩溃恢复时如果已删文件但 tombstone 未写完，从 journal 的 facts
//! 幂等补齐。
//!
//! ## 与 WorkspaceChangeJournal 的关系
//!
//! 本模块的 plan/apply 接入统一 durable workspace journal
//! （`WorkspaceChangeJournal`，op_type=`DeleteStarMap`，
//! delete_target=`StarMap{starmap_id}`）。`delete_starmap` 的完整流程为：
//! 1. `plan_delete_starmap`：构造变更集 + `PlannedStarmapDelete`（不修改磁盘）；
//! 2. `WorkspaceChangeJournal::save_pending`：在 apply 前把完整 plan 落盘；
//! 3. `apply_planned_delete_starmap`：断 index → 移动 graph/meta 到 trash →
//!    写 `SyncState` tombstones；
//! 4. `mark_local_applied` → `record_workspace_change_set_history` →
//!    `mark_history_recorded` → `clear_journal`。
//!
//! 这样进程死在 apply 和写 tombstone 之间，重启后 bootstrap 能根据 journal 的
//! `sync_delete_facts` 幂等补齐 tombstone，不再得到 "known file missing
//! without tombstone"。history 失败也保留 journal，下次启动补记。
//!
//! ## 与 GraphMeta.deleted_since_last_sync 的关系
//!
//! `GraphMeta.deleted_since_last_sync`（逻辑对象删除）和
//! `SyncState.tombstones`（文件路径级删除）是两套不同语义。StarMap 删除
//! 产生前者（由 `delete_starmap` 写 meta.deleted_since_last_sync），本模块
//! 补上后者（文件级 LWW tombstone）。同一次删除事务同时驱动两边。

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;

use crate::storage::journal::workspace_change::{
    ensure_sync_tombstones_from_facts, DeleteTarget, PlannedWorkspaceDelete, SyncDeleteFact,
};
use crate::storage::workspace_git::WorkspaceChangeSet;

/// 星图删除计划（不修改磁盘），在任何物理删除前构造。
///
/// 包含固定的 trash 路径和完整的 `sync_delete_facts`，供 apply 阶段
/// 幂等执行本地删除 + 补齐 tombstone。
///
/// 关键不变量：
/// - `trash_rel_path` 在 plan 阶段生成，apply 阶段不得重新生成。
/// - `sync_delete_facts` 在 plan 阶段（源文件还存在时）遍历构造，
///   apply 阶段直接消费，不重新扫描磁盘。
/// - `sync_delete_facts` 至少含 meta 文件一条；graph dir 非空时还含对象文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedStarmapDelete {
    /// 待删除的星图 ID。
    pub starmap_id: String,
    /// 固定的 trash 目录路径（相对于 app_data_root，正斜杠）。
    /// 例如 `sync/trash/1234567890_uuid_starmap_id`。
    pub trash_rel_path: String,
    /// 完整的同步删除事实（每个待删文件一条），至少含 meta 文件。
    pub sync_delete_facts: Vec<SyncDeleteFact>,
}

impl PlannedStarmapDelete {
    /// 转成统一 journal 使用的 `PlannedWorkspaceDelete`。
    ///
    /// `delete_target` 设为 `DeleteTarget::StarMap { starmap_id }`，
    /// `trash_rel_path` 和 `sync_delete_facts` 直接复用，供
    /// `WorkspaceChangeJournal::save_pending` 落盘。
    pub fn to_workspace_planned(&self) -> PlannedWorkspaceDelete {
        PlannedWorkspaceDelete {
            delete_target: DeleteTarget::StarMap {
                starmap_id: self.starmap_id.clone(),
            },
            trash_rel_path: self.trash_rel_path.clone(),
            sync_delete_facts: self.sync_delete_facts.clone(),
        }
    }
}

/// 构造星图删除计划（不修改磁盘）。
///
/// 在任何物理删除之前：
/// 1. 校验星图存在；
/// 2. 校验无外部引用（embed/link/edge/portal 指向此星图），有则拒绝；
/// 3. 生成固定 trash token；
/// 4. 遍历 `starmaps/{id}.meta.json` + `starmaps/{id}/` 目录下所有对象文件，
///    构造 `SyncDeleteFact`（`original_hash` 从 SyncState `known_files` 读取）；
/// 5. 组装 `WorkspaceChangeSet`：`Delete(meta) + DeleteTree(starmaps/{id}) +
///    Upsert(starmaps/index.json)`。
///
/// 返回 `(WorkspaceChangeSet, PlannedStarmapDelete)`，供
/// `delete_starmap` 先 `save_pending` 落盘 journal 再 apply。
///
/// `device_id` 用真实设备 ID，不写固定 `"local"`。
pub fn plan_delete_starmap(
    app_data_root: &Path,
    starmap_id: &str,
    device_id: &str,
) -> Result<(WorkspaceChangeSet, PlannedStarmapDelete)> {
    // 校验星图存在 + 无外部引用。复用 starmap::delete_starmap 的前置校验。
    // 这里只读不写，安全。
    let refs = crate::starmap::find_starmap_references(app_data_root, starmap_id)?;
    let external_refs: Vec<_> = refs
        .into_iter()
        .filter(|r| r.host_starmap_id != starmap_id)
        .collect();
    if !external_refs.is_empty() {
        return Err(crate::error::Error::Io(std::io::Error::other(format!(
            "plan_delete_starmap: cannot delete StarMap {} because it is referenced by {} external places.",
            starmap_id,
            external_refs.len()
        ))));
    }

    // meta 文件必须存在（星图存在的证据）。
    let meta_rel = format!("starmaps/{}.meta.json", starmap_id);
    let meta_abs = app_data_root.join(&meta_rel);
    if !meta_abs.exists() {
        return Err(crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "plan_delete_starmap: StarMap meta not found: {}",
                starmap_id
            ),
        )));
    }

    // 生成固定 trash token（在任何 rename 之前）。
    let trash_token = format!(
        "{}_{}_{}",
        chrono::Utc::now().timestamp_millis(),
        uuid::Uuid::new_v4(),
        starmap_id
    );
    let trash_rel_path = format!("sync/trash/{trash_token}");

    // 构造 sync_delete_facts：meta 文件 + graph dir 下所有对象文件。
    let facts = build_starmap_delete_facts(app_data_root, starmap_id, &trash_rel_path, device_id)?;
    if facts.is_empty() {
        return Err(crate::error::Error::Other(format!(
            "plan_delete_starmap: no sync_delete_facts generated for starmap {starmap_id} \
             — meta file should always produce one fact"
        )));
    }

    // 组装 WorkspaceChangeSet。
    let change_set = WorkspaceChangeSet::new()
        .add_delete(PathBuf::from(&meta_rel))
        .add_delete_tree(PathBuf::from(format!("starmaps/{starmap_id}")))
        .add_upsert(PathBuf::from("starmaps/index.json"));

    let planned = PlannedStarmapDelete {
        starmap_id: starmap_id.to_string(),
        trash_rel_path,
        sync_delete_facts: facts,
    };
    Ok((change_set, planned))
}

/// 消费 `PlannedStarmapDelete` 执行星图删除。
///
/// apply 顺序固定为：
/// 1. 校验 `planned.starmap_id` 匹配；
/// 2. 断 index 引用（retain 排除该 id）；
/// 3. 移动 graph dir 到 trash（durable_rename，存在时）；
/// 4. 移动 meta 文件到 trash（durable_rename，存在时）；
/// 5. 写 `SyncState` tombstones（`ensure_sync_tombstones_from_facts`）。
///
/// 步骤 2-4 是物理删除，步骤 5 是 LWW tombstone 落盘。
/// 两步都成功后才算本地删除完成。不在 apply 阶段重新生成 trash 路径或
/// 重新扫描磁盘——事实来自 plan 阶段。
///
/// 幂等性：如果 graph dir / meta 已不存在（重复 apply），跳过 rename；
/// tombstone 按 `original_path + trash_path` 匹配跳过已存在记录。
pub fn apply_planned_delete_starmap(
    app_data_root: &Path,
    starmap_id: &str,
    planned: &PlannedStarmapDelete,
) -> Result<()> {
    if planned.starmap_id != starmap_id {
        return Err(crate::error::Error::Other(format!(
            "apply_planned_delete_starmap: starmap_id mismatch — expected {starmap_id}, got {}",
            planned.starmap_id
        )));
    }

    // 1. 断 index 引用。load_index/save_index 来自 starmap 模块。
    //    使用 starmap 的私有 index 操作需要 pub(crate) 暴露。这里通过
    //    starmap 模块提供的公开接口操作 index。
    //
    //    实际上 starmap::delete_starmap 已经做了 index 断引用 + 删 graph dir +
    //    删 meta。但那个函数不写 tombstone，且顺序是"先删完再返回"。
    //    本函数按 plan/apply 顺序执行，不调用 delete_starmap，而是自己
    //    按事务顺序操作，确保 trash + tombstone 落盘。
    //
    //    为了不暴露 starmap 的私有 index 函数，这里复用 starmap 模块已公开的
    //    load/save 能力。但 starmap 的 index 函数是私有的。
    //    解决方案：在 starmap 模块新增 pub(crate) 的 index 操作，或直接
    //    在本函数里读写 index.json。
    //
    //    选择后者：直接读写 index.json，避免扩大 starmap 模块 API。
    //    index.json 格式由 StarMapIndexRecord 定义，已 pub。
    apply_starmap_delete_internal(app_data_root, starmap_id, planned)
}

/// 内部 apply 实现，直接操作 index.json + graph dir + meta + tombstone。
fn apply_starmap_delete_internal(
    app_data_root: &Path,
    starmap_id: &str,
    planned: &PlannedStarmapDelete,
) -> Result<()> {
    use crate::starmap::StarMapIndexRecord;

    // ── 1. 断 index 引用 ──
    let index_path = app_data_root.join("starmaps/index.json");
    let mut idx: StarMapIndexRecord = if index_path.exists() {
        let content = fs::read_to_string(&index_path)?;
        serde_json::from_str(&content)?
    } else {
        StarMapIndexRecord {
            schema_version: crate::starmap::migration::NEW_INDEX_SCHEMA_VERSION,
            starmap_ids: vec![],
            main_starmap_by_project: std::collections::HashMap::new(),
            updated_at: crate::starmap::now_epoch(),
        }
    };
    idx.starmap_ids.retain(|id| id != starmap_id);
    idx.main_starmap_by_project.retain(|_, v| v != starmap_id);
    idx.updated_at = crate::starmap::now_epoch();
    // 落盘 index。
    fs::create_dir_all(app_data_root.join("starmaps"))?;
    let index_content = serde_json::to_string_pretty(&idx)?;
    crate::storage::atomic_write_string(&index_path, &index_content)?;

    // ── 2. 移动 graph dir 到 trash ──
    let graph_dir = app_data_root.join("starmaps").join(starmap_id);
    let trash_root = app_data_root.join(&planned.trash_rel_path);
    if graph_dir.exists() {
        fs::create_dir_all(&trash_root)?;
        let graph_trash = trash_root.join(starmap_id);
        crate::storage::durable_rename(&graph_dir, &graph_trash)?;
    }

    // ── 3. 移动 meta 文件到 trash ──
    let meta_abs = app_data_root.join(format!("starmaps/{starmap_id}.meta.json"));
    if meta_abs.exists() {
        fs::create_dir_all(&trash_root)?;
        let meta_trash = trash_root.join(format!("{starmap_id}.meta.json"));
        crate::storage::durable_rename(&meta_abs, &meta_trash)?;
    }

    // ── 4. 写 SyncState tombstones（关键修复） ──
    //    这是 Issue #805 的根因修复：删除星图后必须补 LWW 文件级 tombstone，
    //    否则 snapshot_local_records_read_only 会因 "known file missing +
    //    no tombstone" 返回 Err。
    ensure_sync_tombstones_from_facts(app_data_root, &planned.sync_delete_facts)?;

    Ok(())
}

/// 在源文件还存在时遍历所有待删文件，提前构造 `SyncDeleteFact` 列表。
///
/// 遍历范围：
/// - `starmaps/{id}.meta.json`（meta 文件，必有一条）
/// - `starmaps/{id}/` 目录下所有对象文件（nodes/edges/embeds/links/...）
///
/// `original_path`：文件相对于 `app_data_root` 的正斜杠路径（与 sync scanner
/// 的 relative_path 一致，因为星图在 App scope 下同步）。
/// `original_hash`：从 `app_data_root` 的 SyncState `known_files` 读取，缺失则为空。
/// `trash_path`：按固定 `trash_rel_path` 推导的 trash 内相对路径。
/// `deleted_by`：使用传入的 `device_id`。
fn build_starmap_delete_facts(
    app_data_root: &Path,
    starmap_id: &str,
    trash_rel_path: &str,
    device_id: &str,
) -> Result<Vec<SyncDeleteFact>> {
    let state = crate::sync::SyncService::load_sync_state(app_data_root)?;
    let now = chrono::Utc::now().timestamp();

    let mut facts = Vec::new();

    // ── meta 文件 ──
    let meta_rel = format!("starmaps/{starmap_id}.meta.json");
    let meta_hash = state
        .known_files
        .get(&meta_rel)
        .cloned()
        .unwrap_or_default();
    let meta_trash = format!("{trash_rel_path}/{starmap_id}.meta.json");
    facts.push(SyncDeleteFact {
        original_path: meta_rel,
        original_hash: meta_hash,
        deleted_at: now,
        deleted_by: device_id.to_string(),
        trash_path: meta_trash,
    });

    // ── graph dir 下所有对象文件 ──
    let graph_dir = app_data_root.join("starmaps").join(starmap_id);
    if graph_dir.exists() {
        for item in walkdir::WalkDir::new(&graph_dir).into_iter() {
            let entry = item.map_err(|e| {
                crate::error::Error::Io(std::io::Error::other(format!(
                    "build_starmap_delete_facts: walkdir error under {}: {e}",
                    graph_dir.display()
                )))
            })?;
            if !entry.file_type().is_file() {
                continue;
            }
            // original_path: starmaps/{id}/nodes/.../n1.json
            let rel_under_graph = entry
                .path()
                .strip_prefix(&graph_dir)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            let original_path = format!("starmaps/{starmap_id}/{rel_under_graph}");
            let trash_path = format!("{trash_rel_path}/{starmap_id}/{rel_under_graph}");
            let original_hash = state
                .known_files
                .get(&original_path)
                .cloned()
                .unwrap_or_default();
            facts.push(SyncDeleteFact {
                original_path,
                original_hash,
                deleted_at: now,
                deleted_by: device_id.to_string(),
                trash_path,
            });
        }
    }

    Ok(facts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planned_starmap_delete_is_debug_clone_eq() {
        let p = PlannedStarmapDelete {
            starmap_id: "sm_x".to_string(),
            trash_rel_path: "sync/trash/x".to_string(),
            sync_delete_facts: vec![],
        };
        let _ = format!("{p:?}");
        let _ = p.clone();
    }
}
