//! # 星图对象级删除事务（Durable Object-Level Delete Transaction）
//!
//! 解决 Issue #805 评论 5912394108：对象级删除（node/edge/embed/link/hyperlink）
//! 不是 durable transaction，崩溃窗口在 `delete_*_file_to_trash()` 和
//! `object_delete_facts.push(...)` 之间——重启后原路径已没文件，也没有任何
//! durable fact，还是会重新得到 "known file missing without tombstone"。
//!
//! ## 问题背景
//!
//! 旧 `flush_save_queue` 的删除流程：
//! 1. 生成 trash token
//! 2. `delete_*_file_to_trash()` 先 durable rename 文件
//! 3. rename 成功后才 push `SyncDeleteFact`
//! 4. `ensure_sync_tombstones_from_facts()`
//!
//! 如果进程崩在步骤 2 和 3 之间，重启后原路径已经没文件，也没有任何 durable
//! fact，还是会重新得到 "known file missing without tombstone"。
//! 而且 tombstone 写失败时只写 recovery_log 然后继续往下写 GraphMeta 并清
//! `deleted_*_ids`，这等于 tombstone 写失败时主动把可重试状态清掉。
//!
//! ## 解决方案
//!
//! 把对象级删除做成 durable transaction：
//! 1. **plan 阶段**：从 `deleted_*_ids` 固定删除集合 → 计算所有原文件路径
//!    （不碰磁盘）→ 加载 SyncState 用 `known_files` 填真实 `original_hash`、
//!    用真实 `device_id` 填 `deleted_by` → 生成固定 trash path → durable 写
//!    journal（`phase=Planned`）
//! 2. **rename 阶段**：逐个 `durable_rename` 文件到 trash
//! 3. **tombstone 阶段**：写 `SyncState.tombstones`（**失败直接返回 Err**，
//!    不写 GraphMeta、不清 `deleted_*_ids`）→ 更新 `phase=Tombstoned`
//! 4. **GraphMeta 阶段**：写 GraphMeta 的 deletion revision → 更新
//!    `phase=GraphMetaWritten`
//! 5. **commit 阶段**：清 `deleted_*_ids` → 删除 journal
//!
//! 关键变化：
//! - **不再** 在 rename 后临时构造 `SyncDeleteFact`，而是在 rename 前就构造
//!   好完整 facts 并 durable 写入 journal
//! - **不再** 用空字符串填 `original_hash` 和 `deleted_by`，而是从 SyncState
//!   的 `known_files` 填真实 hash，用真实 `device_id`
//! - tombstone 写失败时 **必须返回 Err**，不能只写 recovery_log 然后继续
//!
//! ## journal 文件路径
//!
//! `app-meta/starmap-object-delete-journals/{starmap_id}.json`
//!
//! 每个星图同时只有一个活跃的对象删除事务（按 `starmap_id` 命名）。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::storage::journal::workspace_change::SyncDeleteFact;

/// journal 文件所在目录（app_meta 下）。
const STARMAP_OBJECT_DELETE_JOURNALS_DIR: &str = "app-meta/starmap-object-delete-journals";

/// 对象种类。
///
/// 标识被删除的星图对象类型，供恢复阶段按类型幂等重放 rename。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StarMapObjectKind {
    Node,
    Edge,
    Embed,
    Link,
    Hyperlink,
}

/// 单个删除目标。
///
/// `original_path` 是文件相对于 `app_data_root` 的正斜杠路径，
/// 在 plan 阶段从 ID 纯函数计算（不碰磁盘），apply/recover 阶段直接消费。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StarMapObjectDeleteTarget {
    pub kind: StarMapObjectKind,
    pub id: String,
    pub original_path: String,
}

/// 事务阶段。
///
/// 写入 journal，供崩溃恢复判断事务进度：
/// - `Planned`: plan/facts 已 durable 写入 journal，尚未执行 rename
/// - `Tombstoned`: rename 完成，tombstone 已写入
/// - `GraphMetaWritten`: GraphMeta 已写入，事务完成，可清 journal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StarMapObjectDeletePhase {
    Planned,
    Tombstoned,
    GraphMetaWritten,
}

