//! # 统一错误类型
//!
//! 所有 Core 模块共享此错误类型。
//! 客户端通过 `Result<T>` 统一处理错误，不允许吞掉 Core 错误。
//!
//! ## 设计原则
//!
//! - **错误码稳定**：`code()` 返回的字符串是跨端 API 契约，不可随意更改
//! - **可恢复性**：`recoverable()` 标记错误是否可重试或自动恢复
//! - **结构化参数**：`params()` 返回错误上下文，UI 层用参数做本地化，不靠正则匹配 message
//! - **debug_message 不展示**：仅用于日志和调试，不直接展示给普通用户

use serde::Serialize;
use std::collections::HashMap;
use thiserror::Error;

/// Core 层统一错误枚举。
///
/// 所有错误变体都携带足够上下文，便于客户端决定如何展示给用户。
///
/// 错误分类原则：
/// - `code()` 返回稳定字符串，是跨端 API 契约，不可随意更改
/// - `recoverable()` 标记是否可重试——UI 据此决定是否显示"重试"按钮
/// - `params()` 返回结构化参数，UI 用 code + params 做本地化，不依赖正则匹配 message
/// - `debug_message` 仅用于日志和调试，不直接展示给普通用户
///
/// 平台端不得依赖错误文案的包含关系作为主判断（见 AGENTS.md），
/// 必须使用 `code()` 或 `SyncErrorCategory::from_code()` 做分类。
#[derive(Error, Debug)]
pub enum Error {
    /// 文件系统 I/O 错误。可恢复（磁盘临时不可用等）。
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON 序列化/反序列化错误。不可恢复（数据格式损坏）。
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),
    /// 项目 ID 未找到。
    #[error("Project not found")]
    ProjectNotFound,
    /// 卷 ID 未找到。
    #[error("Volume not found")]
    VolumeNotFound,
    /// 章节 ID 未找到。
    #[error("Chapter not found")]
    ChapterNotFound,
    /// 空覆写被阻止——章节已有内容时不允许用空字符串覆盖。
    /// 防止误操作清空章节正文。
    #[error("blocked_empty_overwrite: chapter_id={chapter_id}, old_len={old_len}, new_len={new_len}, reason={reason}")]
    EmptyOverwriteBlocked {
        chapter_id: String,
        old_len: usize,
        new_len: usize,
        reason: String,
    },
    /// 功能未实现。
    #[error("Not implemented")]
    NotImplemented,
    /// 拒绝删除根目录。
    #[error("Refuse to delete root")]
    RefuseToDeleteRoot,
    /// 删除目标无效（非项目/卷/章节路径）。
    #[error("Invalid delete target: {0}")]
    InvalidDeleteTarget(String),

    // --- Sync errors ---
    /// 同步认证失败（Token 无效/权限不足）。不可恢复，需用户干预。
    #[error("Sync auth failed: {reason}")]
    SyncAuthFailed { reason: String },
    /// 同步网络不可用（DNS/TLS/连接失败）。可恢复，可重试。
    #[error("Sync network unavailable: {reason}")]
    SyncNetworkUnavailable { reason: String },
    /// 同步 API 速率限制。可恢复，等待 retry_after_secs 后重试。
    #[error("Sync rate limited: retry_after_secs={retry_after_secs}")]
    SyncRateLimited { retry_after_secs: u64 },
    /// 正文文件双端修改冲突。不可恢复，需用户选择 keep local / take remote / mark merged。
    #[error(
        "Sync document conflict: path={path}, local_hash={local_hash}, remote_hash={remote_hash}"
    )]
    SyncDocumentConflict {
        path: String,
        local_hash: String,
        remote_hash: String,
    },
    /// 同步事务不完整（部分文件上传/下载失败）。可恢复，重试补全。
    #[error("Sync incomplete transaction: tx_id={transaction_id}, missing={missing_files:?}")]
    SyncIncompleteTransaction {
        transaction_id: String,
        missing_files: Vec<String>,
    },
    /// Git checkout 冲突（本地有未提交变更阻止 pull）。不可恢复，需用户处理。
    #[error("Sync checkout conflict: {summary_json}")]
    SyncCheckoutConflict { summary_json: String },
    /// 同步冲突已检测到（通用标记）。
    #[error("Sync conflict detected")]
    SyncConflictDetected,
    /// Git non-fast-forward 错误（远端有新提交，本地也有）。
    #[error("Sync non-fast-forward: {detail}")]
    SyncNonFastForward { detail: String },
    /// Git unrelated histories 错误（本地和远端无共同祖先）。
    #[error("Sync unrelated histories: {detail}")]
    SyncUnrelatedHistories { detail: String },
    /// 远端分支不存在。
    #[error("Sync remote branch not found: {detail}")]
    SyncRemoteBranchNotFound { detail: String },

    /// 远端 Provider 错误——包含分类和上下文，便于平台端做错误映射。
    /// Provider-neutral：GitHub/WebDAV/CloudKit 等远端实现共用此变体。
    /// `category` 为 provider-neutral 的错误分类字符串（如 "file_not_found"、
    /// "remote_sha_conflict"、"api_error"），不包含 "github_" 前缀。
    #[error(
        "Remote provider error [{category}]: {context} failed with status {status}: {body_preview}"
    )]
    SyncRemoteError {
        category: String,
        context: String,
        status: u16,
        body_preview: String,
    },

    // --- Storage errors ---
    /// 磁盘空间不足。
    #[error("Disk full: path={path}, required={required_bytes} bytes")]
    DiskFull { path: String, required_bytes: u64 },
    /// 存储事务不完整（部分文件写入失败）。
    #[error("Storage transaction incomplete: tx_id={transaction_id}")]
    StorageTransactionIncomplete { transaction_id: String },
    /// 保存队列部分写入/删除失败，仍有未落盘数据。
    #[error("Save queue flush incomplete: failed_types={failed_types:?}, remaining_queue_len={remaining_queue_len}")]
    SaveQueueFlushIncomplete {
        failed_types: Vec<String>,
        remaining_queue_len: usize,
    },

    /// 兜底错误——不应频繁使用，新错误应添加具体变体。
    #[error("Other error: {0}")]
    Other(String),
}

