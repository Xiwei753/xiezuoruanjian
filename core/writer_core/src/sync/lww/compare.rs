//!   ：LWW 内容分类与三方比较。
//!
//!   ：纯分类/比较部分（`ContentClass`、
//! `classify_content_path`、`is_document_content_path`、`three_way_resolve`、
//! `ThreeWayResult`）提升为 [`crate::sync::content_class`]（始终可用，
//! 不依赖 `github-api` feature）。本模块只保留依赖 `ManifestFileRecord` 的
//! `PathDecision` / `resolve_path_decision`，供 `lww::mod` 在 feature gate 下调用。
//!
//!   ：逐路径三方/LWW 决策也收归本模块。
//! `resolve_path_decision` 对单个路径做三方或 LWW 决策，返回 `PathDecision`，
//! 由调用方（`mod.rs`）执行实际 IO（下载/上传/冲突副本保存）。

use crate::sync::content_class::{
    lww_record_from_manifest, resolve_lww, three_way_resolve, LwwWinner, ThreeWayResult,
};
use crate::sync::types::ManifestFileRecord;

/// 单个路径的同步决策结果。
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

/// 单个路径的完整决策结果。
pub(super) struct PathResolution {
    pub decision: PathDecision,
    /// LWW 决胜确实覆盖了另一侧内容（仅双方相对基线都有变化时成立）。
    pub overwritten: bool,
    /// 双方相对 `known_files` 基线都发生了真实变化，由 LWW 自动裁决。
    ///
    /// 单侧上传/下载/删除不是冲突裁决；没有基线时也无法证明双方都改过，
    /// 因此这两种情况都必须为 `false`。
    pub auto_adjudicated: bool,
}

impl PathResolution {
    fn plain(decision: PathDecision) -> Self {
        Self {
            decision,
            overwritten: false,
            auto_adjudicated: false,
        }
    }
}

/// 对单个路径做三方或 LWW 决策。
///
/// 纯计算，不执行 IO。
///
/// `base_hash` 来自 `state.known_files`（三路比较基准，LWW 也用它判断
/// 双方是否都相对基线发生了变化）。`is_document` 表示是否为
/// UserTextDocument（走三路比较）。
pub(super) fn resolve_path_decision(
    local_rec: &ManifestFileRecord,
    remote_rec: &ManifestFileRecord,
    base_hash: &str,
    is_document: bool,
) -> PathResolution {
    if is_document {
        resolve_document_path(local_rec, remote_rec, base_hash)
    } else {
        resolve_lww_path(local_rec, remote_rec, base_hash)
    }
}

/// 正文文件的三路比较决策。
fn resolve_document_path(
    local_rec: &ManifestFileRecord,
    remote_rec: &ManifestFileRecord,
    base_hash: &str,
) -> PathResolution {
    let local_hash = &local_rec.content_hash;
    let remote_hash = &remote_rec.content_hash;

    match three_way_resolve(base_hash, local_hash, remote_hash) {
        ThreeWayResult::NoConflict => PathResolution::plain(PathDecision::NoOp),
        ThreeWayResult::LocalChanged => PathResolution::plain(PathDecision::UploadLocal),
        ThreeWayResult::RemoteChanged => {
            if remote_rec.op == "upsert" {
                PathResolution::plain(PathDecision::DownloadRemote)
            } else if remote_rec.op == "delete" {
                PathResolution::plain(PathDecision::DeleteLocal)
            } else {
                PathResolution::plain(PathDecision::NoOp)
            }
        }
        ThreeWayResult::BothChanged => {
            if remote_rec.op == "delete" {
                PathResolution::plain(PathDecision::DocumentConflictRemoteDeleted)
            } else {
                PathResolution::plain(PathDecision::DocumentConflictBothChanged)
            }
        }
    }
}

/// Metadata/GeneratedCache 的 LWW 时间戳决胜。
///
/// 只有 `base_hash` 存在、且本地与远端记录都相对基线发生变化时，
/// 才把这次决胜标记为 `auto_adjudicated`（真正的双方分歧自动裁决）。
/// 单侧变化是普通上传/下载；没有基线时不能宣称发生过冲突裁决。
fn resolve_lww_path(
    local_rec: &ManifestFileRecord,
    remote_rec: &ManifestFileRecord,
    base_hash: &str,
) -> PathResolution {
    let local = lww_record_from_manifest(local_rec);
    let remote = lww_record_from_manifest(remote_rec);
    let has_baseline = !base_hash.is_empty();
    let local_changed =
        has_baseline && (local_rec.op == "delete" || local_rec.content_hash != base_hash);
    let remote_changed =
        has_baseline && (remote_rec.op == "delete" || remote_rec.content_hash != base_hash);
    // 双方都相对基线变化，且变化结果确实不同，才算真正的分歧；双方独立
    // 得到相同内容或都删除同一文件不属于冲突裁决。
    let diverged = local_rec.op != remote_rec.op
        || local_rec.content_hash != remote_rec.content_hash;
    let both_changed = local_changed && remote_changed && diverged;

    match resolve_lww(&local, &remote) {
        LwwWinner::Tie => PathResolution::plain(PathDecision::NoOp),
        LwwWinner::Remote => {
            if remote_rec.op == "upsert" {
                PathResolution {
                    decision: PathDecision::LwwRemoteWinsDownload,
                    overwritten: both_changed,
                    auto_adjudicated: both_changed,
                }
            } else if remote_rec.op == "delete" {
                PathResolution {
                    decision: PathDecision::LwwRemoteWinsDelete,
                    overwritten: both_changed,
                    auto_adjudicated: both_changed,
                }
            } else {
                PathResolution::plain(PathDecision::NoOp)
            }
        }
        LwwWinner::Local => {
            if local_rec.op == "upsert" {
                PathResolution {
                    decision: PathDecision::LwwLocalWinsUpload,
                    overwritten: both_changed,
                    auto_adjudicated: both_changed,
                }
            } else if local_rec.op == "delete" {
                PathResolution {
                    decision: PathDecision::LwwLocalWinsDeleteRecord,
                    overwritten: both_changed,
                    auto_adjudicated: both_changed,
                }
            } else {
                PathResolution::plain(PathDecision::NoOp)
            }
        }
    }
}
