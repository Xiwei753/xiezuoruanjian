//! 同步取消令牌 — 平台无关的同步生命周期标记。
//!
//! 核心职责：
//! - 为一次同步操作提供线程安全的「是否已取消」标记
//! - 由平台层（如 Linux_Qt AppBackend）持有，在切换工作区时调用 `cancel()`
//! - 同步执行路径可轮询 `is_cancelled()` 提前终止（本轮由平台层持有与检查，
//!   core sync service 内部暂不集成；类型放在 Core 因其属于同步语义）
//!
//! 设计约束：
//! - 纯 Rust 类型，不依赖任何平台 crate，符合 Core 不依赖平台类型的边界
//! - 用 `Arc<AtomicBool>` 实现，`Send + Sync` 由 `Arc` 自动推导，不手写 `unsafe impl`
//! - `Ordering::SeqCst` 保证取消标记在所有线程间立即可见且无重排

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 线程安全的同步取消令牌。
///
/// 一次同步操作对应一个令牌。平台层在启动同步时创建令牌并持有 `Arc` 副本，
/// 在切换工作区或主动取消时调用 `cancel()`。同步执行路径（或回调身份校验）
/// 可通过 `is_cancelled()` 感知取消状态。
///
/// `clone()` 廉价（仅复制 `Arc`），可在多线程间共享同一令牌的多个句柄。
#[derive(Debug, Clone)]
pub struct SyncCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl SyncCancellationToken {
    /// 创建一个未取消的令牌。
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 标记此令牌已取消。幂等：多次调用效果一致。
    /// 使用 `SeqCst` 保证取消信号跨线程立即可见。
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// 查询此令牌是否已被取消。
    /// 使用 `SeqCst` 与 `cancel()` 配对，保证不读到过期值。
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

impl Default for SyncCancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for SyncCancellationToken {
    /// 两个令牌相等当且仅当它们共享同一底层 `AtomicBool`。
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.cancelled, &other.cancelled)
    }
}

impl Eq for SyncCancellationToken {}

// ── 同步进度共享状态（Issue #763） ──

/// 同步 target 进度快照 — `SyncProgressSink` 内部承载的可变状态。
///
/// 纯数据结构，由 `SyncProgressSink::snapshot` clone 出一份给调用方。
/// 字段与 `SyncCurrentTargetDto` + `finished_targets`/`total_targets` 对齐，
/// 供诊断包导出时映射。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncTargetProgressDto {
    pub current_target_remote_prefix: Option<String>,
    pub current_project_id: Option<String>,
    pub current_phase: Option<String>,
    pub finished_targets: u32,
    pub total_targets: u32,
}

/// 同步进度共享状态 — 后台线程写，主线程读，用于诊断包导出时获取实时 target 进度。
///
/// 核心职责：
/// - 为一次全量同步提供线程安全的「当前处理到哪个 target」标记
/// - 由平台层（如 Linux_Qt AppBackend）持有，同步执行路径在 target 开始/结束时写入
/// - 诊断包导出时调 `snapshot()` 读一份当前进度
///
/// 设计约束：
/// - 纯 Rust 类型，不依赖任何平台 crate，符合 Core 不依赖平台类型的边界
/// - 用 `Arc<Mutex<SyncTargetProgressDto>>` 实现，`Send + Sync` 由 `Arc` 自动推导，
///   不手写 `unsafe impl`
/// - `clone()` 廉价（仅复制 `Arc`），可在多线程间共享同一 sink 的多个句柄
#[derive(Debug, Clone)]
pub struct SyncProgressSink {
    inner: Arc<std::sync::Mutex<SyncTargetProgressDto>>,
}

impl SyncProgressSink {
    /// 创建一个进度 sink，初始 `finished_targets = 0`、`total_targets` 为传入值。
    pub fn new(total_targets: u32) -> Self {
        Self {
            inner: Arc::new(std::sync::Mutex::new(SyncTargetProgressDto {
                current_target_remote_prefix: None,
                current_project_id: None,
                current_phase: None,
                finished_targets: 0,
                total_targets,
            })),
        }
    }

