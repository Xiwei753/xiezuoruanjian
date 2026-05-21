use crate::error::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Chapter {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub order: i32,
    pub word_count: u32,
    pub hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChapterContent {
    pub meta: Chapter,
    pub content: String,
}

pub fn list_chapters(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
) -> Result<Vec<Chapter>> {
    let chapters_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes")
        .join(volume_id)
        .join("chapters");
    if !chapters_dir.exists() {
        return Ok(Vec::new());
    }

    let mut chapters = Vec::new();
    for entry in fs::read_dir(chapters_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let meta_path = path.join("chapter.meta.json");
            if meta_path.exists() {
                let content = fs::read_to_string(&meta_path)?;
                if let Ok(chapter) = serde_json::from_str::<Chapter>(&content) {
                    chapters.push(chapter);
                }
            }
        }
    }
    chapters.sort_by_key(|c| c.order);
    Ok(chapters)
}

pub fn create_chapter(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    title: &str,
) -> Result<Chapter> {
    let chapters = list_chapters(workspace_path, project_id, volume_id)?;
    let order = chapters
        .iter()
        .map(|c| c.order)
        .max()
        .map(|m| m + 1)
        .unwrap_or(0);

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let chapter = Chapter {
        id: id.clone(),
        title: title.to_string(),
        created_at: now.clone(),
        updated_at: now,
        order,
        word_count: 0,
        hash: String::new(),
        note: None,
    };

    let chapter_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(&id);
    fs::create_dir_all(&chapter_dir)?;

    let meta_path = chapter_dir.join("chapter.meta.json");
    let content = serde_json::to_string_pretty(&chapter)?;
    crate::storage::atomic_write_string(&meta_path, &content)?;

    let md_path = chapter_dir.join("chapter.md");
    crate::storage::atomic_write_string(&md_path, "")?;

    Ok(chapter)
}

pub fn read_chapter(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
) -> Result<ChapterContent> {
    let chapter_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(chapter_id);
    let meta_path = chapter_dir.join("chapter.meta.json");
    let md_path = chapter_dir.join("chapter.md");

    if !meta_path.exists() || !md_path.exists() {
        return Err(crate::error::Error::ChapterNotFound);
    }

    let meta_str = fs::read_to_string(&meta_path)?;
    let meta: Chapter = serde_json::from_str(&meta_str)?;
    let content = fs::read_to_string(&md_path)?;

    Ok(ChapterContent { meta, content })
}

pub fn save_chapter(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
    content: &str,
) -> Result<()> {
    let chapter_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(chapter_id);
    let meta_path = chapter_dir.join("chapter.meta.json");
    let md_path = chapter_dir.join("chapter.md");

    if !meta_path.exists() {
        return Err(crate::error::Error::ChapterNotFound);
    }

    let meta_str = fs::read_to_string(&meta_path)?;
    let mut meta: Chapter = serde_json::from_str(&meta_str)?;

    meta.updated_at = Utc::now().to_rfc3339();
    meta.word_count = content.chars().filter(|c| !c.is_whitespace()).count() as u32; // Simple word count

    // Simple hash for demonstration
    meta.hash = format!("{:x}", md5::compute(content.as_bytes()));

    let updated_meta_str = serde_json::to_string_pretty(&meta)?;

    crate::storage::atomic_write_string(&md_path, content)?;
    crate::storage::atomic_write_string(&meta_path, &updated_meta_str)?;

    Ok(())
}

pub fn update_chapter_note(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
    note: &str,
) -> Result<()> {
    let chapter_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(chapter_id);
    let meta_path = chapter_dir.join("chapter.meta.json");

    if !meta_path.exists() {
        return Err(crate::error::Error::ChapterNotFound);
    }

    let meta_str = fs::read_to_string(&meta_path)?;
    let mut meta: Chapter = serde_json::from_str(&meta_str)?;

    meta.updated_at = Utc::now().to_rfc3339();
    meta.note = Some(note.to_string());

    let updated_meta_str = serde_json::to_string_pretty(&meta)?;
    crate::storage::atomic_write_string(&meta_path, &updated_meta_str)?;

    Ok(())
}

