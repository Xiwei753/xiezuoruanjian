//! 章节保存入口：把正文、元数据与附加文件交给统一的版本化提交协议。
//!
//! 这里不再维护第二套 `SaveTransaction` 备份/恢复路径。所有真实写入、
//! no-op 判定、安全空覆盖、版本推进和附加文件发布都由
//! [`super::commit::commit_chapter`] 完成；本模块只负责把结果转成既有
//! `ChapterSaveOutcome`，供 change set 与 API 回执使用。

use super::*;
use crate::error::Error;

/// 一次章节保存事务的真实写入结果。
///
/// `written_files` 是 project-relative 路径，只包含事务真正写入的文件；
/// 为空表示正文、元数据与附加文件都没有实质变化（no-op）。变更集由它构建，
/// 不再通过保存后的读写比较猜路径。
pub(crate) struct ChapterSaveOutcome {
    pub receipt: ChapterSaveReceipt,
    pub written_files: Vec<String>,
}

fn project_id_for(project_root: &Path) -> String {
    project_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

/// Save the chapter and additional project-local records through the single
/// versioned commit protocol.
///
/// `workspace_lock` 必须是调用方已持有的跨进程工作区写锁；直接调用
/// （测试/导入）可传 `None`，但生产写入入口必须传入。
#[allow(clippy::too_many_arguments)]
pub(crate) fn save_chapter_verified_with_options_and_files(
    project_root: &Path,
    workspace_lock: Option<&crate::storage::WorkspaceWriteLock>,
    volume_id: &str,
    chapter_id: &str,
    content: &str,
    allow_empty_overwrite: bool,
    additional_files: &[(String, String)],
    expected_version: Option<ChapterVersion>,
    base_content: Option<&str>,
    projection_ids: &[String],
    proof_events: &[super::commit::ChapterProofProjection],
    proof_snapshot_revision: Option<u64>,
) -> Result<ChapterSaveOutcome> {
    let key = ChapterKey::new(
        project_id_for(project_root),
        volume_id.to_string(),
        chapter_id.to_string(),
    );
    let mut request = ChapterCommitRequest::new(key, content);
    request.allow_empty_overwrite = allow_empty_overwrite;
    request.additional_files = additional_files.to_vec();
    request.expected = expected_version;
    request.base_content = base_content.map(str::to_string);
    request.projection_ids = projection_ids.to_vec();
    request.proof_events = proof_events.to_vec();
    request.proof_snapshot_revision = proof_snapshot_revision;
    request.event_ids = proof_events
        .iter()
        .map(|event| event.event_id().to_string())
        .collect();
    request.source = ChapterCommitSource::Api;
    request.mutation = if content.is_empty() {
        ChapterMutationKind::Clear
    } else {
        ChapterMutationKind::Replace
    };

    match super::commit::commit_chapter_with_guard(project_root, workspace_lock, request)? {
        ChapterCommitOutcome::Committed(receipt) => {
            let path = receipt.key.body_relative_path().to_string();
            Ok(ChapterSaveOutcome {
                written_files: receipt.written_files.clone(),
                receipt: receipt.into_save_receipt(path),
            })
        }
        ChapterCommitOutcome::Conflict(conflict) => Err(Error::ChapterVersionConflict {
            chapter_id: chapter_id.to_string(),
            transaction_id: conflict.transaction_id,
            expected_hash: conflict
                .expected
                .as_ref()
                .map(|version| version.content_sha256.clone())
                .unwrap_or_default(),
            actual_hash: conflict.actual.content_sha256,
            conflict_dir: conflict.conflict_dir,
        }),
    }
}
