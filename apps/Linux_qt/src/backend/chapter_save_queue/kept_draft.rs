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
use crate::backend::chapter_draft;

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
    pub(super) messages: Vec<String>,
}

impl RestoreCounters {
    pub(super) fn note(&mut self, chapter_id: &str, message: impl AsRef<str>) {
        self.messages
            .push(format!("{chapter_id}: {}", message.as_ref()));
    }

    pub(super) fn block(&mut self, chapter_id: &str) {
        if !self.blocked_chapters.iter().any(|id| id == chapter_id) {
            self.blocked_chapters.push(chapter_id.to_string());
        }
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
        // Issue #1062 评论 #7210360212 第 2 项：先写“是否要自动重试”的意图，
        // 再让 quarantine 把意图与草稿原件一起 rename；外部索引在 rename 之后
        // 才写，崩溃也不会丢掉自动恢复引用。人工归档前先清掉遗留意图。
        //
        // Issue #1062 评论 #7213128711 第 2 项：持久意图是归档的前置决议——
        // 写入/清除失败时保持草稿在活动槽位并阻断该章，不能先搬进只读归档
        // 再丢恢复指针。
        match &disposition {
            KeptDisposition::Archive => {
                if let Err(error) =
                    chapter_draft::clear_retry_intent(projects_root, project_id, chapter_id)
                {
                    self.failed += 1;
                    self.kept += 1;
                    self.block(chapter_id);
                    self.note(
                        chapter_id,
                        format!("clear retry intent failed, chapter blocked: {error}"),
                    );
                    return;
                }
            }
            KeptDisposition::Retry {
                volume_id,
                reason,
                attempts,
            } => {
                if let Err(error) = chapter_draft::write_retry_intent(
                    projects_root,
                    project_id,
                    chapter_id,
                    volume_id,
                    reason,
                    *attempts,
                ) {
                    self.failed += 1;
                    self.kept += 1;
                    self.block(chapter_id);
                    self.note(
                        chapter_id,
                        format!("write retry intent failed, chapter blocked: {error}"),
                    );
                    return;
                }
            }
        }
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
                        // Issue #1062 评论 #7210163429 第 1 项：归档**当前**草稿
                        // 与同章其它归档事务的重试标记无关；这里不再按
                        // chapter_id 清理任何标记。
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
                            std::path::Path::new(&archived),
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
    ///
    /// Issue #1062 评论 #7210163429 第 1 项：结算当前草稿只删除它自己的活动
    /// 目录；同章其它归档事务（含延后重试的 A）的标记必须原样保留，下一次
    /// 进入项目时继续 materialize。
    pub(super) fn settle_removed(
        &mut self,
        projects_root: &Path,
        project_id: &str,
        chapter_id: &str,
    ) -> bool {
        match crate::backend::chapter_draft::remove(projects_root, project_id, chapter_id) {
            Ok(()) => true,
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

    /// 带磁盘剩余事务计数的状态：`pending=false` 只代表本轮结束。
    pub(super) fn status_with(
        &self,
        remaining: usize,
        deferred: usize,
        scan_incomplete: bool,
    ) -> ChapterDraftRestoreStatus {
        ChapterDraftRestoreStatus {
            running: false,
            remaining,
            deferred,
            conflicted: self.blocked_chapters.len(),
            scan_incomplete,
            // 扫描不完整时项目级恢复仍未决：QML 必须继续等待一次完整发现，
            // 不能把已知章节当成“全部草稿”。
            pending: scan_incomplete,
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
