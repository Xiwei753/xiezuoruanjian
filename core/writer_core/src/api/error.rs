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
            Error::DiskFull { path, required_bytes } => WriterError::Io(format!(
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
