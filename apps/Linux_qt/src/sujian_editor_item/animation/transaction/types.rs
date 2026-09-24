use std::time::Instant;

use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};
use crate::sujian_editor_item::edit_motion::CursorRect;
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, LineSnapshotId, ShapingIdentity,
};
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;

use super::timeline::{TransactionTimeline, VisualUnitTiming};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextVisualTransactionState {
    Pending,
    Prepared,
    Rendering,
    Paused,
    Completed,
    Cancelled,
}

impl TextVisualTransactionState {
    /// Issue #727 评论 5757225958 问题3: 事务是否允许收集 clip_rects。
    ///
    /// 只有 Prepared / Rendering / Paused 状态的事务才允许静态层隐藏
    ///（Pending 无论 texture_prepared 是什么都不能隐藏正文，否则资源还没
    /// 准备好就会出现空洞；Completed/Cancelled 已移除）。
    /// 把状态检查集中到这个方法，让 build_render_plan_full 的 clip_rects
    /// 收集逻辑不再内联 `TextVisualTransactionState::Rendering` 等枚举值，
    /// 便于结构守卫测试断言"完成帧事务被 keys_to_complete 排除"。
    pub(crate) fn is_clip_eligible(self) -> bool {
        matches!(
            self,
            TextVisualTransactionState::Prepared
                | TextVisualTransactionState::Rendering
                | TextVisualTransactionState::Paused
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextVisualOperationKind {
    Insert,
    Delete,
    CompositionUpdate,
    CompositionCommitOrCancel,
}

/// Issue #690 评论 5675007226 步骤 3: 单个视觉单元，拥有自己的动画生命期。
///
/// Issue #722 评论 5747719529 核心语义：光标本身就是吞字/吐字的视觉边界。
/// 文字不能再维护一套会和 caret 分叉的"自己什么时候完全出现/完全消失"的位置/
/// 可见度进度。真正决定当前 reveal/conceal 截止位置的是这一帧的 caret geometry
/// （caret_geometry_determines_clip / clip_from_coordinated_caret）。
///
/// Issue #727 评论 5754041813 约束 2: 计时语义拆分为 `VisualUnitTiming`。
/// - `InsertReveal` / `DeleteConceal` → `CaretDriven`：无独立时间线，裁切边界由本帧
///   coordinated caret 位置决定。
/// - `ReflowMove` / `ReflowCrossFade` → `Timed`：独立时间线几何插值。
///
/// 快速连续输入时，旧事务被 cancel 并 rebase：匹配的旧 unit 通过 `rebase_from_frame`
/// 把当前 `visible_fraction` 写入 `start_fraction`，文字从"已经吐/吞到一半"的位置继续。
/// 只有被新编辑实际覆盖的 unit 才结束/替换。
#[derive(Clone, Debug)]
pub(crate) struct PreparedVisualUnit {
    pub slice: AnimatedSlice,
    pub timing: VisualUnitTiming,
}

impl PreparedVisualUnit {
    /// 把一个 `AnimatedSlice` 包成拥有独立生命期的视觉单元。
    ///
    /// `duration_ms` 取自事务（与 `TransactionTimeline::duration_ms` 一致），
    /// 仅对 Timed unit（ReflowMove / ReflowCrossFade）有效。
    pub fn wrap(slice: AnimatedSlice, duration_ms: u64) -> Self {
        let timing = VisualUnitTiming::default_for_kind(slice.kind, duration_ms);
        Self { slice, timing }
    }

    /// 从自己的时间线计算当前 progress（0..1）。
    /// Issue #727 约束 2: CaretDriven unit 返回 0.0（无独立时间线）。
    pub fn progress(&self, now: Instant) -> f64 {
        self.timing.progress(now)
    }

    /// Issue #727 约束 2: 判断单元是否已到达终态（不应再交棒）。
    /// CaretDriven unit：`start_fraction == target_fraction` 表示已到终态。
    /// Timed unit：`progress >= 1.0` 表示已播完。
    #[cfg(test)]
    pub fn is_finished(&self, now: Instant) -> bool {
        match &self.timing {
            VisualUnitTiming::CaretDriven {
                start_fraction,
                target_fraction,
            } => (start_fraction - target_fraction).abs() < 1e-9,
            VisualUnitTiming::Timed { .. } => self.timing.progress(now) >= 1.0,
        }
    }

    /// 单元在 `now` 时刻的真实可见比例（0..1）。
    ///
    /// Issue #727 约束 2: CaretDriven unit（InsertReveal / DeleteConceal）不再从
    /// 独立时间线驱动，直接返回 `start_fraction`（由 rebase 交棒设置）。
    /// Timed unit（ReflowMove / ReflowCrossFade）从自己的时间线算可见比例。
    pub fn current_visible_fraction(&self, now: Instant) -> f64 {
        self.timing.current_visible_fraction(now)
    }

    /// 按旧单元的当前帧续播本单元。
    ///
    /// Issue #690 评论 5675007226 步骤 3: 可见比例成为新单元的起点。
    /// Issue #690 评论 5683759796: started_at 留 None，等进入 Rendering 再用
    /// sample.frame_now 启动，不再用 frame.sampled_at 提前计时。
    pub fn rebase_from_frame(&mut self, frame: &RebaseFrame) {
        self.slice
            .rebase_from(frame.x, frame.y, frame.opacity, frame.visible_fraction);
        // Issue #727: 对 ReflowMove/ReflowCrossFade，slice.rebase_from 已修改 from_document_rect
        // 为屏幕位置（current_x），timing 的 start_fraction 应重置为 0——from 已被改写为
        // 屏幕位置，不需要再通过 start_fraction 表达已演进状态，否则进度会被重复应用。
        // 对 InsertReveal/DeleteConceal（CaretDriven），slice.rebase_from 只修改 start_fraction
        //（不修改几何），timing 的 start_fraction 需要保留 visible_fraction 作为交棒载体。
        let timing_start_fraction = match self.slice.kind {
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => 0.0,
            _ => frame.visible_fraction,
        };
        self.timing
            .rebase_from_frame(timing_start_fraction, frame.remaining_duration_ms);
    }
}

/// 旧事务某个视觉单元在当前时刻的视觉帧，交棒给新事务继续播放。
///
/// Issue #690 评论 5675007226 步骤 3: 除了位置与透明度，必须带当前 `visible_fraction`
/// 和单元自己的时间线。只传 `(x, y, opacity)` 时 Reveal/Conceal 会按新事务 progress
/// 重新 0→1 / 1→0，快速连打时上一笔的字被反复重启。
///
/// Issue #690 评论 5679744253 问题 1: 原来同时携带 `started_at`（旧起点）和 `duration_ms`
/// （旧总时长），下一帧 `current_visible_fraction()` 用旧时间线算 progress，再从
/// `start_fraction` 到 target 做 easing，进度被重复应用，快速连打时制造跳变。
/// 现在改为携带 `sampled_at`（采集帧时间点）和 `remaining_duration_ms`（旧 unit 剩余
/// 播放时长），retarget 时从当前帧重新起一段：`duration_ms = remaining_duration_ms`，
/// 不再沿用旧起始时间。
///
/// Issue #690 评论 5683759796: `sampled_at` 不再直接作为新 unit 的 `started_at`——
/// `rebase_from_frame` 现在把 `started_at` 留 `None`，等进入 Rendering 才用
/// `sample.frame_now` 启动。`sampled_at` 字段保留做诊断/连续性断言
/// （`issue690_collect_rebase_frames_uses_per_unit_progress` 断言 `frame.sampled_at == now`，
/// `collect_rebase_frames` 仍设置 `sampled_at: now`）。
#[derive(Clone, Debug)]
pub(crate) struct RebaseFrame {
    pub byte_start: usize,
    pub byte_end: usize,
    pub x: f64,
    pub y: f64,
    pub opacity: f64,
    pub shaping_identity: Option<ShapingIdentity>,
    pub visible_fraction: f64,
    /// 采集本帧的时间点。Issue #690 评论 5683759796: 不再作为新单元的 `started_at`，
    /// 仅保留做诊断/连续性断言；新单元 `started_at = None`，等进入 Rendering 再启动。
    /// Issue #701 评论 5699573227: `match_rebase_frames` 读取此字段写动画诊断日志。
    pub sampled_at: Instant,
    /// 旧单元剩余的播放时长；retarget 后作为新单元的 `duration_ms`。
    pub remaining_duration_ms: u64,
}

/// Issue #690 评论 5681206040: coordinated caret 的正式视觉 track。
///
/// 之前 `cursor_visual_from` / `cursor_visual_to` 只是 `CursorRect` 端点，没有自己的
/// 时间状态（`started_at` / `duration_ms`），所以 `compute_coordinated_cursor_position`
/// 不得不借第一个 reflow unit 的 progress，`sample_coordinated_cursor_rect_at` 干脆
/// 不读这两个字段、固定用 `old_cursor_rect` / `new_cursor_rect`。连续交棒时第二次
/// rebase 采样到的光标回到逻辑 old caret 起算，与旧事务当前屏幕光标不一致。
///
/// 收成为一个完整 caret track 后，两处问题一起消失：
/// - 首次正文事务：`from = old_cursor_rect`，`to = new_cursor_rect`，
///   `started_at = now`，`duration_ms = 事务时长`。
/// - rebase 时：先用这个 track 在同一个 `now` 采样当前屏幕 caret；
///   新事务 `from = sampled caret`，`to = 最新 new_cursor_rect`，`started_at = now`，
///   `duration_ms` 用旧 track 剩余时长，不再借任何文字 unit 的 progress。
/// - InsertReveal / Backspace 有明确文字边界时，最终屏幕 x/y 仍直接取文字边界；
///   Enter、删除换行、软换行、纯 reflow 等没有明确边界时，直接 sample 这个 caret track。
/// - 下一次 rebase 再从同一个 caret track 采样，不能回头使用逻辑 `old_cursor_rect`。
#[derive(Clone, Debug)]
pub(crate) struct PreparedCursorVisualTrack {
    pub from: CursorRect,
    pub to: CursorRect,
    /// Issue #722 评论 5749164244 问题1: from/to 端的视觉行 id。
    ///
    /// `CursorRect`（Core 类型）不带 visual_line_id，但 `layout::CaretRect` 有。
    /// pipeline.rs 构造事务时把 `CaretRect.visual_line_id` 传进来，
    /// 不在 `make_cursor_rect_from_caret_doc()` 后丢掉。
    /// `None` 表示未知（fallback 路径或测试构造），采样时走 y fallback。
    pub from_visual_line_id: Option<usize>,
    pub to_visual_line_id: Option<usize>,
    /// Issue #722 评论 5749791161: from/to 端真实视觉行的 top/bottom。
    ///
    /// 这些值直接来自对应 `VisualLine.y` 和 `VisualLine.y + VisualLine.height`，
    /// 不是 caret 自己的 `CursorRect.top/bottom`（caret 矩形只是光标那条细矩形，
    /// 不等于整条视觉行的边界）。`sampled_visual_line_id_at_progress` 用这些值
    /// 判断 caret 是否已进入下一条视觉行，避免跨软换行时因 caret top 离开旧
    /// caret 细矩形就提前切换行 id。
    /// fallback 路径（行几何未知）传 0.0/0.0，采样时走 y fallback。
    pub from_line_top: f64,
    pub from_line_bottom: f64,
    pub to_line_top: f64,
    pub to_line_bottom: f64,
    /// Issue #690 评论 5682867529: caret track 不再在事务创建时就启动计时，
    /// 而是等到进入 `Rendering` 状态才和文字 unit 共用同一个 `frame_now` 起跑。
    /// `None` 表示尚未开始播放，`progress` 返回 0、`remaining_duration_ms` 返回全长。
    pub started_at: Option<Instant>,
    pub duration_ms: u64,
    /// 暂停起点；`Some` 表示当前处于暂停中，`resume` 时把 `started_at` 推前暂停时长。
    pub pause_start: Option<Instant>,
}

impl PreparedCursorVisualTrack {
    /// 从自己的 `started_at` / `duration_ms` 计算当前 progress（0..1）。
    /// `started_at = None`（尚未进入 Rendering）时返回 0。
    pub fn progress(&self, now: Instant) -> f64 {
        match self.started_at {
            None => 0.0,
            Some(start) => {
                if self.duration_ms == 0 {
                    return 1.0;
                }
                let elapsed = now.duration_since(start).as_millis() as f64;
                (elapsed / self.duration_ms as f64).clamp(0.0, 1.0)
            }
        }
    }

    /// Issue #722 评论 5749164244 问题1: 按 progress 采样当前 caret 所在视觉行 id。
    ///
    /// Issue #722 评论 5749572808 问题2: 不再用 `progress < 0.5` 硬切 from/to 行。
    /// 改为按采样后的 caret y 与 from/to 行真实 top/bottom 判断：caret y 落在
    /// from 行 y 范围内 → 还在 from 行；落在 to 行 y 范围内 → 已到 to 行；
    /// 过渡中间空隙按 y 方向（向下/向上移动）判断。这样跨软换行交棒时
    /// 不会因 progress 过 0.5 就提前认为 caret 已进入 to 行。
    ///
    /// Issue #722 评论 5749791161: 行 y 范围用 `from_line_top/bottom` 和
    /// `to_line_top/bottom`（真实视觉行边界），不用 `self.from.top/bottom` 和
    /// `self.to.top/bottom`（caret 自己的细矩形边界）。向下跨软换行时，
    /// caret top 只要离开旧 caret 细矩形就可能直接切成 to 行，但这并不等于
    /// caret 已进入下一条视觉行。用真实行边界判断才能正确反映 caret 所在行。
    /// `None` 表示 from/to 行 id 未知（fallback 路径），调用方走 y fallback。
    pub fn sampled_visual_line_id_at_progress(&self, progress: f64) -> Option<usize> {
        match (self.from_visual_line_id, self.to_visual_line_id) {
            (Some(f_id), Some(t_id)) => {
                if f_id == t_id {
                    return Some(f_id);
                }
                let eased = AnimatedSlice::ease_out_quad(progress.clamp(0.0, 1.0));
                let caret_y = self.from.top + (self.to.top - self.from.top) * eased;
                // from 行 y 范围 [from_line_top, from_line_bottom)，
                // to 行 [to_line_top, to_line_bottom)。
                if caret_y >= self.from_line_top && caret_y < self.from_line_bottom {
                    Some(f_id)
                } else if caret_y >= self.to_line_top && caret_y < self.to_line_bottom {
                    Some(t_id)
                } else {
                    // 过渡中间空隙：按 y 方向判断。
                    if self.to_line_top > self.from_line_top {
                        // 向下移动：caret_y >= from_line_bottom 说明已离开 from 行，归 to。
                        if caret_y >= self.from_line_bottom {
                            Some(t_id)
                        } else {
                            Some(f_id)
                        }
                    } else if self.to_line_top < self.from_line_top {
                        // 向上移动：caret_y <= to_line_bottom 说明已进入 to 行。
                        if caret_y <= self.to_line_bottom {
                            Some(t_id)
                        } else {
                            Some(f_id)
                        }
                    } else {
                        // to_line_top == from_line_top：fallback 用 progress < 0.5。
                        if progress < 0.5 {
                            Some(f_id)
                        } else {
                            Some(t_id)
                        }
                    }
                }
            }
            (Some(f_id), None) => Some(f_id),
            (None, Some(t_id)) => Some(t_id),
            (None, None) => None,
        }
    }

    /// Issue #702: 用外部传入的 progress（来自文字 unit 的可见进度）采样 caret rect，
    /// 而非 caret track 自己的 timeline。消除删除事务里 caret track 与 DeleteConceal
    /// unit 帧基准分叉导致的"光标先完成、旧字晚消失"错拍。
    /// Issue #722 评论 5747719529: 此方法是 caret track 的主路径 API，
    /// `sample_caret_driven_clip` 和 `sample_coordinated_cursor_rect_at` 均通过
    /// `sampled_rect_at_progress(progress(now))` 调用。
    pub fn sampled_rect_at_progress(&self, progress: f64) -> CursorRect {
        let eased = AnimatedSlice::ease_out_quad(progress.clamp(0.0, 1.0));
        let x = self.from.x + (self.to.x - self.from.x) * eased;
        let top = self.from.top + (self.to.top - self.from.top) * eased;
        let h = self.to.bottom - self.to.top;
        CursorRect {
            x,
            top,
            bottom: top + h,
            // Issue #712 评论 5739517945 第 2 项: baseline_y 从 from 到 to 插值，
            // 不再直接取 to.baseline_y，消除垂直动画跳终点。
            baseline_y: self.from.baseline_y + (self.to.baseline_y - self.from.baseline_y) * eased,
        }
    }

    /// 旧 track 剩余的播放时长：`duration - elapsed`，下溢保护为 0。
    /// `started_at = None`（尚未进入 Rendering）时返回 `duration_ms` 全长。
    pub fn remaining_duration_ms(&self, now: Instant) -> u64 {
        match self.started_at {
            None => self.duration_ms,
            Some(start) => {
                let elapsed_ms = now.duration_since(start).as_millis() as u64;
                self.duration_ms.saturating_sub(elapsed_ms)
            }
        }
    }

    /// 首次事务：`from = old_cursor_rect`，`to = new_cursor_rect`，
    /// `started_at = None`（等进入 Rendering 再启动），`duration_ms = 事务时长`。
    ///
    /// Issue #722 评论 5749791161: `from_line_top/bottom` 和 `to_line_top/bottom`
    /// 来自对应 `VisualLine.y` 和 `VisualLine.y + VisualLine.height`，
    /// 是真实视觉行边界，不是 caret 自己的细矩形边界。
    pub fn new_first(
        from: CursorRect,
        to: CursorRect,
        from_visual_line_id: Option<usize>,
        to_visual_line_id: Option<usize>,
        from_line_top: f64,
        from_line_bottom: f64,
        to_line_top: f64,
        to_line_bottom: f64,
        duration_ms: u64,
    ) -> Self {
        Self {
            from,
            to,
            from_visual_line_id,
            to_visual_line_id,
            from_line_top,
            from_line_bottom,
            to_line_top,
            to_line_bottom,
            started_at: None,
            duration_ms,
            pause_start: None,
        }
    }

    /// Issue #690 评论 5682867529: caret track 跟文字视觉单元一起暂停/恢复，
    /// 不自己按墙钟继续走。暂停时记录 pause_start，resume 时把 started_at 推前
    /// 暂停时长，这样 progress(now) 自动跳过暂停区间。
    pub fn pause(&mut self, now: Instant) {
        if self.started_at.is_none() || self.pause_start.is_some() {
            return;
        }
        self.pause_start = Some(now);
    }

    pub fn resume(&mut self, now: Instant) {
        if let (Some(start), Some(pause_start)) =
            (self.started_at.as_mut(), self.pause_start.take())
        {
            let paused_duration = now.duration_since(pause_start);
            *start += paused_duration;
        }
    }
}

/// Issue #701 评论 5699573227: `rebase_to` 仅在测试中直接调用（生产代码走
/// `build_cursor_visual_track` 的 handoff 分支，逻辑等价）。放在 `#[cfg(test)]`
/// impl 块里，避免 clippy 误报 dead_code，也不需要 `#[allow(dead_code)]`。
/// Issue #722 评论 5747719529: `eased` 和 `sampled_rect` 也仅在此 `#[cfg(test)]`
/// 块中被 `rebase_to` 调用，主路径改用 `sampled_rect_at_progress(progress(now))`。
#[cfg(test)]
impl PreparedCursorVisualTrack {
    /// 与文字帧同一条 easing（`AnimatedSlice::ease_out_quad`）。
    pub fn eased(&self, now: Instant) -> f64 {
        AnimatedSlice::ease_out_quad(self.progress(now))
    }

    /// 在 `now` 时刻按 `from -> to` 插值采样当前屏幕 caret rect。
    pub fn sampled_rect(&self, now: Instant) -> CursorRect {
        let eased = self.eased(now);
        let x = self.from.x + (self.to.x - self.from.x) * eased;
        let top = self.from.top + (self.to.top - self.from.top) * eased;
        let h = self.to.bottom - self.to.top;
        CursorRect {
            x,
            top,
            bottom: top + h,
            // Issue #712 评论 5739517945 第 2 项: baseline_y 从 from 到 to 插值，
            // 不再直接取 to.baseline_y，消除垂直动画跳终点。
            baseline_y: self.from.baseline_y + (self.to.baseline_y - self.from.baseline_y) * eased,
        }
    }

    /// 从当前帧重新起一段：`from = sampled caret`，`to = new_to`，
    /// `started_at = None`（等进入 Rendering 再启动），`duration_ms = 旧 track 剩余时长`（至少 1ms 保证非零）。
    ///
    /// Issue #722 评论 5749791161: 行几何字段在 rebase 时用 0.0/0.0（测试专用方法，
    /// 生产代码走 `build_cursor_visual_track` 的 handoff 分支，由 handoff 传递行几何）。
    pub fn rebase_to(&self, new_to: CursorRect, now: Instant) -> Self {
        let sampled = self.sampled_rect(now);
        let remaining = self.remaining_duration_ms(now).max(1);
        Self {
            from: sampled,
            to: new_to,
            from_visual_line_id: self.to_visual_line_id,
            to_visual_line_id: self.to_visual_line_id,
            from_line_top: 0.0,
            from_line_bottom: 0.0,
            to_line_top: 0.0,
            to_line_bottom: 0.0,
            started_at: None,
            duration_ms: remaining,
            pause_start: None,
        }
    }
}

/// 一次平台视觉事务持有的全部资源。
///
/// 它拥有一次动画所需的 units、patches、cursor transition
/// 和 old/new snapshots。事务 key 负责资源和 snapshot 所有权；
/// 逐个 `PreparedVisualUnit` 负责自己显示到哪里（见 `PreparedVisualUnit`）。
/// `texture_prepared` 为 true 后，静态层才允许隐藏对应范围，否则会出现一帧空洞。
/// 事务完成、取消或超时移除后，对应快照资源才可以释放。
#[derive(Clone, Debug)]
pub(crate) struct PreparedTextVisualTransaction {
    pub key: VisualTransactionKey,
    pub state: TextVisualTransactionState,
    pub operation_kind: TextVisualOperationKind,
    pub timeline: TransactionTimeline,
    pub units: Vec<PreparedVisualUnit>,
    pub old_cursor_rect: Option<CursorRect>,
    pub new_cursor_rect: Option<CursorRect>,
    /// Issue #690 评论 5681206040: coordinated caret 的正式视觉 track。
    ///
    /// 替代之前的 `cursor_visual_from` / `cursor_visual_to`（只有端点没有时间状态）。
    /// 自带 `started_at` / `duration_ms`，不再借任何文字 unit 的 progress。
    ///
    /// - 首次正文事务：`from = old_cursor_rect`，`to = new_cursor_rect`。
    /// - rebase 时：先用这个 track 在同一个 `now` 采样当前屏幕 caret；
    ///   新事务 `from = sampled caret`，`to = 最新 new_cursor_rect`，
    ///   `started_at = now`，`duration_ms` 用旧 track 剩余时长。
    /// - InsertReveal / Backspace 有明确文字边界时，最终屏幕 x/y 仍直接取文字边界；
    ///   纯 reflow 等没有明确边界时，直接 sample 这个 caret track。
    /// - `None` 表示本事务没有视觉 caret track（CursorOnly 或无 old/new cursor rect）。
    pub cursor_visual_track: Option<PreparedCursorVisualTrack>,
    pub cancel_reason: Option<String>,
    pub texture_prepared: bool,
    pub old_snapshot: Option<EditorLayoutSnapshot>,
    pub new_snapshot: Option<EditorLayoutSnapshot>,
    /// Issue #705 评论 5717380886: 创建本事务时记录的 `cursor_owner_epoch`。
    ///
    /// 这笔正文事务只在该 epoch 下拥有 coordinated caret。之后任何非正文事务
    /// 导致的逻辑 cursor 移动（鼠标点击、方向键、Home/End、拖选等）会 bump
    /// `CursorController::cursor_owner_epoch`，使本事务的 `cursor_owner_epoch`
    /// 不再等于当前 epoch。
    ///
    /// Issue #735 评论 5773604666 问题3: 失去 caret ownership 时，CaretDriven units
    /// （InsertReveal/DeleteConceal）立即落到 canonical final state
    /// （`start_fraction` 设为 `target_fraction`），caret motion 同时结束。
    /// ReflowMove/ReflowCrossFade 作为独立 passive reflow track 继续。
    /// 不再存在"同一笔正文吞吐 transaction 还活着，但 caret_owner 已经不是它"
    /// 的状态。
    pub cursor_owner_epoch: u64,
    /// Issue #727 评论 5760650874 / Issue #735 评论 5773604666 问题3:
    /// 该事务是否已永久失去 caret motion ownership。
    ///
    /// 一旦在 `build_text_animation_plan_with_sample` 中发现 `has_caret_driven_units
    /// && !owns_caret`（本帧有 CaretDriven units 但不是 owner），或在
    /// `find_cursor_transaction_for_target` 中发现 epoch 不一致时，此字段置 true，
    /// 同时调用 `retire_caret_driven_units` 把 CaretDriven units 的 `start_fraction`
    /// 设为 `target_fraction`（终态）。
    ///
    /// 之后 `active_text_transaction_key_with_epoch` 永远跳过此事务，
    /// `sample_coordinated_motion_frame` 不会再给它 `owner_key`，
    /// 已 Snap 回 canonical 的旧 caret / 吞吞吐字轨迹不会重新接管。
    /// ReflowMove/ReflowCrossFade 作为独立 passive reflow track 继续播完，
    /// 事务只等剩余 Timed unit 完成。
    pub caret_motion_retired: bool,
    /// Issue #710 评论 5731145076 症状五/六 / 评论 5732160521 问题 3:
    /// 事务的视觉 affected byte range，分 old/new 两侧保存。
    ///
    /// 基于段落边界扩展后的 byte range，而非原始的 inserted_range/deleted_range。
    /// 用于判断视觉区域重叠——newline 这种"改动 byte 很小、视觉影响很大"的事务，
    /// 原始 byte range 很小，但视觉 affected range 覆盖整个段落。
    ///
    /// 之前只有一个 `visual_affected_byte_range: Option<(usize, usize)>`，Insert 存
    /// new-text 坐标系、Delete 存 old-text 坐标系，`find_conflicting_transaction`
    /// 直接做数值 overlap，跨 revision 不可比，导致误判/漏判冲突。
    ///
    /// 现在拆成 old/new 两侧：
    /// - `visual_affected_byte_range_old`: old_text 坐标系（事务应用前的文本）
    /// - `visual_affected_byte_range_new`: new_text 坐标系（事务应用后的文本）
    /// Insert 事务 old 侧是插入点、new 侧是 inserted_range；
    /// Delete 事务 old 侧是 deleted_range、new 侧是删除后落点。
    ///
    /// `find_conflicting_transaction` 接收新事务的 `OffsetMap`（从旧事务 new 坐标系
    /// 到新事务查询坐标系的映射），把旧事务的 `visual_affected_byte_range_new`
    /// 映射到查询坐标系再做 overlap。映射失败时保守判定为冲突（避免漏判）。
    ///
    /// `None` 表示事务没有视觉 affected region（如 CursorOnly）。
    pub visual_affected_byte_range_old: Option<(usize, usize)>,
    pub visual_affected_byte_range_new: Option<(usize, usize)>,
    /// Issue #738 评论 5787277777: 活动事务绑定的 canonical layout basis revision。
    ///
    /// 事务创建时记录当时的 canonical layout revision。当 canonical layout 推进
    ///（换行导致后面整段 y 下移、宽度变化、字号变化等）时，`rebind_timed_units_to_canonical`
    /// 把仍存活的 Timed Reflow unit 从旧 basis 几何重绑到新 canonical 几何，并把这个
    /// 字段推进到当前 revision。`build_render_plan_full` 只接受 basis revision 不旧于
    /// 当前 canonical revision 的 unit，避免过期绝对坐标混进 scene graph。
    ///
    /// byte range + shaping identity 仍保留在 `PreparedVisualUnit` / `AnimatedSlice`，
    /// 作为到下一份 canonical layout 里重新找目标几何的锚点。
    pub layout_basis_revision: LayoutRevision,
}

impl PreparedTextVisualTransaction {
    pub fn is_composition(&self) -> bool {
        matches!(
            self.operation_kind,
            TextVisualOperationKind::CompositionUpdate
                | TextVisualOperationKind::CompositionCommitOrCancel
        )
    }

    pub fn is_expired(&self, now: Instant) -> bool {
        let Some(effective_start) = self.timeline.effective_start() else {
            return false;
        };
        let elapsed = now.duration_since(effective_start).as_millis() as u64;
        let effective_duration =
            self.timeline.duration_ms + self.timeline.accumulated_paused_duration_ms;
        let timeout = effective_duration * 3 + 500;
        elapsed > timeout
    }

    pub fn progress(&self, now: Instant) -> f64 {
        self.timeline.progress(now)
    }

    /// Issue #735 评论 5773604666 问题3: 收口本事务的 CaretDriven units。
    ///
    /// 当正文 edit motion 失去 caret ownership（`cursor_owner_epoch` 不再等于
    /// 当前 epoch）时调用。把所有 CaretDriven unit（InsertReveal/DeleteConceal）
    /// 的 `start_fraction` 设为 `target_fraction`（终态）：
    /// - InsertReveal: `start_fraction = 1.0`（完全可见）
    /// - DeleteConceal: `start_fraction = 0.0`（完全消失）
    ///
    /// ReflowMove/ReflowCrossFade（Timed unit）保留不动，它们作为独立
    /// passive reflow track 继续播完自己的几何插值。
    ///
    /// 调用后 CaretDriven units 立即落到 canonical final state，不再继续播。
    /// 如果事务中还有 Timed unit，事务不立即 Completed（等 Reflow 播完）；
    /// 如果没有 Timed unit，事务可在下一帧 Completed。
    pub(crate) fn retire_caret_driven_units(&mut self) {
        for unit in &mut self.units {
            match unit.slice.kind {
                AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal => {
                    // CaretDriven unit: 把 start_fraction 设为 target_fraction（终态）。
                    if let VisualUnitTiming::CaretDriven {
                        start_fraction,
                        target_fraction,
                    } = &mut unit.timing
                    {
                        *start_fraction = *target_fraction;
                    }
                }
                AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
                    // Timed unit: 保留不动，作为独立 passive reflow track 继续。
                }
            }
        }
    }

    /// Issue #735 评论 5773604666 问题3: 判断本事务是否还有未播完的 Timed unit
    ///（ReflowMove/ReflowCrossFade）。
    ///
    /// 供测试验证收口语义：`retire_caret_driven_units` 后，
    /// - 返回 `false`：没有 Timed unit 或 Timed unit 已全部播完，事务可立即 Completed。
    /// - 返回 `true`：还有 Timed unit 在播，事务需等它们播完再 Completed。
    #[cfg(test)]
    pub(crate) fn has_active_timed_units(&self, now: Instant) -> bool {
        self.units.iter().any(|u| match u.slice.kind {
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
                u.timing.progress(now) < 1.0
            }
            _ => false,
        })
    }

    /// Issue #735 评论 5773604666 问题3: 判断本事务是否含 CaretDriven units
    ///（InsertReveal/DeleteConceal）。
    pub(crate) fn has_caret_driven_units(&self) -> bool {
        self.units.iter().any(|u| {
            matches!(
                u.slice.kind,
                AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal
            )
        })
    }

    /// 采集本事务中尚未播完的视觉单元当前帧，交棒给下一个事务。
    ///
    /// Issue #690 评论 5675007226 步骤 3: 逐单元用自己的 `progress`，不再用事务级
    /// timeline progress 一刀切——后者会把"已经吐到 60%"的单元算成事务的 30%，
    /// 交棒后视觉上仍会跳回一半。已播完（progress >= 1）的单元已是稳定终态，不采集。
    ///
    /// Issue #690 评论 5679744253 问题 1: 采集时计算剩余时长，retarget 时从当前帧
    /// 重新起一段，避免同时继承可见比例和已走过的时间线导致进度被重复应用。
    ///
    /// Issue #722 评论 5749164244 问题3: 生产路径（take_rebase_frames）不再调用此方法，
    /// 改用 `animation_coordinator::collect_rebase_frame_for_unit_without_caret` 对 Reveal/Conceal
    /// 从 caret track progress 推导 visible_fraction。此方法保留供 #690 测试验证 per-unit progress 行为。
    ///
    /// Issue #727 约束 2+3: CaretDriven unit 的 visible_fraction 从 caret track progress 推导，
    /// remaining_duration_ms 从 caret track 剩余时长计算。
    #[cfg(test)]
    pub fn collect_rebase_frames(&self, now: Instant) -> Vec<RebaseFrame> {
        let caret_track_progress = self
            .cursor_visual_track
            .as_ref()
            .map(|track| track.progress(now));
        let caret_remaining_ms = self
            .cursor_visual_track
            .as_ref()
            .map(|track| track.remaining_duration_ms(now))
            .unwrap_or(0);
        self.units
            .iter()
            .filter(|unit| {
                if unit.is_finished(now) {
                    return false;
                }
                // Issue #727 约束 2: CaretDriven unit 的终态由 caret track progress 决定。
                // is_finished() 只看 start_fraction == target_fraction，
                // 但 CaretDriven unit 的可见比例从 caret track progress 推导，
                // caret track progress >= 1.0 时 unit 已播完，不应再交棒。
                if unit.timing.is_caret_driven() {
                    if let Some(progress) = caret_track_progress {
                        if progress >= 1.0 {
                            return false;
                        }
                    }
                }
                true
            })
            .map(|unit| {
                let visible_fraction = match unit.slice.kind {
                    AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal => {
                        let progress = caret_track_progress.unwrap_or(0.0);
                        let eased = AnimatedSlice::ease_out_quad(progress);
                        let start = unit.timing.start_fraction();
                        let target = unit.timing.target_fraction();
                        start + (target - start) * eased
                    }
                    AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
                        unit.current_visible_fraction(now)
                    }
                };
                let frame = unit.slice.compute_frame(visible_fraction);
                // Issue #727 约束 2: 通过 VisualUnitTiming 访问 started_at / duration_ms。
                // CaretDriven unit 无独立时间线，remaining_duration_ms 从 caret track 计算。
                let (elapsed_ms, duration_ms) = match &unit.timing {
                    VisualUnitTiming::CaretDriven { .. } => (0u64, caret_remaining_ms),
                    VisualUnitTiming::Timed {
                        started_at,
                        duration_ms,
                        ..
                    } => {
                        let elapsed = match started_at {
                            Some(start) => now.duration_since(*start).as_millis() as u64,
                            None => 0,
                        };
                        (elapsed, *duration_ms)
                    }
                };
                let remaining_duration_ms = duration_ms.saturating_sub(elapsed_ms);
                RebaseFrame {
                    byte_start: unit.slice.byte_start,
                    byte_end: unit.slice.byte_end,
                    x: frame.x,
                    y: frame.y,
                    opacity: frame.opacity,
                    shaping_identity: unit.slice.shaping_identity.clone(),
                    visible_fraction,
                    sampled_at: now,
                    remaining_duration_ms,
                }
            })
            .collect()
    }

