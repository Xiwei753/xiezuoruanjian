// =============================================================================
// sync_operations.rs — 同步执行逻辑
// =============================================================================
//
// 引用了什么：
// - super::*：引入 AppBackend 核心后端的全部方法与结构体。
// - crate::sync_bridge：引入 SyncTaskOutcome、mask_sync_error 等同步工具函数。
// - writer_core::api::WriterCoreApi：核心库对外的统一 API 入口。
//
// 干什么的：
// - 实现 AppBackend 上的同步执行方法：perform_sync、perform_sync_dry_run、perform_sync_internal。
// - 实现同步结果处理：handle_sync_outcome、handle_sync_content_refresh。
// - 所有同步操作通过 UUID operation_id 机制保证并发安全，通过 QPointer + queued_callback 实现线程安全回调。
//
// 被什么引用：
// - 被 apps/Linux_qt/src/backend/app_backend/sync_backend.rs 中的 SyncBackend QObject 间接调用。
// =============================================================================

use super::SyncBackend;
use super::*;
use crate::sync_bridge::{mask_sync_error, sync_error_category_from_code, SyncTaskOutcome};

/// 同步结果对当前工作区内容的影响。
///
/// `handle_sync_outcome` 返回此枚举，调用方（SyncBackend::handle_outcome）
/// 据此决定是否发 `sync_content_applied` signal。
/// - `ContentChanged`：同步确实修改/重新加载了工作区内容（success、
///   branch_missing_recovered、冲突后 tree reload），QML 需刷新正文/树。
/// - `StatusOnly`：只更新同步状态，不刷新工作区内容（过期回调、diagnostics、
///   dry_run、配置保存、error 等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SyncOutcomeEffect {
    StatusOnly,
    ContentChanged,
}

/// 构造异步同步结果的 callback。
///
/// 如果提供 `sync_qptr`（来自 SyncBackend），callback 通过 SyncBackend::handle_outcome
/// 进入，走 with_app_mut 刷新 DomainSnapshot 并发 SyncBackend signal。
/// 否则（测试场景）回退到 QPointer<AppBackend> 直接 handle_sync_outcome。
pub(super) fn make_outcome_callback(
    app_qptr: QPointer<AppBackend>,
    sync_qptr: Option<QPointer<SyncBackend>>,
) -> Box<dyn FnOnce(SyncTaskOutcome) + Send> {
    if let Some(sq) = sync_qptr {
        Box::new(qmetaobject::queued_callback(
            move |outcome: SyncTaskOutcome| {
                sq.as_pinned().map(|this| {
                    let mut this = this.borrow_mut();
                    this.handle_outcome(outcome);
                });
            },
        ))
    } else {
        Box::new(qmetaobject::queued_callback(
            move |outcome: SyncTaskOutcome| {
                app_qptr.as_pinned().map(|this| {
                    let mut this = this.borrow_mut();
                    // 测试回退路径：无 SyncBackend，effect 无消费者，忽略返回值。
                    let _ = this.handle_sync_outcome(outcome, None);
                });
            },
        ))
    }
}

// 同步执行按关注点拆成三个子模块：
//   - gc.rs：generation GC maintenance 后台线程
//   - dry_run.rs：预演（只算不落盘）
//   - perform.rs：真实执行 perform_sync_internal
// 本文件保留同步结果处理、perform_sync 入口与共享的 format_conflict_files。
mod dry_run;
mod gc;
mod perform;

