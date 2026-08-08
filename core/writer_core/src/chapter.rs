//! # 章节管理（Core 层 - 最核心模块）
//!
//! 负责章节（Chapter）的 CRUD、内容读写、验证保存。
//!
//! ## 职责边界
//!
//! - **做**：章节创建/列表/重命名/删除/排序/内容读写/验证保存
//! - **不做**：排版格式化（由客户端 SujianEditorItem 负责）
//! - **正文永远是纯文本**：`chapter.md` 文件内容是纯文本，不接受 HTML
//! - **删除安全**：所有删除操作经过 `delete_guard` 验证，删除后移入 trash 目录并记录 tombstone
//!
//! ## 核心安全机制
//!
//! 1. **空内容覆盖保护**：`save_chapter_verified` 默认拒绝用空内容覆盖非空章节
//! 2. **事务写入**：所有文件写入通过 `SaveTransaction` 事务完成
//! 3. **写入后验证**：写入后重新读取文件并计算 hash，确保数据完整性
//! 4. **删除进入 trash/tombstone**：删除的章节移入 trash 目录并记录 tombstone
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
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

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
///
/// `content_hash` 是正文内容的 MD5，`meta_hash` 是元数据 JSON 的 MD5。
/// 客户端可据此验证传输完整性，但 MD5 不用于安全目的。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChapterSaveReceipt {
    pub chapter_relative_path: String,
    pub content_len: usize,
    pub content_hash: String,
    pub meta_hash: String,
    pub updated_at: String,
    pub word_count: u32,
}

