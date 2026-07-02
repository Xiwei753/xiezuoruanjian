//! # 卷管理（Core 层）
//!
//! 负责卷（Volume）的 CRUD、排序、重命名、删除。
//!
//! ## 职责边界
//!
//! - **做**：卷创建/列表/重命名/删除/排序
//! - **不做**：章节管理（由 `chapter.rs` 负责）
//! - **删除安全**：所有删除操作经过 `delete_guard` 验证，删除后移入 trash 目录并记录 tombstone
//!
//! ## 目录结构
//!
//! ```text
//! projects/{project_id}/volumes/
//!   {volume_id}/
//!     volume.json           # 卷元数据（id、title、order、时间戳）
//!     chapters/             # 所有章节
//! ```

use crate::error::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;
use rayon::prelude::*;

/// Normalize path for tombstone, safely handling paths that are not prefixed
pub(crate) fn normalize_rel_path(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace("\\", "/")
}

/// 卷元数据结构体。
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

    let mut dir_paths = Vec::new();
    for entry in fs::read_dir(volumes_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            dir_paths.push(path);
        }
    }

    let volumes_result: Result<Vec<Option<Volume>>> = dir_paths
        .into_par_iter()
        .map(|path| {
            let meta_path = path.join("volume.json");
            if meta_path.exists() {
                let content = fs::read_to_string(&meta_path)?;
                if let Ok(volume) = serde_json::from_str::<Volume>(&content) {
                    Ok(Some(volume))
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        })
        .collect();

    let mut volumes: Vec<Volume> = volumes_result?.into_iter().flatten().collect();
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
    let project_id = crate::delete_guard::validate_id_segment(project_id)?;
    let volume_id = crate::delete_guard::validate_id_segment(volume_id)?;
    let volume_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes")
        .join(volume_id);
    let target_canon =
        crate::delete_guard::validate_delete_target(workspace_path, &volume_dir, "volume.json")?;

    let trash_dir = workspace_path.join("app-meta/sync/trash");
    let _ = fs::create_dir_all(&trash_dir);
    let trash_path = trash_dir.join(format!(
        "{}_{}_{}",
        chrono::Utc::now().timestamp_millis(),
        uuid::Uuid::new_v4(),
        volume_id
    ));
    fs::rename(&target_canon, &trash_path)?;

    // Also update tombstone
    if let Ok(mut state) = crate::sync::SyncService::load_sync_state(workspace_path) {
        let rel_volume_dir = normalize_rel_path(&volume_dir, workspace_path);
        let rel_trash_path = normalize_rel_path(&trash_path, workspace_path);

        crate::trash::generate_tombstones(
            &mut state,
            &trash_path,
            &rel_volume_dir,
            &rel_trash_path,
        );
        let _ = crate::sync::SyncService::save_sync_state(workspace_path, &state);
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
