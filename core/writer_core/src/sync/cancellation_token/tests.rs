//! cancellation_token.rs 的单元测试。
//!
//! 按仓库结构门禁（`tools/check_source_structure.py` 的
//! production-test-bloat 规则）从生产文件拆出，测试逻辑保持不变。
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
