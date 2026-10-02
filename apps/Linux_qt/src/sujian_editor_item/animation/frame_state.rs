//! Issue #819 评论 5956495850 第 2 节：协同动画唯一的「当前屏幕帧」。
//!
//! `SampledEditVisualState` 表达的是**现在屏幕上真的画成什么样**，
//! 不是「理论 progress 是多少」。它由 [`super::sample::sample_transaction_visual_state`]
//! 在一个 `now` 上一次性采样得到，文字层、光标层、rebase 交棒全部消费这一份，
//! 不再各自从 `current_visible_fraction()` / `track.progress(now)` 推一遍。
//!
//! - `SampledCaretFrame`：从 `render_plan` 收口过来的光标帧（文档坐标 rect +
//!   吞吐行序 + side + 段内局部进度）。`render_plan` re-export 本类型，保留旧路径。
//! - `SampledSliceFrame`：一个视觉单元在本帧的真实绘制帧（destination rect +
//!   source rect + opacity + 行身份 + snapshot side）。
//! - `SampledEditVisualState`：一笔事务在本帧的完整屏幕状态（transaction key +
//!   layout basis revision + caret + slices）。

use crate::sujian_editor_item::animated_slice::AnimatedSliceKind;
use crate::sujian_editor_item::animation::transaction::types::IngestSnapshotSide;
use crate::sujian_editor_item::edit_motion::CursorRect;
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::layout_snapshot::{LineSnapshotId, ShapingIdentity, SourceRect};
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;

/// 协同动画每帧的**唯一** caret 采样。
///
/// Issue #819 评论 5956495850 第 2 节：本类型从 `render_plan` 收口到 `frame_state`，
/// 让「屏幕画的帧」和「rebase 交棒的帧」天然是同一份算法。`render_plan` 通过
/// `pub(crate) use` re-export 本类型，保留旧引用路径。
///
/// Issue #815 评论 6042062633 修改 3：cursor layer 和文字层（InsertReveal/DeleteConceal
/// 的吞吐 clip）消费的是同一份采样。文字层不准再自己算一次时间，也不准把 caret track
/// 的 progress 换算成独立 0..1 visible fraction。
///
/// `None`（在 `SampledEditVisualState.caret` 里）表示本帧无有效 caret motion track
///（无活跃正文事务 / epoch 不一致 / 无 cursor_visual_track）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SampledCaretFrame {
    /// caret 在文档坐标系的 x（横向裁切边界）。
    pub x: f64,
    /// caret 在文档坐标系的 y（跨行裁切判断）。
    pub y: f64,
    /// caret 所在 visual line id（跨行裁切判断）。
    pub visual_line_id: Option<usize>,
    /// caret track 的当前 progress（0..1）。
    pub progress: f64,
    /// 本帧 caret 的完整 rect（文档坐标），和 x/y/visual_line_id/progress 出自同一次
    /// track 采样，光标层直接用它画 caret。
    pub rect: CursorRect,
    /// 本帧 caret 运动轨迹当前所在吞吐行的 canonical 行序。
    pub ingest_line_ord: Option<usize>,
    /// 本帧是否正处在这条轨迹的吞吐段（`IngestLine`）。
    pub is_ingest_segment: bool,
    /// 本帧吞吐边界属于哪一侧 canonical。
    pub ingest_side: Option<IngestSnapshotSide>,
    /// 本帧所处路由段的局部进度（0..1）。
    pub ingest_progress: f64,
}

impl Default for SampledCaretFrame {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            visual_line_id: None,
            progress: 0.0,
            rect: CursorRect {
                x: 0.0,
                top: 0.0,
                bottom: 0.0,
                baseline_y: 0.0,
            },
            ingest_line_ord: None,
            is_ingest_segment: false,
            ingest_side: None,
            ingest_progress: 0.0,
        }
    }
}

