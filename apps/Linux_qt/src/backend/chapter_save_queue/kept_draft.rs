// =============================================================================
// chapter_save_queue/kept_draft.rs — 恢复失败草稿的按章收口
// =============================================================================
//
// Issue #1062 评论 7195690539 第 1 项：恢复结果必须是明确的按章契约，而不是
// 三个计数器 + 一条 warn。所有“未能恢复”的分支只能通过 [`RestoreCounters::keep`]
// 收口，由它决定本章去向：
//   - 永久归档（需要人处理/外部已解决）：原件搬进 chapter-draft-kept；
//   - 可自动重试：归档 + chapter-draft-retry 标记，下次进入作品搬回活动槽位；
//   - 归档失败：章节进入 blocked 集合，QML 必须在打开该章前拒绝写入。

use std::path::Path;

use super::ChapterDraftRestoreStatus;

/// 一章草稿离开可写槽位后的去向。
pub(super) enum KeptDisposition {
    /// 需要人处理/已外部解决：原件进归档区，本次不再自动重试。
    Archive,
    /// 可自动重试：归档 + 重试标记，下次进入作品时搬回活动槽位再试。
    Retry {
        volume_id: String,
        reason: String,
        attempts: u32,
    },
}

/// 一次项目恢复的计数器与按章结论。
///
/// 所有 kept 分支只能通过 [`RestoreCounters::keep`] 收口，避免再次出现
/// “某条分支忘了隔离”。
#[derive(Default)]
pub(super) struct RestoreCounters {
    pub(super) restored: usize,
    pub(super) failed: usize,
    pub(super) kept: usize,
    /// 所有已离开可写槽位的章节（兼容旧字段）。
    pub(super) kept_chapters: Vec<String>,
    /// 已独立归档、本次不再自动恢复。
    pub(super) archived_chapters: Vec<String>,
    /// 已归档且登记了自动重试。
    pub(super) retry_chapters: Vec<String>,
    /// 归档失败/活动槽位冲突：原目录仍可能被新会话覆写，必须按章节阻断。
    pub(super) blocked_chapters: Vec<String>,
    /// 本章已有被延后的重试副本（活动槽位有新草稿，原件留在归档区）：
    /// 处理活动草稿时不能顺手清掉旧事务的重试标记。
    pub(super) deferred_retry: std::collections::BTreeSet<String>,
    pub(super) messages: Vec<String>,
}

impl RestoreCounters {
    pub(super) fn note(&mut self, chapter_id: &str, message: impl AsRef<str>) {
        self.messages
            .push(format!("{chapter_id}: {}", message.as_ref()));
    }

    /// 统一收口：把本轮的 kept 草稿搬出可写槽位。
    ///
    /// - 归档成功：记录归档/重试身份；
    /// - 目录已不存在（上一轮已处理）：不阻断；
    /// - 归档失败：原目录仍可被新会话覆写，必须把章节加入阻断集合，QML 在
    ///   打开该章之前拒绝写入。
    pub(super) fn keep(
        &mut self,
        projects_root: &Path,
        project_id: &str,
        chapter_id: &str,
        disposition: KeptDisposition,
    ) {
        let chapter_draft = crate::backend::chapter_draft;
        match chapter_draft::quarantine(projects_root, project_id, chapter_id) {
            Ok(Some(archived)) => {
                if !self.kept_chapters.iter().any(|id| id == chapter_id) {
                    self.kept_chapters.push(chapter_id.to_string());
                }
                log::info!(
                    "kept chapter draft archived: project={project_id} chapter={chapter_id} path={archived}"
                );
                match disposition {
                    KeptDisposition::Archive => {
                        // 延后的重试标记属于归档区里的旧事务，不是当前活动
                        // 草稿；归档当前草稿时不能把它清掉。
                        if !self.deferred_retry.contains(chapter_id) {
                            if let Err(error) =
                                chapter_draft::clear_retry(projects_root, project_id, chapter_id)
                            {
                                self.note(
                                    chapter_id,
                                    format!("clear retry marker failed: {error}"),
                                );
                            }
                        }
                        self.archived_chapters.push(chapter_id.to_string());
                    }
                    KeptDisposition::Retry {
                        volume_id,
                        reason,
                        attempts,
                    } => {
                        match chapter_draft::record_retry(
                            projects_root,
                            project_id,
                            chapter_id,
                            &volume_id,
                            &archived,
                            &reason,
                            attempts,
                        ) {
                            Ok(_) => self.retry_chapters.push(chapter_id.to_string()),
                            Err(error) => {
                                // 归档已完成，只是下次不再自动重试；原件仍安全。
                                self.note(
                                    chapter_id,
                                    format!("record retry marker failed: {error}"),
                                );
                            }
                        }
                    }
                }
            }
            Ok(None) => {
                // 目录已经不存在：没有可被新会话覆写的内容。
                self.note(chapter_id, "kept (no active draft dir to archive)");
            }
            Err(error) => {
                if !self.blocked_chapters.iter().any(|id| id == chapter_id) {
                    self.blocked_chapters.push(chapter_id.to_string());
                }
                self.note(
                    chapter_id,
                    format!("quarantine failed, chapter blocked: {error}"),
                );
            }
        }
    }

    /// terminal 决议落盘后收敛草稿；tombstone 写失败说明目录仍在活动槽位，
    /// 必须立即归档，不能让新会话与旧事务混写。
    pub(super) fn settle_removed(
        &mut self,
        projects_root: &Path,
        project_id: &str,
        chapter_id: &str,
    ) -> bool {
        match crate::backend::chapter_draft::remove(projects_root, project_id, chapter_id) {
            Ok(()) => {
                if let Err(error) = crate::backend::chapter_draft::clear_retry(
                    projects_root,
                    project_id,
                    chapter_id,
                ) {
                    self.note(chapter_id, format!("clear retry marker failed: {error}"));
                }
                true
            }
            Err(error) => {
                self.failed += 1;
                self.kept += 1;
                self.note(chapter_id, format!("settle_failed kept: {error}"));
                self.keep(
                    projects_root,
                    project_id,
                    chapter_id,
                    KeptDisposition::Archive,
                );
                false
            }
        }
    }

    pub(super) fn status(&self) -> ChapterDraftRestoreStatus {
        ChapterDraftRestoreStatus {
            pending: false,
            restored: self.restored,
            failed: self.failed,
            kept: self.kept,
            kept_chapters: self.kept_chapters.clone(),
            archived_chapters: self.archived_chapters.clone(),
            retry_chapters: self.retry_chapters.clone(),
            blocked_chapters: self.blocked_chapters.clone(),
            message: self.messages.join("; "),
        }
    }
}
