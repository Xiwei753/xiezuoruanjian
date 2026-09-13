//! 统一 workspace 变更事务 journal。
//!
//! 用于作品、卷、章节删除产生的 `WorkspaceChangeSet` 的持久化事务。
//! 流程固定为：
//! 1. 先写 pending journal（`save_pending`）
//! 2. 执行本地物理删除
//! 3. 标记 local_applied（`mark_local_applied`）
//! 4. 写 workspace history/tombstone
//! 5. 标记 history_recorded（`mark_history_recorded`）
//! 6. 清 journal（`clear_journal`）
//!
//! 启动 workspace 时扫描未完成记录（`recover_unfinished`）：
//! - 如果本地删除已经完成（`LocalApplied`）但 history 没写进去，就补 history
//! - 只有 history 成功后才清 journal
//!
//! `project_delete.rs` 保留项目删除特定的多阶段事务（worktree move、git move、
//! tombstone、starmap unbind 等），本模块提供通用的轻量事务 journal 供
//! 作品/卷/章节删除使用。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::error::Result;
use crate::storage::workspace_git::WorkspaceChangeSet;

/// workspace 变更 journal 文件名前缀。
const WORKSPACE_CHANGE_JOURNAL_PREFIX: &str = ".sujian-workspace-change-journal-";

/// workspace 变更 journal 所在目录（app_meta 下）。
const WORKSPACE_CHANGE_JOURNALS_DIR: &str = "app-meta/workspace-change-journals";

/// workspace 变更事务阶段。
///
/// 写入 journal，供崩溃恢复判断变更进度：
/// - `Pending`: journal 已落盘，尚未执行本地删除
/// - `LocalApplied`: 本地物理删除已完成，尚未写 workspace history
/// - `HistoryRecorded`: workspace history 已记录，可以清 journal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceChangePhase {
    Pending,
    LocalApplied,
    HistoryRecorded,
}

/// workspace 变更操作类型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceChangeOpType {
    DeleteProject,
    DeleteVolume,
    DeleteChapter,
}

/// 明确的删除目标，供恢复阶段幂等执行本地删除。
///
/// 不再从 change_set 反推 ID，避免 change_set 路径格式变化时恢复出错。
/// `Option` + `#[serde(default)]` 保证旧 journal（无此字段）能反序列化。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteTarget {
    Project {
        project_id: String,
    },
    Volume {
        project_id: String,
        volume_id: String,
    },
    Chapter {
        project_id: String,
        volume_id: String,
        chapter_id: String,
    },
}

/// 单个文件的同步删除事实，供恢复阶段幂等补齐 tombstone。
///
/// 物理删除前在 journal 里保存每个被删除文件的 tombstone 事实。
/// 恢复 Pending 阶段时，即使源目录已在崩溃前被 move 掉，也能根据这些事实
/// 幂等补齐项目自己的 tombstone，再允许推进到 LocalApplied。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncDeleteFact {
    /// 文件在 project_root 下的原始相对路径（正斜杠，Git/远端约定）。
    pub original_path: String,
    /// 删除前的内容哈希（MD5 hex），可为空（未知时）。
    pub original_hash: String,
    /// 删除时间（epoch seconds）。
    pub deleted_at: i64,
    /// 发起删除的设备/来源标识。
    pub deleted_by: String,
    /// trash 目录下的相对路径。
    pub trash_path: String,
}