impl AppBackend {
    pub(crate) fn handle_sync_outcome(
        &mut self,
        outcome: SyncTaskOutcome,
        sync_qptr: Option<QPointer<SyncBackend>>,
    ) -> SyncOutcomeEffect {
        if outcome.operation_id != self.current_sync_operation_id {
            self.debug_log(
                "sync",
                "sync_outcome_discarded",
                &format!(
                    "Discarded outdated outcome. Expected: {}, got: {}",
                    self.current_sync_operation_id, outcome.operation_id
                ),
            );
            return SyncOutcomeEffect::StatusOnly;
        }

        // Issue #729：workspace generation 身份校验。
        // 同步线程启动时捕获当时的 generation 并填入 outcome。若回调到达时
        // current_workspace_generation 已变（切工作区/reset_workspace_state 递增），
        // 说明此结果属于旧工作区，必须丢弃，避免旧同步污染新工作区状态。
        if outcome.workspace_generation != self.current_workspace_generation {
            self.debug_log(
                "sync",
                "sync_outcome_discarded_workspace_changed",
                &format!(
                    "Discarded outcome from stale workspace. expected_gen={}, got_gen={}",
                    self.current_workspace_generation, outcome.workspace_generation
                ),
            );
            // 旧工作区的回调不应清新工作区的 in_progress（reset_workspace_state 已清）。
            return SyncOutcomeEffect::StatusOnly;
        }

        // Issue #729 评论 5763441474：data_root 身份校验。
        // 同步线程启动时捕获当时的 data_root 并填入 outcome。若回调到达时
        // current_data_root 已变（切工作区），说明此结果属于旧工作区，必须丢弃。
        // operation_id + workspace_generation + data_root 三者同时匹配才接受结果。
        if outcome.data_root != self.current_data_root {
            self.debug_log(
                "sync",
                "sync_outcome_discarded_data_root_changed",
                &format!(
                    "Discarded outcome from stale data_root. expected={}, got={}",
                    self.current_data_root, outcome.data_root
                ),
            );
            return SyncOutcomeEffect::StatusOnly;
        }

        let status = outcome.sync_status.clone();
        let result_trunc = if outcome.action_result.chars().count() > 1000 {
            outcome.action_result.chars().take(1000).collect::<String>() + "..."
        } else {
            outcome.action_result.clone()
        };
        let sanitized_result = mask_sync_error(&result_trunc);
        self.debug_log(
            "sync",
            "sync_outcome_received",
            &format!("status={}, result={}", status, sanitized_result),
        );

        self.current_sync_status = outcome.sync_status.clone();
        // Issue #779 评论 5853718466：Core 返回终态后立即结束同步 operation；
        // generation GC 作为 Core 内部独立 maintenance 异步执行（spawn 后台线程，
        // fire-and-forget），不占用本 operation 的 busy 生命周期。Core 修改后
        // perform_full_sync 拿到终态即返回，outcome 及时到达，此处立即清
        // current_sync_in_progress，UI 不会因 GC 慢而一直显示 syncing。
        self.current_sync_in_progress = false;
        // 同步结束：丢弃进度 sink，后续诊断导出不再附带 target 进度。
        self.current_sync_progress = None;
        self.current_last_sync_time = Self::now_epoch_seconds();
        self.current_sync_operation_state = outcome.action_result.clone();
        let status_str = outcome.sync_status.as_str();

        // Issue #754 评论 5814866116 改动2: handle_sync_outcome 返回 SyncOutcomeEffect，
        // 由 SyncBackend::handle_outcome 据此决定是否发 sync_content_applied。
        // 改动3: github init 旧路径已删除，不再有 pending_github_init_path 特判。
        let sync_success = matches!(status_str, "success" | "branch_missing_recovered");
        // Issue #754 评论 5816573119: 所有"内容已变化"的同步结果统一走同一个刷新入口，
        // 确保 reload_tree + reconcile_selection_after_tree_reload 成对执行。
        // conflict/partial_conflict/unrelated_histories 也会真实修改本地工作区内容
        // （Core issue_644: PartialConflict 时安全完成的非冲突文件继续提交到 live），
        // 若包含远端删除/结构变化，selection 必须同步 reconcile，否则
        // selected_project_id/selected_volume_id/selected_chapter_id 可能指向已删除对象。
        let content_changed = self.has_workspace()
            && (sync_success
                || status_str == "conflict"
                || status_str == "partial_conflict"
                || status_str == "unrelated_histories");

        let effect = if content_changed {
            self.handle_sync_content_refresh();
            SyncOutcomeEffect::ContentChanged
        } else {
            SyncOutcomeEffect::StatusOnly
        };

        // Issue #779 评论 5854734343：GC 触发要看完整成功类终态，不仅字面上的 success，
        // 还包括 no_changes / latest_wins_applied / branch_missing_recovered —
        // 这些都是 Core aggregate_full_sync_result() 的成功类终态。LWW 正常覆盖时
        // overall_status 就是 latest_wins_applied，无变化时是 no_changes，都应启动 GC。
        // 注意：sync_success（用于 content_changed）保持原义 success|branch_missing_recovered，
        // 不在此扩展，避免 no_changes 触发无意义的 content refresh。
        let gc_success = matches!(
            status_str,
            "success" | "no_changes" | "latest_wins_applied" | "branch_missing_recovered"
        );
        if gc_success && !self.manual_sync_pending {
            if self.current_gc_maintenance_cancel_token.is_some() {
                // 旧 GC 还没退干净（done callback 还没清 token），标记 pending，
                // 等 done callback 清 token 后补启动。不直接丢，避免本轮 GC 永久跳过。
                self.gc_maintenance_pending = true;
                self.debug_log(
                    "sync",
                    "gc_maintenance_pending_set",
                    "GC needed but previous GC token still present — deferring to pending",
                );
            } else {
                // token 空闲，直接启动。清掉可能残留的 pending（本轮 GC 已启动）。
                self.gc_maintenance_pending = false;
                self.start_gc_maintenance();
            }
        }

        // 当前同步任务真正结束并把 busy 清掉后，检查 manual_sync_pending。
        // 为 true 时先清 flag，再启动一次 manual sync。
        // 排队的同步还没执行，当前 outcome 的 effect 才是返回值。
        if self.manual_sync_pending {
            self.manual_sync_pending = false;
            self.debug_log(
                "sync",
                "manual_sync_pending_triggered",
                "starting queued manual sync after previous sync completed",
            );
            self.perform_sync_internal("manual", false, sync_qptr);
        }
        effect
    }
    pub(crate) fn handle_sync_content_refresh(&mut self) {
        self.reload_tree();
        let chapter_deleted = self.reconcile_selection_after_tree_reload();
        // Issue #754 评论 5815901258: trigger_projects_reloaded 已删除，
        // 领域通知由 ProjectBackend::emit_changed() 负责。
        if chapter_deleted {
            self.current_save_status = "chapter.deleted_remotely_refreshed".to_string();
        }

        self.debug_log("sync", "sync_refresh_applied", "tree_reloaded=true");
    }
    pub(crate) fn perform_sync(&mut self, sync_qptr: Option<QPointer<SyncBackend>>) -> QString {
        self.perform_sync_internal("manual", false, sync_qptr)
    }
}

fn format_conflict_files(files: &[String]) -> String {
    if files.is_empty() {
        "sync.result.no_conflict_files".to_string()
    } else {
        let display_files = if files.len() > 100 {
            let mut subset = files[0..100].to_vec();
            subset.push(format!("sync.result.more_files_count: {}", files.len()));
            subset
        } else {
            files.to_vec()
        };
        display_files.join("\n  - ")
    }
}
