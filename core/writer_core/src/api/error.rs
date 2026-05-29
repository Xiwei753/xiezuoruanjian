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
    #[error("Other error: {0}")]
    Other(String),
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
            Error::Other(s) => WriterError::Other(s),
        }
    }
}

impl From<serde_json::Error> for WriterError {
    fn from(e: serde_json::Error) -> Self {
        WriterError::Json(e.to_string())
    }
}
