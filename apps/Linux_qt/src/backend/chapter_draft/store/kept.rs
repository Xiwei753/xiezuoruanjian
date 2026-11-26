// =============================================================================
// chapter_draft/store/kept.rs — 不可自动恢复草稿的独立归档区
// =============================================================================
//
// Issue #1062 评论 7194373427 第 1 项：恢复失败、需要人处理的草稿不能继续留在
// `chapter-drafts/<chapter_id>/`，否则编辑器新会话 `note_text` 会直接覆写同一
// 可写槽位，旧事实与新会话混成 MIXED_SESSION_ERROR，旧草稿正文也可能被覆盖。
//
// 归档区与 `chapter-drafts/` 同级：`app-meta/chapter-draft-kept/<chapter_id>/<stamp>/`，
// 永不被扫描成待恢复草稿，但磁盘上仍可完整访问。

use std::path::{Path, PathBuf};

use super::{is_safe_id, FACTS_DIR, META_FILE, TEXTS_DIR, TEXT_FILE};

/// 被保留（kept）草稿的独立归档根。
pub(super) fn kept_root(projects_root: &Path, project_id: &str) -> PathBuf {
    projects_root
        .join(project_id)
        .join("app-meta")
        .join("chapter-draft-kept")
}

/// 列出已归档的保留草稿章节 id：`kept_root` 下的一级子目录（章节 id）中，
/// 存在草稿内容标记的才算。归档布局是 `<chapter_id>/<stamp>/`，所以标记既
/// 可能直接落在章节目录，也可能落在某个 stamp 子目录。排序去重。
pub(super) fn list_kept_chapter_ids(projects_root: &Path, project_id: &str) -> Vec<String> {
    if !is_safe_id(project_id) {
        return Vec::new();
    }
    let root = kept_root(projects_root, project_id);
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            log::warn!("read kept chapter drafts dir {}: {error}", root.display());
            return Vec::new();
        }
    };
    let mut chapter_ids = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(chapter_id) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !is_safe_id(chapter_id) || !kept_dir_has_content(&path) {
            continue;
        }
        chapter_ids.push(chapter_id.to_string());
    }
    chapter_ids.sort();
    chapter_ids.dedup();
    chapter_ids
}

fn kept_dir_has_content(dir: &Path) -> bool {
    if has_draft_marker(dir) {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.is_dir() && has_draft_marker(&path)
    })
}

fn has_draft_marker(dir: &Path) -> bool {
    dir.join(META_FILE).is_file()
        || dir.join(FACTS_DIR).is_dir()
        || dir.join(TEXTS_DIR).is_dir()
        || dir.join(TEXT_FILE).is_file()
        || dir.join(super::super::ack::ACK_DIR).is_dir()
}
