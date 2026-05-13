use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SyncTransport {
    HttpsToken,
    SshDeployKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    pub enabled: bool,
    pub remote_url: String,
    pub transport: SyncTransport,
    pub token: Option<String>,
    pub auto_sync: bool,
    pub sync_interval_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Idle,
    Syncing,
    Error(String),
    Conflict,
    Success,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflict {
    pub file_path: String,
    pub local_hash: String,
    pub remote_hash: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPlan {
    pub files_to_upload: Vec<String>,
    pub files_to_download: Vec<String>,
    pub files_to_delete_local: Vec<String>,
    pub files_to_delete_remote: Vec<String>,
    pub ignored_files: Vec<String>,
}

impl Default for SyncPlan {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncPlan {
    pub fn new() -> Self {
        Self {
            files_to_upload: Vec::new(),
            files_to_download: Vec::new(),
            files_to_delete_local: Vec::new(),
            files_to_delete_remote: Vec::new(),
            ignored_files: Vec::new(),
        }
    }
}

pub struct SyncService {
    pub config: Option<SyncConfig>,
    pub status: SyncStatus,
}

impl SyncService {
    pub fn new() -> Self {
        Self {
            config: None,
            status: SyncStatus::Idle,
        }
    }

    pub fn build_sync_plan(&self, mock_changed_files: Vec<&str>) -> crate::Result<SyncPlan> {
        let mut plan = SyncPlan::new();

        let ignored_patterns = [
            "local_settings.json",
            "settings.local.json",
            "sqlite_cache",
            "logs",
            "tmp",
            "backups",
            "trash",
        ];

        for file in mock_changed_files {
            let is_ignored = ignored_patterns
                .iter()
                .any(|pattern| file.contains(pattern));

            if is_ignored {
                plan.ignored_files.push(file.to_string());
            } else {
                plan.files_to_upload.push(file.to_string());
            }
        }

        Ok(plan)
    }

    pub fn sync(&self) -> crate::Result<()> {
        Err(crate::Error::NotImplemented)
    }
}

impl Default for SyncService {
    fn default() -> Self {
        Self::new()
    }
}
