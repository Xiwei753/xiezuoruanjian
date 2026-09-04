use serde::{Deserialize, Serialize};

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

/// 事务生命周期阶段。
///
/// 写入 manifest，供崩溃恢复判断事务进度：
/// - `Prepared`：manifest 已写入，文件尚未 rename。
/// - `FilesCommitted`：所有 rename 完成。
/// - `Finished`：事务成功完成，可以清理。
/// - `RolledBack`：rollback 已执行，可以清理。
///
/// #645 评论 5504296097 第4点：`FilesCommittedPendingGit` 是旧遗留阶段。
/// 新代码不再产生此变体；恢复时直接走 rollback 路径。
/// 保留此变体仅供 legacy manifest DTO 反序列化。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionPhase {
    Prepared,
    FilesCommitted,
    /// 旧遗留：历史事务可能停在此阶段。新代码不再产生此变体。
    /// 恢复时按 `FilesCommitted` + rollback 处理。
    #[serde(rename = "files_committed_pending_git")]
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