/// 完整的对象删除事务计划。
///
/// 在任何 `rename()` 之前构造并 durable 写入 journal。
/// 包含固定的 trash 路径、完整的删除目标和 `sync_delete_facts`，
/// 供 apply 阶段和恢复阶段幂等执行。
///
/// 关键不变量：
/// - `trash_rel_path` 在 plan 阶段生成，apply/recover 阶段不得重新生成。
/// - `sync_delete_facts` 在 plan 阶段从 SyncState `known_files` 读取
///   `original_hash`，用真实 `device_id` 填 `deleted_by`，apply/recover 阶段
///   直接消费，不重新扫描磁盘或重新加载 SyncState。
/// - `objects` 和 `sync_delete_facts` 一一对应（按 `original_path`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedStarMapObjectDelete {
    /// 本次事务的唯一 token。
    pub token: String,
    /// 待删除对象的星图 ID。
    pub starmap_id: String,
    /// 固定的 trash 目录路径（相对于 app_data_root，正斜杠）。
    pub trash_rel_path: String,
    /// 完整的删除目标列表。
    pub objects: Vec<StarMapObjectDeleteTarget>,
    /// 完整的同步删除事实（每个待删文件一条），与 `objects` 一一对应。
    pub sync_delete_facts: Vec<SyncDeleteFact>,
    /// 当前事务阶段。
    pub phase: StarMapObjectDeletePhase,
}

impl PlannedStarMapObjectDelete {
    /// journal 文件路径：`app-meta/starmap-object-delete-journals/{starmap_id}.json`。
    ///
    /// 每个星图同时只有一个活跃的对象删除事务。
    fn journal_file_path(app_data_root: &Path, starmap_id: &str) -> PathBuf {
        app_data_root
            .join(STARMAP_OBJECT_DELETE_JOURNALS_DIR)
            .join(format!("{starmap_id}.json"))
    }

