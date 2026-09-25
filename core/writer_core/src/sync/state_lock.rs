//! 跨 `WriterCoreApi` 实例串行化同一 sync root 的 state/conflict mutation。
//!
//! Issue #762 评论 5834136935：后台同步线程和 UI resolve 线程各自持有独立的
//! `WriterCoreApi` 实例，`core_instance: RwLock<WriterCore>` 是每个实例自己的字段，
//! 无法跨实例互斥。这导致 Commit 过程中（三方 merge 已算好 merged state 但尚未写回）
//! 用户从 UI resolve 的结果会被旧 merged state 覆盖。
//!
//! 本模块按规范化后的 `sync_root` 分桶，提供进程级 `Mutex<()>`：同一 project root
//! 永远拿到同一把锁，不同 project 仍可并行。
//!
//! 只覆盖本地 state/conflicts 的 Commit 和 resolve 串行化，不覆盖网络 Transfer
//! （Transfer 阶段不修改本地 state/conflicts）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// 全局分桶表：规范化后的 sync_root → 对应的进程级锁。
///
/// 用 `OnceLock<Mutex<BucketMap>>`：
/// - 外层 `OnceLock` 延迟初始化全局表；
/// - 中层 `Mutex` 保护 HashMap 插入/查找（只在查桶时短持，不覆盖用户闭包执行）；
/// - 内层 `Arc<Mutex<()>>` 是真正的 per-root 串行锁，clone 出 Arc 后即可释放中层锁。
type BucketMap = HashMap<PathBuf, Arc<Mutex<()>>>;
static BUCKETS: OnceLock<Mutex<BucketMap>> = OnceLock::new();

/// 规范化 sync_root 作为分桶 key。
///
/// `canonicalize` 成功就用规范化路径（消除 `.`/`..`/symlink 等别名），
/// 失败（路径尚不存在等）回退到 `sync_root.to_path_buf()`——同一原路径仍拿同一把锁，
/// 分桶语义不受影响。
fn normalize_root(sync_root: &Path) -> PathBuf {
    std::fs::canonicalize(sync_root).unwrap_or_else(|_| sync_root.to_path_buf())
}

/// 在持有 `sync_root` 对应的进程级锁期间执行 `f`。
///
/// 同一个 project root（规范化后）永远拿到同一把 `Mutex<()>`；不同 project 仍可并行。
///
/// 实现要点：
/// 1. 取全局 map 的锁，查/插对应 `Arc<Mutex<()>>`，clone 出 Arc 后立即释放 map 锁
///    （不持有 map 锁期间执行 `f`，避免不必要的串行化）；
/// 2. 拿 `Arc<Mutex<()>>` 的锁，执行 `f()`，返回结果。
///
/// 锁 poisoned 时用 `unwrap_or_else(|e| e.into_inner())` 恢复，与仓库现有
/// `core_write`/`core_read` 风格一致。
pub(crate) fn with_sync_state_lock<R>(sync_root: &Path, f: impl FnOnce() -> R) -> R {
    let key = normalize_root(sync_root);
    // 查桶：短持全局 map 锁，clone 出 Arc 后立即释放。
    let mutex_arc = {
        let mut buckets = BUCKETS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        buckets
            .entry(key)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    // 拿 per-root 串行锁，执行用户闭包。
    let _guard = mutex_arc.lock().unwrap_or_else(|e| e.into_inner());
    f()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    use tempfile::TempDir;

    /// 同一规范化路径永远拿到同一把锁，两个线程必须串行执行：
    /// 线程1 先拿锁（用 barrier 保证顺序），持锁期间设置 flag 并 sleep；
    /// 线程2 后拿锁，持锁时 flag 必为 true（证明它等到了线程1 释放）。
    #[test]
    fn same_path_serializes() {
        let dir = TempDir::new().expect("tempdir");
        let first_done = Arc::new(AtomicBool::new(false));
        // barrier 确保线程1 先拿到锁，线程2 之后才开始尝试拿锁。
        let start_order = Arc::new(Barrier::new(2));

        let path1 = dir.path().to_path_buf();
        let first_done1 = first_done.clone();
        let start_order1 = start_order.clone();
        let h1 = thread::spawn(move || {
            with_sync_state_lock(&path1, || {
                // 已拿到锁，通知线程2 可以开始尝试拿锁。
                start_order1.wait();
                first_done1.store(true, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(80));
            });
        });

        let path2 = dir.path().to_path_buf();
        let first_done2 = first_done.clone();
        let start_order2 = start_order.clone();
        let h2 = thread::spawn(move || {
            // 等线程1 拿到锁后再尝试拿锁（此时锁被线程1 持有，必须等待）。
            start_order2.wait();
            with_sync_state_lock(&path2, || {
                assert!(
                    first_done2.load(Ordering::SeqCst),
                    "同一路径必须串行：第二个持锁者应看到第一个已完成"
                );
            });
        });

        h1.join().expect("thread1 join");
        h2.join().expect("thread2 join");
    }

    /// 不同 project root 仍可并行，互不阻塞：
    /// 两个线程各自持不同路径的锁，在持锁期间同时到达 barrier。
    /// 若互斥，barrier 永远无法通过，channel 超时能发现。
    #[test]
    fn different_paths_parallel() {
        let dir1 = TempDir::new().expect("tempdir1");
        let dir2 = TempDir::new().expect("tempdir2");
        let barrier = Arc::new(Barrier::new(2));

        let (tx, rx) = mpsc::channel();
        let barrier1 = barrier.clone();
        let path1 = dir1.path().to_path_buf();
        let h1 = thread::spawn(move || {
            with_sync_state_lock(&path1, || {
                // 持锁期间等 barrier；若 path2 锁与 path1 互斥，这里会死锁。
                barrier1.wait();
            });
            tx.send(()).expect("send ok");
        });

        let barrier2 = barrier.clone();
        let path2 = dir2.path().to_path_buf();
        let h2 = thread::spawn(move || {
            with_sync_state_lock(&path2, || {
                barrier2.wait();
            });
        });

        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(()) => {}
            Err(_) => panic!("不同路径应可并行，但超时了（疑似互相阻塞）"),
        }
        h1.join().expect("thread1 join");
        h2.join().expect("thread2 join");
    }
}