/// 真正的删除计划，在任何 `rename()` 之前构造并持久化到 journal。
///
/// 包含固定的 trash 路径和完整的 `sync_delete_facts`，供 apply 阶段和
/// 恢复阶段幂等执行本地删除 + 补齐 tombstone。
///
/// 关键不变量：
/// - `trash_rel_path` 在 plan 阶段生成，apply/recover 阶段不得重新生成。
/// - `sync_delete_facts` 在 plan 阶段（源目录还存在时）遍历构造，
///   apply/recover 阶段直接消费，不重新扫描磁盘。
/// - 对 `DeleteVolume/DeleteChapter`，`sync_delete_facts` 不允许为空。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedWorkspaceDelete {
    /// 删除目标。
    pub delete_target: DeleteTarget,
    /// 固定的 trash 目录路径（相对于 app_data_root，正斜杠）。
    /// 例如 `sync/trash/1234567890_uuid_volume_id`。
    pub trash_rel_path: String,
    /// 完整的同步删除事实（每个待删文件一条），不允许为空。
    pub sync_delete_facts: Vec<SyncDeleteFact>,
}

impl PlannedWorkspaceDelete {
    /// 在源目录还存在时遍历所有待删文件，提前构造 `SyncDeleteFact` 列表。
    ///
    /// - `original_path`：文件在 project_root 下的原始相对路径（正斜杠）。
    /// - `original_hash`：从 project_root 的 SyncState `known_files` 读取，缺失则为空。
    /// - `trash_path`：按固定 `trash_rel_path` 推导的 trash 内相对路径。
    /// - `deleted_by`：使用传入的 `device_id`，不写固定 `"local"`。
    ///
    /// 跳过 `app-meta/` 前缀的 sync 引擎内部状态文件。
    pub fn build_facts(
        project_root: &Path,
        source_dir: &Path,
        trash_rel_path: &str,
        device_id: &str,
    ) -> crate::error::Result<Vec<SyncDeleteFact>> {
        let state = crate::sync::SyncService::load_sync_state(project_root)?;
        let now = chrono::Utc::now().timestamp();
        let rel_source_dir = source_dir
            .strip_prefix(project_root)
            .unwrap_or(source_dir)
            .to_string_lossy()
            .replace('\\', "/");

        let mut facts = Vec::new();
        for item in walkdir::WalkDir::new(source_dir).into_iter() {
            let entry = item.map_err(|e| {
                crate::error::Error::Io(std::io::Error::other(format!(
                    "build_facts: walkdir error under {}: {e}",
                    source_dir.display()
                )))
            })?;
            if !entry.file_type().is_file() {
                continue;
            }
            let rel_file_path = entry
                .path()
                .strip_prefix(source_dir)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            // 跳过 sync 引擎内部状态文件。
            if rel_file_path.starts_with("app-meta/") {
                continue;
            }
            let original_file_path = if rel_source_dir.ends_with('/') {
                format!("{}{}", rel_source_dir, rel_file_path)
            } else {
                format!("{}/{}", rel_source_dir, rel_file_path)
            };
            let trash_path = if trash_rel_path.ends_with('/') {
                format!("{}{}", trash_rel_path, rel_file_path)
            } else {
                format!("{}/{}", trash_rel_path, rel_file_path)
            };
            let original_hash = state
                .known_files
                .get(&original_file_path)
                .cloned()
                .unwrap_or_default();
            facts.push(SyncDeleteFact {
                original_path: original_file_path,
                original_hash,
                deleted_at: now,
                deleted_by: device_id.to_string(),
                trash_path,
            });
        }
        Ok(facts)
    }
}

