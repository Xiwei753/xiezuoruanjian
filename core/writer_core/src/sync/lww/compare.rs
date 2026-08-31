//! #644 评论 5462823517 第3节：LWW 内容分类与三方比较。
//!
//! 从 lww.rs 抽出的纯比较逻辑：ContentClass、classify_content_path、
//! is_document_content_path、three_way_resolve、ThreeWayResult。
//! full sync Commit 与 GitHub LWW 都调用本模块（共享三方冲突语义）。
//!
//! #644 评论 5473105049 第5节：逐路径三方/LWW 决策也收归本模块。
//! `resolve_path_decision` 对单个路径做三方或 LWW 决策，返回 `PathDecision`，
//! 由调用方（`mod.rs`）执行实际 IO（下载/上传/冲突副本保存）。

use crate::sync::types::ManifestFileRecord;

/// #644 评论 5473105049 第5节：单个路径的同步决策结果。
///
/// 纯计算结果，不含 IO 副作用。调用方根据决策执行实际的下载/上传/冲突处理。
pub(super) enum PathDecision {
    /// 本地独有，上传到远端。
    UploadLocal,
    /// 远端独有（upsert），下载到本地。
    DownloadRemote,
    /// 远端独有（delete），删除本地文件。
    DeleteLocal,
    /// 两边都没变或内容相同，无需操作。
    NoOp,
    /// LWW 远端获胜：下载远端 upsert。
    LwwRemoteWinsDownload,
    /// LWW 远端获胜：远端 delete，删除本地。
    LwwRemoteWinsDelete,
    /// LWW 本地获胜：上传本地 upsert。
    LwwLocalWinsUpload,
    /// LWW 本地获胜：本地 delete，记录远端删除。
    LwwLocalWinsDeleteRecord,
    /// 正文三路冲突：双方都改了，需要保存冲突副本。
    DocumentConflictBothChanged,
    /// 正文三路冲突：远端删除了，本地改了。
    DocumentConflictRemoteDeleted,
}

/// #644 评论 5473105049 第5节：对单个路径做三方或 LWW 决策。
///
/// 纯计算，不执行 IO。返回 `PathDecision` 和是否需要标记 overwritten。
///
/// `base_hash` 来自 `state.known_files`（三路比较基准）。
/// `is_document` 表示是否为 UserTextDocument（走三路比较）。
pub(super) fn resolve_path_decision(
    local_rec: &ManifestFileRecord,
    remote_rec: &ManifestFileRecord,
    base_hash: &str,
    is_document: bool,
) -> (PathDecision, bool) {
    if is_document {
        resolve_document_path(local_rec, remote_rec, base_hash)
    } else {
        resolve_lww_path(local_rec, remote_rec)
    }
}

/// 正文文件的三路比较决策。
fn resolve_document_path(
    local_rec: &ManifestFileRecord,
    remote_rec: &ManifestFileRecord,
    base_hash: &str,
) -> (PathDecision, bool) {
    let local_hash = &local_rec.content_hash;
    let remote_hash = &remote_rec.content_hash;

    match three_way_resolve(base_hash, local_hash, remote_hash) {
        ThreeWayResult::NoConflict => (PathDecision::NoOp, false),
        ThreeWayResult::LocalChanged => (PathDecision::UploadLocal, false),
        ThreeWayResult::RemoteChanged => {
            if remote_rec.op == "upsert" {
                (PathDecision::DownloadRemote, false)
            } else if remote_rec.op == "delete" {
                (PathDecision::DeleteLocal, false)
            } else {
                (PathDecision::NoOp, false)
            }
        }
        ThreeWayResult::BothChanged => {
            if remote_rec.op == "delete" {
                (PathDecision::DocumentConflictRemoteDeleted, false)
            } else {
                (PathDecision::DocumentConflictBothChanged, false)
            }
        }
    }
}

/// Metadata/GeneratedCache 的 LWW 时间戳决胜。
fn resolve_lww_path(
    local_rec: &ManifestFileRecord,
    remote_rec: &ManifestFileRecord,
) -> (PathDecision, bool) {
    use super::manifest::lww_record_time;

    let local_time = lww_record_time(local_rec);
    let remote_time = lww_record_time(remote_rec);
    let mut remote_wins = false;

    if remote_time > local_time {
        remote_wins = true;
    } else if remote_time == local_time {
        if remote_rec.content_hash == local_rec.content_hash && remote_rec.op == local_rec.op {
            return (PathDecision::NoOp, false);
        }
        remote_wins = remote_rec.device_id > local_rec.device_id;
    }

    if remote_wins {
        let overwritten =
            local_rec.op == "delete" || local_rec.content_hash != remote_rec.content_hash;
        if remote_rec.op == "upsert" {
            (PathDecision::LwwRemoteWinsDownload, overwritten)
        } else if remote_rec.op == "delete" {
            let overwritten = local_rec.op == "upsert";
            (PathDecision::LwwRemoteWinsDelete, overwritten)
        } else {
            (PathDecision::NoOp, false)
        }
    } else {
        let overwritten =
            remote_rec.op == "delete" || remote_rec.content_hash != local_rec.content_hash;
        if local_rec.op == "upsert" {
            (PathDecision::LwwLocalWinsUpload, overwritten)
        } else if local_rec.op == "delete" {
            let overwritten = remote_rec.op == "upsert";
            (PathDecision::LwwLocalWinsDeleteRecord, overwritten)
        } else {
            (PathDecision::NoOp, false)
        }
    }
}

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
pub(super) fn three_way_resolve(
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
pub(super) enum ThreeWayResult {
    /// 双方相同，或双方均未修改
    NoConflict,
    /// 仅本地修改，远端未变 → 上传本地版本
    LocalChanged,
    /// 仅远端修改，本地未变 → 下载远端版本
    RemoteChanged,
    /// 双方均修改 → 需要冲突解决策略
    BothChanged,
}
