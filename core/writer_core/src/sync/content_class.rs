//!   ：内容分类与三方比较（始终可用）。
//!
//! 从 `lww/compare.rs` 提升为 `sync` 的直接子模块，让 `staging.rs`（不在
//! `github-api` feature gate 下）也能调用纯分类/比较逻辑。
//!
//! 本模块包含不依赖 `github_api_client` 的纯路径分类、哈希比较和 LWW 决策：
//! - [`ContentClass`] / [`classify_content_path`] / [`is_document_content_path`]
//! - [`three_way_resolve`] / [`ThreeWayResult`]
//! - [`LwwRecord`] / [`LwwWinner`] / [`resolve_lww`] / [`lww_record_time`]
//! - [`lww_record_from_manifest`]：把持久 `ManifestFileRecord` 投影为统一 LWW 记录
//!
//! `PathDecision` / `resolve_path_decision` 仍在 `lww/compare.rs`，但其 LWW
//! 分支只把两侧记录投影为 [`LwwRecord`] 后调用 [`resolve_lww`]，不再保留第二套
//! 时间戳比较。staging commit 同样复用这套记录与决策。

use crate::sync::types::ManifestFileRecord;

/// 内容分类 — 决定同步策略。
///
/// - UserTextDocument：用户创作的文本（章节正文、笔记等），走三路比较，
///   BothChanged 时记录冲突，不静默覆盖。
/// - Metadata：项目/卷/章元数据 JSON，走 LWW 或逐键语义合并。
/// - LocalOnly：本地专用数据（备份、app-meta 内部文件），不同步。
/// - StarMap：星图业务对象（index.json、graph.json、meta、对象文件）。
///   index/graph 走按对象 ID 的三方集合合并；对象文件走 LWW。
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
    /// StarMap business objects (index.json, graph.json, meta, object files).
    /// Semantic three-way merge by object ID for index/graph; LWW for object files.
    StarMap,
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

    // StarMap business objects: semantic merge for index/graph, LWW for object files.
    // 覆盖 starmaps/ 下所有 .json 文件（index.json、graph.json、meta.json、
    // 对象文件、recovery.json）。index.json 和 graph.json 在 compute_commit_plan
    // 中走按对象 ID 的三方集合合并；其余走 LWW（issue #902 第八轮复核）。
    if path.starts_with("starmaps/") && path.ends_with(".json") {
        return ContentClass::StarMap;
    }

    // Each annotation record is an independent JSON file and uses ordinary
    // metadata LWW; there is no project-wide annotations snapshot to overwrite.
    if path.starts_with("annotations/")
        && path.ends_with(".json")
        && path.split('/').nth(1).is_some_and(|collection| {
            matches!(
                collection,
                "tags" | "bindings" | "entries" | "anchors" | "links"
            )
        })
    {
        return ContentClass::Metadata;
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

// ── 纯 LWW 决策 ──

/// 轻量 LWW 比较记录 — 不依赖 `ManifestFileRecord`，始终可用。
///
/// `lww/compare.rs::resolve_lww_path` 有真正 LWW（时间戳 + device_id 决胜），
/// 但依赖 `ManifestFileRecord` 且在 `github-api` feature gate 下。staging commit
/// （无 feature gate）拿不到。本结构体提供纯 LWW 决策所需的最小字段集，
/// 让 staging commit 能做真正 LWW 而非固定 remote-wins。
///
/// 字段语义与 `ManifestFileRecord` 对应字段一致：
/// - `content_hash`：MD5 hex 摘要
/// - `updated_at_ms`：upsert 的 LWW 时间戳
/// - `deleted_at_ms`：delete 的精确删除时间（优先于 `updated_at_ms`）
/// - `device_id`：时间戳相同时字典序决胜
/// - `op`：`"upsert"` 或 `"delete"`
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LwwRecord {
    pub content_hash: String,
    pub updated_at_ms: i64,
    pub deleted_at_ms: Option<i64>,
    pub device_id: String,
    pub op: String,
}

/// LWW 决胜结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LwwWinner {
    /// 本地获胜（保留 local，不应用 incoming）。
    Local,
    /// 远端获胜（应用 incoming）。
    Remote,
    /// 双方内容相同且操作相同，无需动作。
    Tie,
}

/// 把持久 `ManifestFileRecord` 投影为统一的 LWW 记录。
///
/// 同步的版本事实只来自 manifest（`updated_at_ms` / `deleted_at_ms` /
/// `device_id`），不能在这一步用当前文件系统 mtime 或扫描时间覆盖。
pub(crate) fn lww_record_from_manifest(record: &ManifestFileRecord) -> LwwRecord {
    LwwRecord {
        content_hash: record.content_hash.clone(),
        updated_at_ms: record.updated_at_ms,
        deleted_at_ms: record.deleted_at_ms,
        device_id: record.device_id.clone(),
        op: record.op.clone(),
    }
}

/// 获取 LWW 比较时间戳（字段级纯函数，供各 record 类型复用）。
///
/// 对于 delete 操作，优先使用 `deleted_at_ms`（精确的删除时间），
/// 回退到 `updated_at_ms`（删除操作记录的更新时间）。
/// 对于 upsert 操作，直接使用 `updated_at_ms`。
pub(crate) fn lww_effective_time(op: &str, updated_at_ms: i64, deleted_at_ms: Option<i64>) -> i64 {
    if op == "delete" {
        deleted_at_ms.unwrap_or(updated_at_ms)
    } else {
        updated_at_ms
    }
}

/// 获取 LWW 比较时间戳。
pub(crate) fn lww_record_time(record: &LwwRecord) -> i64 {
    lww_effective_time(&record.op, record.updated_at_ms, record.deleted_at_ms)
}

/// 纯 LWW 决策 — 时间戳较大方获胜；同时间 device_id 字典序决胜。
///
/// 与 `lww/compare.rs::resolve_lww_path` 的决策规则完全一致，但不依赖
/// `ManifestFileRecord`，让 staging commit（无 `github-api` feature gate）
/// 也能做真正 LWW。
///
/// 不变量：
/// - 时间戳较大方获胜
/// - 时间戳相同时：内容相同且操作相同 → `Tie`；否则字典序较大的 device_id 获胜
/// - 决策结果与 `resolve_lww_path` 一致，保证 GitHub API 内层和 staging 外层
///   不会出现两套相反的 Metadata 语义
pub(crate) fn resolve_lww(local: &LwwRecord, remote: &LwwRecord) -> LwwWinner {
    let local_time = lww_record_time(local);
    let remote_time = lww_record_time(remote);

    if remote_time > local_time {
        LwwWinner::Remote
    } else if remote_time < local_time {
        LwwWinner::Local
    } else {
        // 时间戳相同
        if remote.content_hash == local.content_hash && remote.op == local.op {
            LwwWinner::Tie
        } else if remote.device_id > local.device_id {
            LwwWinner::Remote
        } else {
            LwwWinner::Local
        }
    }
}

#[cfg(test)]
mod tests;
