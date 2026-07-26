use serde::{Deserialize, Serialize};

use super::visual::{CursorRect, Rect};

/// #517: 快照所有权状态 — 单一所有权，不允许 Manager 与事务共享同一个可释放资源引用。
///
/// 如果 Kotlin 层难以表达 move semantics，使用显式 owner token/state。
/// 任何 release 前必须校验 owner。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotOwner {
    /// 由 CompositionSession 持有
    OwnedBySession { session_id: u64 },
    /// 由指定事务持有
    OwnedByTransaction { transaction_id: u64 },
    /// 已释放
    Released,
}

/// 连续事务 rebase — 新事务与旧事务冲突时。
///
/// 预输入开始、更新、提交、取消以及连续正文输入都不得调用 pauseAll 叠加另一条事务。
/// 新事务与旧事务冲突时：
/// 1. 读取旧事务当前 progress
/// 2. 计算当前视觉帧
/// 3. 将当前 frame rect/alpha/scale 作为新事务 old state
/// 4. 取消旧事务
/// 5. 启动新事务
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionRebase {
    /// 被取消的旧事务 ID
    pub cancelled_transaction_id: u64,
    /// 旧事务在 rebase 瞬间的 progress（0.0–1.0）
    pub old_progress: f64,
    /// 旧事务当前帧的视觉状态快照（由平台层填充）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_frame_snapshot: Option<RebaseFrameSnapshot>,
}

/// Rebase 瞬间的帧快照 — 将当前 frame rect/alpha/scale 作为新事务 old state。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RebaseFrameSnapshot {
    /// 各切片的当前帧矩形
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slice_rects: Vec<Rect>,
    /// 各切片的当前帧透明度
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slice_alphas: Vec<f64>,
    /// 光标当前位置
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_rect: Option<CursorRect>,
}

/// 事务取消原因 — #516: 取消事务必须记录原因，用于 rebase 和资源释放判断。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransactionCancelReason {
    /// 被新事务 rebase 取代
    Rebased,
    /// 修订已变更，事务失效
    RevisionChanged,
    /// 系统抑制（滚动/加载/章节切换）
    SystemSuppressed,
    /// 用户手动取消
    UserCancelled,
    /// 预输入提交完成
    CompositionCommitted,
    /// 预输入取消
    CompositionCancelled,
}
