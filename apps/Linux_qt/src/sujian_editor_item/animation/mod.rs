pub(crate) mod composition;
pub(crate) mod coordinator;
pub(crate) mod cursor_motion;
/// Issue #819 评论 5956495850 第 2 节：协同动画唯一的「当前屏幕帧」类型。
pub(crate) mod frame_state;
pub(crate) mod rebase;
pub(crate) mod render_plan_builder;
/// Issue #824 评论 5971089641 第 1 节：唯一「当前屏幕状态 → 最新目标」的正文运动
/// 入口 `retarget(...)`；只保留当前运动，不保存历史输入队列。
pub(crate) mod retarget_motion;
/// Issue #819 评论 5956495850 第 3 节：唯一采样入口 `sample_transaction_visual_state`。
pub(crate) mod sample;
pub(crate) mod transaction;
pub(crate) mod transaction_builder;

pub(crate) use coordinator::{find_line_geometry_in_snapshot, LinuxEditorAnimationCoordinator};
pub(crate) use transaction::{
    PreparedTextVisualTransaction, PreparedTransactionQueue, PreparedVisualUnit, RebaseFrame,
    TextVisualOperationKind, TextVisualTransactionState, TransactionTimeline, VisualUnitTiming,
};
