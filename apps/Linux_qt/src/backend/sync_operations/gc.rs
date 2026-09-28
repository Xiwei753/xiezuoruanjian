//! generation GC maintenance 的后台线程驱动。
//!
//! 从 `sync_operations.rs` 拆出：GC 与同步执行是两条独立的生命周期，一个走
//! Core 的后台维护接口，一个走 operation_id + QPointer 回调。

use super::*;

impl AppBackend {
    /// Issue #779 评论 5854082763：启动 generation GC maintenance 后台线程。
    ///
    /// - single-flight：如果 `current_gc_maintenance_cancel_token` 已存在，跳过
    ///   （说明上一轮 GC 还在跑或还没清理）。
    /// - 创建独立的 maintenance token（不复用用户同步 token）。
    /// - spawn 后台线程调 Core 的 `perform_generation_gc_maintenance`。
    /// - 线程结束后通过 queued_callback 清 `current_gc_maintenance_cancel_token = None`。
    /// - GC 失败只 log warn，不影响任何用户状态。
    pub(super) fn start_gc_maintenance(&mut self) {
        // single-flight：已有 maintenance 在跑，跳过。
        if self.current_gc_maintenance_cancel_token.is_some() {
            self.debug_log(
                "sync",
                "gc_maintenance_skipped",
                "GC maintenance already running — skipping",
            );
            return;
        }

        let data_root = self.current_data_root.clone();
        let projects_root = self.current_projects_root.clone();
        if data_root.is_empty() {
            return;
        }

        // 获取 workspace git layout 快照，供后台线程构造 API。
        let layout = match self.current_workspace_git_layout.clone() {
            Some(l) => l,
            None => return,
        };

        // 创建独立的 maintenance cancellation token。
        let token = Arc::new(writer_core::sync::SyncCancellationToken::new());
        self.current_gc_maintenance_cancel_token = Some(token.clone());
        let cancel_token_for_thread: writer_core::sync::SyncCancellationToken = (*token).clone();

        self.debug_log(
            "sync",
            "gc_maintenance_start",
            "starting generation GC maintenance",
        );

        let app_qptr = QPointer::from(&*self);
        // Issue #779 评论 5854511049：done callback 捕获启动时的身份（workspace_generation、
        // data_root、本次 token 的 Arc 身份），只有三者都仍一致时才清 None。
        // 旧 workspace 的 callback 不能修改新 workspace 的 maintenance 状态。
        let captured_workspace_generation = self.current_workspace_generation;
        let captured_data_root = data_root.clone();
        let captured_token = token.clone();
        // GC 线程结束后回到主线程清 token。
        let gc_done_callback = qmetaobject::queued_callback(move |_result: ()| {
            app_qptr.as_pinned().map(|this| {
                let mut this = this.borrow_mut();
                // 身份校验：workspace_generation、data_root、token Arc 身份三者都一致才清。
                let still_same_workspace =
                    this.current_workspace_generation == captured_workspace_generation;
                let still_same_data_root = this.current_data_root == captured_data_root;
                let still_same_token = this
                    .current_gc_maintenance_cancel_token
                    .as_ref()
                    .map(|t| Arc::ptr_eq(t, &captured_token))
                    .unwrap_or(false);
                if still_same_workspace && still_same_data_root && still_same_token {
                    this.current_gc_maintenance_cancel_token = None;
                    this.debug_log(
                        "sync",
                        "gc_maintenance_done",
                        "generation GC maintenance completed",
                    );
                    // Issue #779 评论 5854734343：done callback 清 token 后，如果 pending
                    // 且当前没有用户同步在跑，补启动一次新的 maintenance。
                    // still_same_workspace 已确保仍是同一 workspace，无需再校验。
                    if this.gc_maintenance_pending && !this.current_sync_in_progress {
                        this.gc_maintenance_pending = false;
                        this.debug_log(
                            "sync",
                            "gc_maintenance_pending_triggered",
                            "starting deferred GC maintenance after previous GC completed",
                        );
                        this.start_gc_maintenance();
                    }
                } else {
                    this.debug_log(
                        "sync",
                        "gc_maintenance_done_skipped",
                        &format!(
                            "GC done callback skipped: workspace_changed={}, data_root_changed={}, token_changed={} — not clearing current_gc_maintenance_cancel_token",
                            !still_same_workspace, !still_same_data_root, !still_same_token
                        ),
                    );
                }
            });
        });

        thread::spawn(move || {
            // SAFETY: catch_unwind requires the closure to be UnwindSafe. The closure only
            // captures owned String data (data_root, projects_root), a GitRepoLayout snapshot,
            // and a SyncCancellationToken (Arc<AtomicBool>). All of these auto-implement
            // UnwindSafe: String/GitRepoLayout are plain data, and SyncCancellationToken is
            // Arc<AtomicBool> where std impls RefUnwindSafe for both Arc<T> and AtomicBool.
            // No hand-rolled `unsafe impl` or AssertUnwindSafe is needed.
            let result = std::panic::catch_unwind(|| {
                let api = crate::backend::app_backend::with_layout_core_api(
                    &data_root,
                    &projects_root,
                    &layout,
                );
                let config = match prepare_sync_profile(&api) {
                    Ok(c) => c,
                    Err(e) => {
                        log::warn!(
                            "[sync] gc_maintenance: prepare_sync_profile failed: {}",
                            e.raw_error()
                        );
                        return;
                    }
                };
                // GC maintenance 不需要 network permission 检查 —
                // GC 是低优先级 maintenance，网络不可用时 Core 内部自然失败。
                if let Err(e) =
                    api.perform_generation_gc_maintenance(config, Some(cancel_token_for_thread))
                {
                    log::warn!(
                        "[sync] gc_maintenance: perform_generation_gc_maintenance failed: {e}"
                    );
                }
            });

            if let Err(err) = result {
                let panic_msg = if let Some(s) = err.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = err.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "panic.unknown".to_string()
                };
                log::error!("[sync] gc_maintenance: panic: {panic_msg}");
            }

            gc_done_callback(());
        });
    }
}
