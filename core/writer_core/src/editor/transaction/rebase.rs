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

/// #606: Rebase slice 对应关系的继续/结束语义。
///
/// 旧事务的某个逻辑 slice 在 rebase 后是否对应到新事务中的 slice。
/// 平台端据此决定旧 slice 动画是接续到新 slice 还是直接结束。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RebaseContinuation {
    /// 旧 slice 动画继续到新事务（同一逻辑对象，progress 接续）
    Continue,
    /// 旧 slice 动画结束（不再对应新事务中的对象）
    End,
}

/// #606: Rebase 匹配依据 — 为什么旧 slice 对应到新 slice。
///
/// 平台端可据此调整动画接续策略（例如 OffsetMapMatched 时需要按映射偏移
/// 调整起止坐标，SameByteRange 时直接复用旧坐标）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RebaseReason {
    /// 相同 UTF-8 byte range + compatible role
    SameByteRange,
    /// OffsetMap 映射的 range
    OffsetMapMatched,
    /// 无对应关系（Core 未给出映射）
    NoMapping,
}

/// #606: 旧事务逻辑 slice → 新事务逻辑 slice 的对应关系。
///
/// Core 在 `compute_rebase` 时唯一计算，平台端不再自己匹配。
/// 不在此结构中复述 byte range/role — 平台端持有旧/新事务的 slice 列表，
/// 通过 `old_slice_index` / `new_slice_index` 索引即可。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RebaseSliceMapping {
    /// 旧事务中的 slice 索引
    pub old_slice_index: usize,
    /// 新事务中的 slice 索引
    pub new_slice_index: usize,
    /// 继续/结束关系
    pub continuation: RebaseContinuation,
    /// 匹配依据
    pub reason: RebaseReason,
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
    /// #606: 旧→新逻辑 slice 对应关系（平台无关，Android 不再自己匹配）。
    ///
    /// 仅包含 `RebaseContinuation::Continue` 的映射；未出现的旧 slice
    /// 视为 `End`，由平台端按 Core 无映射处理。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slice_mappings: Vec<RebaseSliceMapping>,
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