impl Error {
    /// 跨端 Bridge 使用的稳定错误码。
    ///
    /// UI 可以根据 code 做展示分支，message 只用于用户提示或调试。
    /// 这些字符串是 API 契约，不可随意更改。
    pub fn code(&self) -> &'static str {
        match self {
            Error::Io(_) => "IO_ERROR",
            Error::Json(_) => "JSON_ERROR",
            Error::ProjectNotFound => "PROJECT_NOT_FOUND",
            Error::VolumeNotFound => "VOLUME_NOT_FOUND",
            Error::ChapterNotFound => "CHAPTER_NOT_FOUND",
            Error::EmptyOverwriteBlocked { .. } => "EMPTY_OVERWRITE_BLOCKED",
            Error::NotImplemented => "NOT_IMPLEMENTED",
            Error::RefuseToDeleteRoot => "REFUSE_DELETE_ROOT",
            Error::InvalidDeleteTarget(_) => "INVALID_DELETE_TARGET",
            Error::SyncAuthFailed { .. } => "SYNC_AUTH_FAILED",
            Error::SyncNetworkUnavailable { .. } => "SYNC_NETWORK_UNAVAILABLE",
            Error::SyncRateLimited { .. } => "SYNC_RATE_LIMITED",
            Error::SyncDocumentConflict { .. } => "SYNC_DOCUMENT_CONFLICT",
            Error::SyncIncompleteTransaction { .. } => "SYNC_INCOMPLETE_TRANSACTION",
            Error::SyncCheckoutConflict { .. } => "SYNC_CHECKOUT_CONFLICT",
            Error::SyncConflictDetected => "SYNC_CONFLICT_DETECTED",
            Error::SyncNonFastForward { .. } => "SYNC_NON_FAST_FORWARD",
            Error::SyncUnrelatedHistories { .. } => "SYNC_UNRELATED_HISTORIES",
            Error::SyncRemoteBranchNotFound { .. } => "SYNC_REMOTE_BRANCH_NOT_FOUND",
            Error::SyncRemoteError { .. } => "SYNC_REMOTE_API_ERROR",
            Error::DiskFull { .. } => "DISK_FULL",
            Error::StorageTransactionIncomplete { .. } => "STORAGE_TRANSACTION_INCOMPLETE",
            Error::SaveQueueFlushIncomplete { .. } => "SAVE_QUEUE_FLUSH_INCOMPLETE",
            Error::Other(_) => "OTHER",
        }
    }

    /// 错误是否可恢复（可重试或自动修复）。
    ///
    /// UI 用此决定是否显示"重试"按钮或自动重试。
    pub fn recoverable(&self) -> bool {
        match self {
            Error::Io(_) => true,
            Error::Json(_) => false,
            Error::ProjectNotFound => false,
            Error::VolumeNotFound => false,
            Error::ChapterNotFound => false,
            Error::EmptyOverwriteBlocked { .. } => false,
            Error::NotImplemented => false,
            Error::RefuseToDeleteRoot => false,
            Error::InvalidDeleteTarget(_) => false,
            Error::SyncAuthFailed { .. } => false,
            Error::SyncNetworkUnavailable { .. } => true,
            Error::SyncRateLimited { .. } => true,
            Error::SyncDocumentConflict { .. } => false,
            Error::SyncIncompleteTransaction { .. } => true,
            Error::SyncCheckoutConflict { .. } => false,
            Error::SyncConflictDetected => false,
            Error::SyncNonFastForward { .. } => false,
            Error::SyncUnrelatedHistories { .. } => false,
            Error::SyncRemoteBranchNotFound { .. } => true,
            // SyncRemoteError 按 category 结构化判断可恢复性：
            // - remote_sha_conflict：乐观并发冲突（IfMatch 不匹配或 CreateNew 时对象已存在），
            //   不可重试，需上层拉取远端最新版本后重新决策或上报冲突让用户处理。
            // - file_not_found：远端对象不存在，重试也不会出现，不可重试。
            // - 其他 category（如 api_error、network 类临时错误）：保守视为可恢复，允许重试。
            Error::SyncRemoteError { category, .. } => {
                !matches!(category.as_str(), "remote_sha_conflict" | "file_not_found")
            }
            Error::DiskFull { .. } => false,
            Error::StorageTransactionIncomplete { .. } => true,
            Error::SaveQueueFlushIncomplete { .. } => true,
            Error::Other(_) => true,
        }
    }

    /// 结构化错误参数，供 UI 层做本地化。
    ///
    /// UI 根据 `code` + `params` 生成用户提示，不依赖 Rust 的 debug message。
    pub fn params(&self) -> HashMap<String, String> {
        let mut m = HashMap::new();
        match self {
            Error::EmptyOverwriteBlocked {
                chapter_id,
                old_len,
                new_len,
                reason,
            } => {
                m.insert("chapter_id".into(), chapter_id.clone());
                m.insert("old_len".into(), old_len.to_string());
                m.insert("new_len".into(), new_len.to_string());
                m.insert("reason".into(), reason.clone());
            }
            Error::SyncAuthFailed { reason } => {
                m.insert("reason".into(), reason.clone());
            }
            Error::SyncNetworkUnavailable { reason } => {
                m.insert("reason".into(), reason.clone());
            }
            Error::SyncRateLimited { retry_after_secs } => {
                m.insert("retry_after_secs".into(), retry_after_secs.to_string());
            }
            Error::SyncDocumentConflict {
                path,
                local_hash,
                remote_hash,
            } => {
                m.insert("path".into(), path.clone());
                m.insert("local_hash".into(), local_hash.clone());
                m.insert("remote_hash".into(), remote_hash.clone());
            }
            Error::SyncIncompleteTransaction {
                transaction_id,
                missing_files,
            } => {
                m.insert("transaction_id".into(), transaction_id.clone());
                m.insert("missing_files".into(), missing_files.join(","));
            }
            Error::SyncCheckoutConflict { summary_json } => {
                m.insert("summary_json".into(), summary_json.clone());
            }
            Error::SyncNonFastForward { detail } => {
                m.insert("detail".into(), detail.clone());
            }
            Error::SyncUnrelatedHistories { detail } => {
                m.insert("detail".into(), detail.clone());
            }
            Error::SyncRemoteBranchNotFound { detail } => {
                m.insert("detail".into(), detail.clone());
            }
            Error::SyncRemoteError {
                category,
                context,
                status,
                body_preview,
            } => {
                m.insert("category".into(), category.clone());
                m.insert("context".into(), context.clone());
                m.insert("status".into(), status.to_string());
                m.insert("body_preview".into(), body_preview.clone());
            }
            Error::DiskFull {
                path,
                required_bytes,
            } => {
                m.insert("path".into(), path.clone());
                m.insert("required_bytes".into(), required_bytes.to_string());
            }
            Error::StorageTransactionIncomplete { transaction_id } => {
                m.insert("transaction_id".into(), transaction_id.clone());
            }
            Error::SaveQueueFlushIncomplete {
                failed_types,
                remaining_queue_len,
            } => {
                m.insert("failed_types".into(), failed_types.join(","));
                m.insert(
                    "remaining_queue_len".into(),
                    remaining_queue_len.to_string(),
                );
            }
            Error::InvalidDeleteTarget(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            _ => {}
        }
        m
    }

    /// 同步错误分类键，供 SyncErrorCategory::from_code 直接使用。
    ///
    /// 对于 SyncRemoteError，返回结构化的 category 字段；
    /// 对于其他同步错误，返回与 code() 相同的值。
    /// 对于非同步错误，返回空字符串。
    pub fn sync_category(&self) -> &str {
        match self {
            Error::SyncRemoteError { category, .. } => category,
            Error::SyncAuthFailed { .. } => "auth_error",
            Error::SyncNetworkUnavailable { .. } => "network_failed",
            Error::SyncRateLimited { .. } => "api_rate_limited",
            Error::SyncDocumentConflict { .. } => "conflict",
            Error::SyncIncompleteTransaction { .. } => "local_io_error",
            Error::SyncCheckoutConflict { .. } => "checkout_conflict",
            Error::SyncConflictDetected => "conflict",
            Error::SyncNonFastForward { .. } => "non_fast_forward",
            Error::SyncUnrelatedHistories { .. } => "unrelated_histories",
            Error::SyncRemoteBranchNotFound { .. } => "branch_missing",
            _ => "",
        }
    }
}

