use crate::error::Result;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
pub fn create_workspace(path: &Path) -> Result<()> {
    fs::create_dir_all(path.join("app-meta/settings"))?;
    fs::create_dir_all(path.join("app-meta/logs"))?;
    fs::create_dir_all(path.join("projects"))?;
    fs::create_dir_all(path.join("backups"))?;
    fs::create_dir_all(path.join("trash"))?;
    fs::create_dir_all(path.join("sqlite_cache"))?;

    let manifest_path = path.join("workspace_manifest.json");
    if !manifest_path.exists() {
        crate::storage::atomic_write_string(&manifest_path, r#"{"version": 1}"#)?;
    }
    Ok(())
}

pub fn validate_workspace(path: &Path) -> Result<bool> {
    if path.join("workspace_manifest.json").exists() && path.join("projects").exists() {
        Ok(true)
    } else {
        Ok(false)
    }
}

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RecentEdit {
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub timestamp: String,
}

use std::collections::HashMap;
use std::path::PathBuf;
static RECENT_EDITS_CACHE: OnceLock<Mutex<HashMap<PathBuf, Vec<RecentEdit>>>> = OnceLock::new();

pub fn get_recent_edits(workspace_path: &Path) -> Result<Vec<RecentEdit>> {
    let mutex = RECENT_EDITS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = mutex.lock().unwrap();

    if let Some(edits) = cache.get(workspace_path) {
        return Ok(edits.to_vec());
    }

    let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");
    if !recent_path.exists() {
        cache.insert(workspace_path.to_path_buf(), Vec::new());
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&recent_path)?;
    let edits: Vec<RecentEdit> = serde_json::from_str(&content).unwrap_or_default();

    cache.insert(workspace_path.to_path_buf(), edits.clone());
    Ok(edits)
}

pub fn record_recent_edit(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
) -> Result<()> {
    let mutex = RECENT_EDITS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = mutex.lock().unwrap();

    let mut edits = if let Some(e) = cache.get(workspace_path) {
        e.to_vec()
    } else {
        let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");
        if recent_path.exists() {
            let content = fs::read_to_string(&recent_path).unwrap_or_else(|_| "[]".to_string());
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        }
    };

    // Remove existing entry for same chapter if exists
    edits.retain(|e| e.chapter_id != chapter_id);

    // Add new entry at the beginning
    edits.insert(
        0,
        RecentEdit {
            project_id: project_id.to_string(),
            volume_id: volume_id.to_string(),
            chapter_id: chapter_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        },
    );

    // Keep only top 20
    edits.truncate(20);

    cache.insert(workspace_path.to_path_buf(), edits.to_vec());

    // Basic Debounce: Only flush to disk at most once every 5 seconds to reduce I/O.
    static LAST_FLUSH: OnceLock<Mutex<std::time::Instant>> = OnceLock::new();
    let flush_mutex = LAST_FLUSH.get_or_init(|| Mutex::new(std::time::Instant::now() - std::time::Duration::from_secs(10)));
    let mut last_flush = flush_mutex.lock().unwrap();

    if last_flush.elapsed().as_secs() >= 5 {
        let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");
        let content = serde_json::to_string_pretty(&edits)?;
        crate::storage::atomic_write_string(&recent_path, &content)?;
        *last_flush = std::time::Instant::now();
    }

    Ok(())
}
