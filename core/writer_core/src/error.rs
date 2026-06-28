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
#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Workspace not found or invalid")]
    InvalidWorkspace,
    #[error("Project not found")]
    ProjectNotFound,
    #[error("Volume not found")]
    VolumeNotFound,
    #[error("Chapter not found")]
    ChapterNotFound,
    #[error("blocked_empty_overwrite: chapter_id={chapter_id}, old_len={old_len}, new_len={new_len}, reason={reason}")]
    EmptyOverwriteBlocked {
        chapter_id: String,
        old_len: usize,
        new_len: usize,
        reason: String,
    },
    #[error("Not implemented")]
    NotImplemented,
    #[error("Refuse to delete workspace root")]
    RefuseToDeleteWorkspaceRoot,
    #[error("Invalid delete target: {0}")]
    InvalidDeleteTarget(String),

    // --- Sync errors ---
    #[error("Sync auth failed: {reason}")]
    SyncAuthFailed { reason: String },
    #[error("Sync network unavailable: {reason}")]
    SyncNetworkUnavailable { reason: String },
    #[error("Sync rate limited: retry_after_secs={retry_after_secs}")]
    SyncRateLimited { retry_after_secs: u64 },
    #[error(
        "Sync document conflict: path={path}, local_hash={local_hash}, remote_hash={remote_hash}"
    )]
    SyncDocumentConflict {
        path: String,
        local_hash: String,
        remote_hash: String,
    },
    #[error("Sync incomplete transaction: tx_id={transaction_id}, missing={missing_files:?}")]
    SyncIncompleteTransaction {
        transaction_id: String,
        missing_files: Vec<String>,
    },

    // --- Storage errors ---
    #[error("Disk full: path={path}, required={required_bytes} bytes")]
    DiskFull { path: String, required_bytes: u64 },
    #[error("Storage transaction incomplete: tx_id={transaction_id}")]
    StorageTransactionIncomplete { transaction_id: String },

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
            Error::InvalidWorkspace => "INVALID_WORKSPACE",
            Error::ProjectNotFound => "PROJECT_NOT_FOUND",
            Error::VolumeNotFound => "VOLUME_NOT_FOUND",
            Error::ChapterNotFound => "CHAPTER_NOT_FOUND",
            Error::EmptyOverwriteBlocked { .. } => "EMPTY_OVERWRITE_BLOCKED",
            Error::NotImplemented => "NOT_IMPLEMENTED",
            Error::RefuseToDeleteWorkspaceRoot => "REFUSE_DELETE_WORKSPACE_ROOT",
            Error::InvalidDeleteTarget(_) => "INVALID_DELETE_TARGET",
            Error::SyncAuthFailed { .. } => "SYNC_AUTH_FAILED",
            Error::SyncNetworkUnavailable { .. } => "SYNC_NETWORK_UNAVAILABLE",
            Error::SyncRateLimited { .. } => "SYNC_RATE_LIMITED",
            Error::SyncDocumentConflict { .. } => "SYNC_DOCUMENT_CONFLICT",
            Error::SyncIncompleteTransaction { .. } => "SYNC_INCOMPLETE_TRANSACTION",
            Error::DiskFull { .. } => "DISK_FULL",
            Error::StorageTransactionIncomplete { .. } => "STORAGE_TRANSACTION_INCOMPLETE",
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
            Error::InvalidWorkspace => false,
            Error::ProjectNotFound => false,
            Error::VolumeNotFound => false,
            Error::ChapterNotFound => false,
            Error::EmptyOverwriteBlocked { .. } => false,
            Error::NotImplemented => false,
            Error::RefuseToDeleteWorkspaceRoot => false,
            Error::InvalidDeleteTarget(_) => false,
            Error::SyncAuthFailed { .. } => false,
            Error::SyncNetworkUnavailable { .. } => true,
            Error::SyncRateLimited { .. } => true,
            Error::SyncDocumentConflict { .. } => false,
            Error::SyncIncompleteTransaction { .. } => true,
            Error::DiskFull { .. } => false,
            Error::StorageTransactionIncomplete { .. } => true,
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
            Error::InvalidDeleteTarget(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            _ => {}
        }
        m
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
    fn test_error_codes_stable() {
        assert_eq!(Error::ProjectNotFound.code(), "PROJECT_NOT_FOUND");
        assert_eq!(Error::ChapterNotFound.code(), "CHAPTER_NOT_FOUND");
        assert_eq!(
            Error::SyncDocumentConflict {
                path: "x".into(),
                local_hash: "a".into(),
                remote_hash: "b".into(),
            }
            .code(),
            "SYNC_DOCUMENT_CONFLICT"
        );
    }

    #[test]
    fn test_recoverable() {
        assert!(Error::Io(std::io::Error::new(std::io::ErrorKind::Other, "test")).recoverable());
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

    #[test]
    fn test_bridge_error_fields() {
        let err = Error::SyncRateLimited {
            retry_after_secs: 60,
        };
        let bridge = BridgeError::from(&err);
        assert_eq!(bridge.code, "SYNC_RATE_LIMITED");
        assert!(bridge.recoverable);
        assert_eq!(bridge.params.get("retry_after_secs").unwrap(), "60");
    }
}