/// workspace 变更 journal。
///
/// 统一保存作品、卷、章节删除产生的 `WorkspaceChangeSet`。
/// 至少包含 change set、device_id、操作类型、创建时间和事务阶段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceChangeJournal {
    /// 本次变更的唯一 token。
    pub token: String,
    /// 变更集。
    pub change_set: WorkspaceChangeSet,
    /// 发起变更的设备 ID。
    #[serde(default)]
    pub device_id: String,
    /// 操作类型。
    pub op_type: WorkspaceChangeOpType,
    /// 创建时间（epoch seconds）。
    pub created_at: i64,
    /// 当前事务阶段。
    pub phase: WorkspaceChangePhase,
    /// 明确的删除目标，供恢复阶段幂等执行本地删除。
    ///
    /// 旧 journal（无此字段）反序列化为 `None`，恢复时按 change_set 路径
    /// 尽力推断（向后兼容）。
    #[serde(default)]
    pub delete_target: Option<DeleteTarget>,
    /// 本次删除对应的同步删除事实，供恢复阶段幂等补齐 tombstone。
    ///
    /// 旧 journal（无此字段）反序列化为空 Vec，恢复时按现有逻辑处理（向后兼容）。
    #[serde(default)]
    pub sync_delete_facts: Vec<SyncDeleteFact>,
    /// 完整的删除计划（固定 trash 路径 + 完整 facts），供 apply/recover 阶段
    /// 幂等执行本地删除。
    ///
    /// 新格式 journal（DeleteVolume/DeleteChapter）必须携带此字段；
    /// 旧 journal（无此字段）反序列化为 None，恢复时按向后兼容逻辑处理。
    #[serde(default)]
    pub planned_delete: Option<PlannedWorkspaceDelete>,
}

/// recover 返回的待处理记录。
///
/// 启动时扫描未完成 journal，根据阶段返回不同处理指令：
/// - `Pending`: journal 落盘但本地删除未完成，需先幂等完成本地删除再推进到 LocalApplied。
/// - `LocalApplied`: 本地删除已完成但 history 没写，需补 history。
/// - `HistoryRecorded`: 已在 recover 内部清理（history 已记）。
#[derive(Debug, Clone)]
pub struct RecoveredWorkspaceChange {
    /// journal token，用于后续 mark/clear。
    pub journal_token: String,
    /// 待补 history 的变更集。
    pub changes: WorkspaceChangeSet,
    /// 操作类型（用于 commit message）。
    pub op_type: WorkspaceChangeOpType,
    /// 当前阶段，决定 bootstrap 如何处理。
    pub phase: WorkspaceChangePhase,
    /// 明确的删除目标（Pending 阶段幂等删除用）。
    pub delete_target: Option<DeleteTarget>,
    /// 原始 journal 的 device_id，推进 phase 时保留原元数据。
    pub device_id: String,
    /// 原始 journal 的 created_at，推进 phase 时保留原元数据。
    pub created_at: i64,
    /// 原始 journal 的 sync_delete_facts，供恢复阶段幂等补齐 tombstone。
    pub sync_delete_facts: Vec<SyncDeleteFact>,
    /// 原始 journal 的 planned_delete，供恢复阶段幂等重放本地删除。
    pub planned_delete: Option<PlannedWorkspaceDelete>,
}

