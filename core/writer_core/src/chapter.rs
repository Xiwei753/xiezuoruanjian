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
    Ok(chapters)
}

pub fn create_chapter(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    title: &str,
) -> Result<Chapter> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let chapter = Chapter {
        id: id.clone(),
        title: title.to_string(),
        created_at: now.clone(),
        updated_at: now,
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
