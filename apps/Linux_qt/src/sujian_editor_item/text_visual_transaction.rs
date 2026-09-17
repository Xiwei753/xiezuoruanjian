use std::time::Instant;

use writer_core::editor::CursorRect;

use super::animated_slice::{AnimatedSlice, AnimatedSliceKind};
use super::layout_snapshot::{EditorLayoutSnapshot, LineSnapshotId, ShapingIdentity};
use super::static_line_patch::StaticLinePatch;
use super::transaction_key::VisualTransactionKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextVisualTransactionState {
    Pending,
    Prepared,
    Rendering,
    Paused,
    Completed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextVisualOperationKind {
    Insert,
    Delete,
    CompositionUpdate,
    CompositionCommitOrCancel,
}

/// 统一事务时钟 — 文字切片、光标、预输入装饰全部消费同一个 progress。
///
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

    pub fn mark_first_frame(&mut self) {
        let now = Instant::now();
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

/// Issue #690 评论 5675007226 步骤 3: 单个视觉单元，拥有自己的动画生命期。
///
/// 不再让整笔 `PreparedTextVisualTransaction` 单一的 `TransactionTimeline` 同时驱动
/// 所有 slice 的 0→1。每个 unit 保存自己的 `started_at` / `duration_ms`，从自己的
/// 时间线计算 progress；`start_fraction` / `target_fraction` 描述这一帧单元在
/// 0→1 范围内的可视起点/终点：
/// - InsertReveal：`start_fraction` = 当前已吐出比例（0→1），`target_fraction` = 1。
/// - DeleteConceal：`start_fraction` = 当前还剩比例（1→0），`target_fraction` = 0。
/// - ReflowMove / ReflowCrossFade：`start_fraction` = 0，`target_fraction` = 1。
///
/// 快速连续输入时，旧事务被 cancel 并 rebase：匹配的旧 unit 通过 `rebase_from` 把当前
/// `visible_fraction` 写入 `start_fraction`，文字从"已经吐/吞到一半"的位置继续，
/// 而不是重新 0→1 / 1→0。新插入的字追加新的 unit，沿用自己独立的 `started_at`。
/// 只有被新编辑实际覆盖的 unit 才结束/替换。
#[derive(Clone, Debug)]
pub(crate) struct PreparedVisualUnit {
    pub slice: AnimatedSlice,
    pub started_at: Option<Instant>,
    pub duration_ms: u64,
    pub start_fraction: f64,
    pub target_fraction: f64,
}

impl PreparedVisualUnit {
    fn target_for_kind(kind: AnimatedSliceKind) -> f64 {
        match kind {
            AnimatedSliceKind::DeleteConceal => 0.0,
            _ => 1.0,
        }
    }

    /// 新建单元的起点比例 = 这类动画第一帧的可见状态。
    ///
    /// 不能取 `slice.start_fraction`：builder 生成的 slice 该字段恒为 0.0，
    /// 对 `DeleteConceal` 意味着"已经吞完"，被删的字会一帧都不显示。
    /// slice 上的 `start_fraction` 只作为 rebase 交棒的载体（见 `rebase_from_frame`）。
    fn initial_fraction_for_kind(kind: AnimatedSliceKind) -> f64 {
        match kind {
            AnimatedSliceKind::DeleteConceal => 1.0,
            _ => 0.0,
        }
    }

    /// 把一个 `AnimatedSlice` 包成拥有独立生命期的视觉单元。
    ///
    /// `duration_ms` 取自事务（与 `TransactionTimeline::duration_ms` 一致）。
    /// `started_at` 在事务首次进入 Rendering 时由 coordinator 填入。
    pub fn wrap(slice: AnimatedSlice, duration_ms: u64) -> Self {
        let target_fraction = Self::target_for_kind(slice.kind);
        let start_fraction = Self::initial_fraction_for_kind(slice.kind);
        Self {
            slice,
            started_at: None,
            duration_ms,
            start_fraction,
            target_fraction,
        }
    }

    /// 从自己的 `started_at` / `duration_ms` 计算当前 progress（0..1）。
    /// `started_at` 为 `None` 表示尚未开始，返回 0。
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

    /// 单元在 `now` 时刻的真实可见比例（0..1）。
    ///
    /// Issue #690 评论 5675007226 步骤 2+3: 文字帧、协同光标、rebase 采集共用这一个公式，
    /// 全部走 `AnimatedSlice::ease_out_quad`，不再各自施加一遍 easing。
    pub fn current_visible_fraction(&self, now: Instant) -> f64 {
        let eased = AnimatedSlice::ease_out_quad(self.progress(now));
        (self.start_fraction + (self.target_fraction - self.start_fraction) * eased).clamp(0.0, 1.0)
    }

    /// 按旧单元的当前帧续播本单元。
    ///
    /// Issue #690 评论 5675007226 步骤 3: 可见比例成为新单元的起点，时间线沿用旧单元的
    /// `started_at` / `duration_ms`——事务 key 换了也不归零，否则上一笔吐到 60% 的字
    /// 会被重新从 0 吐一遍。
    ///
    /// Issue #690 评论 5679744253 问题 1: 原实现同时继承 `start_fraction`（已走过的可见
    /// 比例）和 `started_at`/`duration_ms`（已走过的时间线），下一帧 progress 用旧时间线
    /// 算，再从 `start_fraction` 到 target 做 easing，进度被重复应用。现在改为从当前帧
    /// 重新起一段：`start_fraction` 已是当前可见比例，`duration_ms` 用剩余时长，
    /// 不再沿用旧起始时间。
    ///
    /// Issue #690 评论 5683759796: `started_at` 留 `None`，不再写成 `Some(frame.sampled_at)`。
    /// `frame.sampled_at` 是旧事务交棒时刻（t0），新事务此时通常还在 Pending/Prepared，
    /// 直接用它会让 rebased 文字 unit 从 t0 起跑，而 caret track（`rebase_to` /
    /// `new_first` / handoff 全部 `started_at = None`）等到进入 Rendering 才用
    /// `sample.frame_now` 启动，第一帧文字 progress > 0 而 caret track progress = 0，
    /// 造成"文字已经走了一截，光标才刚起步"的错拍。改成 `None` 后，rebased unit 跟
    /// fresh unit、caret track 一样，由 `build_text_animation_plan_with_sample` 在
    /// Prepared→Rendering 时用同一个 `sample.frame_now` 启动，三者同帧起跑。
    pub fn rebase_from_frame(&mut self, frame: &RebaseFrame) {
        self.slice
            .rebase_from(frame.x, frame.y, frame.opacity, frame.visible_fraction);
        self.start_fraction = self.slice.start_fraction;
        // Issue #690 评论 5683759796: 从当前帧重新起一段：start_fraction 已是当前可见比例，
        // duration 用剩余时长；started_at 留 None，等进入 Rendering 再用 sample.frame_now 启动，
        // 不再用 frame.sampled_at（旧事务交棒时刻）提前计时。
        self.started_at = None;
        self.duration_ms = frame.remaining_duration_ms.max(1);
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
/// 收成一个完整 caret track 后，两处问题一起消失：
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
            baseline_y: self.to.baseline_y,
        }
    }

    /// Issue #702: 用外部传入的 progress（来自文字 unit 的可见进度）采样 caret rect，
    /// 而非 caret track 自己的 timeline。消除删除事务里 caret track 与 DeleteConceal
    /// unit 帧基准分叉导致的"光标先完成、旧字晚消失"错拍。
    pub fn sampled_rect_at_progress(&self, progress: f64) -> CursorRect {
        let eased = AnimatedSlice::ease_out_quad(progress.clamp(0.0, 1.0));
        let x = self.from.x + (self.to.x - self.from.x) * eased;
        let top = self.from.top + (self.to.top - self.from.top) * eased;
        let h = self.to.bottom - self.to.top;
        CursorRect {
            x,
            top,
            bottom: top + h,
            baseline_y: self.to.baseline_y,
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
    pub fn new_first(from: CursorRect, to: CursorRect, duration_ms: u64) -> Self {
        Self {
            from,
            to,
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
#[cfg(test)]
impl PreparedCursorVisualTrack {
    /// 从当前帧重新起一段：`from = sampled caret`，`to = new_to`，
    /// `started_at = None`（等进入 Rendering 再启动），`duration_ms = 旧 track 剩余时长`（至少 1ms 保证非零）。
    pub fn rebase_to(&self, new_to: CursorRect, now: Instant) -> Self {
        let sampled = self.sampled_rect(now);
        let remaining = self.remaining_duration_ms(now).max(1);
        Self {
            from: sampled,
            to: new_to,
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
    pub static_patches: Vec<StaticLinePatch>,
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

    /// 采集本事务中尚未播完的视觉单元当前帧，交棒给下一个事务。
    ///
    /// Issue #690 评论 5675007226 步骤 3: 逐单元用自己的 `progress`，不再用事务级
    /// timeline progress 一刀切——后者会把"已经吐到 60%"的单元算成事务的 30%，
    /// 交棒后视觉上仍会跳回一半。已播完（progress >= 1）的单元已是稳定终态，不采集。
    ///
    /// Issue #690 评论 5679744253 问题 1: 采集时计算剩余时长，retarget 时从当前帧
    /// 重新起一段，避免同时继承可见比例和已走过的时间线导致进度被重复应用。
    pub fn collect_rebase_frames(&self, now: Instant) -> Vec<RebaseFrame> {
        self.units
            .iter()
            .filter(|unit| unit.progress(now) < 1.0)
            .map(|unit| {
                let visible_fraction = unit.current_visible_fraction(now);
                let frame = unit.slice.compute_frame(visible_fraction);
                // 旧单元剩余的播放时长：duration - elapsed，下溢保护为 0。
                let elapsed_ms = match unit.started_at {
                    Some(start) => now.duration_since(start).as_millis() as u64,
                    None => 0,
                };
                let remaining_duration_ms = unit.duration_ms.saturating_sub(elapsed_ms);
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

    pub fn overlaps_byte_range(&self, byte_start: usize, byte_end: usize) -> bool {
        self.units
            .iter()
            .any(|u| u.slice.byte_end > byte_start && u.slice.byte_start < byte_end)
            || self
                .static_patches
                .iter()
                .any(|p| p.intersects(byte_start, byte_end))
    }

    pub fn snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> = self.units.iter().map(|u| u.slice.snapshot_id).collect();
        for patch in &self.static_patches {
            ids.push(patch.snapshot_id);
        }
        ids.sort_by_key(|id| (id.layout_revision, id.paragraph_id, id.visual_line_ordinal));
        ids.dedup();
        ids
    }
}

/// Linux 当前唯一事务队列。
///
/// 冲突判断基于 byte range 与仍活跃的视觉资源，不是简单"新输入清空旧动画"：
/// 新事务的 byte range 与现有事务的 slices/patches 有重叠时，先从旧事务的当前视觉帧
/// rebase（保证连续输入无跳变），再取消旧事务。
///
/// rebase 语义：新事务的 old_snapshot 取自被取消事务的当前视觉帧而非原始快照，
/// 使动画起点与用户当前看到的画面一致。
#[derive(Clone, Debug, Default)]
pub(crate) struct PreparedTransactionQueue {
    transactions: Vec<PreparedTextVisualTransaction>,
}

impl PreparedTransactionQueue {
    pub fn new() -> Self {
        Self {
            transactions: Vec::new(),
        }
    }

    pub fn enqueue(&mut self, tx: PreparedTextVisualTransaction) {
        self.transactions.push(tx);
    }

    /// Issue #679 评论 5657313927: 把"资源准备完成"和"Pending -> Prepared 状态推进"
    /// 绑在同一个入口，避免事务卡在 Pending 导致光标不移动、静态层错位。
    ///
    /// 状态链：`Pending -> Prepared -> Rendering -> Completed/Cancelled`。
    /// 已 Completed/Cancelled 的事务不再推进，返回 false。
    pub fn mark_prepared(&mut self, key: VisualTransactionKey) -> bool {
        let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) else {
            return false;
        };
        if matches!(
            tx.state,
            TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
        ) {
            return false;
        }

        tx.texture_prepared = true;
        if tx.state == TextVisualTransactionState::Pending {
            tx.state = TextVisualTransactionState::Prepared;
        }
        true
    }

    pub fn complete(&mut self, key: VisualTransactionKey) -> Option<Vec<LineSnapshotId>> {
        if let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) {
            tx.state = TextVisualTransactionState::Completed;
            let ids = tx.snapshot_ids();
            self.transactions.retain(|t| t.key != key);
            return Some(ids);
        }
        None
    }

    pub fn cancel(&mut self, key: VisualTransactionKey, reason: &str) -> bool {
        if let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) {
            tx.state = TextVisualTransactionState::Cancelled;
            tx.cancel_reason = Some(reason.to_string());
            self.transactions.retain(|t| t.key != key);
            return true;
        }
        false
    }

    pub fn cancel_all(&mut self, reason: &str) {
        for tx in &mut self.transactions {
            tx.state = TextVisualTransactionState::Cancelled;
            tx.cancel_reason = Some(reason.to_string());
        }
        self.transactions.clear();
    }

    pub fn tick(&mut self, now: Instant) -> Vec<VisualTransactionKey> {
        let mut expired = Vec::new();
        for tx in &mut self.transactions {
            if tx.is_expired(now) {
                tx.state = TextVisualTransactionState::Cancelled;
                tx.cancel_reason = Some("expired".to_string());
                expired.push(tx.key);
            }
        }
        self.transactions.retain(|t| !expired.contains(&t.key));
        expired
    }

    pub fn find_conflicting_transaction(
        &self,
        byte_start: usize,
        byte_end: usize,
    ) -> Option<VisualTransactionKey> {
        self.transactions
            .iter()
            .filter(|t| {
                t.state != TextVisualTransactionState::Cancelled
                    && t.state != TextVisualTransactionState::Completed
            })
            .find(|t| {
                t.overlaps_byte_range(byte_start, byte_end) || t.is_composition()
            })
            .map(|t| t.key)
    }

    pub fn active_transactions(&self) -> &[PreparedTextVisualTransaction] {
        &self.transactions
    }

    pub fn active_transactions_mut(&mut self) -> &mut [PreparedTextVisualTransaction] {
        &mut self.transactions
    }

    /// Issue #702 评论 5708436497: 只认 `TextVisualOperationKind::Insert`。
    ///
    /// 之前这里只判断 state 不是 Completed/Cancelled，没有过滤 `operation_kind`，
    /// 导致 Delete / CompositionUpdate / CompositionCommitOrCancel 事务也会返回 true，
    /// 被 `qquickitem_impl.rs` / `editing.rs` / `properties.rs` 里"输入期间抑制 blink"
    /// 的逻辑错误当成 Insert。现在明确过滤 Insert，让本方法名与实现一致。
    /// 任意正文事务的判断由 `has_active_text_transaction()` 负责。
    pub fn has_active_insert(&self) -> bool {
        self.transactions.iter().any(|t| {
            t.operation_kind == TextVisualOperationKind::Insert
                && t.state != TextVisualTransactionState::Cancelled
                && t.state != TextVisualTransactionState::Completed
        })
    }

    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }
}
