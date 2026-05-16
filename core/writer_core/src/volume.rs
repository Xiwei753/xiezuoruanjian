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
    let volume_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes")
        .join(volume_id);
    if volume_dir.exists() {
        fs::remove_dir_all(volume_dir)?;
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