    /// 读当前进度快照（clone 一份）。
    ///
    /// 锁中毒时返回默认空进度，不 panic（同步线程 panic 不应让诊断导出也崩）。
    pub fn snapshot(&self) -> SyncTargetProgressDto {
        self.inner.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// 标记开始处理某个 target。
    pub fn update_target_start(
        &self,
        remote_prefix: &str,
        project_id: Option<&str>,
        phase: &str,
        finished: u32,
        total: u32,
    ) {
        if let Ok(mut g) = self.inner.lock() {
            g.current_target_remote_prefix = Some(remote_prefix.to_string());
            g.current_project_id = project_id.map(|s| s.to_string());
            g.current_phase = Some(phase.to_string());
            g.finished_targets = finished;
            g.total_targets = total;
        }
    }

    /// 标记完成某个 target（phase 清空，表示该 target 已结束）。
    pub fn update_target_finish(
        &self,
        remote_prefix: &str,
        project_id: Option<&str>,
        finished: u32,
        total: u32,
    ) {
        if let Ok(mut g) = self.inner.lock() {
            g.current_target_remote_prefix = Some(remote_prefix.to_string());
            g.current_project_id = project_id.map(|s| s.to_string());
            g.current_phase = None;
            g.finished_targets = finished;
            g.total_targets = total;
        }
    }

    /// 只更新当前阶段，不改变 target 信息。
    ///
    /// 用于 generation GC / Commit 等非 target 阶段，让诊断包能区分
    /// 同步卡在 transfer / generation_gc / commit 哪一步（Issue #763）。
    ///
    /// 注意：该方不清 `current_target_remote_prefix` / `current_project_id`，
    /// 在 generation GC / Commit 等需要重置或精确指向当前 target 的场景应改用
    /// `set_target_phase` / `set_global_phase`（Issue #763 评论 5831610228）。
    pub fn set_phase(&self, phase: &str) {
        if let Ok(mut g) = self.inner.lock() {
            g.current_phase = Some(phase.to_string());
        }
    }

    /// target 级阶段：更新 `remote_prefix` / `project_id` / `phase`。
    ///
    /// 用于 generation GC 循环里每处理一个 planned target 时，
    /// 让诊断包能准确指向"当前卡在哪个作品的哪一步"
    /// （Issue #763 评论 5831610228）。
    ///
    /// 不更新 `finished_targets` / `total_targets`：GC 复用 Transfer 已完成的总数，
    /// 不把 GC 当成新的 target 计数。
    pub fn set_target_phase(&self, remote_prefix: &str, project_id: Option<&str>, phase: &str) {
        if let Ok(mut g) = self.inner.lock() {
            g.current_target_remote_prefix = Some(remote_prefix.to_string());
            g.current_project_id = project_id.map(|s| s.to_string());
            g.current_phase = Some(phase.to_string());
        }
    }

    /// 全局阶段：清 current target，只写 `phase`。
    ///
    /// 用于 Commit 等整轮全局操作，语义是"当前处于全局 commit，没有单一 current target"。
    /// 必须把 `current_target_remote_prefix` 和 `current_project_id` 清成 `None`
    /// （Issue #763 评论 5831610228）。
    ///
    /// 不更新 `finished_targets` / `total_targets`，保留 Transfer 阶段写入的总数。
    pub fn set_global_phase(&self, phase: &str) {
        if let Ok(mut g) = self.inner.lock() {
            g.current_target_remote_prefix = None;
            g.current_project_id = None;
            g.current_phase = Some(phase.to_string());
        }
    }
}

impl Default for SyncProgressSink {
    fn default() -> Self {
        Self::new(0)
    }
}

impl PartialEq for SyncProgressSink {
    /// 两个 sink 相等当且仅当它们共享同一底层 `Mutex`。
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for SyncProgressSink {}

#[cfg(test)]
mod tests {
    use super::{SyncCancellationToken, SyncProgressSink, SyncTargetProgressDto};

