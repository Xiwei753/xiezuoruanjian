use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub(crate) const TRANSACTIONS_DIR: &str = "app-meta/transactions";
pub(crate) const MANIFEST_FILENAME: &str = "manifest.json";
#[cfg(test)]
pub(crate) const COMMIT_MARKER: &str = "committed";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionManifest {
    pub transaction_id: String,
    pub created_at_ms: i64,
    pub entries: Vec<TransactionEntry>,
    /// #644 评论 5475805198 第1节：事务生命周期阶段。
    /// 旧 manifest 中无此字段时反序列化为 `FilesCommitted`（向后兼容）。
    #[serde(default = "default_phase")]
    pub phase: TransactionPhase,
    /// #644 评论 5475805198 第1节：backup 模式的备份条目，写入 manifest 供崩溃恢复。
    /// 旧 manifest 中无此字段时反序列化为空 Vec（向后兼容）。
    #[serde(default)]
    pub backup_entries: Vec<BackupEntry>,
    /// #644 评论 5476546134 第2节：Git finalize 崩溃恢复记录。
    /// 仅 backup_mode 且需要 Git finalize 时有值。
    /// 旧 manifest 中无此字段时反序列化为 None（向后兼容）。
    #[serde(default)]
    pub git_finalize: Option<crate::sync::git::GitFinalizeRecoveryRecord>,
}

fn default_phase() -> TransactionPhase {
    TransactionPhase::FilesCommitted
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionEntry {
    pub staging_filename: String,
    pub target_relative: String,
    /// #644 评论 5473105049 第2节：支持 delete 操作。
    /// `true` 表示 commit 时应删除 `target_relative`（staging_filename 忽略）。
    /// 旧 manifest 中无此字段时反序列化为 `false`（向后兼容）。
    #[serde(default)]
    pub is_delete: bool,
}

/// #644 评论 5475805198 第1节：事务生命周期阶段。
///
/// 写入 manifest，供崩溃恢复判断事务进度：
/// - `Prepared`：manifest 已写入，文件尚未 rename。
/// - `FilesCommitted`：所有 rename 完成，无需 Git finalize（非 backup_mode）。
/// - `FilesCommittedPendingGit`：rename 完成，等待 Git metadata finalize。
/// - `Finished`：Git finalize 成功，可以清理。
/// - `RolledBack`：rollback 已执行，可以清理。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionPhase {
    Prepared,
    FilesCommitted,
    FilesCommittedPendingGit,
    Finished,
    RolledBack,
}

/// #644 评论 5475413230 第1节：备份条目，替代空字符串哨兵。
///
/// `String::new()` 表示"旧文件不存在"时，`backup_dir.join("")` 就是 backup 目录本身，
/// `exists()` 为 true，随后 `fs::copy(目录, 文件)` 会失败。
/// 用明确枚举消除歧义。
///
/// #644 评论 5475805198 第1节：可序列化，写入 manifest 供崩溃恢复使用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackupEntry {
    /// 旧文件存在，rollback 时从备份恢复。
    RestoreFile {
        target_relative: String,
        backup_filename: String,
    },
    /// 旧文件不存在（commit 新建了它），rollback 时删除。
    RemoveCreated { target_relative: String },
}

#[derive(Debug)]
pub struct TransactionRecovery {
    pub transaction_id: String,
    pub recovered_files: Vec<String>,
    pub missing_files: Vec<String>,
}

/// #644 评论 5475805198 第1节：FilesCommittedPendingGit 状态的事务恢复信息。
///
/// 文件已 commit 到 live，但 Git metadata finalize 尚未完成。
/// 调用方需要决定：完成 Git finalize 或回滚文件。
pub struct PendingGitTransactionRecovery {
    pub transaction_id: String,
    pub manifest: TransactionManifest,
    pub target_root: PathBuf,
    pub tx_dir: PathBuf,
}
