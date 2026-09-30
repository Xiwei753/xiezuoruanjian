use std::time::Instant;

use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};

/// 统一事务时钟 — 文字切片、光标、预输入装饰全部消费同一个 progress。
///
/// Issue #808: 文字动画继续单独算 `current_visible_fraction()`，不要恢复 CaretDriven，
/// 也不要从 cursor track progress 推文字 visible fraction。文字自己的 easing
///（`ease_out_quad`）留在文字 timeline 里；光标 track 用自己的 easing（`ease_out_cubic`）。
/// Choreographer 和 Qt update 只负责请求帧，不得给光标维护独立开始时间。
/// Paused 状态必须返回暂停瞬间的 progress，不能返回 0。
/// resume 后从暂停进度继续。
#[derive(Clone, Debug)]
pub(crate) struct TransactionTimeline {
    pub duration_ms: u64,
    pub first_render_frame: Option<Instant>,
    /// `first_render_frame` 对应的 Unix 毫秒，供诊断事件使用
    /// （`Instant` 只有本进程意义，不能直接进日志）。
    pub first_render_wall_ms: Option<i64>,
    pub rendering_started_at: Option<Instant>,
    pub pause_start: Option<Instant>,
    pub accumulated_paused_duration_ms: u64,
    pub paused_progress: f64,
}

impl TransactionTimeline {
    pub fn new(duration_ms: u64) -> Self {
        Self {
            duration_ms,
            first_render_frame: None,
            first_render_wall_ms: None,
            rendering_started_at: None,
            pause_start: None,
            accumulated_paused_duration_ms: 0,
            paused_progress: 0.0,
        }
    }

    pub fn progress(&self, now: Instant) -> f64 {
        let Some(start) = self.effective_start() else {
            return 0.0;
        };
        if self.duration_ms == 0 {
            return 1.0;
        }
        if self.pause_start.is_some() {
            return self.paused_progress;
        }
        let elapsed_ms = now.duration_since(start).as_millis() as f64;
        let adjusted = elapsed_ms - self.accumulated_paused_duration_ms as f64;
        (adjusted / self.duration_ms as f64).clamp(0.0, 1.0)
    }

    /// Issue #727 评论 5760431554 问题2: 接收外部统一 `now`（frame_now），
    /// 不再内部 `Instant::now()`。transaction timeline / Timed units /
    /// cursor track 全部消费同一个 frame_now，消除首帧两套起点的采样偏差。
    /// `first_render_wall_ms` 诊断墙钟时间继续单独取，不影响动画时钟。
    pub fn mark_first_frame(&mut self, now: Instant) {
        if self.first_render_frame.is_none() {
            self.first_render_frame = Some(now);
            self.first_render_wall_ms = Some(crate::sujian_editor_item::diagnostic_now_ms());
        }
        if self.rendering_started_at.is_none() {
            self.rendering_started_at = Some(now);
        }
    }

    pub fn pause(&mut self, now: Instant) {
        if self.pause_start.is_some() {
            return;
        }
        self.paused_progress = self.progress(now);
        self.pause_start = Some(now);
    }

    pub fn resume(&mut self, now: Instant) {
        if let Some(pause_start) = self.pause_start.take() {
            self.accumulated_paused_duration_ms +=
                now.duration_since(pause_start).as_millis() as u64;
        }
    }

    pub fn is_started(&self) -> bool {
        self.first_render_frame.is_some()
    }

    pub fn effective_start(&self) -> Option<Instant> {
        self.rendering_started_at.or(self.first_render_frame)
    }
}

/// Issue #727 评论 5754041813 约束 2 / Issue #808: 视觉单元的计时语义。
///
/// Issue #808: 文字动画继续单独算 `current_visible_fraction()`，不要恢复 CaretDriven，
/// 也不要从 cursor track progress 推文字 visible fraction。文字自己的 easing
///（`ease_out_quad`）留在文字 timeline 里。所有文字 unit（含协同模式
/// InsertReveal/DeleteConceal）统一用 `Timed` timing，拥有独立
/// started_at / duration_ms / progress。协同只表示同事务/同首帧/同 rebase，
/// 不表示同速度/同曲线——文字与 caret 各自按自己的 duration/easing 推进。
#[derive(Clone, Debug)]
pub(crate) enum VisualUnitTiming {
    /// 所有视觉单元（InsertReveal / DeleteConceal / ReflowMove / ReflowCrossFade）
    /// 统一使用独立时间线。Issue #808: 文字用 `ease_out_quad`，光标 track 用 `ease_out_cubic`，
    /// 两条时间线完全独立。协同不再切 CaretDriven，不再共用同一条 easing。
    Timed {
        started_at: Option<Instant>,
        duration_ms: u64,
        start_fraction: f64,
        target_fraction: f64,
    },
}

impl VisualUnitTiming {
    /// 从 `AnimatedSliceKind` 推断默认计时语义。
    /// Issue #785: 所有 kind 统一返回 `Timed`（含 InsertReveal/DeleteConceal）。
    pub(crate) fn default_for_kind(kind: AnimatedSliceKind, duration_ms: u64) -> Self {
        let target_fraction = match kind {
            AnimatedSliceKind::DeleteConceal => 0.0,
            _ => 1.0,
        };
        let start_fraction = match kind {
            AnimatedSliceKind::DeleteConceal => 1.0,
            _ => 0.0,
        };
        VisualUnitTiming::Timed {
            started_at: None,
            duration_ms,
            start_fraction,
            target_fraction,
        }
    }