    #[test]
    fn new_token_is_not_cancelled() {
        let token = SyncCancellationToken::new();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn cancel_marks_token_cancelled() {
        let token = SyncCancellationToken::new();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancel_is_idempotent() {
        let token = SyncCancellationToken::new();
        token.cancel();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn clone_shares_cancellation_state() {
        let token = SyncCancellationToken::new();
        let handle = token.clone();
        assert!(!handle.is_cancelled());

        // 在一个句柄上取消，另一个应立即可见（共享底层 AtomicBool）
        token.cancel();
        assert!(handle.is_cancelled());
        assert!(token.is_cancelled());
    }

    #[test]
    fn independent_tokens_are_isolated() {
        let a = SyncCancellationToken::new();
        let b = SyncCancellationToken::new();
        a.cancel();
        assert!(a.is_cancelled());
        assert!(!b.is_cancelled(), "取消 a 不应影响独立令牌 b");
    }

    #[test]
    fn cloned_token_is_equal_to_original() {
        let token = SyncCancellationToken::new();
        let handle = token.clone();
        assert_eq!(token, handle);
    }

    #[test]
    fn distinct_tokens_are_not_equal() {
        let a = SyncCancellationToken::new();
        let b = SyncCancellationToken::new();
        assert_ne!(a, b);
    }

    #[test]
    fn default_is_not_cancelled() {
        let token = SyncCancellationToken::default();
        assert!(!token.is_cancelled());
    }

    /// 跨线程取消可见性：在子线程中 cancel，主线程应能观察到。
    /// SeqCst 保证此可见性，不会因重排丢失取消信号。
    #[test]
    fn cancellation_visible_across_threads() {
        let token = SyncCancellationToken::new();
        let handle = token.clone();

        let join = std::thread::spawn(move || {
            handle.cancel();
        });
        join.join().expect("worker thread panicked");

        assert!(token.is_cancelled(), "主线程应观察到子线程的取消");
    }

    // ── SyncProgressSink 测试 ──

    #[test]
    fn progress_sink_new_initializes_zero_finished() {
        let sink = SyncProgressSink::new(5);
        let snap = sink.snapshot();
        assert_eq!(snap.finished_targets, 0);
        assert_eq!(snap.total_targets, 5);
        assert!(snap.current_target_remote_prefix.is_none());
        assert!(snap.current_project_id.is_none());
        assert!(snap.current_phase.is_none());
    }

    #[test]
    fn progress_sink_update_target_start_sets_current() {
        let sink = SyncProgressSink::new(3);
        sink.update_target_start("projects/abc", Some("abc"), "uploading", 1, 3);
        let snap = sink.snapshot();
        assert_eq!(
            snap.current_target_remote_prefix.as_deref(),
            Some("projects/abc")
        );
        assert_eq!(snap.current_project_id.as_deref(), Some("abc"));
        assert_eq!(snap.current_phase.as_deref(), Some("uploading"));
        assert_eq!(snap.finished_targets, 1);
        assert_eq!(snap.total_targets, 3);
    }

    #[test]
    fn progress_sink_update_target_finish_clears_phase() {
        let sink = SyncProgressSink::new(3);
        sink.update_target_start("app", None, "downloading", 0, 3);
        sink.update_target_finish("app", None, 1, 3);
        let snap = sink.snapshot();
        assert_eq!(snap.current_target_remote_prefix.as_deref(), Some("app"));
        assert!(snap.current_phase.is_none(), "finish 应清空 phase");
        assert_eq!(snap.finished_targets, 1);
    }

    #[test]
    fn progress_sink_clone_shares_state() {
        let sink = SyncProgressSink::new(2);
        let handle = sink.clone();
        handle.update_target_start("projects/x", Some("x"), "syncing", 0, 2);
        let snap = sink.snapshot();
        assert_eq!(snap.current_project_id.as_deref(), Some("x"));
        assert_eq!(sink, handle, "clone 应共享同一底层状态");
    }

    #[test]
    fn progress_sink_default_is_empty() {
        let sink = SyncProgressSink::default();
        let snap = sink.snapshot();
        assert_eq!(snap, SyncTargetProgressDto::default());
    }

    #[test]
    fn progress_sink_snapshot_visible_across_threads() {
        let sink = SyncProgressSink::new(4);
        let handle = sink.clone();
        let join = std::thread::spawn(move || {
            handle.update_target_start("projects/t", Some("t"), "uploading", 2, 4);
        });
        join.join().expect("worker thread panicked");
        let snap = sink.snapshot();
        assert_eq!(snap.finished_targets, 2);
        assert_eq!(snap.current_phase.as_deref(), Some("uploading"));
    }

    // ── Issue #763 评论 5831610228 回归测试 ──

    #[test]
    fn set_target_phase_updates_target_and_phase() {
        let sink = SyncProgressSink::new(3);
        // 先模拟 Transfer 最后处理 p2
        sink.update_target_start("projects/p2", Some("p2"), "transfer", 1, 3);
        sink.update_target_finish("projects/p2", Some("p2"), 2, 3);
        // GC 开始处理 p1
        sink.set_target_phase("projects/p1", Some("p1"), "generation_gc");
        let snap = sink.snapshot();
        assert_eq!(
            snap.current_target_remote_prefix.as_deref(),
            Some("projects/p1")
        );
        assert_eq!(snap.current_project_id.as_deref(), Some("p1"));
        assert_eq!(snap.current_phase.as_deref(), Some("generation_gc"));
        // finished/total 保持 Transfer 的值，不被 GC 重置
        assert_eq!(snap.finished_targets, 2);
        assert_eq!(snap.total_targets, 3);
    }

    #[test]
    fn set_global_phase_clears_current_target() {
        let sink = SyncProgressSink::new(3);
        // 先模拟 Transfer 最后处理 p2
        sink.update_target_start("projects/p2", Some("p2"), "transfer", 1, 3);
        sink.update_target_finish("projects/p2", Some("p2"), 2, 3);
        // 进入全局 commit
        sink.set_global_phase("commit");
        let snap = sink.snapshot();
        assert!(
            snap.current_target_remote_prefix.is_none(),
            "commit 应清 current target"
        );
        assert!(
            snap.current_project_id.is_none(),
            "commit 应清 current project_id"
        );
        assert_eq!(snap.current_phase.as_deref(), Some("commit"));
        // finished/total 保持不变
        assert_eq!(snap.finished_targets, 2);
        assert_eq!(snap.total_targets, 3);
    }
}
