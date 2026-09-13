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
}

/// recover 返回的待补 history 记录。
///
/// 启动时扫描未完成 journal，如果本地删除已完成（`LocalApplied`）但 history
/// 没写进去，返回此记录供 bootstrap 调 `record_workspace_change_set` 补 history。
/// history 成功后调 `mark_history_recorded` + `clear_journal`。
#[derive(Debug, Clone)]
pub struct RecoveredWorkspaceChange {
    /// journal token，用于后续 mark/clear。
    pub journal_token: String,
    /// 待补 history 的变更集。
    pub changes: WorkspaceChangeSet,
    /// 操作类型（用于 commit message）。
    pub op_type: WorkspaceChangeOpType,
}

impl WorkspaceChangeJournal {
    /// 创建新的 pending journal 并落盘。
    ///
    /// 在执行本地物理删除前调用，确保崩溃后能恢复。
    pub fn save_pending(
        app_data_root: &Path,
        change_set: &WorkspaceChangeSet,
        device_id: &str,
        op_type: WorkspaceChangeOpType,
    ) -> Result<Self> {
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
        atomic_write(&journal_path, &content)?;
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
/// 遍历 `app-meta/workspace-change-journals/` 下所有 journal：
/// - `HistoryRecorded` phase 的 journal 直接清理（history 已记）。
/// - `LocalApplied` phase 的 journal 返回供 bootstrap 补 history。
/// - `Pending` phase 的 journal 返回供 bootstrap 补 history（本地删除可能已完成，
///   也可能未完成——由调用方根据 change_set 判断）。
///
/// history 补记成功后调用方应调 `mark_history_recorded` + `clear_journal`。
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
/// 返回 `Ok(Some(recovered))` 表示需要补 history；
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
        WorkspaceChangePhase::LocalApplied | WorkspaceChangePhase::Pending => {
            // 本地删除可能已完成但 history 没写，返回供 bootstrap 补 history。
            Ok(Some(RecoveredWorkspaceChange {
                journal_token: journal.token.clone(),
                changes: journal.change_set.clone(),
                op_type: journal.op_type.clone(),
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
    atomic_write(&path, &content)
}

/// 原子写入：先写临时文件再 rename，确保崩溃安全。
fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, content)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}