    /// durable 写入 journal 文件（原子写入）。
    ///
    /// 在执行任何 `rename()` 之前调用，确保崩溃后能恢复。
    /// 覆盖该星图已有的 journal（正常情况下不应有，除非上一轮事务未完成）。
    pub fn save_planned(app_data_root: &Path, plan: &Self) -> Result<()> {
        let path = Self::journal_file_path(app_data_root, &plan.starmap_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_vec(plan).map_err(|e| {
            crate::error::Error::Io(std::io::Error::other(format!(
                "PlannedStarMapObjectDelete::save_planned: serialize: {e}"
            )))
        })?;
        crate::storage::atomic_write_bytes(&path, &content)
    }

    /// 读取未完成的 journal。
    ///
    /// 返回 `Ok(None)` 表示无活跃事务。
    /// 返回 `Ok(Some(plan))` 表示有未完成事务，供 bootstrap 恢复。
    pub fn load(app_data_root: &Path, starmap_id: &str) -> Result<Option<Self>> {
        let path = Self::journal_file_path(app_data_root, starmap_id);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read(&path)?;
        let plan: Self = serde_json::from_slice(&content).map_err(|e| {
            crate::error::Error::Io(std::io::Error::other(format!(
                "PlannedStarMapObjectDelete::load: parse {}: {e}",
                path.display()
            )))
        })?;
        Ok(Some(plan))
    }

    /// 更新事务阶段并 durable 写盘。
    ///
    /// 在 rename + tombstone 成功后更新为 `Tombstoned`；
    /// 在 GraphMeta 成功后更新为 `GraphMetaWritten`。
    /// journal 不存在时返回错误（不应在正常流程中发生）。
    pub fn update_phase(
        app_data_root: &Path,
        starmap_id: &str,
        phase: StarMapObjectDeletePhase,
    ) -> Result<()> {
        let path = Self::journal_file_path(app_data_root, starmap_id);
        if !path.exists() {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "PlannedStarMapObjectDelete::update_phase: journal not found for starmap {starmap_id}"
                ),
            )));
        }
        let content = fs::read(&path)?;
        let mut plan: Self = serde_json::from_slice(&content).map_err(|e| {
            crate::error::Error::Io(std::io::Error::other(format!(
                "PlannedStarMapObjectDelete::update_phase: parse {}: {e}",
                path.display()
            )))
        })?;
        plan.phase = phase;
        let new_content = serde_json::to_vec(&plan).map_err(|e| {
            crate::error::Error::Io(std::io::Error::other(format!(
                "PlannedStarMapObjectDelete::update_phase: serialize: {e}"
            )))
        })?;
        crate::storage::atomic_write_bytes(&path, &new_content)
    }

    /// 删除 journal 文件（幂等）。
    ///
    /// 在事务完成（`GraphMetaWritten` + 清 `deleted_*_ids`）后调用。
    /// journal 已不存在时返回 `Ok(())`。
    pub fn clear(app_data_root: &Path, starmap_id: &str) -> Result<()> {
        let path = Self::journal_file_path(app_data_root, starmap_id);
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn planned_starmap_object_delete_roundtrip() {
        let dir = tempdir().unwrap();
        let plan = PlannedStarMapObjectDelete {
            token: "tok1".to_string(),
            starmap_id: "sm_x".to_string(),
            trash_rel_path: "sync/trash/tok1".to_string(),
            objects: vec![StarMapObjectDeleteTarget {
                kind: StarMapObjectKind::Node,
                id: "n1".to_string(),
                original_path: "starmaps/sm_x/nodes/00/n1.json".to_string(),
            }],
            sync_delete_facts: vec![SyncDeleteFact {
                original_path: "starmaps/sm_x/nodes/00/n1.json".to_string(),
                original_hash: "abc".to_string(),
                deleted_at: 123,
                deleted_by: "dev1".to_string(),
                trash_path: "sync/trash/tok1/starmaps/sm_x/nodes/00/n1.json".to_string(),
            }],
            phase: StarMapObjectDeletePhase::Planned,
        };
        PlannedStarMapObjectDelete::save_planned(dir.path(), &plan).unwrap();
        let loaded = PlannedStarMapObjectDelete::load(dir.path(), "sm_x")
            .unwrap()
            .unwrap();
        assert_eq!(loaded, plan);
    }

    #[test]
    fn load_nonexistent_returns_none() {
        let dir = tempdir().unwrap();
        let loaded = PlannedStarMapObjectDelete::load(dir.path(), "sm_none").unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn update_phase_persists() {
        let dir = tempdir().unwrap();
        let plan = PlannedStarMapObjectDelete {
            token: "tok2".to_string(),
            starmap_id: "sm_y".to_string(),
            trash_rel_path: "sync/trash/tok2".to_string(),
            objects: vec![],
            sync_delete_facts: vec![],
            phase: StarMapObjectDeletePhase::Planned,
        };
        PlannedStarMapObjectDelete::save_planned(dir.path(), &plan).unwrap();
        PlannedStarMapObjectDelete::update_phase(
            dir.path(),
            "sm_y",
            StarMapObjectDeletePhase::Tombstoned,
        )
        .unwrap();
        let loaded = PlannedStarMapObjectDelete::load(dir.path(), "sm_y")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.phase, StarMapObjectDeletePhase::Tombstoned);
    }

    #[test]
    fn clear_is_idempotent() {
        let dir = tempdir().unwrap();
        // 不存在时也 OK
        PlannedStarMapObjectDelete::clear(dir.path(), "sm_z").unwrap();
        // 存在时删除
        let plan = PlannedStarMapObjectDelete {
            token: "tok3".to_string(),
            starmap_id: "sm_z".to_string(),
            trash_rel_path: "sync/trash/tok3".to_string(),
            objects: vec![],
            sync_delete_facts: vec![],
            phase: StarMapObjectDeletePhase::GraphMetaWritten,
        };
        PlannedStarMapObjectDelete::save_planned(dir.path(), &plan).unwrap();
        PlannedStarMapObjectDelete::clear(dir.path(), "sm_z").unwrap();
        assert!(PlannedStarMapObjectDelete::load(dir.path(), "sm_z")
            .unwrap()
            .is_none());
        // 再清一次也 OK
        PlannedStarMapObjectDelete::clear(dir.path(), "sm_z").unwrap();
    }
}
