//! #644 评论 5473789298 第3节：内容分类与三方比较（始终可用）。
//!
//! 从 `lww/compare.rs` 提升为 `sync` 的直接子模块，让 `staging.rs`（不在
//! `github-api` feature gate 下）也能调用纯分类/比较逻辑。
//!
//! 本模块只包含**不依赖** `ManifestFileRecord` / `github_api_client` 的纯路径
//! 分类和哈希比较：
//! - [`ContentClass`] / [`classify_content_path`] / [`is_document_content_path`]
//! - [`three_way_resolve`] / [`ThreeWayResult`]
//!
//! 依赖 `ManifestFileRecord` 的 `PathDecision` / `resolve_path_decision` 仍留在
//! `lww/compare.rs`（在 `github-api` feature gate 下）。

/// 内容分类 — 决定同步策略。
///
/// - UserTextDocument：用户创作的文本（章节正文、笔记等），走三路比较，
///   BothChanged 时记录冲突，不静默覆盖。
/// - Metadata：项目/卷/章元数据 JSON，走 LWW 或逐键语义合并。
/// - LocalOnly：本地专用数据（备份、app-meta 内部文件），不同步。
/// - GeneratedCache：生成/缓存数据，LWW 可接受。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentClass {
    /// User-authored text: chapter.md, note.md, outline.md, scene.md, etc.
    /// Three-way merge on sync; never silently overwritten by LWW.
    UserTextDocument,
    /// Project/volume/chapter metadata JSON. LWW or semantic merge.
    Metadata,
    /// Local-only data (backups, app-meta internals). Never synced.
    LocalOnly,
    /// Generated or cache data. LWW is acceptable.
    GeneratedCache,
}

/// Classify a sync-root-relative path into a content category.
///
/// Uses suffix-based rules so it works for any project/volume/chapter ID.
/// The path is normalized to forward slashes before matching to ensure
/// correct behavior on Windows where local paths may contain backslashes.
pub(crate) fn classify_content_path(raw_path: &str) -> ContentClass {
    // Normalize backslashes to forward slashes for consistent matching on Windows
    let path = if raw_path.contains('\\') {
        std::borrow::Cow::Owned(raw_path.replace('\\', "/"))
    } else {
        std::borrow::Cow::Borrowed(raw_path)
    };

    // Local-only directories
    if path.starts_with("backups/") || path.starts_with("app-meta/") {
        return ContentClass::LocalOnly;
    }

    // User text documents: any .md file under /chapters/, plus
    // note.md, outline.md, scene.md, character_notes.md, timeline_notes.md
    // anywhere in the sync root
    if path.ends_with(".md") {
        if path.contains("/chapters/") {
            return ContentClass::UserTextDocument;
        }
        let filename = path.rsplit('/').next().unwrap_or(&path);
        if matches!(
            filename,
            "note.md"
                | "outline.md"
                | "scene.md"
                | "character_notes.md"
                | "timeline_notes.md"
                | "draft.md"
        ) {
            return ContentClass::UserTextDocument;
        }
        return ContentClass::GeneratedCache;
    }

    // Metadata JSON files
    if path.ends_with(".json") {
        let filename = path.rsplit('/').next().unwrap_or(&path);
        if matches!(
            filename,
            "project.json"
                | "volume.json"
                | "chapter.meta.json"
                | "settings.sync.json"
                | "starmap.json"
                | "writing_stats.json"
        ) {
            return ContentClass::Metadata;
        }
    }

    ContentClass::GeneratedCache
}

/// `classify_content_path == UserTextDocument` 的快捷判断。
/// 用于在同步流程中快速识别需要走三路比较的正文类文件。
pub(crate) fn is_document_content_path(path: &str) -> bool {
    classify_content_path(path) == ContentClass::UserTextDocument
}

/// 基于内容哈希的三路比较。
///
/// 以 `base_hash` 作为双方上次同步后的共识版本，比较 local 和 remote
/// 各自是否相对 base 发生了变化。用于 UserTextDocument 类型的冲突检测：
/// 仅一方修改时直接取修改方；双方都修改时返回 BothChanged，需走冲突解决流程。
///
/// 不变量：
/// - local_hash == remote_hash 时一定返回 NoConflict（即使两者都 != base），
///   因为内容相同无需选择。
/// - 三路比较仅用于 UserTextDocument；Metadata/GeneratedCache 走 LWW 时间戳决胜。
/// - LWW 决胜不变量：时间戳较大方获胜；时间戳相同时按 device_id 字典序决胜
///   （字典序较大的 device_id 获胜），保证双方独立计算结果一致。
pub(crate) fn three_way_resolve(
    base_hash: &str,
    local_hash: &str,
    remote_hash: &str,
) -> ThreeWayResult {
    if local_hash == remote_hash {
        return ThreeWayResult::NoConflict;
    }
    if local_hash == base_hash && remote_hash != base_hash {
        return ThreeWayResult::RemoteChanged;
    }
    if local_hash != base_hash && remote_hash == base_hash {
        return ThreeWayResult::LocalChanged;
    }
    if local_hash != base_hash && remote_hash != base_hash {
        return ThreeWayResult::BothChanged;
    }
    ThreeWayResult::NoConflict
}

/// 基于内容哈希的三路比较结果。
///
/// base 是上次同步后双方共识的文件版本哈希。
/// 通过比较 local/remote 与 base 的差异判断冲突情况。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreeWayResult {
    /// 双方相同，或双方均未修改
    NoConflict,
    /// 仅本地修改，远端未变 → 上传本地版本
    LocalChanged,
    /// 仅远端修改，本地未变 → 下载远端版本
    RemoteChanged,
    /// 双方均修改 → 需要冲突解决策略
    BothChanged,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_document_paths() {
        assert_eq!(
            classify_content_path("volumes/v1/chapters/c1/chapter.md"),
            ContentClass::UserTextDocument
        );
        assert_eq!(
            classify_content_path("note.md"),
            ContentClass::UserTextDocument
        );
        assert_eq!(
            classify_content_path("volumes/v1/outline.md"),
            ContentClass::UserTextDocument
        );
    }

    #[test]
    fn classify_metadata_paths() {
        assert_eq!(
            classify_content_path("project.json"),
            ContentClass::Metadata
        );
        assert_eq!(
            classify_content_path("volumes/v1/volume.json"),
            ContentClass::Metadata
        );
    }

    #[test]
    fn classify_local_only_paths() {
        assert_eq!(
            classify_content_path("backups/x.md"),
            ContentClass::LocalOnly
        );
        assert_eq!(
            classify_content_path("app-meta/sync/state.local.json"),
            ContentClass::LocalOnly
        );
    }

    #[test]
    fn three_way_no_conflict_when_equal() {
        assert_eq!(
            three_way_resolve("h1", "h2", "h2"),
            ThreeWayResult::NoConflict
        );
    }

    #[test]
    fn three_way_both_changed() {
        assert_eq!(
            three_way_resolve("h1", "h2", "h3"),
            ThreeWayResult::BothChanged
        );
    }
}