    /// Issue #756 / Issue #785 / Issue #808 评论 5916391891 修改 4: 按 `coordinated` 决定
    /// InsertReveal/DeleteConceal 的计时语义。
    ///
    /// Issue #808 评论 5916391891 修改 4: `coordinated` 不再影响 timing 选择（所有 kind
    /// 都返回 `Timed`），但**不再被完全忽略**——coordinated 的遮罩语义通过 slice 的
    /// `is_caret_line` / `caret_anchor_x` 字段体现（在 `build_insert_reveal_slices` /
    /// `build_delete_conceal_slices` 中设置）：
    /// - coordinated=true：slice 的 caret_anchor_x 取真实 caret x，is_caret_line=true
    ///   （文字从 caret 处吐出/被 caret 吞进）。
    /// - coordinated=false：slice 的 caret_anchor_x 取文字自己的边缘，is_caret_line=false
    ///   （遮罩从文字边缘展开，不用 caret 锚点）。
    /// timing 本身不需要区分 coordinated——文字与 caret 各自按自己的 duration 推进，
    /// 拥有独立 started_at / duration_ms / progress。
    pub(crate) fn default_for_kind_with_coordinated(
        kind: AnimatedSliceKind,
        duration_ms: u64,
        coordinated: bool,
    ) -> Self {
        // coordinated 的遮罩语义通过 slice 字段体现（见上方文档注释），timing 不分叉。
        // 保留参数签名避免大量调用点编译错误；`let _ = coordinated` 明确标记不在此处使用。
        let _ = coordinated;
        Self::default_for_kind(kind, duration_ms)
    }

    /// Issue #785: 是否为 CaretDriven（由 caret frame 驱动裁切的吞吐字）。
    ///
    /// 删除 CaretDriven 变体后始终返回 `false`。保留方法签名避免大量调用点
    /// 编译错误；调用方拿到 false 后会走 Timed 路径（独立时间线驱动裁切）。
    pub(crate) fn is_caret_driven(&self) -> bool {
        false
    }

    /// 获取 `start_fraction`（rebase 交棒载体）。
    pub fn start_fraction(&self) -> f64 {
        match self {
            VisualUnitTiming::Timed { start_fraction, .. } => *start_fraction,
        }
    }

    /// 获取 `target_fraction`。
    pub fn target_fraction(&self) -> f64 {
        match self {
            VisualUnitTiming::Timed {
                target_fraction, ..
            } => *target_fraction,
        }
    }

    /// 从自己的 `started_at` / `duration_ms` 计算当前 progress（0..1）。
    /// Issue #785: 所有 unit 都是 Timed，统一从自己的时间线算 progress。
    pub fn progress(&self, now: Instant) -> f64 {
        match self {
            VisualUnitTiming::Timed {
                started_at,
                duration_ms,
                ..
            } => match started_at {
                None => 0.0,
                Some(start) => {
                    if *duration_ms == 0 {
                        return 1.0;
                    }
                    let elapsed = now.duration_since(*start).as_millis() as f64;
                    (elapsed / *duration_ms as f64).clamp(0.0, 1.0)
                }
            },
        }
    }

    /// 单元在 `now` 时刻的真实可见比例（0..1）。
    ///
    /// Issue #785: 所有 unit 都是 Timed，统一从自己的时间线算可见比例：
    /// `start_fraction + (target_fraction - start_fraction) * ease_out_quad(progress)`。
    pub fn current_visible_fraction(&self, now: Instant) -> f64 {
        match self {
            VisualUnitTiming::Timed {
                start_fraction,
                target_fraction,
                ..
            } => {
                let eased = AnimatedSlice::ease_out_quad(self.progress(now));
                (start_fraction + (target_fraction - start_fraction) * eased).clamp(0.0, 1.0)
            }
        }
    }

    /// Issue #690 评论 5683759796: 在事务进入 Rendering 时打上统一起始时间。
    /// Issue #785: 所有 unit 都是 Timed，统一设置 started_at。
    pub fn mark_started(&mut self, frame_now: Instant) {
        let VisualUnitTiming::Timed { started_at, .. } = self;
        if started_at.is_none() {
            *started_at = Some(frame_now);
        }
    }

    /// Issue #690 评论 5683759796 / Issue #785: rebase 交棒时更新可见比例载体。
    ///
    /// 所有 unit 都是 Timed：更新 start_fraction + 重置时间线
    ///（started_at = None, duration = remaining）。文字从当前 `visible_fraction`
    /// 继续，不从 0 重播。
    pub fn rebase_from_frame(&mut self, visible_fraction: f64, remaining_duration_ms: u64) {
        match self {
            VisualUnitTiming::Timed {
                start_fraction,
                started_at,
                duration_ms,
                ..
            } => {
                *start_fraction = visible_fraction.clamp(0.0, 1.0);
                *started_at = None;
                *duration_ms = remaining_duration_ms.max(1);
            }
        }
    }
}
