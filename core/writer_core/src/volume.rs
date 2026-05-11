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
    Ok(volumes)
}

pub fn create_volume(workspace_path: &Path, project_id: &str, title: &str) -> Result<Volume> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let volume = Volume {
        id: id.clone(),
        title: title.to_string(),
        created_at: now.clone(),
        updated_at: now,
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
