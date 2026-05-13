use crate::error::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Project {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

pub fn list_projects(workspace_path: &Path) -> Result<Vec<Project>> {
    let projects_dir = workspace_path.join("projects");
    if !projects_dir.exists() {
        return Ok(Vec::new());
    }

    let mut projects = Vec::new();
    for entry in fs::read_dir(projects_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let meta_path = path.join("project.json");
            if meta_path.exists() {
                let content = fs::read_to_string(&meta_path)?;
                if let Ok(project) = serde_json::from_str::<Project>(&content) {
                    projects.push(project);
                }
            }
        }
    }
    Ok(projects)
}

pub fn create_project(workspace_path: &Path, title: &str) -> Result<Project> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let project = Project {
        id: id.clone(),
        title: title.to_string(),
        created_at: now.clone(),
        updated_at: now,
    };

    let project_dir = workspace_path.join("projects").join(&id);
    fs::create_dir_all(&project_dir)?;
    fs::create_dir_all(project_dir.join("volumes"))?;
    fs::create_dir_all(project_dir.join("characters"))?;

    let meta_path = project_dir.join("project.json");
    let content = serde_json::to_string_pretty(&project)?;
    crate::storage::atomic_write_string(&meta_path, &content)?;

    // Create a default volume to maintain consistency with product requirements
    let _ = crate::volume::create_volume(workspace_path, &id, "第一卷")?;

    Ok(project)
}
