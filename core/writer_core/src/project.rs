//! # 项目管理（Core 层）
//!
//! 负责作品（Project）的 CRUD、统计、排序、重命名、删除。
//!
//! ## 职责边界
//!
//! - **做**：项目创建/列表/重命名/删除/排序/统计
//! - **不做**：卷和章节管理（由 `volume.rs` / `chapter.rs` 负责）
//! - **删除安全**：所有删除操作经过 `delete_guard` 验证，删除后移入 trash 目录并记录 tombstone
//!
//! ## 目录结构
//!
//! ```text
//! projects/
//!   {project_id}/
//!     project.json          # 项目元数据（id、title、order、时间戳）
//!     volumes/              # 所有卷
//!     characters/           # 角色数据（预留）
//! ```

use crate::error::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

/// 项目元数据结构体。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Project {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub order: i32,
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
    projects.sort_by_key(|p| p.order);
    Ok(projects)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProjectStats {
    pub total_word_count: u32,
    pub volume_count: u32,
    pub chapter_count: u32,
}

pub fn get_project_stats(workspace_path: &Path, project_id: &str) -> Result<ProjectStats> {
    let mut stats = ProjectStats {
        total_word_count: 0,
        volume_count: 0,
        chapter_count: 0,
    };

    let volumes = crate::volume::list_volumes(workspace_path, project_id)?;
    stats.volume_count = volumes.len() as u32;

    for volume in volumes {
        let chapters = crate::chapter::list_chapters(workspace_path, project_id, &volume.id)?;
        stats.chapter_count += chapters.len() as u32;
        for chapter in chapters {
            stats.total_word_count += chapter.word_count;
        }
    }

    Ok(stats)
}

pub fn create_project(workspace_path: &Path, title: &str) -> Result<Project> {
    let projects = list_projects(workspace_path)?;
    let order = projects
        .iter()
        .map(|p| p.order)
        .max()
        .map(|m| m + 1)
        .unwrap_or(0);

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let project = Project {
        id: id.clone(),
        title: title.to_string(),
        created_at: now.clone(),
        updated_at: now,
        order,
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

pub fn rename_project(workspace_path: &Path, project_id: &str, new_title: &str) -> Result<()> {
    let project_dir = workspace_path.join("projects").join(project_id);
    let meta_path = project_dir.join("project.json");

    if !meta_path.exists() {
        return Err(crate::error::Error::ProjectNotFound);
    }

    let meta_str = fs::read_to_string(&meta_path)?;
    let mut meta: Project = serde_json::from_str(&meta_str)?;

    meta.title = new_title.to_string();
    meta.updated_at = Utc::now().to_rfc3339();

    let updated_meta_str = serde_json::to_string_pretty(&meta)?;
    crate::storage::atomic_write_string(&meta_path, &updated_meta_str)?;

    Ok(())
}

pub fn delete_project(workspace_path: &Path, project_id: &str) -> Result<()> {
    let project_id = crate::delete_guard::validate_id_segment(project_id)?;
    let project_dir = workspace_path.join("projects").join(project_id);
    let target_canon =
        crate::delete_guard::validate_delete_target(workspace_path, &project_dir, "project.json")?;

    let trash_dir = workspace_path.join("app-meta/sync/trash");
    let _ = fs::create_dir_all(&trash_dir);
    let trash_path = trash_dir.join(format!(
        "{}_{}_{}",
        chrono::Utc::now().timestamp_millis(),
        uuid::Uuid::new_v4(),
        project_id
    ));
    fs::rename(&target_canon, &trash_path)?;

    // Also update tombstone
    if let Ok(mut state) = crate::sync_service::SyncService::load_sync_state(workspace_path) {
        let rel_project_dir = project_dir
            .strip_prefix(workspace_path)
            .unwrap_or(&project_dir)
            .to_string_lossy()
            .replace("\\", "/");
        let rel_trash_path = trash_path
            .strip_prefix(workspace_path)
            .unwrap_or(&trash_path)
            .to_string_lossy()
            .replace("\\", "/");

        for entry in walkdir::WalkDir::new(&trash_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let rel_file_path = entry
                .path()
                .strip_prefix(&trash_path)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace("\\", "/");
            let original_file_path = format!("{}/{}", rel_project_dir, rel_file_path);
            let new_trash_path = format!("{}/{}", rel_trash_path, rel_file_path);

            let tombstone = crate::sync_service::Tombstone {
                original_path: original_file_path.clone(),
                trash_path: new_trash_path,
                deleted_at: chrono::Utc::now().timestamp(),
                purge_after: chrono::Utc::now().timestamp() + 30 * 24 * 3600,
                deleted_by: "local".to_string(),
                original_hash: state
                    .known_files
                    .get(&original_file_path)
                    .cloned()
                    .unwrap_or_default(),
                kind: "local_delete".to_string(),
            };
            state.tombstones.push(tombstone);
        }
        let _ = crate::sync_service::SyncService::save_sync_state(workspace_path, &state);
    }
    Ok(())
}

pub fn reorder_projects(workspace_path: &Path, ordered_ids: &[String]) -> Result<()> {
    let projects = list_projects(workspace_path)?;
    let existing_ids: std::collections::HashSet<_> =
        projects.iter().map(|p| p.id.clone()).collect();
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
        let project_dir = workspace_path.join("projects").join(id);
        let meta_path = project_dir.join("project.json");

        if meta_path.exists() {
            let meta_str = std::fs::read_to_string(&meta_path)?;
            let mut meta = serde_json::from_str::<Project>(&meta_str)?;
            meta.order = index as i32;
            meta.updated_at = Utc::now().to_rfc3339();
            let updated_meta_str = serde_json::to_string_pretty(&meta)?;
            crate::storage::atomic_write_string(&meta_path, &updated_meta_str)?;
        } else {
            return Err(crate::error::Error::ProjectNotFound);
        }
    }
    Ok(())
}