/// 跨端 Bridge 错误 DTO，供 JNI/Qt 层稳定序列化。
///
/// 前端根据 `code` + `params` 做本地化展示。
/// `debug_message` 仅用于日志，不展示给普通用户。
/// `recoverable` 标记是否可重试。
#[derive(Serialize, Debug, Clone)]
pub struct BridgeError {
    pub code: String,
    pub debug_message: String,
    pub recoverable: bool,
    pub params: HashMap<String, String>,
}

impl From<&Error> for BridgeError {
    fn from(error: &Error) -> Self {
        Self {
            code: error.code().to_string(),
            debug_message: error.to_string(),
            recoverable: error.recoverable(),
            params: error.params(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recoverable() {
        assert!(Error::Io(std::io::Error::other("test")).recoverable());
        assert!(!Error::ProjectNotFound.recoverable());
        assert!(Error::SyncNetworkUnavailable {
            reason: "timeout".into()
        }
        .recoverable());
        assert!(!Error::SyncAuthFailed {
            reason: "bad token".into()
        }
        .recoverable());
    }

    #[test]
    fn test_recoverable_sync_remote_error_by_category() {
        // remote_sha_conflict：乐观并发冲突，不可重试
        let conflict = Error::SyncRemoteError {
            category: "remote_sha_conflict".into(),
            context: "conditional_write".into(),
            status: 409,
            body_preview: "sha mismatch".into(),
        };
        assert!(!conflict.recoverable());

        // file_not_found：远端对象不存在，不可重试
        let not_found = Error::SyncRemoteError {
            category: "file_not_found".into(),
            context: "read".into(),
            status: 404,
            body_preview: "missing".into(),
        };
        assert!(!not_found.recoverable());

        // 其他 category（如 api_error）：保守视为可恢复，允许重试
        let api_error = Error::SyncRemoteError {
            category: "api_error".into(),
            context: "upload".into(),
            status: 500,
            body_preview: "server error".into(),
        };
        assert!(api_error.recoverable());

        // 未知 category 也视为可恢复（保守处理，避免误判不可恢复导致用户卡死）
        let unknown = Error::SyncRemoteError {
            category: "something_unexpected".into(),
            context: "read".into(),
            status: 503,
            body_preview: "tmp".into(),
        };
        assert!(unknown.recoverable());
    }

    #[test]
    fn test_params() {
        let err = Error::SyncDocumentConflict {
            path: "projects/p1/chapter.md".into(),
            local_hash: "abc".into(),
            remote_hash: "def".into(),
        };
        let p = err.params();
        assert_eq!(p.get("path").unwrap(), "projects/p1/chapter.md");
        assert_eq!(p.get("local_hash").unwrap(), "abc");
    }
}
