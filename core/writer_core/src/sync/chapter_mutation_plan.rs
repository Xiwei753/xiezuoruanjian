//! 同步正文变更的类型化计划。
//!
//! 同步 staging 产出的是一组通用 `CommitAction`。章节协调器不能直接从
//! 文件路径猜“这次是新建、替换、只改 meta 还是删除”，否则正文、meta、
//! 删除事实和证明事件会在不同分支里各自落盘。这里先把 action 按
//! `ChapterKey` 聚合成明确的 mutation，再交给统一提交入口执行。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::chapter::{Chapter, ChapterKey};
use crate::sync::staging::CommitAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncedChapterMutation {
    Replace,
    Meta,
    Delete,
}

#[derive(Debug, Clone)]
pub(crate) struct SyncedChapterChange {
    pub(crate) key: ChapterKey,
    pub(crate) mutation: SyncedChapterMutation,
    pub(crate) body: Option<String>,
    pub(crate) meta: Option<Chapter>,
    pub(crate) action_index: usize,
}

fn chapter_body_parts(path: &Path) -> Option<(Option<String>, String, String)> {
    let parts: Vec<String> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    let (offset, project_id) = if parts.len() == 7 && parts[0] == "projects" {
        (2, Some(parts[1].clone()))
    } else if parts.len() == 5 && parts[0] == "volumes" {
        (0, None)
    } else {
        return None;
    };
    if parts[offset] == "volumes"
        && parts[offset + 2] == "chapters"
        && parts[offset + 4] == "chapter.md"
    {
        Some((
            project_id,
            parts[offset + 1].clone(),
            parts[offset + 3].clone(),
        ))
    } else {
        None
    }
}

fn chapter_meta_parts(path: &Path) -> Option<(Option<String>, String, String)> {
    let parts: Vec<String> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    let (offset, project_id) = if parts.len() == 7 && parts[0] == "projects" {
        (2, Some(parts[1].clone()))
    } else if parts.len() == 5 && parts[0] == "volumes" {
        (0, None)
    } else {
        return None;
    };
    if parts[offset] == "volumes"
        && parts[offset + 2] == "chapters"
        && parts[offset + 4] == "chapter.meta.json"
    {
        Some((
            project_id,
            parts[offset + 1].clone(),
            parts[offset + 3].clone(),
        ))
    } else {
        None
    }
}

/// 把通用 action 聚合成每章一个 mutation。
///
/// 同一章同时有正文和 meta action 时合并为一条 Replace；只有 meta 的章节
/// 标为 Meta；正文删除标为 Delete。真实是 Create 还是 Replace 由执行阶段
/// 读取 live 状态决定，计划本身不猜测磁盘现状。
pub(crate) fn plan_synced_chapter_mutations(
    project_id: &str,
    actions: &[CommitAction],
) -> crate::error::Result<Vec<SyncedChapterChange>> {
    let mut grouped: BTreeMap<(String, String, String), SyncedChapterChange> = BTreeMap::new();
    for (action_index, action) in actions.iter().enumerate() {
        match action {
            CommitAction::Apply { rel_path, content } => {
                if let Some((project_override, volume_id, chapter_id)) =
                    chapter_body_parts(rel_path)
                {
                    let content = std::str::from_utf8(content)
                        .map_err(|error| {
                            crate::error::Error::Other(format!(
                                "sync chapter body is not UTF-8 {}: {error}",
                                rel_path.display()
                            ))
                        })?
                        .to_string();
                    let entry = grouped
                        .entry((
                            project_override
                                .clone()
                                .unwrap_or_else(|| project_id.to_string()),
                            volume_id.clone(),
                            chapter_id.clone(),
                        ))
                        .or_insert_with(|| SyncedChapterChange {
                            key: ChapterKey::new(
                                project_override.unwrap_or_else(|| project_id.to_string()),
                                volume_id.clone(),
                                chapter_id.clone(),
                            ),
                            mutation: SyncedChapterMutation::Replace,
                            body: None,
                            meta: None,
                            action_index,
                        });
                    entry.body = Some(content);
                    entry.mutation = SyncedChapterMutation::Replace;
                } else if let Some((project_override, volume_id, chapter_id)) =
                    chapter_meta_parts(rel_path)
                {
                    let meta = serde_json::from_slice::<Chapter>(content).map_err(|error| {
                        crate::error::Error::Other(format!(
                            "sync chapter meta is invalid {}: {error}",
                            rel_path.display()
                        ))
                    })?;
                    let entry = grouped
                        .entry((
                            project_override
                                .clone()
                                .unwrap_or_else(|| project_id.to_string()),
                            volume_id.clone(),
                            chapter_id.clone(),
                        ))
                        .or_insert_with(|| SyncedChapterChange {
                            key: ChapterKey::new(
                                project_override.unwrap_or_else(|| project_id.to_string()),
                                volume_id.clone(),
                                chapter_id.clone(),
                            ),
                            mutation: SyncedChapterMutation::Meta,
                            body: None,
                            meta: None,
                            action_index,
                        });
                    entry.meta = Some(meta);
                }
            }
            CommitAction::Delete { rel_path } => {
                if let Some((project_override, volume_id, chapter_id)) =
                    chapter_body_parts(rel_path)
                {
                    let entry = grouped
                        .entry((
                            project_override
                                .clone()
                                .unwrap_or_else(|| project_id.to_string()),
                            volume_id.clone(),
                            chapter_id.clone(),
                        ))
                        .or_insert_with(|| SyncedChapterChange {
                            key: ChapterKey::new(
                                project_override.unwrap_or_else(|| project_id.to_string()),
                                volume_id.clone(),
                                chapter_id.clone(),
                            ),
                            mutation: SyncedChapterMutation::Delete,
                            body: None,
                            meta: None,
                            action_index,
                        });
                    entry.mutation = SyncedChapterMutation::Delete;
                } else if let Some((project_override, volume_id, chapter_id)) =
                    chapter_meta_parts(rel_path)
                {
                    let entry = grouped
                        .entry((
                            project_override
                                .clone()
                                .unwrap_or_else(|| project_id.to_string()),
                            volume_id.clone(),
                            chapter_id.clone(),
                        ))
                        .or_insert_with(|| SyncedChapterChange {
                            key: ChapterKey::new(
                                project_override.unwrap_or_else(|| project_id.to_string()),
                                volume_id.clone(),
                                chapter_id.clone(),
                            ),
                            mutation: SyncedChapterMutation::Delete,
                            body: None,
                            meta: None,
                            action_index,
                        });
                    // 同章还有正文 Apply 时以正文为准，不能把章节删掉。
                    if entry.body.is_none() && entry.mutation != SyncedChapterMutation::Replace {
                        entry.mutation = SyncedChapterMutation::Delete;
                    }
                }
            }
        }
    }
    Ok(grouped.into_values().collect())
}

/// 章节在 project 内的正文相对路径。
pub(crate) fn chapter_body_rel_path(key: &ChapterKey) -> PathBuf {
    PathBuf::from("volumes")
        .join(&key.volume_id)
        .join("chapters")
        .join(&key.chapter_id)
        .join("chapter.md")
}

pub(crate) fn chapter_meta_rel_path(key: &ChapterKey) -> PathBuf {
    PathBuf::from("volumes")
        .join(&key.volume_id)
        .join("chapters")
        .join(&key.chapter_id)
        .join("chapter.meta.json")
}