pub fn rename_chapter(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
    new_title: &str,
) -> Result<()> {
    let chapter_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(chapter_id);
    let meta_path = chapter_dir.join("chapter.meta.json");

    if !meta_path.exists() {
        return Err(crate::error::Error::ChapterNotFound);
    }

    let meta_str = fs::read_to_string(&meta_path)?;
    let mut meta: Chapter = serde_json::from_str(&meta_str)?;

    meta.title = new_title.to_string();
    meta.updated_at = Utc::now().to_rfc3339();

    let updated_meta_str = serde_json::to_string_pretty(&meta)?;
    crate::storage::atomic_write_string(&meta_path, &updated_meta_str)?;

    Ok(())
}

pub fn delete_chapter(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
) -> Result<()> {
    let project_id = project_id.trim();
    if project_id.is_empty() || project_id.contains("..") || project_id.contains("/") || project_id.contains("\\") {
        return Err(crate::error::Error::Other(format!("Invalid parameter: {}", project_id)));
    }

    let volume_id = volume_id.trim();
    if volume_id.is_empty() || volume_id.contains("..") || volume_id.contains("/") || volume_id.contains("\\") {
        return Err(crate::error::Error::Other(format!("Invalid parameter: {}", volume_id)));
    }

    let chapter_id = chapter_id.trim();
    if chapter_id.is_empty() || chapter_id.contains("..") || chapter_id.contains("/") || chapter_id.contains("\\") {
        return Err(crate::error::Error::Other(format!("Invalid parameter: {}", chapter_id)));
    }

    let chapter_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(chapter_id);

    if chapter_dir.exists() {
        let trash_dir = workspace_path.join("app-meta/sync/trash");
        let _ = fs::create_dir_all(&trash_dir);
        let trash_path = trash_dir.join(format!("{}_{}_{}", chrono::Utc::now().timestamp_millis(), uuid::Uuid::new_v4(), chapter_id));
        fs::rename(&chapter_dir, &trash_path)?;

        // Also update tombstone
        if let Ok(mut state) = crate::sync_service::SyncService::load_sync_state(workspace_path) {
             let rel_chapter_dir = chapter_dir.strip_prefix(workspace_path).unwrap_or(&chapter_dir).to_string_lossy().replace("\\", "/");
             let rel_trash_path = trash_path.strip_prefix(workspace_path).unwrap_or(&trash_path).to_string_lossy().replace("\\", "/");

             // The hash can be the hash of the folder, but currently we track files.
             // To be consistent, we might want to register tombstones for all files in this directory.
             for entry in walkdir::WalkDir::new(&trash_path).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
                 let rel_file_path = entry.path().strip_prefix(&trash_path).unwrap_or(entry.path()).to_string_lossy().replace("\\", "/");
                 let original_file_path = format!("{}/{}", rel_chapter_dir, rel_file_path);
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
        return Err(crate::error::Error::ChapterNotFound);
    }
    Ok(())
}

pub fn reorder_chapters(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    ordered_ids: &[String],
) -> Result<()> {
    let chapters = list_chapters(workspace_path, project_id, volume_id)?;
    let existing_ids: std::collections::HashSet<_> =
        chapters.iter().map(|c| c.id.clone()).collect();
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
        let chapter_dir = workspace_path
            .join("projects")
            .join(project_id)
            .join("volumes")
            .join(volume_id)
            .join("chapters")
            .join(id);
        let meta_path = chapter_dir.join("chapter.meta.json");

        if meta_path.exists() {
            let meta_str = fs::read_to_string(&meta_path)?;
            let mut meta = serde_json::from_str::<Chapter>(&meta_str)?;
            meta.order = index as i32;
            meta.updated_at = Utc::now().to_rfc3339();
            let updated_meta_str = serde_json::to_string_pretty(&meta)?;
            crate::storage::atomic_write_string(&meta_path, &updated_meta_str)?;
        } else {
            return Err(crate::error::Error::ChapterNotFound);
        }
    }
    Ok(())
}
