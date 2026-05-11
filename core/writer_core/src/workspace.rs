use crate::error::Result;
use std::fs;
use std::path::Path;

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