/// Issue #819 评论 5956495850 第 2 节：一个视觉单元在本帧的真实绘制帧。
///
/// 这是「现在屏幕上这一片字真的画成什么样」——destination rect 是当前帧实际
/// 绘制位置/大小，source_rect 是纹理裁剪，opacity 是混合系数。它们全部来自
/// [`crate::sujian_editor_item::animated_slice::AnimatedSlice::compute_frame`] 或
/// [`crate::sujian_editor_item::animated_slice::AnimatedSlice::compute_frame_by_caret_ingest`]
/// 在本帧 `now` 的输出，不是「理论 progress」。
///
/// `snapshot_side` 标记本片字属于哪一侧 canonical：
/// - `Some(Old)`：`DeleteConceal` 吞的是 old snapshot 的字。
/// - `Some(New)`：`InsertReveal` 吐的是 new snapshot 的字。
/// - `None`：`ReflowMove`/`ReflowCrossFade` 不参与吞吐，side 无意义。
#[derive(Clone, Debug)]
pub(crate) struct SampledSliceFrame {
    /// 切片种类（InsertReveal / DeleteConceal / ReflowMove / ReflowCrossFade）。
    pub kind: AnimatedSliceKind,
    /// 本片字在文本中的字节范围（属于本事务 new 坐标系或 old 坐标系，由 kind 决定）。
    pub byte_start: usize,
    pub byte_end: usize,
    /// shaping identity，用于 rebase 匹配。
    pub shaping_identity: Option<ShapingIdentity>,
    /// 当前帧的 destination rect（文档坐标，屏幕上真正画的位置/大小）。
    pub dest_rect: SourceRect,
    /// 当前帧的 source rect（纹理裁剪区域）。
    pub source_rect: SourceRect,
    /// 当前帧的 opacity。
    pub opacity: f64,
    /// 本片字所属视觉行 id（来自 `VisualLine.id`，跨行裁切判断用）。
    pub visual_line_id: Option<usize>,
    /// 本片字属于哪一侧 canonical（Old = DeleteConceal，New = InsertReveal，None = Reflow）。
    pub snapshot_side: Option<IngestSnapshotSide>,
    /// 本片字所属的 line snapshot id（纹理来源）。
    pub snapshot_id: LineSnapshotId,
    /// Issue #819 评论 5956495850 第 4 节：本帧的 visible_fraction（0..1），
    /// rebase 交棒时传给 `RebaseFrame.visible_fraction`。
    /// CaretTrack unit 设 0.0（`rebase_from_frame` 对 CaretTrack 直接 return，不使用此值）。
    pub visible_fraction: f64,
    /// Issue #819 评论 5956495850 第 4 节：本 unit 剩余播放时长（ms），
    /// rebase 交棒时传给 `RebaseFrame.remaining_duration_ms`。
    /// CaretTrack unit 设 0（连续性由 `RebaseCaretHandoff` 承担）。
    pub remaining_duration_ms: u64,
    /// Issue #819 评论 5956495850 第 4 节：本 unit 的 timeline progress（0..1），
    /// rebase 终态过滤用（ReflowMove/ReflowCrossFade 用 `progress >= 1.0` 判断终态）。
    /// CaretTrack unit 设 0.0。
    pub progress: f64,
}

/// Issue #819 评论 5956495850 第 2 节：协同动画唯一的「当前屏幕帧」。
///
/// 由 [`super::sample::sample_transaction_visual_state`] 在一个 `now` 上一次性采样
/// 得到。文字层、光标层、rebase 交棒全部消费这一份，不再各自从 progress 推一遍。
///
/// 这个结构表达的是：**现在屏幕上真的画成什么样。** 不是「理论 progress 是多少」。
#[derive(Clone, Debug)]
pub(crate) struct SampledEditVisualState {
    /// 本帧所属的事务 key。
    pub transaction_key: VisualTransactionKey,
    /// 本事务绑定的 canonical layout basis revision。
    pub layout_basis_revision: LayoutRevision,
    /// 本帧的 caret 采样。`None` 表示本事务无有效 caret motion track。
    pub caret: Option<SampledCaretFrame>,
    /// 本帧所有视觉单元的绘制帧。
    pub slices: Vec<SampledSliceFrame>,
}