/// 列出项目下所有有效的章节 ID（跨卷扫描）。
/// 用于同步模块验证远端引用的章节是否存在，不返回章节元数据。
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub fn list_valid_chapter_ids(project_root: &Path) -> Result<std::collections::HashSet<String>> {
    let mut chapter_ids = std::collections::HashSet::new();
    let volumes_dir = project_root.join("volumes");

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

/// 列出指定卷下的所有章节，按 `order` 字段升序排列。
/// 使用 `par_iter` 并行读取各章节的 `chapter.meta.json`，适用于章节数量较多的场景。
/// 不存在的 meta 文件或解析失败的章节静默跳过（不返回错误）。
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub fn list_chapters(project_root: &Path, volume_id: &str) -> Result<Vec<Chapter>> {
    let chapters_dir = project_root
        .join("volumes")
        .join(volume_id)
        .join("chapters");
    if !chapters_dir.exists() {
        return Ok(Vec::new());
    }

    let entries: Vec<_> = fs::read_dir(chapters_dir)?.collect::<std::io::Result<Vec<_>>>()?;

    let mut chapters: Vec<Chapter> = entries
        .into_par_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                let meta_path = path.join("chapter.meta.json");
                if meta_path.exists() {
                    match fs::read_to_string(&meta_path) {
                        Ok(content) => {
                            if let Ok(chapter) = serde_json::from_str::<Chapter>(&content) {
                                Some(Ok(chapter))
                            } else {
                                None
                            }
                        }
                        Err(e) => Some(Err(e)),
                    }
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect::<std::result::Result<Vec<_>, std::io::Error>>()?;

    chapters.sort_by_key(|c| c.order);
    Ok(chapters)
}

/// 计算字数。
///
/// 当前实现为非空白字符计数，适用于 CJK 文本（每个汉字计为一个字）。
/// 对英文等空格分词语言，此计数不等于传统"单词数"。
#[allow(clippy::cast_possible_truncation)]
pub fn calculate_word_count(text: &str) -> u32 {
    text.chars().filter(|c| !c.is_whitespace()).count() as u32
}

/// 创建章节。`order` 自动递增（当前最大 order + 1）。
/// 空标题时自动生成"第N章"格式。创建后原子写入 meta 和空正文文件。
pub fn create_chapter(project_root: &Path, volume_id: &str, title: &str) -> Result<Chapter> {
    let chapters = list_chapters(project_root, volume_id)?;
    let order = chapters
        .iter()
        .map(|c| c.order)
        .max()
        .map(|m| m + 1)
        .unwrap_or(0);

    // 空标题兜底：按当前卷章节数量生成默认标题
    let effective_title = if title.trim().is_empty() {
        format!("第{}章", chapters.len() + 1)
    } else {
        title.to_string()
    };

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let chapter = Chapter {
        id: id.clone(),
        title: effective_title,
        created_at: now.clone(),
        updated_at: now,
        order,
        word_count: 0,
        hash: String::new(),
        note: None,
    };

    let chapter_dir = project_root
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

/// 读取章节内容（元数据 + 正文）。meta 或 md 文件不存在时返回 ChapterNotFound。
pub fn read_chapter(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
) -> Result<ChapterContent> {
    let chapter_dir = project_root
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

/// 保存章节正文（带空内容覆盖保护）。成功时丢弃回执。
pub fn save_chapter(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    content: &str,
) -> Result<()> {
    save_chapter_verified(project_root, volume_id, chapter_id, content).map(|_| ())
}

/// 保存章节正文并返回验证回执（默认拒绝空内容覆盖非空章节）。
pub fn save_chapter_verified(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    content: &str,
) -> Result<ChapterSaveReceipt> {
    save_chapter_verified_with_options(project_root, volume_id, chapter_id, content, false)
}

/// 保存章节正文（可控制是否允许空内容覆盖）。
pub fn save_chapter_verified_with_allow_empty_overwrite(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    content: &str,
    allow_empty_overwrite: bool,
) -> Result<ChapterSaveReceipt> {
    save_chapter_verified_with_options(
        project_root,
        volume_id,
        chapter_id,
        content,
        allow_empty_overwrite,
    )
}

/// 清空章节正文（成功时丢弃回执）。等效于 `save_chapter_verified_with_options(_, "", true)`。
pub fn clear_chapter_content(project_root: &Path, volume_id: &str, chapter_id: &str) -> Result<()> {
    clear_chapter_content_verified(project_root, volume_id, chapter_id).map(|_| ())
}

/// 清空章节正文并返回验证回执。
pub fn clear_chapter_content_verified(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
) -> Result<ChapterSaveReceipt> {
    save_chapter_verified_with_options(project_root, volume_id, chapter_id, "", true)
}

/// 章节保存的核心实现。
///
/// ## 空内容覆盖保护
///
/// 当 `allow_empty_overwrite == false` 时，拒绝用空/纯空白内容覆盖非空章节。
/// 这是防止客户端 bug 导致数据丢失的最后一道防线。
///
/// ## 事务写入 + 写后验证
///
/// 1. 通过 `SaveTransaction` 原子写入 `chapter.md` 和 `chapter.meta.json`
/// 2. 写入后重新读取文件并计算 hash，与预期 hash 比对
/// 3. 验证失败返回错误，但文件已被事务提交（无法自动回滚）
fn save_chapter_verified_with_options(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    content: &str,
    allow_empty_overwrite: bool,
) -> Result<ChapterSaveReceipt> {
    let chapter_dir = project_root
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
        log::warn!(
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

    let meta_str = fs::read_to_string(&meta_path)?;
    let mut meta: Chapter = serde_json::from_str(&meta_str)?;

    meta.updated_at = Utc::now().to_rfc3339();
    meta.word_count = calculate_word_count(content);

    // Simple hash for demonstration
    meta.hash = format!("{:x}", md5::compute(content.as_bytes()));

    let updated_meta_str = serde_json::to_string_pretty(&meta)?;

    // Transactional write: all files staged first, then atomic rename
    let mut tx = crate::storage::transaction::SaveTransaction::new(project_root);

    let md_relative = md_path
        .strip_prefix(project_root)
        .unwrap_or(&md_path)
        .to_string_lossy()
        .replace('\\', "/");
    let meta_relative = meta_path
        .strip_prefix(project_root)
        .unwrap_or(&meta_path)
        .to_string_lossy()
        .replace('\\', "/");

    tx.add_file(&md_relative, content)?;
    tx.add_file(&meta_relative, &updated_meta_str)?;

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
        .strip_prefix(project_root)
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

pub fn update_chapter_note(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    note: &str,
) -> Result<()> {
    let chapter_dir = project_root
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
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    new_title: &str,
) -> Result<()> {
    let chapter_dir = project_root
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

/// 删除章节。经过 `delete_guard` 验证后，将章节目录移入 `app-meta/sync/trash/`
/// 并记录 tombstone（30 天后可清理）。删除后同步状态中保留墓碑记录，
/// 确保下次同步时远端能感知到本地删除。
pub fn delete_chapter(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    app_data_root: &Path,
) -> Result<()> {
    let volume_id = crate::delete_guard::validate_id_segment(volume_id)?;
    let chapter_id = crate::delete_guard::validate_id_segment(chapter_id)?;
    let chapter_dir = project_root
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(chapter_id);
    let target_canon = crate::delete_guard::validate_delete_target(
        project_root,
        &chapter_dir,
        "chapter.meta.json",
    )?;

    let trash_dir = app_data_root.join("sync/trash");
    let _ = fs::create_dir_all(&trash_dir);
    let trash_path = trash_dir.join(format!(
        "{}_{}_{}",
        chrono::Utc::now().timestamp_millis(),
        uuid::Uuid::new_v4(),
        chapter_id
    ));
    fs::rename(&target_canon, &trash_path)?;

    // Also update tombstone
    if let Ok(mut state) = crate::sync::SyncService::load_sync_state(app_data_root) {
        let rel_chapter_dir = chapter_dir
            .strip_prefix(project_root)
            .unwrap_or(&chapter_dir)
            .to_string_lossy()
            .replace("\\", "/");
        let rel_trash_path = trash_path
            .strip_prefix(app_data_root)
            .unwrap_or(&trash_path)
            .to_string_lossy()
            .replace("\\", "/");

        // The hash can be the hash of the folder, but currently we track files.
        // To be consistent, we might want to register tombstones for all files in this directory.
        crate::trash::generate_tombstones(
            &mut state,
            &trash_path,
            &rel_chapter_dir,
            &rel_trash_path,
        );
        let _ = crate::sync::SyncService::save_sync_state(app_data_root, &state);
    }
    Ok(())
}

#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn reorder_chapters(
    project_root: &Path,
    volume_id: &str,
    ordered_ids: &[String],
) -> Result<()> {
    let chapters = list_chapters(project_root, volume_id)?;
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

    let now_str = Utc::now().to_rfc3339();
    let base_chapters_dir = project_root
        .join("volumes")
        .join(volume_id)
        .join("chapters");

    for (index, id) in ordered_ids.iter().enumerate() {
        let meta_path = base_chapters_dir.join(id).join("chapter.meta.json");

        if meta_path.exists() {
            let meta_str = fs::read_to_string(&meta_path)?;
            let mut meta = serde_json::from_str::<Chapter>(&meta_str)?;

            if meta.order != index as i32 {
                meta.order = index as i32;
                meta.updated_at = now_str.clone();
                let updated_meta_str = serde_json::to_string_pretty(&meta)?;
                crate::storage::atomic_write_string(&meta_path, &updated_meta_str)?;
            }
        } else {
            return Err(crate::error::Error::ChapterNotFound);
        }
    }
    Ok(())
}

#[cfg(test)]
mod inline_tests {
    use super::*;
    use tempfile::tempdir;

    /// 验证 save_chapter_verified_with_options 只修改 chapter.md 和 chapter.meta.json，
    /// 不污染父级 volume.json 和 project.json。
    #[test]
    fn test_save_chapter_does_not_pollute_parent_json() {
        let temp_dir = tempdir().unwrap();
        let data_root = temp_dir.path();

        // Create project structure
        std::fs::create_dir_all(data_root.join("projects")).unwrap();

        // Create project and volume
        let project =
            crate::project::create_project(&data_root.join("projects"), "TestProject").unwrap();
        let volumes =
            crate::volume::list_volumes(&data_root.join("projects").join(&project.id)).unwrap();
        let volume = &volumes[0]; // create_project auto-creates "第一卷"

        // Create a chapter
        let chapter = create_chapter(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            "Ch1",
        )
        .unwrap();

        // Record volume.json and project.json content BEFORE saving
        let vol_json_path = data_root
            .join("projects")
            .join(&project.id)
            .join("volumes")
            .join(&volume.id)
            .join("volume.json");
        let proj_json_path = data_root
            .join("projects")
            .join(&project.id)
            .join("project.json");

        let vol_content_before = fs::read_to_string(&vol_json_path).unwrap();
        let proj_content_before = fs::read_to_string(&proj_json_path).unwrap();

        // Save chapter content
        let receipt = save_chapter_verified(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter.id,
            "Hello world content",
        )
        .unwrap();

        // Verify the save receipt is valid
        assert_eq!(receipt.content_len, "Hello world content".len());
        assert!(receipt.word_count > 0);

        // Verify chapter.md was written correctly
        let md_path = data_root
            .join("projects")
            .join(&project.id)
            .join("volumes")
            .join(&volume.id)
            .join("chapters")
            .join(&chapter.id)
            .join("chapter.md");
        let md_content = fs::read_to_string(&md_path).unwrap();
        assert_eq!(md_content, "Hello world content");

        // Verify chapter.meta.json was updated
        let meta_path = data_root
            .join("projects")
            .join(&project.id)
            .join("volumes")
            .join(&volume.id)
            .join("chapters")
            .join(&chapter.id)
            .join("chapter.meta.json");
        let meta_content = fs::read_to_string(&meta_path).unwrap();
        let meta: Chapter = serde_json::from_str(&meta_content).unwrap();
        assert_eq!(meta.id, chapter.id);
        assert!(meta.word_count > 0);

        // CRITICAL: Verify volume.json was NOT modified
        let vol_content_after = fs::read_to_string(&vol_json_path).unwrap();
        assert_eq!(
            vol_content_before, vol_content_after,
            "volume.json should NOT be modified when saving a chapter"
        );

        // CRITICAL: Verify project.json was NOT modified
        let proj_content_after = fs::read_to_string(&proj_json_path).unwrap();
        assert_eq!(
            proj_content_before, proj_content_after,
            "project.json should NOT be modified when saving a chapter"
        );
    }

    /// 验证多次保存章节不会污染父级 JSON。
    #[test]
    fn test_multiple_saves_do_not_pollute_parent_json() {
        let temp_dir = tempdir().unwrap();
        let data_root = temp_dir.path();

        std::fs::create_dir_all(data_root.join("projects")).unwrap();

        let project =
            crate::project::create_project(&data_root.join("projects"), "TestProject2").unwrap();
        let volumes =
            crate::volume::list_volumes(&data_root.join("projects").join(&project.id)).unwrap();
        let volume = &volumes[0];

        let chapter1 = create_chapter(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            "Ch1",
        )
        .unwrap();
        let chapter2 = create_chapter(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            "Ch2",
        )
        .unwrap();

        let vol_json_path = data_root
            .join("projects")
            .join(&project.id)
            .join("volumes")
            .join(&volume.id)
            .join("volume.json");
        let proj_json_path = data_root
            .join("projects")
            .join(&project.id)
            .join("project.json");

        let vol_content_before = fs::read_to_string(&vol_json_path).unwrap();
        let proj_content_before = fs::read_to_string(&proj_json_path).unwrap();

        // Save chapter1
        save_chapter_verified(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter1.id,
            "Content for chapter 1",
        )
        .unwrap();

        // Save chapter2
        save_chapter_verified(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter2.id,
            "Content for chapter 2",
        )
        .unwrap();

        // Verify parent JSONs are untouched
        let vol_content_after = fs::read_to_string(&vol_json_path).unwrap();
        let proj_content_after = fs::read_to_string(&proj_json_path).unwrap();
        assert_eq!(vol_content_before, vol_content_after);
        assert_eq!(proj_content_before, proj_content_after);
    }

    #[test]
    fn test_rename_chapter_success() {
        let temp_dir = tempdir().unwrap();
        let data_root = temp_dir.path();

        std::fs::create_dir_all(data_root.join("projects")).unwrap();
        let project =
            crate::project::create_project(&data_root.join("projects"), "TestProject").unwrap();
        let volumes =
            crate::volume::list_volumes(&data_root.join("projects").join(&project.id)).unwrap();
        let volume = &volumes[0];

        let chapter = create_chapter(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            "Old Title",
        )
        .unwrap();

        let meta_path = data_root
            .join("projects")
            .join(&project.id)
            .join("volumes")
            .join(&volume.id)
            .join("chapters")
            .join(&chapter.id)
            .join("chapter.meta.json");

        let meta_before: Chapter =
            serde_json::from_str(&fs::read_to_string(&meta_path).unwrap()).unwrap();
        assert_eq!(meta_before.title, "Old Title");

        rename_chapter(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter.id,
            "New Title",
        )
        .unwrap();

        let meta_after: Chapter =
            serde_json::from_str(&fs::read_to_string(&meta_path).unwrap()).unwrap();
        assert_eq!(meta_after.title, "New Title");
        assert_eq!(meta_after.id, chapter.id);
        assert!(meta_after.updated_at >= meta_before.updated_at);
    }

    #[test]
    fn test_rename_chapter_not_found() {
        let temp_dir = tempdir().unwrap();
        let data_root = temp_dir.path();

        std::fs::create_dir_all(data_root.join("projects")).unwrap();
        let project =
            crate::project::create_project(&data_root.join("projects"), "TestProject").unwrap();
        let volumes =
            crate::volume::list_volumes(&data_root.join("projects").join(&project.id)).unwrap();
        let volume = &volumes[0];

        let result = rename_chapter(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            "nonexistent-chapter-id",
            "New Title",
        );

        assert!(matches!(result, Err(crate::error::Error::ChapterNotFound)));
    }

    /// 验证空标题时自动生成默认标题"第N章"
    #[test]
    fn test_create_chapter_empty_title_generates_default() {
        let temp_dir = tempdir().unwrap();
        let data_root = temp_dir.path();

        std::fs::create_dir_all(data_root.join("projects")).unwrap();
        let project =
            crate::project::create_project(&data_root.join("projects"), "TestProject").unwrap();
        let volumes =
            crate::volume::list_volumes(&data_root.join("projects").join(&project.id)).unwrap();
        let volume = &volumes[0];

        // 空标题应生成"第1章"
        let chapter1 = create_chapter(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            "",
        )
        .unwrap();
        assert_eq!(chapter1.title, "第1章");

        // 纯空格标题也应生成默认标题
        let chapter2 = create_chapter(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            "   ",
        )
        .unwrap();
        assert_eq!(chapter2.title, "第2章");

        // 有标题时不应被覆盖
        let chapter3 = create_chapter(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            "自定义标题",
        )
        .unwrap();
        assert_eq!(chapter3.title, "自定义标题");

        // 再创建空标题应生成"第4章"（因为已有3个章节）
        let chapter4 = create_chapter(
            &data_root.join("projects").join(&project.id),
            &volume.id,
            "",
        )
        .unwrap();
        assert_eq!(chapter4.title, "第4章");
    }
}

#[cfg(test)]
mod tests;
