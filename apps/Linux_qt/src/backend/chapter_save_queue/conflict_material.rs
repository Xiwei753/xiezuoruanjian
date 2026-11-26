// =============================================================================
// chapter_save_queue/conflict_material.rs — 冲突材料的只读定位
// =============================================================================

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{ChapterSaveRequest, CONFLICT_MATERIAL_WINDOW};

/// 在 Core 的章节提交冲突材料目录里定位本次快照刚写入的 `incoming.md`。
///
/// 目录布局由 Core 持有（`app-meta/chapter-commits/conflicts/<transaction_id>`），
/// 这里只读、只用于把“冲突正文已持久化”的路径回给 QML，不参与任何业务决议。
pub(super) fn find_conflict_material(request: &ChapterSaveRequest) -> String {
    let root = Path::new(&request.context.projects_root)
        .join(&request.identity.project_id)
        .join("app-meta")
        .join("chapter-commits")
        .join("conflicts");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return String::new();
    };
    let mut candidates: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect();
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    let Some(since) = SystemTime::now().checked_sub(CONFLICT_MATERIAL_WINDOW) else {
        return String::new();
    };
    for (modified, path) in candidates.into_iter().take(8) {
        if modified < since {
            break;
        }
        let Ok(bytes) = std::fs::read(path.join("incoming.md")) else {
            continue;
        };
        if bytes == request.content.as_bytes() {
            return path.to_string_lossy().into_owned();
        }
    }
    // 找不到目录不代表冲突正文没落盘（可能有清理/权限问题），调用方按
    // Core 的 CHAPTER_VERSION_CONFLICT 语义把冲突材料视为已持久化即可。
    String::new()
}