impl WorkspaceChangeJournal {
    /// 创建新的 pending journal 并落盘。
    ///
    /// 在执行本地物理删除前调用，确保崩溃后能恢复。
    /// `delete_target` 明确记录删除目标，供恢复阶段幂等执行本地删除。
    /// `planned_delete` 携带固定的 trash 路径和完整 `sync_delete_facts`，
    /// 供 apply/recover 阶段消费。
    ///
    /// 对 `DeleteVolume/DeleteChapter`：
    /// - 必须提供 `planned_delete`（`None` 视为编程错误）。
    /// - `planned_delete.sync_delete_facts` 不允许为空——空 facts 会落一个
    ///   无法恢复的 Pending journal（恢复时无法补齐 tombstone），直接返回错误。
    /// - `planned_delete.delete_target` 必须与 `delete_target` 一致。
    ///
    /// 对 `DeleteProject`：`planned_delete` 应为 `None`（项目删除有独立事务）。
    pub fn save_pending(
        app_data_root: &Path,
        change_set: &WorkspaceChangeSet,
        device_id: &str,
        op_type: WorkspaceChangeOpType,
        delete_target: Option<DeleteTarget>,
        planned_delete: Option<PlannedWorkspaceDelete>,
    ) -> Result<Self> {
        // 校验：DeleteVolume/DeleteChapter 必须携带非空 planned_delete + 非空 facts。
        let sync_delete_facts: Vec<SyncDeleteFact> = match (&op_type, &planned_delete) {
            (
                WorkspaceChangeOpType::DeleteVolume | WorkspaceChangeOpType::DeleteChapter,
                Some(plan),
            ) => {
                if plan.sync_delete_facts.is_empty() {
                    return Err(crate::error::Error::Other(format!(
                        "save_pending: {op_type:?} requires non-empty sync_delete_facts — \
                         refusing to write an unrecoverable Pending journal"
                    )));
                }
                // 校验 delete_target 一致性。
                if Some(&plan.delete_target) != delete_target.as_ref() {
                    return Err(crate::error::Error::Other(format!(
                        "save_pending: planned_delete.delete_target ({:?}) != delete_target ({:?})",
                        plan.delete_target, delete_target
                    )));
                }
                plan.sync_delete_facts.clone()
            }
            (WorkspaceChangeOpType::DeleteVolume | WorkspaceChangeOpType::DeleteChapter, None) => {
                return Err(crate::error::Error::Other(format!(
                    "save_pending: {op_type:?} requires planned_delete — \
                     refusing to write an unrecoverable Pending journal"
                )));
            }
            (WorkspaceChangeOpType::DeleteProject, Some(_)) => {
                return Err(crate::error::Error::Other(
                    "save_pending: DeleteProject must not carry planned_delete \
                     (project delete uses its own multi-phase transaction)"
                        .to_string(),
                ));
            }
            (WorkspaceChangeOpType::DeleteProject, None) => Vec::new(),
        };

        let token = Uuid::new_v4().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
            .unwrap_or(0);
        let journal = Self {
            token: token.clone(),
            change_set: change_set.clone(),
            device_id: device_id.to_string(),
            op_type,
            created_at: now,
            phase: WorkspaceChangePhase::Pending,
            delete_target,
            sync_delete_facts,
            planned_delete,
        };
        let journal_path = journal_file_path(app_data_root, &token);
        if let Some(parent) = journal_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_vec(&journal).map_err(|e| {
            crate::error::Error::Io(std::io::Error::other(format!(
                "save_pending: serialize journal: {e}"
            )))
        })?;
        crate::storage::atomic_write_bytes(&journal_path, &content)?;
        Ok(journal)
    }

    /// 推进 phase 到 `LocalApplied` 并落盘。
    ///
    /// 在本地物理删除成功后调用。
    pub fn mark_local_applied(&self, app_data_root: &Path) -> Result<()> {
        let updated = Self {
            phase: WorkspaceChangePhase::LocalApplied,
            ..self.clone()
        };
        write_journal(app_data_root, &updated)
    }

    /// 推进 phase 到 `HistoryRecorded` 并落盘。
    ///
    /// 在 workspace history 记录成功后调用。
    pub fn mark_history_recorded(&self, app_data_root: &Path) -> Result<()> {
        let updated = Self {
            phase: WorkspaceChangePhase::HistoryRecorded,
            ..self.clone()
        };
        write_journal(app_data_root, &updated)
    }

    /// 清理 journal 文件。
    ///
    /// 在 `HistoryRecorded` 后调用，完成事务。
    /// 幂等：journal 已不存在时返回 `Ok(())`。
    pub fn clear_journal(&self, app_data_root: &Path) -> Result<()> {
        let path = journal_file_path(app_data_root, &self.token);
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }
}

