//! # 章节管理（Core 层 - 最核心模块）
//!
//! 负责章节（Chapter）的 CRUD、内容读写、备份、验证保存。
//!
//! ## 职责边界
//!
//! - **做**：章节创建/列表/重命名/删除/排序/内容读写/备份/验证保存
//! - **不做**：排版格式化（由客户端 `DocumentHandler` 负责）
//! - **正文永远是纯文本**：`chapter.md` 文件内容是纯文本，不接受 HTML
//! - **删除安全**：所有删除操作经过 `delete_guard` 验证，删除后移入 trash 目录并记录 tombstone
//!
//! ## 核心安全机制
//!
//! 1. **空内容覆盖保护**：`save_chapter_verified` 默认拒绝用空内容覆盖非空章节
//! 2. **写入后验证**：写入后重新读取文件并计算 hash，确保数据完整性
//! 3. **备份机制**：每次保存前自动备份旧版本（保留最近 20 个版本）
//! 4. **原子写入**：所有文件写入通过 `storage::atomic_write_string` 完成
//!
//! ## 目录结构
//!
//! ```text
//! projects/{project_id}/volumes/{volume_id}/chapters/
//!   {chapter_id}/
//!     chapter.meta.json     # 章节元数据（id、title、order、word_count、hash）
//!     chapter.md            # 正文内容（纯文本）
//! ```

use crate::error::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

/// 单个章节最多保留的备份数量。
const CHAPTER_BACKUP_KEEP: usize = 20;

/// 章节元数据结构体。
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

/// 章节内容（元数据 + 正文）。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChapterContent {
    pub meta: Chapter,
    pub content: String,
}

/// 章节保存回执（用于客户端确认保存成功）。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChapterSaveReceipt {
    pub chapter_relative_path: String,
    pub content_len: usize,
    pub content_hash: String,
    pub meta_hash: String,
    pub updated_at: String,
    pub word_count: u32,
}

fn touch_json_updated_at(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let raw = fs::read_to_string(path)?;
    let mut val: serde_json::Value = serde_json::from_str(&raw)?;
    if let Some(obj) = val.as_object_mut() {
        obj.insert(
            "updated_at".to_string(),
            serde_json::Value::String(Utc::now().to_rfc3339()),
        );
    }
    crate::storage::atomic_write_string(path, &serde_json::to_string_pretty(&val)?)
}

