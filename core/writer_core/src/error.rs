//! # 统一错误类型
//!
//! 所有 Core 模块共享此错误类型。
//! 客户端通过 `Result<T>` 统一处理错误，不允许吞掉 Core 错误。

use serde::Serialize;
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

impl Error {
    /// 跨端 Bridge 使用的稳定错误码。
    ///
    /// UI 可以根据 code 做展示分支，message 只用于用户提示或调试。
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
            Error::Other(_) => "OTHER",
        }
    }
}

/// 跨端 Bridge 错误 DTO，供 JNI/Qt 层稳定序列化。
#[derive(Serialize, Debug, Clone)]
pub struct BridgeError {
    pub code: String,
    pub message: String,
}

impl From<&Error> for BridgeError {
    fn from(error: &Error) -> Self {
        Self {
            code: error.code().to_string(),
            message: error.to_string(),
        }
    }
}

/// 兼容旧端的 Bridge Result 包装。
///
/// 新代码应优先使用领域 Bridge DTO；旧 JSON 调用保留该统一包装。
#[derive(Serialize, Debug, Clone)]
pub struct BridgeResult<T: Serialize> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T: Serialize> BridgeResult<T> {
    pub fn from_result(result: Result<T>) -> Self {
        match result {
            Ok(data) => Self {
                success: true,
                data: Some(data),
                code: None,
                error: None,
            },
            Err(error) => Self {
                success: false,
                data: None,
                code: Some(error.code().to_string()),
                error: Some(error.to_string()),
            },
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