/// 启动时恢复未完成的 workspace 变更事务。
///
/// 遍历 `app-meta/workspace-change-journals/` 下所有 journal，按阶段返回处理指令：
/// - `HistoryRecorded`: history 已记，直接清理 journal（返回 None）。
/// - `LocalApplied`: 本地删除已完成但 history 没写，返回供 bootstrap 补 history。
/// - `Pending`: journal 落盘但本地删除未完成，返回供 bootstrap 先幂等完成本地删除
///   再推进到 LocalApplied。
///
/// 不再让 Pending 直接进入 history——bootstrap 必须先完成本地删除。
pub fn recover_unfinished(app_data_root: &Path) -> Result<Vec<RecoveredWorkspaceChange>> {
    let journals_dir = app_data_root.join(WORKSPACE_CHANGE_JOURNALS_DIR);
    if !journals_dir.exists() {
        return Ok(Vec::new());
    }

    let mut recovered_list = Vec::new();
    for entry in fs::read_dir(&journals_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name() else {
            continue;
        };
        let file_name = file_name.to_string_lossy();
        if !file_name.starts_with(WORKSPACE_CHANGE_JOURNAL_PREFIX) {
            continue;
        }

        match recover_single_journal(&path) {
            Ok(Some(recovered)) => {
                recovered_list.push(recovered);
            }
            Ok(None) => {
                // 无需补 history（已 HistoryRecorded，已清理）。
            }
            Err(e) => {
                // 恢复失败，保留 journal，下次重启继续。
                log::error!(
                    "[recover_unfinished] failed to recover {}: {}",
                    path.display(),
                    e
                );
            }
        }
    }
    Ok(recovered_list)
}

/// 恢复单个 journal。
///
/// 返回 `Ok(Some(recovered))` 表示需要 bootstrap 按阶段处理；
/// 返回 `Ok(None)` 表示已 HistoryRecorded，已清理。
fn recover_single_journal(journal_path: &Path) -> Result<Option<RecoveredWorkspaceChange>> {
    let content = fs::read(journal_path)?;
    let journal: WorkspaceChangeJournal = serde_json::from_slice(&content).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "recover_single_journal: parse {}: {e}",
            journal_path.display()
        )))
    })?;

    match journal.phase {
        WorkspaceChangePhase::HistoryRecorded => {
            // history 已记，直接清理 journal。
            fs::remove_file(journal_path)?;
            Ok(None)
        }
        WorkspaceChangePhase::LocalApplied => {
            // 本地删除已完成但 history 没写，返回供 bootstrap 补 history。
            Ok(Some(RecoveredWorkspaceChange {
                journal_token: journal.token.clone(),
                changes: journal.change_set.clone(),
                op_type: journal.op_type.clone(),
                phase: WorkspaceChangePhase::LocalApplied,
                delete_target: journal.delete_target.clone(),
                device_id: journal.device_id.clone(),
                created_at: journal.created_at,
                sync_delete_facts: journal.sync_delete_facts.clone(),
                planned_delete: journal.planned_delete.clone(),
            }))
        }
        WorkspaceChangePhase::Pending => {
            // journal 落盘但本地删除未完成，返回供 bootstrap 先幂等完成本地删除。
            // 不再直接进入 history——bootstrap 必须先完成本地删除再推进到 LocalApplied。
            Ok(Some(RecoveredWorkspaceChange {
                journal_token: journal.token.clone(),
                changes: journal.change_set.clone(),
                op_type: journal.op_type.clone(),
                phase: WorkspaceChangePhase::Pending,
                delete_target: journal.delete_target.clone(),
                device_id: journal.device_id.clone(),
                created_at: journal.created_at,
                sync_delete_facts: journal.sync_delete_facts.clone(),
                planned_delete: journal.planned_delete.clone(),
            }))
        }
    }
}

/// 根据 token 计算 journal 文件路径。
fn journal_file_path(app_data_root: &Path, token: &str) -> PathBuf {
    app_data_root
        .join(WORKSPACE_CHANGE_JOURNALS_DIR)
        .join(format!("{}{}", WORKSPACE_CHANGE_JOURNAL_PREFIX, token))
}

/// 写入 journal 文件（覆盖）。
fn write_journal(app_data_root: &Path, journal: &WorkspaceChangeJournal) -> Result<()> {
    let path = journal_file_path(app_data_root, &journal.token);
    let content = serde_json::to_vec(journal).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "write_journal: serialize: {e}"
        )))
    })?;
    crate::storage::atomic_write_bytes(&path, &content)
}
