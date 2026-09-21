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

#[cfg(test)]
mod tests {
    use super::SyncCancellationToken;

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
}
