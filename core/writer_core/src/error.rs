//! # 统一错误类型
//!
//! 所有 Core 模块共享此错误类型。
//! 客户端通过 `Result<T>` 统一处理错误，不允许吞掉 Core 错误。

use thiserror::Error;

/// Core 层统一错误枚举。
///
/// 所有错误变体都携带足够上下文，便于客户端决定如何展示给用户。
/// `EmptyOverwriteBlocked` 是核心安全机制：防止空内容覆盖非空章节。
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
    #[error("Other error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
