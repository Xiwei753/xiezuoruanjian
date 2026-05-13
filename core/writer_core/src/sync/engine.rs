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
