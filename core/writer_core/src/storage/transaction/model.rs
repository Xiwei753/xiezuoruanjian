use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    /// 本事务期望写入的目标内容 MD5。恢复时目标已存在且摘要不同，说明
    /// 之后已有更新版本写入，绝不能再用旧 staging 覆盖。旧 manifest 无此
    /// 字段时为 `None`，保持旧恢复语义。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_after_hash: Option<String>,
    /// 事务准备时目标文件的 MD5。commit 前目标已变化则拒绝覆盖。
    /// 旧 manifest 无此字段时为 `None`，表示“当时目标不存在”；若目标在
    /// commit 前出现，同样视为冲突。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_before_hash: Option<String>,
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
    /// 事务所属的 live 根目录（app_data_root 或 projects/<pid>）。
    /// 旧事务缺少证明恢复材料时，需要据此读取正文终态并追溯备份。
    pub target_root: std::path::PathBuf,
}

/// 一次事务目录扫描的错误，绑定到具体事务。
///
/// `transaction_id` 为 `None` 表示错误发生在事务目录枚举层，无法归属到
/// 某笔事务；manifest 解析失败时用目录名（即事务 ID）兜底。调用方不能靠
/// 字符串包含关系猜测错误属于哪笔事务。
#[derive(Debug)]
pub(crate) struct TransactionRecoveryError {
    pub transaction_id: Option<String>,
    pub message: String,
}

/// 一次事务目录扫描的完整结果。
///
/// `errors` 保留无法解析/无法读取的事务目录；证明恢复不能把这些目录当作
/// “事务不存在”，否则会把可能已提交的意图误删。
#[derive(Debug, Default)]
pub(crate) struct TransactionRecoveryScan {
    pub transactions: Vec<TransactionRecovery>,
    pub errors: Vec<TransactionRecoveryError>,
}

/// 事务目标文件的完整内容事实。
///
/// 恢复不能只看 MD5：章节还要比较 generation、revision、content_sha256，
/// 并区分“staging 里准备写的新版本”和“live 已经被更高版本覆盖”。这里
/// 一次性读取目标文件并保存三种事实，供预检与整章校验共用。
#[derive(Debug, Clone)]
pub(crate) struct TransactionFileSnapshot {
    pub md5: String,
    pub sha256: String,
    pub json: Option<serde_json::Value>,
}

/// 读取事务目标/暂存文件的快照；`NotFound` 返回 `None`，其他 IO 错误保留。
///
/// `parse_json` 由调用方按**目标相对路径**决定：staging/backup 文件名是
/// `file_N`/`backup_N`，不能靠文件名判断它是不是 `chapter.meta.json`。
pub(crate) fn read_transaction_file_snapshot(
    path: &Path,
    parse_json: bool,
) -> std::io::Result<Option<TransactionFileSnapshot>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let md5 = format!("{:x}", md5::compute(&bytes));
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let json = if parse_json {
        serde_json::from_slice(&bytes).ok()
    } else {
        None
    };
    Ok(Some(TransactionFileSnapshot { md5, sha256, json }))
}

/// 对象 meta 的跨版本身份事实。
#[derive(Debug, Default)]
pub(crate) struct TransactionMetadataFacts {
    pub revision: Option<u64>,
    pub generation: Option<String>,
    pub content_sha256: Option<String>,
}

pub(crate) fn transaction_metadata_facts(
    value: Option<&serde_json::Value>,
) -> TransactionMetadataFacts {
    let Some(value) = value else {
        return TransactionMetadataFacts::default();
    };
    let generation = value
        .get("object_generation")
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.is_empty())
        .or_else(|| {
            value
                .get("generation")
                .and_then(serde_json::Value::as_str)
                .filter(|text| !text.is_empty())
        })
        .map(str::to_string);
    let content_sha256 = value
        .get("content_sha256")
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string);
    TransactionMetadataFacts {
        revision: value.get("revision").and_then(serde_json::Value::as_u64),
        generation,
        content_sha256,
    }
}

/// 目标根下的持久删除事实（同步 tombstone / 远端删除集合）。
///
/// Prepared 恢复准备重新创建不存在目标时，必须先排除“对象已被删除”的
/// 事实；删除仍存在的对象由 before/after 哈希与 generation 校验证明是同一
/// 对象，不再用 tombstone 阻断。事务创建之前的 tombstone 只说明历史，不算
/// “事务之后的删除事实”。
pub(crate) struct TransactionTombstoneFacts {
    paths: std::collections::HashSet<String>,
    unavailable: Option<String>,
}

impl TransactionTombstoneFacts {
    pub(crate) fn load(target_root: &Path, created_at_ms: i64) -> Self {
        let state_path = target_root.join("app-meta/sync/state.local.json");
        let legacy_path = target_root.join("app-meta/sync/sync_state.json");
        if !state_path.exists() && !legacy_path.exists() {
            return Self {
                paths: std::collections::HashSet::new(),
                unavailable: None,
            };
        }
        match crate::sync::SyncService::load_sync_state_read_only(target_root, None) {
            Ok(state) => {
                let mut paths = std::collections::HashSet::new();
                for tombstone in &state.tombstones {
                    add_recent_tombstone_path(&mut paths, tombstone, created_at_ms);
                }
                for path in state.deleted_files {
                    paths.insert(normalized_transaction_relative(&path));
                }
                Self {
                    paths,
                    unavailable: None,
                }
            }
            Err(error) => Self {
                paths: std::collections::HashSet::new(),
                unavailable: Some(format!(
                    "sync state {} unreadable: {error}",
                    state_path.display()
                )),
            },
        }
    }

    pub(crate) fn blocks(&self, relative: &str) -> Option<String> {
        let relative = normalized_transaction_relative(relative);
        if self.paths.contains(&relative) {
            return Some("durable tombstone/delete fact exists for target".to_string());
        }
        self.unavailable.clone()
    }
}

/// 事务路径统一使用 `/`，并去掉 manifest 里可能出现的绝对前导 `/`。
pub(crate) fn normalized_transaction_relative(relative: &str) -> String {
    relative
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string()
}

fn add_recent_tombstone_path(
    paths: &mut std::collections::HashSet<String>,
    tombstone: &crate::sync::types::Tombstone,
    created_at_ms: i64,
) {
    // tombstone 只精确到秒；同一秒内的事务开始与删除无法区分时按“事务之后
    // 的删除事实”保守处理，避免复活刚被删除的对象。
    let deleted_at_ms = tombstone
        .deleted_at
        .saturating_mul(1000)
        .saturating_add(1000);
    if created_at_ms <= 0 || deleted_at_ms >= created_at_ms {
        paths.insert(normalized_transaction_relative(&tombstone.original_path));
    }
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
