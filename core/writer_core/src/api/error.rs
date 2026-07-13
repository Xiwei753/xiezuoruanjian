use std::collections::HashMap;

use crate::error::Error;

#[derive(Debug, thiserror::Error)]
pub enum WriterError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("JSON parsing error: {0}")]
    Json(String),
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
        old_len: u32,
        new_len: u32,
        reason: String,
    },
    #[error("Not implemented")]
    NotImplemented,
    #[error("Refuse to delete workspace root")]
    RefuseToDeleteWorkspaceRoot,
    #[error("Invalid delete target: {0}")]
    InvalidDeleteTarget(String),
    #[error("Sync conflict: {0}")]
    SyncConflict(String),
    #[error("Sync failed: {0}")]
    SyncFailed(String),
    #[error("Other error: {0}")]
    Other(String),
}

impl WriterError {
    pub fn code(&self) -> &'static str {
        match self {
            WriterError::Io(_) => "IO_ERROR",
            WriterError::Json(_) => "JSON_ERROR",
            WriterError::InvalidWorkspace => "INVALID_WORKSPACE",
            WriterError::ProjectNotFound => "PROJECT_NOT_FOUND",
            WriterError::VolumeNotFound => "VOLUME_NOT_FOUND",
            WriterError::ChapterNotFound => "CHAPTER_NOT_FOUND",
            WriterError::EmptyOverwriteBlocked { .. } => "EMPTY_OVERWRITE_BLOCKED",
            WriterError::NotImplemented => "NOT_IMPLEMENTED",
            WriterError::RefuseToDeleteWorkspaceRoot => "REFUSE_DELETE_WORKSPACE_ROOT",
            WriterError::InvalidDeleteTarget(_) => "INVALID_DELETE_TARGET",
            WriterError::SyncConflict(_) => "SYNC_CONFLICT",
            WriterError::SyncFailed(_) => "SYNC_FAILED",
            WriterError::Other(_) => "OTHER",
        }
    }

    /// 返回稳定的 i18n message key，供 UI 层做本地化映射。
    ///
    /// 这些 key 是跨端 API 契约，不可随意更改。
    pub fn message_key(&self) -> &'static str {
        match self {
            WriterError::Io(_) => "error.io",
            WriterError::Json(_) => "error.json",
            WriterError::InvalidWorkspace => "error.invalid_workspace",
            WriterError::ProjectNotFound => "error.project_not_found",
            WriterError::VolumeNotFound => "error.volume_not_found",
            WriterError::ChapterNotFound => "error.chapter_not_found",
            WriterError::EmptyOverwriteBlocked { .. } => "error.empty_overwrite_blocked",
            WriterError::NotImplemented => "error.not_implemented",
            WriterError::RefuseToDeleteWorkspaceRoot => "error.refuse_delete_workspace_root",
            WriterError::InvalidDeleteTarget(_) => "error.invalid_delete_target",
            WriterError::SyncConflict(_) => "error.sync_conflict",
            WriterError::SyncFailed(_) => "error.sync_failed",
            WriterError::Other(_) => "error.other",
        }
    }

    /// 结构化错误参数，供 UI 层做本地化模板插值。
    ///
    /// UI 根据 `message_key` + `params` 生成用户提示，不依赖硬编码中文文案。
    pub fn params(&self) -> HashMap<String, String> {
        let mut m = HashMap::new();
        match self {
            WriterError::EmptyOverwriteBlocked {
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
            WriterError::InvalidDeleteTarget(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::SyncConflict(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::SyncFailed(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::Io(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::Json(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::Other(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            _ => {}
        }
        m
    }
}

impl From<crate::error::Error> for WriterError {
    fn from(e: crate::error::Error) -> Self {
        match e {
            Error::Io(e) => WriterError::Io(e.to_string()),
            Error::Json(e) => WriterError::Json(e.to_string()),
            Error::InvalidWorkspace => WriterError::InvalidWorkspace,
            Error::ProjectNotFound => WriterError::ProjectNotFound,
            Error::VolumeNotFound => WriterError::VolumeNotFound,
            Error::ChapterNotFound => WriterError::ChapterNotFound,
            Error::EmptyOverwriteBlocked {
                chapter_id,
                old_len,
                new_len,
                reason,
            } => WriterError::EmptyOverwriteBlocked {
                chapter_id,
                old_len: old_len as u32,
                new_len: new_len as u32,
                reason,
            },
            Error::NotImplemented => WriterError::NotImplemented,
            Error::RefuseToDeleteWorkspaceRoot => WriterError::RefuseToDeleteWorkspaceRoot,
            Error::InvalidDeleteTarget(s) => WriterError::InvalidDeleteTarget(s),
            Error::SyncAuthFailed { reason } => WriterError::SyncFailed(reason),
            Error::SyncNetworkUnavailable { reason } => WriterError::SyncFailed(reason),
            Error::SyncRateLimited { retry_after_secs } => {
                WriterError::SyncFailed(format!("rate_limited: retry_after={}", retry_after_secs))
            }
            Error::SyncDocumentConflict {
                path,
                local_hash,
                remote_hash,
            } => WriterError::SyncConflict(format!(
                "path={} local={} remote={}",
                path, local_hash, remote_hash
            )),
            Error::SyncIncompleteTransaction {
                transaction_id,
                missing_files,
            } => WriterError::Other(format!(
                "incomplete_transaction: tx_id={} missing={}",
                transaction_id,
                missing_files.join(",")
            )),
            Error::DiskFull {
                path,
                required_bytes,
            } => WriterError::Io(format!(
                "disk_full: path={} required={}",
                path, required_bytes
            )),
            Error::StorageTransactionIncomplete { transaction_id } => WriterError::Other(format!(
                "storage_transaction_incomplete: tx_id={}",
                transaction_id
            )),
            Error::Other(s) => WriterError::Other(s),
        }
    }
}

impl WriterError {
    /// Create a sync conflict error with conflict details.
    pub fn sync_conflict(detail: String) -> Self {
        WriterError::SyncConflict(detail)
    }

    /// Create a sync failed error with error message and optional error category.
    pub fn sync_failed(detail: String) -> Self {
        WriterError::SyncFailed(detail)
    }
}

impl From<serde_json::Error> for WriterError {
    fn from(e: serde_json::Error) -> Self {
        WriterError::Json(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_params_empty_overwrite_blocked() {
        let err = WriterError::EmptyOverwriteBlocked {
            chapter_id: "ch1".into(),
            old_len: 100,
            new_len: 0,
            reason: "empty content".into(),
        };
        let p = err.params();
        assert_eq!(p.get("chapter_id").unwrap(), "ch1");
        assert_eq!(p.get("old_len").unwrap(), "100");
        assert_eq!(p.get("new_len").unwrap(), "0");
        assert_eq!(p.get("reason").unwrap(), "empty content");
    }

    #[test]
    fn test_params_sync_conflict() {
        let err = WriterError::SyncConflict("path conflict".into());
        let p = err.params();
        assert_eq!(p.get("detail").unwrap(), "path conflict");
    }

    #[test]
    fn test_params_sync_failed() {
        let err = WriterError::SyncFailed("network timeout".into());
        let p = err.params();
        assert_eq!(p.get("detail").unwrap(), "network timeout");
    }
}
