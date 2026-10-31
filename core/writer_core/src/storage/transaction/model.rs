use serde::{Deserialize, Serialize};

pub(crate) const TRANSACTIONS_DIR: &str = "app-meta/transactions";
pub(crate) const MANIFEST_FILENAME: &str = "manifest.json";
pub(crate) const COMMIT_MARKER: &str = "committed";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionManifest {
    pub transaction_id: String,
    pub created_at_ms: i64,
    pub entries: Vec<TransactionEntry>,
    /// 事务生命周期阶段。
    /// 旧 manifest 中无此字段时反序列化为 `FilesCommitted`（向后兼容）。
    #[serde(default = "default_phase")]
    pub phase: TransactionPhase,
    /// backup 模式的备份条目，写入 manifest 供崩溃恢复。
    /// 旧 manifest 中无此字段时反序列化为空 Vec（向后兼容）。
    #[serde(default)]
    pub backup_entries: Vec<BackupEntry>,
    /// 与正文事务绑定的证明意图事件 ID。
    /// 非空时事务目录必须保留到证明事件写入完成，恢复流程据此判断正文是否已提交。
    #[serde(default)]
    pub proof_intent_ids: Vec<String>,
    /// 事务目录内保存证明恢复材料的文件名。
    /// 意图文件丢失时，恢复流程仍可从该文件重建 proof 事件。
    #[serde(default)]
    pub proof_recovery_file: Option<String>,
}

/// 与正文提交绑定、在事务目录内持久化的证明恢复材料。
///
/// 仅保存 `proof_intent_ids` 时，一旦 `stats/proof-intents/` 中的意图文件丢失，
/// 恢复流程无法判断或重建对应 proof 事件，事务会永久卡住项目同步。
/// 该记录随 `SaveTransaction` 一起落盘，使正文提交、证明入库、事务清理
/// 成为同一个可恢复生命周期。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofRecoveryRecord {
    pub event_id: String,
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub workspace_path: String,
    pub before_text: Option<String>,
    pub after_text: Option<String>,
    pub source_device_id: String,
    pub occurred_at_ms: i64,
}

fn default_phase() -> TransactionPhase {
    TransactionPhase::FilesCommitted
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionEntry {
    pub staging_filename: String,
    pub target_relative: String,
    /// 支持 delete 操作。
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
/// `FilesCommittedPendingGit` 变体已移除。
/// 新代码不再产生此变体。旧 manifest 中可能存在 `"files_committed_pending_git"`
/// 字符串，反序列化时通过自定义 [`Deserialize`] impl 映射到 `FilesCommitted`
/// （恢复时按 `FilesCommitted` + rollback 处理）。`Serialize` 用 derive，
/// 不会产出旧值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionPhase {
    Prepared,
    FilesCommitted,
    Finished,
    RolledBack,
}

///   b)：自定义 `Deserialize`，把旧遗留字符串
/// `files_committed_pending_git` 映射到 `FilesCommitted`。
///
/// 旧 manifest 反序列化时遇到 `"files_committed_pending_git"` 不会失败，
/// 而是返回 `FilesCommitted`，让恢复流程按 `FilesCommitted` + rollback 处理。
impl<'de> Deserialize<'de> for TransactionPhase {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;

        // 中间字符串类型，接收任意 phase 字符串。
        let raw = String::deserialize(deserializer)?;
        match raw.as_str() {
            "prepared" => Ok(TransactionPhase::Prepared),
            "files_committed" => Ok(TransactionPhase::FilesCommitted),
            // 旧遗留：映射到 FilesCommitted，恢复时按 rollback 处理。
            "files_committed_pending_git" => Ok(TransactionPhase::FilesCommitted),
            "finished" => Ok(TransactionPhase::Finished),
            "rolled_back" => Ok(TransactionPhase::RolledBack),
            other => Err(D::Error::custom(format!(
                "unknown TransactionPhase variant: {}",
                other
            ))),
        }
    }
}

/// 备份条目，替代空字符串哨兵。
///
/// `String::new()` 表示"旧文件不存在"时，`backup_dir.join("")` 就是 backup 目录本身，
/// `exists()` 为 true，随后 `fs::copy(目录, 文件)` 会失败。
/// 用明确枚举消除歧义。
///
/// 可序列化，写入 manifest 供崩溃恢复使用。
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

#[derive(Debug, Clone)]
pub struct TransactionRecovery {
    pub transaction_id: String,
    pub recovered_files: Vec<String>,
    pub missing_files: Vec<String>,
    /// 恢复后的持久阶段。`FilesCommitted`/`Finished` 表示正文确实已提交；
    /// `Prepared` 表示重放材料不完整，事务是否落地仍未确认。
    pub phase: TransactionPhase,
    /// 与该事务绑定的证明意图事件 ID。
    pub proof_intent_ids: Vec<String>,
    pub tx_dir: std::path::PathBuf,
}

/// 一次事务目录扫描的完整结果。
///
/// `errors` 保留无法解析/无法读取的事务目录；证明恢复不能把这些目录当作
/// “事务不存在”，否则会把可能已提交的意图误删。
#[derive(Debug, Default)]
pub(crate) struct TransactionRecoveryScan {
    pub transactions: Vec<TransactionRecovery>,
    pub errors: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_roundtrip_current_variants() {
        for p in [
            TransactionPhase::Prepared,
            TransactionPhase::FilesCommitted,
            TransactionPhase::Finished,
            TransactionPhase::RolledBack,
        ] {
            let json = serde_json::to_string(&p).unwrap();
            let back: TransactionPhase = serde_json::from_str(&json).unwrap();
            assert_eq!(p, back);
        }
    }

    ///   b)：旧遗留字符串映射到 FilesCommitted。
    #[test]
    fn phase_legacy_files_committed_pending_git_maps_to_files_committed() {
        let back: TransactionPhase =
            serde_json::from_str("\"files_committed_pending_git\"").unwrap();
        assert_eq!(back, TransactionPhase::FilesCommitted);
    }

    #[test]
    fn phase_serialize_never_emits_legacy_variant() {
        // Serialize 不应产出 files_committed_pending_git。
        for p in [
            TransactionPhase::Prepared,
            TransactionPhase::FilesCommitted,
            TransactionPhase::Finished,
            TransactionPhase::RolledBack,
        ] {
            let json = serde_json::to_string(&p).unwrap();
            assert!(
                !json.contains("files_committed_pending_git"),
                "Serialize 不应产出旧遗留变体: {}",
                json
            );
        }
    }
}
