use crate::error::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Volume {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub order: i32,
}

pub fn list_volumes(workspace_path: &Path, project_id: &str) -> Result<Vec<Volume>> {
    let volumes_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes");
    if !volumes_dir.exists() {
        return Ok(Vec::new());
    }

    let mut volumes = Vec::new();
    for entry in fs::read_dir(volumes_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let meta_path = path.join("volume.json");
            if meta_path.exists() {
                let content = fs::read_to_string(&meta_path)?;
                if let Ok(volume) = serde_json::from_str::<Volume>(&content) {
                    volumes.push(volume);
                }
            }
        }
    }
    volumes.sort_by_key(|v| v.order);
    Ok(volumes)
}

pub fn create_volume(workspace_path: &Path, project_id: &str, title: &str) -> Result<Volume> {
    let volumes = list_volumes(workspace_path, project_id)?;
    let order = volumes
        .iter()
        .map(|v| v.order)
        .max()
        .map(|m| m + 1)
        .unwrap_or(0);

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let volume = Volume {
        id: id.clone(),
        title: title.to_string(),
        created_at: now.clone(),
        updated_at: now,
        order,
    };

    let volume_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes")
        .join(&id);
    fs::create_dir_all(&volume_dir)?;
    fs::create_dir_all(volume_dir.join("chapters"))?;

    let meta_path = volume_dir.join("volume.json");
    let content = serde_json::to_string_pretty(&volume)?;
    crate::storage::atomic_write_string(&meta_path, &content)?;

    Ok(volume)
}

pub fn rename_volume(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    new_title: &str,
) -> Result<()> {
    let volume_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes")
        .join(volume_id);
    let meta_path = volume_dir.join("volume.json");

    if !meta_path.exists() {
        return Err(crate::error::Error::VolumeNotFound);
    }

    let meta_str = fs::read_to_string(&meta_path)?;
    let mut meta: Volume = serde_json::from_str(&meta_str)?;

    meta.title = new_title.to_string();
    meta.updated_at = Utc::now().to_rfc3339();

    let updated_meta_str = serde_json::to_string_pretty(&meta)?;
    crate::storage::atomic_write_string(&meta_path, &updated_meta_str)?;

    Ok(())
}

pub fn delete_volume(workspace_path: &Path, project_id: &str, volume_id: &str) -> Result<()> {
    let project_id = project_id.trim();
    if project_id.is_empty() || project_id.contains("..") || project_id.contains("/") || project_id.contains("\\") {
        return Err(crate::error::Error::Other(format!("Invalid parameter: {}", project_id)));
    }

    let volume_id = volume_id.trim();
    if volume_id.is_empty() || volume_id.contains("..") || volume_id.contains("/") || volume_id.contains("\\") {
        return Err(crate::error::Error::Other(format!("Invalid parameter: {}", volume_id)));
    }

    let volume_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes")
        .join(volume_id);
    if volume_dir.exists() {
        let trash_dir = workspace_path.join("app-meta/sync/trash");
        let _ = fs::create_dir_all(&trash_dir);
        let trash_path = trash_dir.join(format!("{}_{}_{}", chrono::Utc::now().timestamp_millis(), uuid::Uuid::new_v4(), volume_id));
        fs::rename(&volume_dir, &trash_path)?;

        // Also update tombstone
        if let Ok(mut state) = crate::sync_service::SyncService::load_sync_state(workspace_path) {
             let rel_volume_dir = volume_dir.strip_prefix(workspace_path).unwrap_or(&volume_dir).to_string_lossy().replace("\\", "/");
             let rel_trash_path = trash_path.strip_prefix(workspace_path).unwrap_or(&trash_path).to_string_lossy().replace("\\", "/");

             for entry in walkdir::WalkDir::new(&trash_path).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
                 let rel_file_path = entry.path().strip_prefix(&trash_path).unwrap_or(entry.path()).to_string_lossy().replace("\\", "/");
                 let original_file_path = format!("{}/{}", rel_volume_dir, rel_file_path);
                 let new_trash_path = format!("{}/{}", rel_trash_path, rel_file_path);

                 let tombstone = crate::sync_service::Tombstone {
                     original_path: original_file_path.clone(),
                     trash_path: new_trash_path,
                     deleted_at: chrono::Utc::now().timestamp(),
                     purge_after: chrono::Utc::now().timestamp() + 30 * 24 * 3600,
                     deleted_by: "local".to_string(),
                     original_hash: state.known_files.get(&original_file_path).cloned().unwrap_or_default(),
                     kind: "local_delete".to_string(),
                 };
                 state.tombstones.push(tombstone);
             }
             let _ = crate::sync_service::SyncService::save_sync_state(workspace_path, &state);
        }
    } else {
        return Err(crate::error::Error::VolumeNotFound);
    }
    Ok(())
}

pub fn reorder_volumes(
    workspace_path: &Path,
    project_id: &str,
    ordered_ids: &[String],
) -> Result<()> {
    let volumes = list_volumes(workspace_path, project_id)?;
    let existing_ids: std::collections::HashSet<_> = volumes.iter().map(|v| v.id.clone()).collect();
    let new_ids: std::collections::HashSet<_> = ordered_ids.iter().cloned().collect();

    if existing_ids.len() != new_ids.len()
        || existing_ids != new_ids
        || ordered_ids.len() != new_ids.len()
    {
        return Err(crate::error::Error::Other(
            "Invalid ordered_ids for reorder".to_string(),
        ));
    }

    for (index, id) in ordered_ids.iter().enumerate() {
        let volume_dir = workspace_path
            .join("projects")
            .join(project_id)
            .join("volumes")
            .join(id);
        let meta_path = volume_dir.join("volume.json");

        if meta_path.exists() {
            let meta_str = fs::read_to_string(&meta_path)?;
            let mut meta = serde_json::from_str::<Volume>(&meta_str)?;
            meta.order = index as i32;
            meta.updated_at = Utc::now().to_rfc3339();
            let updated_meta_str = serde_json::to_string_pretty(&meta)?;
            crate::storage::atomic_write_string(&meta_path, &updated_meta_str)?;
        } else {
            return Err(crate::error::Error::VolumeNotFound);
        }
    }
    Ok(())
}
