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

    /// 已废弃：UI 应使用 `code()` + `message_key()` 做本地化映射，不再直接展示此中文文案。
    /// 保留仅作为 fallback/debug 用途。
    #[deprecated(note = "Use error_category for i18n lookup")]
    #[allow(deprecated)]
    pub fn user_message(&self) -> &'static str {
        match self {
            WriterError::Io(_) => "文件读写失败，请检查工作区权限和磁盘状态",
            WriterError::Json(_) => "数据文件格式异常，请检查工作区文件是否损坏",
            WriterError::InvalidWorkspace => "不是有效的工作区",
            WriterError::ProjectNotFound => "作品不存在或已被删除",
            WriterError::VolumeNotFound => "卷不存在或已被删除",
            WriterError::ChapterNotFound => "章节不存在或已被删除",
            WriterError::EmptyOverwriteBlocked { .. } => "已阻止空内容覆盖现有章节",
            WriterError::NotImplemented => "该功能尚未实现",
            WriterError::RefuseToDeleteWorkspaceRoot => "拒绝删除工作区根目录",
            WriterError::InvalidDeleteTarget(_) => "删除目标无效",
            WriterError::SyncConflict(_) => "同步冲突，请手动处理冲突文件后重试",
            WriterError::SyncFailed(_) => "同步失败，请检查网络和配置",
            WriterError::Other(_) => "操作失败",
        }
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
    fn test_message_key_stable() {
        assert_eq!(WriterError::Io("test".into()).message_key(), "error.io");
        assert_eq!(WriterError::Json("test".into()).message_key(), "error.json");
        assert_eq!(WriterError::InvalidWorkspace.message_key(), "error.invalid_workspace");
        assert_eq!(WriterError::ProjectNotFound.message_key(), "error.project_not_found");
        assert_eq!(WriterError::VolumeNotFound.message_key(), "error.volume_not_found");
        assert_eq!(WriterError::ChapterNotFound.message_key(), "error.chapter_not_found");
        assert_eq!(
            WriterError::EmptyOverwriteBlocked {
                chapter_id: "ch1".into(),
                old_len: 100,
                new_len: 0,
                reason: "empty".into(),
            }
            .message_key(),
            "error.empty_overwrite_blocked"
        );
        assert_eq!(WriterError::NotImplemented.message_key(), "error.not_implemented");
        assert_eq!(
            WriterError::RefuseToDeleteWorkspaceRoot.message_key(),
            "error.refuse_delete_workspace_root"
        );
        assert_eq!(
            WriterError::InvalidDeleteTarget("test".into()).message_key(),
            "error.invalid_delete_target"
        );
        assert_eq!(
            WriterError::SyncConflict("detail".into()).message_key(),
            "error.sync_conflict"
        );
        assert_eq!(
            WriterError::SyncFailed("detail".into()).message_key(),
            "error.sync_failed"
        );
        assert_eq!(WriterError::Other("test".into()).message_key(), "error.other");
    }

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

    #[test]
    fn test_params_simple_variants_empty() {
        // 不携带结构化参数的变体应返回空 HashMap
        assert!(WriterError::InvalidWorkspace.params().is_empty());
        assert!(WriterError::ProjectNotFound.params().is_empty());
        assert!(WriterError::VolumeNotFound.params().is_empty());
        assert!(WriterError::ChapterNotFound.params().is_empty());
        assert!(WriterError::NotImplemented.params().is_empty());
        assert!(WriterError::RefuseToDeleteWorkspaceRoot.params().is_empty());
    }

    #[test]
    #[allow(deprecated)]
    fn test_user_message_still_works() {
        // user_message 虽然标记 deprecated，但功能不变
        assert!(!WriterError::ProjectNotFound.user_message().is_empty());
    }
}
