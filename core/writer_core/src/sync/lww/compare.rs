//! #644 评论 5462823517 第3节：LWW 内容分类与三方比较。
//!
//! #644 评论 5473789298 第3节：纯分类/比较部分（`ContentClass`、
//! `classify_content_path`、`is_document_content_path`、`three_way_resolve`、
//! `ThreeWayResult`）提升为 [`crate::sync::content_class`]（始终可用，
//! 不依赖 `github-api` feature）。本模块只保留依赖 `ManifestFileRecord` 的
//! `PathDecision` / `resolve_path_decision`，供 `lww::mod` 在 feature gate 下调用。
//!
//! #644 评论 5473105049 第5节：逐路径三方/LWW 决策也收归本模块。
//! `resolve_path_decision` 对单个路径做三方或 LWW 决策，返回 `PathDecision`，
//! 由调用方（`mod.rs`）执行实际 IO（下载/上传/冲突副本保存）。

use crate::sync::content_class::{three_way_resolve, ThreeWayResult};
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
