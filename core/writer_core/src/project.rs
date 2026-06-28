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

#[derive(Deserialize)]
struct ChapterWordCount {
    #[serde(default)]
    word_count: u32,
}

pub fn get_project_stats(workspace_path: &Path, project_id: &str) -> Result<ProjectStats> {
    let mut stats = ProjectStats {
        total_word_count: 0,
        volume_count: 0,
        chapter_count: 0,
    };

    let volumes_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes");

    if !volumes_dir.exists() {
        return Ok(stats);
    }

    for vol_entry in fs::read_dir(&volumes_dir)? {
        let vol_entry = vol_entry?;

        if !vol_entry.file_type()?.is_dir() {
            continue;
        }

        let vol_path = vol_entry.path();
        if vol_path.join("volume.json").exists() {
            stats.volume_count += 1;

            let chapters_dir = vol_path.join("chapters");
            if let Ok(chap_iter) = fs::read_dir(&chapters_dir) {
                for chap_entry in chap_iter {
                    let chap_entry = chap_entry?;

                    if !chap_entry.file_type()?.is_dir() {
                        continue;
                    }

                    let meta_path = chap_entry.path().join("chapter.meta.json");
                    if let Ok(content) = fs::read(&meta_path) {
                        stats.chapter_count += 1;
                        if let Ok(meta) = serde_json::from_slice::<ChapterWordCount>(&content) {
                            stats.total_word_count += meta.word_count;
                        }
                    }
                }
            }
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
    let projects = list_projects(workspace_path)?;
    if projects
        .iter()
        .any(|p| p.title == new_title && p.id != project_id)
    {
        return Err(crate::error::Error::Other(
            "Project title already exists".to_string(),
        ));
    }

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
    if let Ok(mut state) = crate::sync::SyncService::load_sync_state(workspace_path) {
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

        crate::trash::generate_tombstones(
            &mut state,
            &trash_path,
            &rel_project_dir,
            &rel_trash_path,
        );
        let _ = crate::sync::SyncService::save_sync_state(workspace_path, &state);
    }
    Ok(())
}

/// 从子章节聚合获取 volume 的最近更新时间。
///
/// 遍历该 volume 下所有 chapter.meta.json，取最大的 updated_at。
/// 如果没有子章节，返回 volume.json 的 created_at 作为 fallback。
pub fn get_volume_updated_at_aggregated(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
) -> Result<String> {
    let chapters = crate::chapter::list_chapters(workspace_path, project_id, volume_id)?;

    if let Some(max_updated) = chapters.iter().map(|c| c.updated_at.as_str()).max() {
        return Ok(max_updated.to_string());
    }

    // Fallback: no chapters, use volume.json created_at
    let volume_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes")
        .join(volume_id);
    let meta_path = volume_dir.join("volume.json");
    if meta_path.exists() {
        let raw = fs::read_to_string(&meta_path)?;
        if let Ok(vol) = serde_json::from_str::<crate::volume::Volume>(&raw) {
            return Ok(vol.created_at);
        }
    }

    Ok(Utc::now().to_rfc3339())
}

/// 从子章节聚合获取 project 的最近更新时间。
///
/// 遍历该 project 下所有 volume 下所有 chapter.meta.json，取最大的 updated_at。
/// 如果没有子章节，返回 project.json 的 created_at 作为 fallback。
pub fn get_project_updated_at_aggregated(
    workspace_path: &Path,
    project_id: &str,
) -> Result<String> {
    let volumes = crate::volume::list_volumes(workspace_path, project_id)?;

    let mut max_updated: Option<String> = None;

    for vol in &volumes {
        let chapters = crate::chapter::list_chapters(workspace_path, project_id, &vol.id)?;
        for ch in &chapters {
            match &max_updated {
                Some(current) if ch.updated_at.as_str() > current.as_str() => {
                    max_updated = Some(ch.updated_at.clone());
                }
                None => {
                    max_updated = Some(ch.updated_at.clone());
                }
                _ => {}
            }
        }
    }

    if let Some(updated) = max_updated {
        return Ok(updated);
    }

    // Fallback: no chapters in any volume, use project.json created_at
    let project_dir = workspace_path.join("projects").join(project_id);
    let meta_path = project_dir.join("project.json");
    if meta_path.exists() {
        let raw = fs::read_to_string(&meta_path)?;
        if let Ok(proj) = serde_json::from_str::<Project>(&raw) {
            return Ok(proj.created_at);
        }
    }

    Ok(Utc::now().to_rfc3339())
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// 验证聚合查询：有子章节时返回最大的 chapter updated_at。
    #[test]
    fn test_aggregated_volume_updated_at_with_chapters() {
        let temp_dir = tempdir().unwrap();
        let workspace_path = temp_dir.path();

        crate::workspace::create_workspace(workspace_path).unwrap();
        let project = crate::project::create_project(workspace_path, "TestProject").unwrap();
        let volumes = crate::volume::list_volumes(workspace_path, &project.id).unwrap();
        let volume = &volumes[0];

        // Create two chapters
        let ch1 =
            crate::chapter::create_chapter(workspace_path, &project.id, &volume.id, "Ch1").unwrap();
        let _ch2 =
            crate::chapter::create_chapter(workspace_path, &project.id, &volume.id, "Ch2").unwrap();

        // Save ch1 with content (updates its updated_at)
        crate::chapter::save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &ch1.id,
            "Some content",
        )
        .unwrap();

        // The aggregated updated_at should be the max of all chapter updated_at values
        let aggregated =
            get_volume_updated_at_aggregated(workspace_path, &project.id, &volume.id).unwrap();
        assert!(
            !aggregated.is_empty(),
            "aggregated updated_at should not be empty"
        );

        // Verify it matches the latest chapter's updated_at
        let chapters =
            crate::chapter::list_chapters(workspace_path, &project.id, &volume.id).unwrap();
        let max_updated = chapters
            .iter()
            .map(|c| c.updated_at.as_str())
            .max()
            .unwrap();
        assert_eq!(aggregated, max_updated);
    }

    /// 验证聚合查询：无子章节时 fallback 到 volume.json 的 created_at。
    #[test]
    fn test_aggregated_volume_updated_at_no_chapters() {
        let temp_dir = tempdir().unwrap();
        let workspace_path = temp_dir.path();

        crate::workspace::create_workspace(workspace_path).unwrap();
        let project = crate::project::create_project(workspace_path, "TestProject").unwrap();
        let volumes = crate::volume::list_volumes(workspace_path, &project.id).unwrap();
        let volume = &volumes[0];

        // No chapters created — should fallback to volume's created_at
        let aggregated =
            get_volume_updated_at_aggregated(workspace_path, &project.id, &volume.id).unwrap();
        assert_eq!(aggregated, volume.created_at);
    }

    /// 验证聚合查询：project 级别，有子章节时返回最大的 chapter updated_at。
    #[test]
    fn test_aggregated_project_updated_at_with_chapters() {
        let temp_dir = tempdir().unwrap();
        let workspace_path = temp_dir.path();

        crate::workspace::create_workspace(workspace_path).unwrap();
        let project = crate::project::create_project(workspace_path, "TestProject").unwrap();
        let volumes = crate::volume::list_volumes(workspace_path, &project.id).unwrap();
        let volume = &volumes[0];

        let ch1 =
            crate::chapter::create_chapter(workspace_path, &project.id, &volume.id, "Ch1").unwrap();
        crate::chapter::save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &ch1.id,
            "Project level content",
        )
        .unwrap();

        let aggregated = get_project_updated_at_aggregated(workspace_path, &project.id).unwrap();
        assert!(!aggregated.is_empty());

        // Verify it matches the latest chapter's updated_at across all volumes
        let chapters =
            crate::chapter::list_chapters(workspace_path, &project.id, &volume.id).unwrap();
        let max_updated = chapters
            .iter()
            .map(|c| c.updated_at.as_str())
            .max()
            .unwrap();
        assert_eq!(aggregated, max_updated);
    }

    /// 验证聚合查询：project 级别，无子章节时 fallback 到 project.json 的 created_at。
    #[test]
    fn test_aggregated_project_updated_at_no_chapters() {
        let temp_dir = tempdir().unwrap();
        let workspace_path = temp_dir.path();

        crate::workspace::create_workspace(workspace_path).unwrap();
        let project = crate::project::create_project(workspace_path, "TestProject").unwrap();

        // No chapters — should fallback to project's created_at
        let aggregated = get_project_updated_at_aggregated(workspace_path, &project.id).unwrap();
        assert_eq!(aggregated, project.created_at);
    }

    #[test]
    fn test_delete_project_success() {
        let temp_dir = tempdir().unwrap();
        let workspace_path = temp_dir.path();

        crate::workspace::create_workspace(workspace_path).unwrap();
        let project =
            crate::project::create_project(workspace_path, "TestProjectToDelete").unwrap();

        let project_dir = workspace_path.join("projects").join(&project.id);
        assert!(project_dir.exists());

        let result = delete_project(workspace_path, &project.id);
        assert!(result.is_ok());

        assert!(!project_dir.exists());

        let trash_dir = workspace_path.join("app-meta/sync/trash");
        assert!(trash_dir.exists());

        // Trash should have something
        let trash_contents: Vec<_> = std::fs::read_dir(&trash_dir).unwrap().collect();
        assert!(!trash_contents.is_empty());

        // Verify we can't find it
        let list_res = list_projects(workspace_path).unwrap();
        assert!(list_res.iter().find(|p| p.id == project.id).is_none());
    }

    #[test]
    fn test_delete_project_not_found() {
        let temp_dir = tempdir().unwrap();
        let workspace_path = temp_dir.path();
        crate::workspace::create_workspace(workspace_path).unwrap();

        let result = delete_project(workspace_path, "non_existent_id");
        assert!(result.is_err());
        match result {
            Err(crate::error::Error::InvalidDeleteTarget(_)) => {}
            _ => panic!("Expected InvalidDeleteTarget error for non-existent project"),
        }
    }
}