    pub fn pause(&mut self) {
        if self.state == TextVisualTransactionState::Rendering {
            self.state = TextVisualTransactionState::Paused;
            let now = Instant::now();
            self.timeline.pause(now);
            // Issue #690 评论 5682867529: caret track 跟文字 timeline 一起暂停。
            if let Some(track) = self.cursor_visual_track.as_mut() {
                track.pause(now);
            }
        }
    }

    pub fn resume(&mut self) {
        if self.state == TextVisualTransactionState::Paused {
            let now = Instant::now();
            self.timeline.resume(now);
            // Issue #690 评论 5682867529: caret track 跟文字 timeline 一起恢复。
            if let Some(track) = self.cursor_visual_track.as_mut() {
                track.resume(now);
            }
            self.state = TextVisualTransactionState::Rendering;
        }
    }

    /// Issue #710 评论 5733109905: 冲突检测改为 **current-old 坐标系逐事务映射**。
    ///
    /// 本事务的 `visual_affected_byte_range_new` / units 的 byte range
    /// 都基于**本事务 new 坐标系**（事务应用后的文本）。查询 range `[byte_start, byte_end)`
    /// 基于**current-old 坐标系**（当前事务应用前的文本）。
    ///
    /// 要在同一坐标系比较，需要用 `OffsetMap::build(&本事务.new_text, current_old_text)`
    /// 把本事务 new 坐标系的 range 映射到 current-old 坐标系，再与查询 range 做 overlap。
    /// 本事务的 new_text 取自 `new_snapshot.virtual_text`。
    ///
    /// 如果 `new_snapshot` 为 `None`（无法获取本事务 new_text），退化为保守策略：
    /// 用 `visual_affected_byte_range_old` 直接和查询 range 做 old 坐标系数值比较
    ///（假设 old 坐标系 == current-old，这是无 new_text 时的最佳近似），
    /// units 也退化为裸数值比较。任一侧重叠即判定重叠。
    ///
    /// 映射失败（范围跨映射边界）时保守判定为冲突（返回 true），避免漏判。
    pub fn overlaps_byte_range(
        &self,
        byte_start: usize,
        byte_end: usize,
        current_old_text: &str,
    ) -> bool {
        let tx_new_text = self.new_snapshot.as_ref().map(|s| s.virtual_text.as_str());

        if let Some(tx_new_text) = tx_new_text {
            // 有 new_text：构造 per-tx offset_map（本事务 new 坐标系 → current-old 坐标系）
            let offset_map = writer_core::editor::OffsetMap::build(tx_new_text, current_old_text);

            // 映射 units 的 byte range 到 current-old 坐标系再比较
            for u in &self.units {
                match offset_map.map_old_range_to_new(u.slice.byte_start, u.slice.byte_end) {
                    Some((ms, me)) => {
                        if me > byte_start && ms < byte_end {
                            return true;
                        }
                    }
                    None => return true, // 映射失败，保守判定为冲突
                }
            }

            // 映射 visual_affected_byte_range_new 到 current-old 坐标系再比较
            if let Some((s, e)) = self.visual_affected_byte_range_new {
                match offset_map.map_old_range_to_new(s, e) {
                    Some((ms, me)) => {
                        if me > byte_start && ms < byte_end {
                            return true;
                        }
                    }
                    None => return true, // 映射失败，保守判定为冲突
                }
            }

            false
        } else {
            // new_snapshot 为 None：退化为保守 old 坐标系数值比较
            if let Some((os, oe)) = self.visual_affected_byte_range_old {
                if oe > byte_start && os < byte_end {
                    return true;
                }
            }
            self.units
                .iter()
                .any(|u| u.slice.byte_end > byte_start && u.slice.byte_start < byte_end)
        }
    }

    pub fn snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> = self.units.iter().map(|u| u.slice.snapshot_id).collect();
        ids.sort_by_key(|id| (id.layout_revision, id.paragraph_id, id.visual_line_ordinal));
        ids.dedup();
        ids
    }
}
