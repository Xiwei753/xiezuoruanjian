//! # 同步引擎接口模块
//!
//! 本模块定义了同步引擎的核心抽象接口，包括 Git 操作和版本历史管理功能。
//!
//! ## 主要功能
//!
//! - **Git 同步操作**: 通过 `SyncEngine` trait 提供仓库初始化、远程设置、提交、推送、拉取等操作
//! - **版本历史管理**: 通过 `TimeMachine` trait 提供历史版本查看、恢复和预览功能
//! - **错误处理**: 定义了同步操作专用的错误类型 `SyncError`
//!
//! ## 核心结构
//!
//! - `SyncEngine`: 同步引擎 trait，定义了 Git 操作的标准接口
//! - `TimeMachine`: 时间机器 trait，定义了版本历史管理接口
//! - `CommitRecord`: 提交记录，包含哈希、消息、时间戳和作者信息
//! - `SyncError`: 同步错误枚举，涵盖 Git 错误、认证错误、冲突等场景
//!
//! ## 依赖关系
//!
//! - `chrono`: 日期时间处理，用于提交时间戳
//! - `serde`: 序列化/反序列化支持
//! - `thiserror`: 错误类型派生宏
//!
//! ## 使用场景
//!
//! - Git 仓库的初始化和配置
//! - 文件变更的自动提交和同步
//! - 历史版本的浏览和恢复
//! - 多设备间的代码同步

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("Git operation failed: {0}")]
    GitError(String),
    #[error("Authentication failed or missing PAT")]
    AuthError,
    #[error("Merge conflict detected")]
    Conflict,
    #[error("Feature not yet implemented")]
    NotImplemented,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitRecord {
    pub hash: String,
    pub message: String,
    pub timestamp: DateTime<Utc>,
    pub author_name: String,
}

pub trait SyncEngine {
    fn init_repo(&self, workspace_path: &str) -> Result<(), SyncError>;
    fn set_remote(&self, url: &str, pat: &str) -> Result<(), SyncError>;
    fn auto_commit(&self, message: &str) -> Result<String, SyncError>;
    fn push(&self) -> Result<(), SyncError>;
    fn pull(&self) -> Result<(), SyncError>;
}

pub trait TimeMachine {
    fn get_history(&self, target_path: Option<&str>) -> Result<Vec<CommitRecord>, SyncError>;
    fn restore_version(&self, target_path: &str, commit_hash: &str) -> Result<(), SyncError>;
    fn peek_version(&self, target_path: &str, commit_hash: &str) -> Result<String, SyncError>;
}