pub fn list_valid_chapter_ids(
    workspace_path: &Path,
    project_id: &str,
) -> Result<std::collections::HashSet<String>> {
    let mut chapter_ids = std::collections::HashSet::new();
    let volumes_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes");

    if !volumes_dir.exists() {
        return Ok(chapter_ids);
    }

    // Iterate over volume directories
    if let Ok(entries) = fs::read_dir(volumes_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let chapters_dir = entry.path().join("chapters");
                // Iterate over chapter directories within each volume
                if let Ok(chapter_entries) = fs::read_dir(chapters_dir) {
                    for chapter_entry in chapter_entries.flatten() {
                        let path = chapter_entry.path();
                        if path.is_dir() {
                            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                                chapter_ids.insert(name.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(chapter_ids)
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

pub fn calculate_word_count(text: &str) -> u32 {
    text.chars().filter(|c| !c.is_whitespace()).count() as u32
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
    save_chapter_verified(workspace_path, project_id, volume_id, chapter_id, content).map(|_| ())
}

pub fn save_chapter_verified(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
    content: &str,
) -> Result<ChapterSaveReceipt> {
    save_chapter_verified_with_options(
        workspace_path,
        project_id,
        volume_id,
        chapter_id,
        content,
        false,
    )
}

pub fn save_chapter_verified_with_allow_empty_overwrite(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
    content: &str,
    allow_empty_overwrite: bool,
) -> Result<ChapterSaveReceipt> {
    save_chapter_verified_with_options(
        workspace_path,
        project_id,
        volume_id,
        chapter_id,
        content,
        allow_empty_overwrite,
    )
}

pub fn clear_chapter_content(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
) -> Result<()> {
    clear_chapter_content_verified(workspace_path, project_id, volume_id, chapter_id).map(|_| ())
}

pub fn clear_chapter_content_verified(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
) -> Result<ChapterSaveReceipt> {
    save_chapter_verified_with_options(workspace_path, project_id, volume_id, chapter_id, "", true)
}

fn save_chapter_verified_with_options(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
    content: &str,
    allow_empty_overwrite: bool,
) -> Result<ChapterSaveReceipt> {
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

    let old_content = if md_path.exists() {
        fs::read_to_string(&md_path)?
    } else {
        String::new()
    };

    if !allow_empty_overwrite && !old_content.trim().is_empty() && content.trim().is_empty() {
        let reason = "new_content_empty_or_whitespace_without_allow_empty_overwrite".to_string();
        eprintln!(
            "blocked_empty_overwrite chapter_id={} old_len={} new_len={} reason={}",
            chapter_id,
            old_content.len(),
            content.len(),
            reason
        );
        return Err(crate::error::Error::EmptyOverwriteBlocked {
            chapter_id: chapter_id.to_string(),
            old_len: old_content.len(),
            new_len: content.len(),
            reason,
        });
    }

    if !old_content.is_empty() && old_content != content {
        backup_chapter_content(
            workspace_path,
            project_id,
            volume_id,
            chapter_id,
            &old_content,
        )?;
    }

    let meta_str = fs::read_to_string(&meta_path)?;
    let mut meta: Chapter = serde_json::from_str(&meta_str)?;

    meta.updated_at = Utc::now().to_rfc3339();
    meta.word_count = calculate_word_count(content);

    // Simple hash for demonstration
    meta.hash = format!("{:x}", md5::compute(content.as_bytes()));

    let updated_meta_str = serde_json::to_string_pretty(&meta)?;

    // Transactional write: all files staged first, then atomic rename
    let mut tx = crate::storage::transaction::SaveTransaction::new(workspace_path);

    let md_relative = md_path
        .strip_prefix(workspace_path)
        .unwrap_or(&md_path)
        .to_string_lossy()
        .replace('\\', "/");
    let meta_relative = meta_path
        .strip_prefix(workspace_path)
        .unwrap_or(&meta_path)
        .to_string_lossy()
        .replace('\\', "/");

    tx.add_file(&md_relative, content)?;
    tx.add_file(&meta_relative, &updated_meta_str)?;

    let volume_meta_path = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes")
        .join(volume_id)
        .join("volume.json");
    let project_meta_path = workspace_path
        .join("projects")
        .join(project_id)
        .join("project.json");

    if volume_meta_path.exists() {
        if let Ok(vol_raw) = fs::read_to_string(&volume_meta_path) {
            if let Ok(mut vol_val) = serde_json::from_str::<serde_json::Value>(&vol_raw) {
                if let Some(obj) = vol_val.as_object_mut() {
                    obj.insert(
                        "updated_at".to_string(),
                        serde_json::Value::String(Utc::now().to_rfc3339()),
                    );
                    let vol_relative = volume_meta_path
                        .strip_prefix(workspace_path)
                        .unwrap_or(&volume_meta_path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    let _ = tx.add_file(&vol_relative, &serde_json::to_string_pretty(&vol_val)?);
                }
            }
        }
    }

    if project_meta_path.exists() {
        if let Ok(proj_raw) = fs::read_to_string(&project_meta_path) {
            if let Ok(mut proj_val) = serde_json::from_str::<serde_json::Value>(&proj_raw) {
                if let Some(obj) = proj_val.as_object_mut() {
                    obj.insert(
                        "updated_at".to_string(),
                        serde_json::Value::String(Utc::now().to_rfc3339()),
                    );
                    let proj_relative = project_meta_path
                        .strip_prefix(workspace_path)
                        .unwrap_or(&project_meta_path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    let _ = tx.add_file(&proj_relative, &serde_json::to_string_pretty(&proj_val)?);
                }
            }
        }
    }

    tx.commit()?;

    let read_back = fs::read_to_string(&md_path)?;
    let read_back_hash = format!("{:x}", md5::compute(read_back.as_bytes()));
    if read_back_hash != meta.hash {
        return Err(crate::error::Error::Other(format!(
            "chapter save verification failed: expected_hash={}, actual_hash={}",
            meta.hash, read_back_hash
        )));
    }

    let chapter_relative_path = md_path
        .strip_prefix(workspace_path)
        .unwrap_or(&md_path)
        .to_string_lossy()
        .replace('\\', "/");

    Ok(ChapterSaveReceipt {
        chapter_relative_path,
        content_len: content.len(),
        content_hash: meta.hash.clone(),
        meta_hash: format!("{:x}", md5::compute(updated_meta_str.as_bytes())),
        updated_at: meta.updated_at,
        word_count: meta.word_count,
    })
}

fn backup_chapter_content(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
    content: &str,
) -> Result<()> {
    let backup_dir = workspace_path.join("backups").join("chapters");
    fs::create_dir_all(&backup_dir)?;

    let prefix = chapter_backup_prefix(project_id, volume_id, chapter_id);
    let timestamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
    let backup_path = backup_dir.join(format!("{}{}__{}.md", prefix, timestamp, Uuid::new_v4()));

    crate::storage::atomic_write_string(&backup_path, content)?;
    prune_chapter_backups(&backup_dir, &prefix, CHAPTER_BACKUP_KEEP)
}

fn prune_chapter_backups(backup_dir: &Path, prefix: &str, keep: usize) -> Result<()> {
    let mut backups = Vec::new();
    for entry in fs::read_dir(backup_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }

        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name.starts_with(prefix) && file_name.ends_with(".md") {
            backups.push((file_name, entry.path()));
        }
    }

    if backups.len() <= keep {
        return Ok(());
    }

    backups.sort_by(|a, b| a.0.cmp(&b.0));
    let remove_count = backups.len() - keep;
    for (_, path) in backups.into_iter().take(remove_count) {
        fs::remove_file(path)?;
    }

    Ok(())
}

fn chapter_backup_prefix(project_id: &str, volume_id: &str, chapter_id: &str) -> String {
    format!(
        "{}__{}__{}__",
        backup_file_part(project_id),
        backup_file_part(volume_id),
        backup_file_part(chapter_id)
    )
}

fn backup_file_part(value: &str) -> String {
    let safe: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();

    if safe.is_empty() {
        "empty".to_string()
    } else {
        safe
    }
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
    let project_id = crate::delete_guard::validate_id_segment(project_id)?;
    let volume_id = crate::delete_guard::validate_id_segment(volume_id)?;
    let chapter_id = crate::delete_guard::validate_id_segment(chapter_id)?;
    let chapter_dir = workspace_path
        .join("projects")
        .join(project_id)
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(chapter_id);
    let target_canon = crate::delete_guard::validate_delete_target(
        workspace_path,
        &chapter_dir,
        "chapter.meta.json",
    )?;

    let trash_dir = workspace_path.join("app-meta/sync/trash");
    let _ = fs::create_dir_all(&trash_dir);
    let trash_path = trash_dir.join(format!(
        "{}_{}_{}",
        chrono::Utc::now().timestamp_millis(),
        uuid::Uuid::new_v4(),
        chapter_id
    ));
    fs::rename(&target_canon, &trash_path)?;

    // Also update tombstone
    if let Ok(mut state) = crate::sync::SyncService::load_sync_state(workspace_path) {
        let rel_chapter_dir = chapter_dir
            .strip_prefix(workspace_path)
            .unwrap_or(&chapter_dir)
            .to_string_lossy()
            .replace("\\", "/");
        let rel_trash_path = trash_path
            .strip_prefix(workspace_path)
            .unwrap_or(&trash_path)
            .to_string_lossy()
            .replace("\\", "/");

        // The hash can be the hash of the folder, but currently we track files.
        // To be consistent, we might want to register tombstones for all files in this directory.
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
            let original_file_path = format!("{}/{}", rel_chapter_dir, rel_file_path);
            let new_trash_path = format!("{}/{}", rel_trash_path, rel_file_path);

            let tombstone = crate::sync::Tombstone {
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
        let _ = crate::sync::SyncService::save_sync_state(workspace_path, &state);
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
