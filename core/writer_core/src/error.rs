use thiserror::Error;

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
    #[error("Other error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
