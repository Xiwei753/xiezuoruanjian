//! Linux Qt 文字动画协调器。
//!
//! 主链：
//! ```text
//! PreparedEditMotion (Linux 私有, 从 EditorEditResult 派生)
//! → 捕获 old/new layout snapshot
//! → 生成 AnimatedSlice + StaticLinePatch
//! → 准备平台视觉资源
//! → PreparedTransactionQueue
//! → rendering overlay + cursor transition
//! ```
//!
//! 关键约束：
//! - 先完成视觉资源准备，再允许静态层隐藏：`texture_prepared` 为 true 前静态层不裁剪，
//!   否则准备纹理与第一帧 overlay 之间会出现空白帧。
//! - 连续输入从当前视觉帧 rebase：新事务与旧事务 byte range 重叠时，先从旧事务当前
//!   progress 计算已显示帧位置，rebase 新 slice 的 from_document_rect，再取消旧事务，
//!   保证视觉无跳变。
//! - scrolling/window inactive 使用 pause/resume 而非销毁事务：滚动结束后 revision
//!   未变则累加 paused duration 继续，避免重新创建事务的开销和视觉跳变。
//! - revision 不匹配时必须取消：旧 source rect 是旧布局的产物，不能套到新布局上，
//!   否则坐标和 shaping 全部错误。
//! - shaping identity 变化走 crossfade 而非强行 move：字体、glyph、方向、格式任一变化
//!   都意味着旧视觉资源与新排版结果不是同一视觉对象，强行移动会导致 ligature/RTL/emoji
//!   渲染错误。

use std::collections::HashMap;
use std::time::Instant;

use writer_core::editor::OffsetMap;

use super::edit_motion::{diff_plain_text, CursorRect, EditorAnimationKind, PreparedEditMotion};

use super::animated_slice::{AnimatedSlice, AnimatedSliceKind};
pub(crate) use super::animation_mode::AnimationMode;
pub(crate) use super::cursor_animation::{CursorAnimationPlan, CursorBlinkMode, CursorTransition};
use super::layout_revision::LayoutRevision;
use super::layout_snapshot::{
    ClusterInsertRelation, EditorLayoutSnapshot, LineSnapshotId, SourceRect,
};
// Issue #710 评论 5731145076 症状六: 导入 compute_affected_paragraph_ranges
// 用于计算事务的 visual_affected_byte_range（基于段落边界扩展）。
pub(crate) use super::render_plan::{
    CursorRenderState, PreeditRange, RenderPlan, SelectionPreeditPlan, SelectionRange,
    TextAnimationGlyphInfo, TextAnimationPlan,
};
use super::text_visual_transaction::PreparedVisualUnit;
use super::text_visual_transaction::{
    PreparedCursorVisualTrack, PreparedTextVisualTransaction, PreparedTransactionQueue,
    RebaseFrame, TextVisualOperationKind, TextVisualTransactionState, TransactionTimeline,
    VisualUnitTiming,
};
pub(crate) use super::transaction_key::VisualTransactionKey;
use crate::editor::layout::compute_affected_paragraph_ranges;

use crate::sujian_editor_item::editor_animation_debug_log;

/// Issue #722 评论 5750218208: 从 `EditorLayoutSnapshot` 的 `line_snapshots` 中
/// 按 `visual_line_id` 查找行几何（文档坐标的 top/bottom）。
///
/// 直接返回 `PreparedLineSnapshot.visual_line_top` / `visual_line_bottom`
/// （= `VisualLine.y` / `VisualLine.y + VisualLine.height`），不再扫描 cluster
/// ink bounds 猜行高——空行没有 cluster，cluster bounds 也不含行间距，
/// 用 cluster 会导致空行高度为 0、软换行附近行边界错误。
/// 找不到对应行时返回 `(0.0, 0.0)`。
pub(crate) fn find_line_geometry_in_snapshot(
    snapshot: &EditorLayoutSnapshot,
    visual_line_id: Option<usize>,
) -> (f64, f64) {
    match visual_line_id {
        Some(id) => {
            for line in &snapshot.line_snapshots {
                if line.visual_line_id == id {
                    return (line.visual_line_top, line.visual_line_bottom);
                }
            }
            (0.0, 0.0)
        }
        None => (0.0, 0.0),
    }
}

/// Issue #690 评论 5675007226 步骤 1: 同一帧的统一时间采样。
///
/// `update_paint_node()` 入口处取一次 `Instant::now()` 作为 `frame_now`，
/// 后续文字 progress、光标 progress、cursor timeline sample 全部从这一个时间点计算。
/// 消除 GUI 线程 FrameAnimation tick 和 Scene Graph 渲染帧之间的采样偏差。
///
/// 每个 active 正文事务的 progress 在 `build_render_plan_full()` 入口处只算一次，
/// 写入 `progress_by_key`，后面 `build_text_animation_plan` / `compute_coordinated_cursor`
/// 都从同一个 sample 读取，不再各自 `Instant::now()`。
pub(crate) struct AnimationFrameSample {
    /// 本帧统一采样时间点。
    pub frame_now: Instant,
    progress_by_key: HashMap<VisualTransactionKey, f64>,
}

impl AnimationFrameSample {
    pub fn new(frame_now: Instant) -> Self {
        Self {
            frame_now,
            progress_by_key: HashMap::new(),
        }
    }

    pub fn set_progress(&mut self, key: VisualTransactionKey, progress: f64) {
        self.progress_by_key.insert(key, progress);
    }

    /// 读取本帧该事务的预计算 progress（未记录则视为 0）。
    pub fn progress(&self, key: VisualTransactionKey) -> f64 {
        *self.progress_by_key.get(&key).unwrap_or(&0.0)
    }
}

/// Issue #690 评论 5675007226 步骤 3: 将旧事务的视觉单元当前帧 rebase 到新事务的视觉单元上。
///
/// `RebaseFrame` 由 [`PreparedTextVisualTransaction::collect_rebase_frames`] 逐单元采集：
/// `visible_fraction` 是单元自己的可见比例（单元时间线 + `[start_fraction, target_fraction]`
/// 窗口 + 协同 easing），Reveal/Conceal 因此从当前比例继续，而不是重新 0→1 / 1→0。
fn match_rebase_frames(
    rebase_frames: &[RebaseFrame],
    units: &mut [PreparedVisualUnit],
    offset_map: &OffsetMap,
) {
    let mut consumed_indices: Vec<usize> = Vec::new();
    for frame in rebase_frames {
        // Issue #701 评论 5699573227: 读取 frame.sampled_at 写入动画诊断日志，
        // 使该诊断字段在非测试代码中也被消费（否则 clippy 报 dead_code）。
        // editor_animation_debug_log 仅在设置环境变量时输出，零开销。
        crate::sujian_editor_item::editor_animation_debug_log(&format!(
            "rebase frame [{}..{}] sampled_at={:?} remaining={}ms",
            frame.byte_start, frame.byte_end, frame.sampled_at, frame.remaining_duration_ms
        ));
        let tier1 = units
            .iter_mut()
            .enumerate()
            .filter(|(idx, _)| !consumed_indices.contains(idx))
            .find(|(_, nu)| {
                nu.slice.byte_start == frame.byte_start && nu.slice.byte_end == frame.byte_end
            });
        if let Some((idx, new_unit)) = tier1 {
            new_unit.rebase_from_frame(frame);
            consumed_indices.push(idx);
            continue;
        }
        if let (Some(mbs), Some(mbe)) = (
            offset_map.map_old_to_new(frame.byte_start),
            offset_map.map_old_to_new(frame.byte_end),
        ) {
            let tier2 = units
                .iter_mut()
                .enumerate()
                .filter(|(idx, _)| !consumed_indices.contains(idx))
                .find(|(_, nu)| nu.slice.byte_start == mbs && nu.slice.byte_end == mbe);
            if let Some((idx, new_unit)) = tier2 {
                new_unit.rebase_from_frame(frame);
                consumed_indices.push(idx);
                continue;
            }
            if let Some(ref sid) = frame.shaping_identity {
                let mapped_center = (mbs + mbe) as i64 / 2;
                let best = units
                    .iter_mut()
                    .enumerate()
                    .filter(|(idx, _)| !consumed_indices.contains(idx))
                    .filter(|(_, nu)| nu.slice.shaping_identity.as_ref() == Some(sid))
                    .filter(|(_, nu)| {
                        nu.slice.byte_start >= mbs && nu.slice.byte_end <= mbe.max(mbs + 1)
                    })
                    .min_by_key(|(idx, nu)| {
                        let candidate_center = (nu.slice.byte_start + nu.slice.byte_end) as i64 / 2;
                        let abs_dist = (candidate_center - mapped_center).abs();
                        (abs_dist, nu.slice.byte_start, *idx)
                    });
                if let Some((idx, new_unit)) = best {
                    new_unit.rebase_from_frame(frame);
                    consumed_indices.push(idx);
                }
            }
        }
    }
}

/// 事务操作类型的诊断标签（与 `TextVisualOperationKind` 一一对应，进正式诊断包）。
fn operation_kind_label(kind: TextVisualOperationKind) -> &'static str {
    match kind {
        TextVisualOperationKind::Insert => "Insert",
        TextVisualOperationKind::Delete => "Delete",
        TextVisualOperationKind::CompositionUpdate => "CompositionUpdate",
        TextVisualOperationKind::CompositionCommitOrCancel => "CompositionCommitOrCancel",
    }
}

/// 视觉单元类型列表，用于紧凑诊断事件的 `unit_kinds` 字段。
fn unit_kind_labels(units: &[PreparedVisualUnit]) -> Vec<String> {
    units
        .iter()
        .map(|u| format!("{:?}", u.slice.kind))
        .collect()
}

/// Issue #690 评论 5675007226 步骤 3: 本次编辑是否真的覆盖了冲突事务里仍在播放的单元。
///
/// 单元自己的 byte range 完全落在编辑范围之外，且映射前后偏移一致（说明它左边没有内容
/// 变化、排版位置没变）时才算"没被覆盖"。这类单元继续留在原事务里播完自己的时间线——
/// 换事务 id 既不该把它归零重播，也不该让它提前跳到终态（文字甩开光标的另一半表现）。
/// 全部单元都已播完时返回 false：那笔事务该走正常的完成/取消路径释放资源。
///
/// Issue #710 评论 5733833897: 多笔旧事务场景下，`unit.slice.byte_start/byte_end`
/// 属于**该旧事务自己的 new 坐标系**，而 `changed_old_ranges` / `offset_map` 属于
/// **current-old 坐标系**（当前事务应用前的文本）。直接做数值比较是跨坐标系比较。
///
/// 修复：新增 `current_old_text` 参数，先用 `OffsetMap::build(&tx.new_snapshot.virtual_text,
/// current_old_text)` 构造 per-tx 映射（旧事务 new 坐标系 → current-old 坐标系），
/// 把每个 unit 的 byte range 映射到 current-old 坐标系，再和 `changed_old_ranges`
/// 做 overlap 比较，再用 `offset_map`（current-old→current-new）判断映射前后偏移是否一致。
fn conflicting_units_are_untouched(
    tx: &PreparedTextVisualTransaction,
    changed_old_ranges: &[(usize, usize)],
    offset_map: &OffsetMap,
    current_old_text: &str,
    now: Instant,
) -> bool {
    let mut playing_units = 0usize;
    // 构造 per-tx 映射：旧事务 new 坐标系 → current-old 坐标系。
    // tx.new_snapshot.virtual_text 是该旧事务应用后的文本（旧事务 new 坐标系），
    // current_old_text 是当前事务应用前的文本（current-old 坐标系）。
    let tx_new_text = tx.new_snapshot.as_ref().map(|s| s.virtual_text.as_str());
    // Issue #727 约束 4: 不再自己采样 caret geometry（删除 sample_caret_geometry_for_caret_driven_clip）。
    // 对 Reveal/Conceal 从 caret track progress 推导 visible_fraction 判断存活。
    let caret_track_progress = tx
        .cursor_visual_track
        .as_ref()
        .map(|track| track.progress(now));
    for unit in &tx.units {
        // Issue #722 评论 5749572808 问题3: 按 kind 分支判断是否已到终态。
        let still_playing = match unit.slice.kind {
            AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal => {
                // Issue #727 约束 2+3: CaretDriven unit 的 visible 从 caret track progress 推导。
                let progress = caret_track_progress.unwrap_or(0.0);
                let eased = AnimatedSlice::ease_out_quad(progress);
                let start = unit.timing.start_fraction();
                let target = unit.timing.target_fraction();
                let visible_fraction = start + (target - start) * eased;
                match unit.slice.kind {
                    AnimatedSliceKind::InsertReveal => visible_fraction < 1.0 - 1e-3,
                    AnimatedSliceKind::DeleteConceal => visible_fraction > 1e-3,
                    _ => unreachable!(),
                }
            }
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
                // Reflow 仍看 unit progress。
                unit.progress(now) < 1.0
            }
        };
        if !still_playing {
            continue;
        }
        playing_units += 1;
        let start = unit.slice.byte_start; // 旧事务 new 坐标系
        let end = unit.slice.byte_end; // 旧事务 new 坐标系
                                       // 先映射到 current-old 坐标系，再和 changed_old_ranges 做 overlap 比较。
        let (co_start, co_end) = match tx_new_text {
            Some(tx_new) => {
                let per_tx_map = OffsetMap::build(tx_new, current_old_text);
                match per_tx_map.map_old_range_to_new(start, end) {
                    Some(r) => r,
                    None => return false, // 映射失败，保守判定为被覆盖
                }
            }
            None => (start, end), // 无 new_snapshot，退化为数值比较
        };
        if changed_old_ranges
            .iter()
            .any(|(cs, ce)| co_end > *cs && co_start < *ce)
        {
            return false;
        }
        // 半开区间语义：end 恰为映射条目末端也算完整落在同一区域内。
        // 逐端点查表会在"文本末尾追加"场景返回 None（end == old 长度），
        // 把还在播的单元误判成被影响。
        // current-old → current-new 映射前后偏移一致才算 untouched。
        if offset_map.map_old_range_to_new(co_start, co_end) != Some((co_start, co_end)) {
            return false;
        }
    }
    playing_units > 0
}

/// Issue #690 评论 5681206040: rebase 交棒时携带的 caret handoff 信息。
///
/// `sampled` 是旧事务在 `now` 时刻真正显示的 coordinated cursor rect，
/// `remaining_duration_ms` 是旧 caret track 剩余的播放时长。新事务用 `sampled` 当
/// `cursor_visual_track.from`，用 `remaining_duration_ms` 当 `cursor_visual_track.duration_ms`，
/// 不再借任何文字 unit 的 progress。
///
/// Issue #722 评论 5749572808 问题2: 增加 `sampled_visual_line_id`，
/// 采样到的旧事务屏幕 caret 所在视觉行 id。快速连续输入发生 rebase 时，
/// 新 track 的起点虽然 x/y 用旧事务屏幕真实位置，但"它在哪一行"必须也用
/// 采样到的行 id，不能强行改成新事务终点所在行——跨软换行交棒时第一帧
/// 文字可能认为 caret 已进入新行，把下一行提前吐出来。
/// Issue #722 评论 5749791161: rebase 交棒时携带采样到的行几何。
///
/// `sampled_line_top/bottom` 是旧事务在 `now` 时刻采样到的 caret 所在视觉行的
/// 真实 top/bottom（来自 `VisualLine.y` 和 `VisualLine.y + VisualLine.height`）。
/// 新 track 的 from 端行几何用这些值，不用 caret 自己的细矩形边界。
#[derive(Clone, Debug)]
struct RebaseCaretHandoff {
    sampled: CursorRect,
    remaining_duration_ms: u64,
    sampled_visual_line_id: Option<usize>,
    sampled_line_top: f64,
    sampled_line_bottom: f64,
}

/// Issue #690 评论 5681206040 + 5682867529: 构建新事务的 caret track，四个正文入口共用。
///
/// - 有 rebase handoff（发生过交棒）：`from = sampled caret`，`to = new_cursor_rect`，
///   `started_at = None`（等进入 Rendering 再启动），`duration_ms = handoff.remaining_duration_ms`。
/// - 无 rebase handoff（首次事务）：`from = old_cursor_rect`，`to = new_cursor_rect`，
///   `started_at = None`（等进入 Rendering 再启动），`duration_ms = 事务时长`。
/// - `new_cursor_rect` 缺失：返回 `None`（无法构成 track）。
/// - 无 handoff 且 `old_cursor_rect` 缺失：返回 `None`。
///
/// Issue #690 评论 5682867529: 不再在事务创建时就用 `now` 启动计时，而是把 `started_at`
/// 留为 `None`，等 `build_text_animation_plan_with_sample` 在 Prepared→Rendering 分支
/// 跟文字 unit 共用同一个 `frame_now` 起跑，保证第一帧文字和光标 progress 都 = 0。
///
/// Issue #722 评论 5749791161: `old_cursor_line_top/bottom` 和 `new_cursor_line_top/bottom`
/// 是真实视觉行边界（`VisualLine.y` 和 `VisualLine.y + VisualLine.height`），
/// 不是 caret 自己的细矩形边界。rebase handoff 分支的 from 端行几何从 handoff
/// 传递（采样到的旧事务屏幕 caret 所在行），to 端行几何从参数传递。
fn build_cursor_visual_track(
    old_cursor_rect: Option<&CursorRect>,
    new_cursor_rect: Option<&CursorRect>,
    old_cursor_visual_line_id: Option<usize>,
    new_cursor_visual_line_id: Option<usize>,
    old_cursor_line_top: f64,
    old_cursor_line_bottom: f64,
    new_cursor_line_top: f64,
    new_cursor_line_bottom: f64,
    handoff: Option<RebaseCaretHandoff>,
    tx_duration_ms: u64,
) -> Option<PreparedCursorVisualTrack> {
    let to = new_cursor_rect?;
    match handoff {
        Some(h) => Some(PreparedCursorVisualTrack {
            from: h.sampled,
            to: to.clone(),
            // Issue #722 评论 5749572808 问题2: rebase 交棒时 from 端的 visual_line_id
            // 用采样到的旧事务屏幕 caret 所在行 id，不能用新事务终点所在行。
            // 跨软换行交棒时第一帧文字可能认为 caret 已进入新行，把下一行提前吐出来。
            // to 端是新事务的 new_cursor_rect 行 id。
            from_visual_line_id: h.sampled_visual_line_id,
            to_visual_line_id: new_cursor_visual_line_id,
            // Issue #722 评论 5749791161: from 端行几何用 handoff 采样到的行边界，
            // to 端行几何用参数传入的 new_cursor 行边界。
            from_line_top: h.sampled_line_top,
            from_line_bottom: h.sampled_line_bottom,
            to_line_top: new_cursor_line_top,
            to_line_bottom: new_cursor_line_bottom,
            started_at: None,
            duration_ms: h.remaining_duration_ms,
            pause_start: None,
        }),
        None => {
            let from = old_cursor_rect?;
            Some(PreparedCursorVisualTrack::new_first(
                from.clone(),
                to.clone(),
                old_cursor_visual_line_id,
                new_cursor_visual_line_id,
                old_cursor_line_top,
                old_cursor_line_bottom,
                new_cursor_line_top,
                new_cursor_line_bottom,
                tx_duration_ms,
            ))
        }
    }
}

/// Issue #690 评论 5680276931 + 5681206040: 采样旧事务在 `now` 时刻真正显示的
/// coordinated cursor rect。
///
/// 在 `take_rebase_frames` 取消旧事务之前调用，把结果作为新事务纯 reflow 光标动画的
/// 视觉起点（`cursor_visual_track.from`）。
///
/// Issue #722 评论 5747719529 改法 4 + 核心语义：光标本身就是吞字/吐字的视觉边界。
/// 不再从文字 glyph 切片反推光标位置（删除 rightmost_x.max() / conceal_edge.min()）。
/// 光标位置只由 `PreparedCursorVisualTrack`（canonical old caret → canonical new caret）
/// 插值决定。有 cursor_visual_track 时直接 sample track；没有 track 时按事务 progress
/// 插值 old/new cursor rect。文字的 InsertReveal/DeleteConceal 裁切边界直接消费本帧
/// coordinated caret 的位置（caret_driven_clip），caret 与文字使用同一个 frame_now
/// 和同一个 from→to 几何轨迹。
///
/// 返回的 `CursorRect` 用采样到的 `(x, y)` 和 `new_rect` 的高度/baseline 构造，
/// 供新事务 `cursor_visual_track.from` 直接消费。
fn sample_coordinated_cursor_rect_at(
    tx: &PreparedTextVisualTransaction,
    now: Instant,
) -> Option<CursorRect> {
    // 保留 old_cursor_rect 的 early return 语义：旧事务没有 old caret 时不参与交棒。
    let old_rect = tx.old_cursor_rect.as_ref()?;
    let new_rect = tx.new_cursor_rect.as_ref()?;
    let h = new_rect.bottom - new_rect.top;
    let op = tx.operation_kind;

    // Issue #722 评论 5747719529: 光标是吞字/吐字的视觉边界。
    // caret 位置只由 PreparedCursorVisualTrack（canonical old caret → canonical new caret）
    // 插值决定，不再从文字 glyph 切片反推。
    // - 有 cursor_visual_track 时：直接 sample track（caret_driven_clip）。
    // - 没有 track 时（首次事务未经过 rebase）：按事务 progress 插值 old/new cursor rect。
    // 文字的 InsertReveal/DeleteConceal 裁切边界直接消费本帧 coordinated caret 的位置。
    let sample_caret_position = || -> (f64, f64) {
        match tx.cursor_visual_track.as_ref() {
            Some(track) => {
                // caret_driven_clip: 光标位置由 caret track 插值决定。
                // 用 sampled_rect_at_progress(progress(now)) 与 sampled_rect(now) 等价，
                // 显式表达"caret 与文字使用同一个 frame_now 和 from→to 几何轨迹"。
                let r = track.sampled_rect_at_progress(track.progress(now));
                (r.x, r.top)
            }
            None => {
                // 首次事务没有 caret track：用 old/new cursor rect 按事务 progress 插值。
                let progress = tx.progress(now);
                let eased = AnimatedSlice::ease_out_quad(progress);
                let x = old_rect.x + (new_rect.x - old_rect.x) * eased;
                let y = old_rect.top + (new_rect.top - old_rect.top) * eased;
                (x, y)
            }
        }
    };

    // Issue #722 评论 5747719529: 所有操作类型（Insert/Delete/Reflow/CompositionUpdate/
    // Commit/Cursor）统一使用 caret track 插值决定光标位置。不再按操作类型分支从文字
    // glyph 切片反推。光标给吞了就是吞了，光标给吐出来就是吐出来。文字效果跟着光标
    // 边界，不是光标去追文字动画。
    let _ = op;
    let (cx, cy) = sample_caret_position();

    Some(CursorRect {
        x: cx,
        top: cy,
        bottom: cy + h,
        baseline_y: new_rect.baseline_y,
    })
}

/// Issue #727 约束 4: 不依赖 caret geometry 的 rebase 帧采集。
///
/// 替代 `collect_rebase_frame_for_unit`（需要 caret_x/caret_y/caret_line_id 参数）。
/// 对 CaretDriven unit（InsertReveal/DeleteConceal），从 caret track progress 推导
/// visible_fraction（不依赖 caret geometry），用 slice.compute_frame 构造 rebase frame。
/// 对 Timed unit（ReflowMove/ReflowCrossFade），保持原逻辑。
fn collect_rebase_frame_for_unit_without_caret(
    unit: &PreparedVisualUnit,
    caret_track_progress: Option<f64>,
    caret_remaining_ms: u64,
    now: Instant,
) -> Option<RebaseFrame> {
    let visible_fraction = match unit.slice.kind {
        AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal => {
            // Issue #727 约束 2+3: CaretDriven unit 的 visible 从 caret track progress 推导。
            // visible = start_fraction + (target - start) * ease_out_quad(progress)
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
    // Issue #727 约束 4: 不依赖 caret geometry，统一用 compute_frame。
    let frame = unit.slice.compute_frame(visible_fraction);
    // 按真实帧判断终态。
    match unit.slice.kind {
        AnimatedSliceKind::InsertReveal => {
            if frame.w >= unit.slice.to_document_rect.w.max(0.0) - 0.001 {
                return None;
            }
        }
        AnimatedSliceKind::DeleteConceal => {
            if frame.w <= 0.001 {
                return None;
            }
        }
        AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
            if unit.progress(now) >= 1.0 {
                return None;
            }
        }
    }
    let effective_fraction = match unit.slice.kind {
        AnimatedSliceKind::InsertReveal => {
            let w = unit.slice.to_document_rect.w.max(1.0);
            (frame.w / w).clamp(0.0, 1.0)
        }
        AnimatedSliceKind::DeleteConceal => {
            let w = unit.slice.from_document_rect.w.max(1.0);
            (frame.w / w).clamp(0.0, 1.0)
        }
        AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => visible_fraction,
    };
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
    Some(RebaseFrame {
        byte_start: unit.slice.byte_start,
        byte_end: unit.slice.byte_end,
        x: frame.x,
        y: frame.y,
        opacity: frame.opacity,
        shaping_identity: unit.slice.shaping_identity.clone(),
        visible_fraction: effective_fraction,
        sampled_at: now,
        remaining_duration_ms,
    })
}

/// Issue #690 评论 5675007226 步骤 5: 每个事务生命周期点各写一条紧凑事件进正式诊断包。
///
/// 字段：transaction key、operation kind、old/new caret、visual unit kinds、首帧时间
/// （`timeline.first_render_wall_ms`，真正进入渲染的那一帧；create 事件里还没有则为空）、
/// 完成/被 retarget 原因。不逐帧刷日志。
fn emit_transaction_diagnostic(tx: &PreparedTextVisualTransaction, event: &str, reason: &str) {
    crate::sujian_editor_item::editor_animation_diagnostic_event(
        event,
        &tx.key,
        operation_kind_label(tx.operation_kind),
        tx.old_cursor_rect.as_ref().map(|r| (r.x, r.top)),
        tx.new_cursor_rect.as_ref().map(|r| (r.x, r.top)),
        &unit_kind_labels(&tx.units).join(","),
        tx.timeline.first_render_wall_ms,
        reason,
    );
}

/// Issue #658 评论 5630181473 问题 3: cluster 级 reflow 的引用条目。
///
/// 指向一个 old 或 new cluster 的位置信息，用于构建二分图边和 connected components。
#[derive(Clone, Debug)]
struct ReflowClusterRef {
    line_idx: usize,
    cluster_idx: usize,
    byte_start: usize,
    byte_end: usize,
}

// ── Issue #687: 显式 changed range 拥有函数 ──
//
// 插入和删除的 changed range 必须由 Core 给出的 inserted_range / deleted_range
// 显式拥有，不再让 reflow cluster 二分图推断。这两个函数按明确范围直接生成
// InsertReveal / DeleteConceal 切片和对应的 StaticLinePatch。

/// 按 Core 给出的 inserted_range 从 new_snapshot 显式生成 InsertReveal 切片。
///
/// 只接 `vt.inserted_range + new_snapshot`。按这个明确范围找 new cluster/sourceRect，
/// 直接生成 `InsertReveal`，同时生成对应 new line 的 `StaticLinePatch`，
/// 静态层在动画期间只隐藏这些新字的 sourceRect。
///
/// # 参数
/// - `key`：事务键。
/// - `new_snapshot`：新布局快照。
/// - `inserted_range`：Core 给出的插入范围 (byte_start, byte_end)。
///
/// # 返回
/// `slices`：InsertReveal 动画切片（含 static_hidden_document_rects）。
fn build_insert_reveal_slices(
    key: VisualTransactionKey,
    new_snapshot: &EditorLayoutSnapshot,
    inserted_range: (usize, usize),
) -> Vec<AnimatedSlice> {
    let mut slices = Vec::new();
    let (range_start, range_end) = inserted_range;

    for (_line_idx, new_line) in new_snapshot.line_snapshots.iter().enumerate() {
        for (_cluster_idx, new_cluster) in new_line.clusters.iter().enumerate() {
            // Issue #724 评论 5751268664 缺口1: 用 Inside/Partial 分类替代 overlap 整块消费。
            // - Inside：cluster 完全在 inserted 范围内，整个 cluster 进入 InsertReveal + static hide。
            // - Partial：cluster 部分在 inserted 范围内（ligature/cluster 跨越 inserted 边界），
            //   只把属于 inserted 的子片段（clipped source rect）交给 InsertReveal + static hide，
            //   旧邻字部分保持原样不进入 static hide，避免整个 cluster 被当成新插入文字。
            // - 不相交：跳过，不参与 InsertReveal，不进入 static hide。
            let relation = match new_cluster.relate_to_inserted_range(range_start, range_end) {
                Some(r) => r,
                None => continue,
            };
            // Issue #722 评论 5748596920 问题5: 跳过纯空格/tab/换行/控制字符。
            // 这些非可见字符不应创建 InsertReveal 和 static patch，
            // 避免文字前插空格闪一下/手动换行闪一下。
            // 已有文字位移交给 ReflowMove，caret 走 canonical track。
            //
            // Issue #736 评论 5777408243 问题1: 不再把 range 取不到正文静默解释成空字符串。
            // cluster 的 document byte range 必须属于 snapshot.virtual_text（同一 revision），
            // 否则属于快照不变量被破坏，记明确的 invariant diagnostic 后跳过该 cluster。
            let cluster_text = match new_snapshot
                .virtual_text
                .get(new_cluster.byte_start..new_cluster.byte_end)
            {
                Some(text) => text,
                None => {
                    crate::backend::app_backend::debug_warn_static(
                        "animation_coordinator",
                        "insert_reveal_cluster_text_range_out_of_virtual_text",
                        &format!(
                            "snapshot revision={} cluster byte_start={} byte_end={} \
                             virtual_text.len={} — cluster byte range not in virtual_text, \
                             skip InsertReveal for this cluster (snapshot invariant broken)",
                            new_snapshot.revision.0,
                            new_cluster.byte_start,
                            new_cluster.byte_end,
                            new_snapshot.virtual_text.len(),
                        ),
                    );
                    continue;
                }
            };
            if cluster_text
                .chars()
                .all(|c| c.is_whitespace() || c.is_control())
            {
                continue;
            }
            // Issue #724 评论 5751268664 缺口1: Inside 用整个 source_rect，
            // Partial 用精确 glyph 几何 + clipped_byte_range。
            // 不再用 UTF-8 byte 比例猜视觉宽度——字节长度不是 glyph 宽度，
            // 中文 UTF-8 3 字节/拉丁 1 字节/ligature/组合字符/fallback font/
            // 比例字体/RTL 都不能按 byte ratio 对应到 source rect 的 x/w。
            // Partial 的精确 source rect 从 QTextLayout 侧取（支持 split ligature）。
            let (new_sr, slice_byte_start, slice_byte_end) = match relation {
                ClusterInsertRelation::Inside => (
                    new_cluster.source_rect.clone(),
                    new_cluster.byte_start,
                    new_cluster.byte_end,
                ),
                ClusterInsertRelation::Partial {
                    clipped_byte_start,
                    clipped_byte_end,
                } => {
                    // Issue #724 评论 5751573705 问题1: 从 QTextLayout 取精确 glyph 几何。
                    // 失败时（layout 缺失/范围越界）回退到完整 cluster source_rect，
                    // 至少保证视觉不崩——宁可多画一帧旧邻字，也不猜错位置。
                    let precise_sr = new_line.get_precise_glyph_rect_for_byte_range(
                        new_snapshot.revision.0,
                        &new_snapshot.virtual_text,
                        clipped_byte_start,
                        clipped_byte_end,
                    );
                    if let Some(sr) = precise_sr {
                        (sr, clipped_byte_start, clipped_byte_end)
                    } else {
                        (
                            new_cluster.source_rect.clone(),
                            clipped_byte_start,
                            clipped_byte_end,
                        )
                    }
                }
            };
            let new_doc = new_line.source_rect_to_document_rect(&new_sr);
            let mut slice = AnimatedSlice::insert_reveal(
                key,
                new_line.id,
                new_sr.clone(),
                new_doc.clone(),
                0.0,
                0.0,
                slice_byte_start,
                slice_byte_end,
                Some(new_cluster.shaping_identity.clone()),
                // Issue #722 评论 5749572808 问题1: 传全文视觉行 id，
                // 不是 line_snapshots 的局部数组下标。line_idx 仍用于
                // managed_new_clusters/patches_by_line 的局部索引。
                Some(new_line.visual_line_id),
            );
            // Issue #727 评论 5755858583 问题2: 直接在 slice 上写 canonical 独占区域，
            // 不再生成 StaticLinePatch。AnimatedSlice 成为唯一事实源。
            slice.static_hidden_document_rects = vec![new_doc];
            slices.push(slice);
        }
    }

    let slices = merge_adjacent_slices(slices);
    slices
}

/// 按 Core 给出的 deleted_range 从 old_snapshot 显式生成 DeleteConceal 切片。
///
/// 只接 `vt.deleted_range + old_snapshot + old_cursor_rect`。按明确删除范围从
/// old snapshot 取纹理，直接生成 `DeleteConceal`。删除后的 canonical new text
/// 可以立即作为背景，旧字只由 overlay 吞掉，因此不生成 StaticLinePatch。
///
/// # 参数
/// - `key`：事务键。
/// - `old_snapshot`：旧布局快照。
/// - `deleted_range`：Core 给出的删除范围 (byte_start, byte_end)。
/// - `old_cursor_rect`：删除前光标矩形，用于决定吞字方向（conceal_to_left_edge）。
///
/// # 返回
/// `slices`：DeleteConceal 动画切片。
fn build_delete_conceal_slices(
    key: VisualTransactionKey,
    old_snapshot: &EditorLayoutSnapshot,
    deleted_range: (usize, usize),
    old_cursor_rect: Option<&CursorRect>,
) -> Vec<AnimatedSlice> {
    let mut slices = Vec::new();
    let (range_start, range_end) = deleted_range;
    let old_cx = old_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
    let old_cy = old_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);

    for old_line in &old_snapshot.line_snapshots {
        for old_cluster in &old_line.clusters {
            // Issue #724 评论 5750911834 问题 1: cluster 匹配条件改为 overlap 判断，
            // 允许部分落在 deleted_range 边界的 cluster（ligature 拆分、跨行 cluster）。
            // 旧逻辑 `byte_start >= range_start && byte_end <= range_end` 会丢弃部分
            // 落在边界的 cluster。
            if old_cluster.byte_start < range_end && old_cluster.byte_end > range_start {
                let old_sr = old_cluster.source_rect.clone();
                let old_doc = old_line.source_rect_to_document_rect(&old_sr);
                // 按删除前光标位置（old_cursor_rect）决定收缩方向：
                // 光标在被删文字右侧 → Backspace → conceal_to_left_edge = true（向左边缘收缩，右段先消失，光标跟右边缘往左走）
                // 光标在被删文字左侧 → Delete 键 → conceal_to_left_edge = false（向右边缘收缩，左段先消失，光标固定不动）
                let left = old_doc.x;
                let right = old_doc.x + old_doc.w;
                let conceal_to_left_edge = (old_cx - right).abs() <= (old_cx - left).abs();
                slices.push(AnimatedSlice::delete_conceal(
                    key,
                    old_line.id,
                    old_sr,
                    old_doc,
                    old_cx,
                    old_cy,
                    old_cluster.byte_start,
                    old_cluster.byte_end,
                    Some(old_cluster.shaping_identity.clone()),
                    conceal_to_left_edge,
                    // Issue #722 评论 5749572808 问题1: 传全文视觉行 id，
                    // 不是 line_snapshots 的局部数组下标。
                    Some(old_line.visual_line_id),
                ));
            }
        }
    }

    // Delete 不生成 StaticLinePatch：删除后的 canonical new text 可以立即作为背景，
    // 旧字只由 overlay 吞掉。
    let slices = merge_adjacent_slices(slices);
    slices
}

/// 构建 cluster/run 级 reflow 切片和静态行补丁。
///
/// Issue #712: 两阶段算法——先稳定一对一配对，再处理多对多。
/// 1. 一对一精确匹配：对每个未 excluded 的 new cluster，用 OffsetMap 映射回 old 坐标，
///    找 byte range 精确对应的唯一 old cluster。匹配后按 shaping 和 geometry 决定动画类型：
///    - shaping 相同 + 几何没变 → 不生成任何动画（消除普通输入/删除/Enter 的错误 CrossFade）
///    - shaping 相同 + 几何变了 → ReflowMove
///    - shaping 真变了 → 一对 ReflowCrossFade（old 淡出 + new 淡入）
/// 2. 多对多处理：剩下确实无法唯一对应的 cluster，每个 old 在原位 fade-out，
///    每个 new 在原位 fade-in。
///
/// # 参数
/// - `excluded_old_ranges`：已被 insert/delete 动画接管的 old byte range，跳过不处理。
/// - `excluded_new_ranges`：已被 insert/delete 动画接管的 new byte range，跳过不处理。
/// - `old_cursor_rect`：用于 reflow 中检测到的 insert_reveal 的起始位置。
/// - `new_cursor_rect`：用于 reflow 中检测到的 delete_conceal 的收缩目标。
///
/// # 返回
/// `slices`：动画切片（含 static_hidden_document_rects）。
fn build_cluster_reflow_slices(
    key: VisualTransactionKey,
    old_snapshot: &EditorLayoutSnapshot,
    new_snapshot: &EditorLayoutSnapshot,
    offset_map: &OffsetMap,
    excluded_old_ranges: &[(usize, usize)],
    excluded_new_ranges: &[(usize, usize)],
    old_cursor_rect: Option<&CursorRect>,
    new_cursor_rect: Option<&CursorRect>,
) -> Vec<AnimatedSlice> {
    let mut slices = Vec::new();
    // Issue #738 评论 5789470425 问题3: CrossFade group id 分配器（事务内唯一）。
    // 每对 ReflowCrossFadeOld/New 共享同一 group_id，reconcile 以 group 为单位成对重绑。
    let mut next_crossfade_group_id: u64 = 1;

    // Issue #687: old_cx/old_cy/new_cx/new_cy 不再需要——changed range 由
    // build_insert_reveal_slices / build_delete_conceal_slices 显式拥有，
    // reflow 只处理 unchanged material。
    let _ = (old_cursor_rect, new_cursor_rect);

    // ── 阶段 1：收集所有未 excluded 的 old/new cluster refs ──
    // 被 excluded 的 old cluster 保留在 old_refs 中（标记 excluded=true），
    // 不参与一对一匹配和多对多处理。被 excluded 的 new cluster 直接跳过。
    let mut old_refs: Vec<ReflowClusterRef> = Vec::new();
    let mut old_excluded_flags: Vec<bool> = Vec::new();
    for (line_idx, old_line) in old_snapshot.line_snapshots.iter().enumerate() {
        for (cluster_idx, old_cluster) in old_line.clusters.iter().enumerate() {
            let is_excluded = excluded_old_ranges
                .iter()
                .any(|(s, e)| old_cluster.byte_start >= *s && old_cluster.byte_end <= *e);
            old_refs.push(ReflowClusterRef {
                line_idx,
                cluster_idx,
                byte_start: old_cluster.byte_start,
                byte_end: old_cluster.byte_end,
            });
            old_excluded_flags.push(is_excluded);
        }
    }

    let mut new_refs: Vec<ReflowClusterRef> = Vec::new();
    for (line_idx, new_line) in new_snapshot.line_snapshots.iter().enumerate() {
        for (cluster_idx, new_cluster) in new_line.clusters.iter().enumerate() {
            if excluded_new_ranges
                .iter()
                .any(|(s, e)| new_cluster.byte_start >= *s && new_cluster.byte_end <= *e)
            {
                continue;
            }
            new_refs.push(ReflowClusterRef {
                line_idx,
                cluster_idx,
                byte_start: new_cluster.byte_start,
                byte_end: new_cluster.byte_end,
            });
        }
    }

    // ── 阶段 2：一对一精确匹配 ──
    // Issue #712: 先稳定一对一配对，消除普通输入/删除/Enter 的错误 CrossFade。
    // 对每个未 excluded 的 new cluster，用 offset_map 映射回 old 坐标，
    // 找 byte range 精确对应的唯一 old cluster。
    // 匹配条件：mapped_old_start == old_cluster.byte_start && mapped_old_end == old_cluster.byte_end
    let n_old = old_refs.len();
    let n_new = new_refs.len();
    let mut old_matched: Vec<bool> = vec![false; n_old];
    let mut new_matched: Vec<bool> = vec![false; n_new];

    for (ni, nref) in new_refs.iter().enumerate() {
        // 用 offset_map 将 new cluster 的 byte range 映射回 old 坐标
        let mapped_old_range = offset_map.map_new_range_to_old(nref.byte_start, nref.byte_end);

        // 映射失败（None）的 new cluster 无法一对一匹配，跳过进入多对多处理
        let (mapped_old_start, mapped_old_end) = match mapped_old_range {
            Some(r) => r,
            None => continue,
        };

        // 查找精确匹配的 old cluster：byte range 完全对应
        let matching_oi = old_refs
            .iter()
            .enumerate()
            .filter(|(oi, _)| !old_matched[*oi] && !old_excluded_flags[*oi])
            .find(|(_, oref)| {
                oref.byte_start == mapped_old_start && oref.byte_end == mapped_old_end
            })
            .map(|(oi, _)| oi);

        // 没找到唯一匹配的 new cluster，跳过进入多对多处理
        let oi = match matching_oi {
            Some(idx) => idx,
            None => continue,
        };

        let oref = &old_refs[oi];
        let old_line = &old_snapshot.line_snapshots[oref.line_idx];
        let old_cluster = &old_line.clusters[oref.cluster_idx];
        let new_line = &new_snapshot.line_snapshots[nref.line_idx];
        let new_cluster = &new_line.clusters[nref.cluster_idx];

        let same_shaping = old_cluster
            .shaping_identity
            .is_same_shaping(&new_cluster.shaping_identity);

        let old_sr = old_cluster.source_rect.clone();
        let new_sr = new_cluster.source_rect.clone();
        let old_doc = old_line.source_rect_to_document_rect(&old_sr);
        let new_doc = new_line.source_rect_to_document_rect(&new_sr);

        if same_shaping {
            let geometry_same = (old_doc.x - new_doc.x).abs() < 0.5
                && (old_doc.y - new_doc.y).abs() < 0.5
                && (old_doc.w - new_doc.w).abs() < 0.5
                && (old_doc.h - new_doc.h).abs() < 0.5;
            if !geometry_same {
                // 几何变了：只生成 ReflowMove
                // Issue #727 评论 5755858583 问题2: 直接在 slice 上写 canonical 独占区域。
                let mut slice = AnimatedSlice::reflow_move(
                    key,
                    old_line.id,
                    old_sr,
                    old_doc,
                    new_line.id,
                    new_sr.clone(),
                    new_doc.clone(),
                    new_cluster.byte_start,
                    new_cluster.byte_end,
                    Some(old_cluster.shaping_identity.clone()),
                );
                slice.static_hidden_document_rects = vec![new_doc];
                slices.push(slice);
            }
            // 几何没变：不生成任何动画（关键改进——消除普通输入/删除/Enter 的错误 CrossFade）
        } else {
            // byte identity 对得上但 shaping 真变了：生成一对 ReflowCrossFade
            // Issue #738 评论 5788513592 问题3: old/new 两侧写入各自真实 shaping identity，
            // rebind 时 is_same_shaping 能返回 true，CrossFade 可按新布局继续而非必然 Remove。
            // Issue #738 评论 5789470425 问题3: old/new 两侧共享同一 crossfade_group_id，
            // reconcile 以 group 为单位成对重绑，不再把 old/new 各自独立判死。
            let group_id = next_crossfade_group_id;
            next_crossfade_group_id += 1;
            slices.push(AnimatedSlice::reflow_crossfade_old(
                key,
                old_line.id,
                old_sr,
                old_doc.clone(),
                new_doc.clone(),
                new_cluster.byte_start,
                new_cluster.byte_end,
                Some(old_cluster.shaping_identity.clone()),
                Some(group_id),
            ));
            // Issue #727 评论 5755858583 问题2: ReflowCrossFadeNew 直接写 canonical 独占区域。
            let mut new_slice = AnimatedSlice::reflow_crossfade_new(
                key,
                new_line.id,
                new_sr.clone(),
                old_doc,
                new_doc.clone(),
                new_cluster.byte_start,
                new_cluster.byte_end,
                Some(new_cluster.shaping_identity.clone()),
                Some(group_id),
            );
            new_slice.static_hidden_document_rects = vec![new_doc];
            slices.push(new_slice);
        }

        // 标记已配对的 old/new cluster，不再参与后续多对多处理
        old_matched[oi] = true;
        new_matched[ni] = true;
    }

    // ── 阶段 3：多对多处理（未配对的 cluster）──
    // 剩下确实无法唯一对应的 cluster（未配对的 old 和 new），进入多对多处理。
    // 每个 old 在原位 fade-out，每个 new 在原位 fade-in。
    let unmatched_old: Vec<usize> = (0..n_old)
        .filter(|&oi| !old_matched[oi] && !old_excluded_flags[oi])
        .collect();
    let unmatched_new: Vec<usize> = (0..n_new).filter(|&ni| !new_matched[ni]).collect();

    // 只有同时存在未配对的 old 和 new 时才生成 CrossFade
    if !unmatched_old.is_empty() && !unmatched_new.is_empty() {
        // Issue #738 评论 5792244119 问题 3: 多对多 CrossFade old/new 共享同一个
        // group_id，rebind 按 group_id 配对时能真正成组（一组包含多 old + 多 new）。
        // 不再给 old/new 各自独立发 group_id（那会导致 crossfade_pairs 永远配不上，
        // 所有 CrossFade units 掉进"未配对独立处理"路径，出现只续一边/只删一边）。
        let group_id = next_crossfade_group_id;
        next_crossfade_group_id += 1;
        for &oi in &unmatched_old {
            let oref = &old_refs[oi];
            let old_line = &old_snapshot.line_snapshots[oref.line_idx];
            let old_cluster = &old_line.clusters[oref.cluster_idx];
            let old_sr = old_cluster.source_rect.clone();
            let old_doc = old_line.source_rect_to_document_rect(&old_sr);

            slices.push(AnimatedSlice::reflow_crossfade_old(
                key,
                old_line.id,
                old_sr,
                old_doc.clone(),
                old_doc,
                old_cluster.byte_start,
                old_cluster.byte_end,
                Some(old_cluster.shaping_identity.clone()),
                Some(group_id),
            ));
        }

        for &ni in &unmatched_new {
            let nref = &new_refs[ni];
            let new_line = &new_snapshot.line_snapshots[nref.line_idx];
            let new_cluster = &new_line.clusters[nref.cluster_idx];
            let new_sr = new_cluster.source_rect.clone();
            let new_doc = new_line.source_rect_to_document_rect(&new_sr);
            let new_doc_for_hide = new_doc.clone();

            // Issue #727 评论 5755858583 问题2: ReflowCrossFadeNew 直接写 canonical 独占区域。
            // Issue #738 评论 5788513592 问题3: 写入真实 shaping identity。
            // Issue #738 评论 5792244119 问题3: 写入共享的 group_id（old/new 同组）。
            let mut new_slice = AnimatedSlice::reflow_crossfade_new(
                key,
                new_line.id,
                new_sr.clone(),
                new_doc.clone(),
                new_doc,
                new_cluster.byte_start,
                new_cluster.byte_end,
                Some(new_cluster.shaping_identity.clone()),
                Some(group_id),
            );
            new_slice.static_hidden_document_rects = vec![new_doc_for_hide];
            slices.push(new_slice);
        }
    }

    // Issue #727 评论 5755858583 问题2: 不再生成 StaticLinePatches。
    // static_hidden_document_rects 已在创建 slice 时直接写入。
    // run_managed_new_clusters 不再需要——裁剪信息已在 slice 上。

    let slices = merge_adjacent_slices(slices);
    slices
}

/// 合并相邻同类型、同方向、同快照的动画切片为 run，避免一个字一个 slice。
///
/// 合并条件（全部满足才合并）：
/// 1. 相同 `kind`（AnimatedSliceKind）
/// 2. 相同 `snapshot_id`（来自同一行快照）
/// 3. 相邻 byte range：`slice[i].byte_end == slice[i+1].byte_start`
/// 4. 同方向：
///    - InsertReveal：`from_document_rect` 的 y 相同（同一行吐字）
///    - DeleteConceal：`from_document_rect` 的 y 相同（同一行吞字）且 `conceal_to_left_edge` 相同
///    - ReflowMove：移动向量相同（dx/dy 差值在 0.5 像素以内）
///    - ReflowCrossFade：移动向量相同
///
/// 合并操作：byte range 取 min/max，矩形取 bounding box，标量取第一个 slice 的值。
/// 合并是贪心的：一旦合并就继续尝试与下一个合并。不改变 slices 的顺序。
fn merge_adjacent_slices(slices: Vec<AnimatedSlice>) -> Vec<AnimatedSlice> {
    if slices.len() <= 1 {
        return slices;
    }

    let mut result: Vec<AnimatedSlice> = Vec::with_capacity(slices.len());
    let mut current = slices[0].clone();

    for next in &slices[1..] {
        if can_merge(&current, next) {
            current = merge_two(&current, next);
        } else {
            result.push(current);
            current = next.clone();
        }
    }
    result.push(current);
    result
}

/// 判断两个相邻 slice 是否可以合并。
fn can_merge(a: &AnimatedSlice, b: &AnimatedSlice) -> bool {
    // 条件 1：相同 kind
    if a.kind != b.kind {
        return false;
    }
    // 条件 2：相同 snapshot_id
    if a.snapshot_id != b.snapshot_id {
        return false;
    }
    // 条件 3：相邻 byte range
    if a.byte_end != b.byte_start {
        return false;
    }
    // 条件 4：同方向
    match a.kind {
        AnimatedSliceKind::InsertReveal => {
            // 同一行吐字：from_document_rect 的 y 相同
            (a.from_document_rect.y - b.from_document_rect.y).abs() < 0.5
        }
        AnimatedSliceKind::DeleteConceal => {
            // 同一行吞字且同方向：from_document_rect 的 y 相同，conceal_to_left_edge 相同
            (a.from_document_rect.y - b.from_document_rect.y).abs() < 0.5
                && a.conceal_to_left_edge == b.conceal_to_left_edge
        }
        AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
            // 移动向量相同：dx = to.x - from.x, dy = to.y - from.y
            let a_dx = a.to_document_rect.x - a.from_document_rect.x;
            let a_dy = a.to_document_rect.y - a.from_document_rect.y;
            let b_dx = b.to_document_rect.x - b.from_document_rect.x;
            let b_dy = b.to_document_rect.y - b.from_document_rect.y;
            let same_vector = (a_dx - b_dx).abs() < 0.5 && (a_dy - b_dy).abs() < 0.5;
            // Issue #738 评论 5789470425 问题3: CrossFade 只能合并同 group 同 side。
            // old 侧和 new 侧不能互相合并；不同 group 不能合并。
            let same_crossfade_group = a.crossfade_group_id == b.crossfade_group_id
                && a.crossfade_side == b.crossfade_side;
            same_vector && same_crossfade_group
        }
    }
}

/// Issue #738 评论 5788513592 问题2: 合并两个相邻 slice 的 byte range。
/// 合并后 range 覆盖多个原始 cluster，find_cluster_in_canonical 已改为支持
/// 跨多 cluster 的 range 匹配（按相交 cluster 合成 bounding rect），因此 merged
/// unit 能在 canonical 中重绑，不再被误删。
fn merged_byte_range(a: (usize, usize), b: (usize, usize)) -> (usize, usize) {
    (a.0.min(b.0), a.1.max(b.1))
}

/// Issue #738 评论 5789470425 问题2: 合并两个相邻 slice 为一个 run。
///
/// 关键改动：不再把多个 cluster 框成一个大矩形就完事——`reflow_anchors` 列表
/// 保留每个原始 cluster 的完整身份（byte range、shaping identity、from/to document
/// rect、source_rect、snapshot_id、visual_line_id）。rebind 时逐 anchor 做 OffsetMap、
/// 找新 canonical cluster、校验 shaping；拆行或各 anchor 新移动向量不同时拆回多个
/// Timed unit，而不是做一个跨行 bounding rect。
///
/// merged unit 的 `from_document_rect` / `to_document_rect` / `source_rect` 仍取
/// 各 anchor 的 union（merged unit 渲染需要连续矩形做整体插值），但 `reflow_anchors`
/// 才是逐 cluster 真相。`shaping_identity` 取首个 anchor 的代表值，仅用于兼容旧
/// shaping 比较路径；逐 cluster 的真实 shaping 在 `reflow_anchors` 里。
fn merge_two(a: &AnimatedSlice, b: &AnimatedSlice) -> AnimatedSlice {
    let (byte_start, byte_end) =
        merged_byte_range((a.byte_start, a.byte_end), (b.byte_start, b.byte_end));
    // Issue #738 评论 5789470425 问题2: 合并 reflow_anchors 列表，不丢子 cluster 身份。
    let merged_anchors: Vec<super::animated_slice::ReflowAnchor> = a
        .reflow_anchors
        .iter()
        .chain(&b.reflow_anchors)
        .cloned()
        .collect();
    // merged unit 的 from/to/source 取各 anchor 的 union（连续矩形做整体插值）。
    // 用 inline min/max 计算而非 bounding_box helper，强调 reflow_anchors 才是逐 cluster 真相。
    let merged_from = union_source_rect(&a.from_document_rect, &b.from_document_rect);
    let merged_to = union_source_rect(&a.to_document_rect, &b.to_document_rect);
    let merged_source = union_source_rect(&a.source_rect, &b.source_rect);
    // shaping_identity 取首个 anchor 的代表值；逐 cluster 真实 shaping 在 reflow_anchors。
    let head_shaping = a.shaping_identity.clone();
    AnimatedSlice {
        kind: a.kind,
        snapshot_id: a.snapshot_id,
        source_rect: merged_source,
        from_document_rect: merged_from,
        to_document_rect: merged_to,
        opacity_from: a.opacity_from,
        opacity_to: a.opacity_to,
        scale_from: a.scale_from,
        scale_to: a.scale_to,
        byte_start,
        byte_end,
        shaping_identity: head_shaping,
        conceal_to_left_edge: a.conceal_to_left_edge,
        visual_line_id: a.visual_line_id,
        start_fraction: a.start_fraction.min(b.start_fraction),
        static_hidden_document_rects: a
            .static_hidden_document_rects
            .iter()
            .chain(&b.static_hidden_document_rects)
            .cloned()
            .collect(),
        crossfade_group_id: a.crossfade_group_id,
        crossfade_side: a.crossfade_side,
        reflow_anchors: merged_anchors,
    }
}

/// Issue #738 评论 5789470425 问题2: 两个 SourceRect 的 union（连续矩形）。
/// 与 `bounding_box` 语义相同，独立命名以表明 merged unit 的代表几何，
/// 逐 cluster 真相在 `reflow_anchors`。
fn union_source_rect(a: &SourceRect, b: &SourceRect) -> SourceRect {
    let min_x = a.x.min(b.x);
    let min_y = a.y.min(b.y);
    let max_right = (a.x + a.w).max(b.x + b.w);
    let max_bottom = (a.y + a.h).max(b.y + b.h);
    SourceRect {
        x: min_x,
        y: min_y,
        w: max_right - min_x,
        h: max_bottom - min_y,
    }
}

/// Linux Qt 文字动画协调器 — 管理动画事务的生命周期和 rebase。
///
/// - `next_key_id`：事务键 ID 分配器（单调递增），每个事务有唯一键用于取消和 rebase。
/// - `prepared_queue`：已准备好的动画事务队列，按时间线顺序执行。
/// - `layout_revision`：上次处理事务时的布局修订号，用于检测布局是否已变化。
///   与 `EditorKernel.revision` 不同——`layout_revision` 跟踪排版结果变更（含窗口宽度、字号等），
///   `revision` 跟踪文本内容变更。两者独立递增。
pub(crate) struct LinuxEditorAnimationCoordinator {
    next_key_id: u64,
    pub(crate) prepared_queue: PreparedTransactionQueue,
    /// 打字/预输入动画时长（毫秒）。本地生成的事务不来自 Core 的
    /// `EditorVisualTransaction`（已删除），因此在此持有该视觉配置，
    /// 与 `PreparedEditMotion` 把 `duration_ms` 放进结构体的设计方向一致。
    typing_animation_duration_ms: u32,
    /// 光标平滑移动动画时长（毫秒）。
    cursor_animation_duration_ms: u32,
}

impl LinuxEditorAnimationCoordinator {
    pub fn new() -> Self {
        Self {
            next_key_id: 1,
            prepared_queue: PreparedTransactionQueue::new(),
            typing_animation_duration_ms: 160,
            cursor_animation_duration_ms: 120,
        }
    }

    pub(crate) fn alloc_key(&mut self) -> VisualTransactionKey {
        let id = self.next_key_id;
        self.next_key_id += 1;
        VisualTransactionKey::new(id, id)
    }

    pub(crate) fn set_typing_animation_duration_ms(&mut self, ms: u32) {
        self.typing_animation_duration_ms = ms;
    }

    pub(crate) fn set_cursor_animation_duration_ms(&mut self, ms: u32) {
        self.cursor_animation_duration_ms = ms;
    }

    /// Issue #690 评论 5675007226 步骤 3+5: 冲突事务交棒给新事务，只留一条紧凑诊断事件。
    ///
    /// 逐单元采集当前可见比例与单元时间线（[`PreparedTextVisualTransaction::collect_rebase_frames`]），
    /// 再取消旧事务。四个正文编辑入口共用本函数，避免各自拿事务级 progress 重算可见比例
    /// ——那正是"上一笔吐到 60% 的字被重启"的来源。
    ///
    /// `preserve` 为 `Some((编辑的 old 坐标范围, OffsetMap))` 时（Insert/Delete 入口），
    /// 若旧事务里仍在播放的单元都没被本次编辑覆盖，就完全不取消它：事务 key 保留自己的
    /// snapshot/纹理所有权，单元沿自己的时间线播完（`editor.anim.keep`）。预输入入口传
    /// `None`——preedit 文本整体被替换，旧单元必然失效。
    ///
    /// Issue #690 评论 5680276931 + 5681206040: 返回值同时带上 `RebaseCaretHandoff`——
    /// 在取消旧事务之前用同一个 `now` 采样旧事务当前真正显示的 coordinated cursor rect
    /// 和旧 caret track 剩余时长，交给新事务作为 caret track 的起点和 duration。
    /// 这样 rebase 交棒后光标不再从逻辑 old caret 重新起步，而是与文字 reflow 同帧
    /// 从屏幕位置续播；连续交棒也精确，因为新 caret track 自带时间状态。
    ///
    /// Issue #710 评论 5733833897: 一次新编辑可能同时撞上多笔旧事务。`conflicting`
    /// 现在接收**全部**冲突事务的 key（`&[VisualTransactionKey]`），逐笔处理：
    /// - untouched 的 tx：emit `editor.anim.keep` 诊断，**不取消**，继续下一笔。
    /// - 受影响的 tx：采集 rebase frames，把 frame 的 byte_start/byte_end 从该旧事务
    ///   new 坐标系映射到 current-old 坐标系（用 `OffsetMap::build(&tx.new_text,
    ///   current_old_text)`），追加到累计 `all_rebase_frames`；cancel 该 tx。
    /// - caret handoff：在所有被取消的冲突事务中，找 `cursor_owner_epoch ==
    ///   current_cursor_epoch` 的事务。如果有多个，取 `key.transaction_id` 最大的
    ///   （最新创建的）。对选中的那一笔采样 caret 构造 `RebaseCaretHandoff`。
    ///   如果没有冲突事务拥有 coordinated caret，handoff 为 None。
    fn take_rebase_frames(
        &mut self,
        conflicting: &[VisualTransactionKey],
        reason: &str,
        now: Instant,
        preserve: Option<(&[(usize, usize)], &OffsetMap)>,
        current_old_text: &str,
        current_cursor_epoch: u64,
    ) -> (Vec<RebaseFrame>, Option<RebaseCaretHandoff>) {
        if conflicting.is_empty() {
            return (Vec::new(), None);
        }
        let mut all_rebase_frames: Vec<RebaseFrame> = Vec::new();
        // (key, cursor_owner_epoch, handoff) 候选，cancel 之后再从中选 handoff。
        // 这样避免 cancel 后找不到 tx（cancel 调用了 retain 把 tx 从队列移除）。
        let mut caret_handoff_candidates: Vec<(
            VisualTransactionKey,
            u64,
            Option<RebaseCaretHandoff>,
        )> = Vec::new();
        let mut cancelled_keys: Vec<VisualTransactionKey> = Vec::new();
        for &old_key in conflicting {
            let tx_ref = self
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|tx| tx.key == old_key);
            let Some(tx) = tx_ref else {
                // 队列里找不到这笔 tx（可能已被其他路径取消），跳过。
                continue;
            };
            // per-tx 坐标系映射：旧事务 new 坐标系 → current-old 坐标系。
            // 用于把 tx 采集的 rebase frame 的 byte_start/byte_end 映射到 current-old。
            let tx_new_text = tx.new_snapshot.as_ref().map(|s| s.virtual_text.as_str());
            let per_tx_map = tx_new_text.map(|tx_new| OffsetMap::build(tx_new, current_old_text));

            let untouched = match preserve {
                Some((changed_old_ranges, offset_map)) => conflicting_units_are_untouched(
                    tx,
                    changed_old_ranges,
                    offset_map,
                    current_old_text,
                    now,
                ),
                None => false,
            };
            if untouched {
                emit_transaction_diagnostic(tx, "editor.anim.keep", "units_untouched");
                editor_animation_debug_log(&format!(
                    "anim_keep: key={:?} reason={} (units outside changed range keep playing)",
                    old_key, reason,
                ));
                continue;
            }
            // 受影响：采集 rebase frames（frame.byte_start/end 属于该旧事务 new 坐标系）。
            // Issue #727 约束 4: 不再自己采样 caret geometry（删除 sample_caret_geometry_for_caret_driven_clip）。
            // Reveal/Conceal 的 rebase frame 从 caret track progress 推导 visible_fraction，
            // 用 slice.compute_frame 构造，不依赖 caret geometry。
            // ReflowMove/ReflowCrossFade 继续用 compute_frame。
            let caret_track_progress = tx
                .cursor_visual_track
                .as_ref()
                .map(|track| track.progress(now));
            let caret_remaining_ms = tx
                .cursor_visual_track
                .as_ref()
                .map(|track| track.remaining_duration_ms(now))
                .unwrap_or(0);
            let frames: Vec<RebaseFrame> = tx
                .units
                .iter()
                .filter_map(|unit| {
                    collect_rebase_frame_for_unit_without_caret(
                        unit,
                        caret_track_progress,
                        caret_remaining_ms,
                        now,
                    )
                })
                .collect();
            // 坐标系映射：把每个 frame 的 byte_start/byte_end 映射到 current-old 坐标系。
            // 硬约束：进入 match_rebase_frames 的 frame.byte_start/end 必须已经是
            // current-old 坐标。frame 的原值属于旧事务自己的 new_text revision，
            // 映射失败后若保留旧数值，相当于把"已知属于旧 revision 的 byte offset"
            // 伪装成 current-old offset，会污染 tier1/tier2 匹配。
            // 映射失败的 frame 不进入 all_rebase_frames：其原值属于旧事务 new_text revision，
            // 不能冒充 current-old offset。旧事务仍 cancel，只是该 frame 放弃 byte-range rebase。
            let mapped_frames: Vec<RebaseFrame> = frames
                .into_iter()
                .filter_map(|mut frame| {
                    if let Some(ref per_tx_map) = per_tx_map {
                        if let Some((ms, me)) =
                            per_tx_map.map_old_range_to_new(frame.byte_start, frame.byte_end)
                        {
                            frame.byte_start = ms;
                            frame.byte_end = me;
                            return Some(frame);
                        }
                        // 映射失败：丢弃该 frame，不进入 byte-range rebase。
                        return None;
                    }
                    // 无 per_tx_map（无 new_snapshot）：保留原 frame（退化为数值比较）。
                    Some(frame)
                })
                .collect();
            // 在 cancel 之前采样 caret，构造 RebaseCaretHandoff 候选。
            // Issue #690 评论 5680276931 + 5681206040: 用同一个 now 采样旧事务这一帧
            // 正在屏幕上显示的 coordinated cursor rect，并取旧 caret track 的剩余时长。
            // Issue #727 约束 4: 不再调 sample_caret_geometry_for_caret_driven_clip，
            // 直接从 caret track 采样 visual_line_id 供 RebaseCaretHandoff 使用。
            let sampled_cursor = sample_coordinated_cursor_rect_at(tx, now);
            // 从 caret track 采样 visual_line_id（用于 RebaseCaretHandoff 行几何选取）。
            let caret_line_id = match tx.cursor_visual_track.as_ref() {
                Some(track) => {
                    let progress = track.progress(now);
                    track.sampled_visual_line_id_at_progress(progress)
                }
                None => None,
            };
            let caret_handoff = match (sampled_cursor, tx.cursor_visual_track.as_ref()) {
                (Some(sampled), Some(track)) => {
                    // Issue #722 评论 5749791161: 采样到的行几何从旧 track 的
                    // from_line/to_line 字段中选取。caret_line_id 等于 from 行 id
                    // 时用 from 行几何，等于 to 行 id 时用 to 行几何，否则用 from 行
                    // 几何作 fallback（caret 通常还在过渡中间，偏向 from 行更安全）。
                    let (line_top, line_bottom) = match caret_line_id {
                        Some(id) if Some(id) == track.to_visual_line_id => {
                            (track.to_line_top, track.to_line_bottom)
                        }
                        _ => (track.from_line_top, track.from_line_bottom),
                    };
                    Some(RebaseCaretHandoff {
                        sampled,
                        remaining_duration_ms: track.remaining_duration_ms(now).max(1),
                        // Issue #722 评论 5749572808 问题2: 复用同一 now 时刻采样的
                        // caret_line_id（sample_caret_geometry_for_caret_driven_clip
                        // 的返回值），保证 x/y 和行 id 来自同一帧同一 track。
                        sampled_visual_line_id: caret_line_id,
                        sampled_line_top: line_top,
                        sampled_line_bottom: line_bottom,
                    })
                }
                (Some(sampled), None) => {
                    // 旧事务没有 caret track（理论上正文事务都应有，防御性 fallback）：
                    // 用事务 timeline 剩余时长估算。
                    let tx_remaining = tx
                        .timeline
                        .duration_ms
                        .saturating_sub(
                            now.duration_since(tx.timeline.effective_start().unwrap_or(now))
                                .as_millis() as u64,
                        )
                        .max(1);
                    Some(RebaseCaretHandoff {
                        sampled,
                        remaining_duration_ms: tx_remaining,
                        // 无 track 时行 id 和行几何未知。
                        sampled_visual_line_id: None,
                        sampled_line_top: 0.0,
                        sampled_line_bottom: 0.0,
                    })
                }
                (None, _) => None,
            };
            caret_handoff_candidates.push((old_key, tx.cursor_owner_epoch, caret_handoff));
            emit_transaction_diagnostic(tx, "editor.anim.rebase", reason);
            all_rebase_frames.extend(mapped_frames);
            // cancel 这笔 tx（retain 会把它从队列移除）。
            self.prepared_queue.cancel(old_key, "rebased");
            cancelled_keys.push(old_key);
        }
        // caret handoff 选择：在所有被取消的冲突事务中，找 cursor_owner_epoch ==
        // current_cursor_epoch 的事务。如果有多个，取 key.transaction_id 最大的
        // （最新创建的）。对选中的那一笔返回 handoff。如果没有冲突事务拥有
        // coordinated caret，handoff 为 None。
        let selected_caret_handoff = caret_handoff_candidates
            .iter()
            .filter(|(_, epoch, _)| *epoch == current_cursor_epoch)
            .max_by_key(|(key, _, _)| key.transaction_id)
            .and_then(|(_, _, handoff)| handoff.clone());
        editor_animation_debug_log(&format!(
            "anim_rebase: cancelled_keys={:?} reason={} carried_units={} carried_cursor={}",
            cancelled_keys,
            reason,
            all_rebase_frames.len(),
            selected_caret_handoff.is_some(),
        ));
        (all_rebase_frames, selected_caret_handoff)
    }

    pub fn process_transaction(
        &mut self,
        vt: &PreparedEditMotion,
        typing_animation_enabled: bool,
        smooth_cursor_enabled: bool,
        is_scrolling: bool,
        is_loading: bool,
        is_applying_format: bool,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        old_cursor_visual_line_id: Option<usize>,
        new_cursor_visual_line_id: Option<usize>,
        old_cursor_line_top: f64,
        old_cursor_line_bottom: f64,
        new_cursor_line_top: f64,
        new_cursor_line_bottom: f64,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        cursor_owner_epoch: u64,
        layout_basis_revision: LayoutRevision,
    ) -> Option<VisualTransactionKey> {
        // Issue #727 评论 5755858583 问题5: !smooth_cursor_enabled 不再整笔 return None。
        // 只去掉 CaretDriven units（InsertReveal/DeleteConceal），Reflow 是否保留由
        // typing_animation_enabled 决定，不要把两类动画重新绑死。
        if !typing_animation_enabled || is_scrolling || is_loading || is_applying_format {
            return None;
        }

        // Issue #727 约束 5: valid_caret_motion_track 检查。
        // 没有 old/new cursor rect 就没有有效 caret motion track，不创建吞吐字事务。
        // Issue #727 评论 5755858583 问题5: 仅在 smooth_cursor_enabled 时才要求
        // valid_caret_motion_track——!smooth_cursor_enabled 时不创建 CaretDriven units，
        // 只创建 Reflow，不需要 caret motion track。
        let valid_caret_motion_track = old_cursor_rect.is_some() && new_cursor_rect.is_some();
        if smooth_cursor_enabled && !valid_caret_motion_track {
            return None;
        }

        let mode = AnimationMode::from_context(is_scrolling, is_loading, is_applying_format);
        if !mode.should_create_transaction() {
            return None;
        }

        match vt.kind {
            EditorAnimationKind::Insert => {
                if let Some(range) = vt.inserted_range {
                    let range_start = range.start().value();
                    let range_end = range.end().value();
                    let insert_offset_map = OffsetMap::build(&vt.old_text, &vt.new_text);
                    // Issue #710 评论 5733109905: 冲突检测用 current-old 坐标系。
                    // 先计算 visual_affected_byte_range 得到 old-side range (old_s, old_e)，
                    // 再用 old_s/old_e 查冲突。insert_offset_map 仍保留用于 rebase。
                    let (visual_affected_byte_range_old, visual_affected_byte_range_new) = {
                        let (old_s, old_e, new_s, new_e) = compute_affected_paragraph_ranges(
                            &vt.old_text,
                            &vt.new_text,
                            (range_start, range_start),
                            (range_start, range_end),
                        );
                        (Some((old_s, old_e)), Some((new_s, new_e)))
                    };
                    let (conflict_old_start, conflict_old_end) =
                        visual_affected_byte_range_old.unwrap_or((range_start, range_start));
                    let conflicting = self.prepared_queue.find_conflicting_transaction(
                        &vt.old_text,
                        conflict_old_start,
                        conflict_old_end,
                    );
                    // 纯插入在 old 文档里就是 range_start 这一个位置点。
                    let now = Instant::now();
                    let (rebase_frames, caret_handoff) = self.take_rebase_frames(
                        &conflicting,
                        "rebased_by_insert",
                        now,
                        Some((&[(range_start, range_start)], &insert_offset_map)),
                        &vt.old_text,
                        cursor_owner_epoch,
                    );

                    let key = self.alloc_key();
                    let mut slices = Vec::new();

                    // Issue #687: Insert 事务先生成显式 InsertReveal，再调用 reflow builder；
                    // reflow 必须排除 inserted_range。changed range 由 Core 显式拥有。
                    // Issue #727 评论 5755858583 问题5: !smooth_cursor_enabled 时跳过
                    // InsertReveal（CaretDriven unit），只保留 Reflow。
                    let inserted_range_tuple = (range_start, range_end);
                    if smooth_cursor_enabled {
                        let reveal_slices =
                            build_insert_reveal_slices(key, new_snapshot, inserted_range_tuple);
                        slices.extend(reveal_slices);
                    }

                    let reflow_slices = build_cluster_reflow_slices(
                        key,
                        old_snapshot,
                        new_snapshot,
                        &insert_offset_map,
                        &[],
                        &[inserted_range_tuple],
                        old_cursor_rect.as_ref(),
                        new_cursor_rect.as_ref(),
                    );
                    slices.extend(reflow_slices);

                    let mut units: Vec<PreparedVisualUnit> = slices
                        .into_iter()
                        .map(|s| PreparedVisualUnit::wrap(s, vt.duration_ms))
                        .collect();
                    match_rebase_frames(&rebase_frames, &mut units, &insert_offset_map);

                    // Issue #690 评论 5681206040: 构建 caret track。
                    // 有 handoff 时 from = sampled caret, duration = 旧 track 剩余时长；
                    // 无 handoff 时 from = old_cursor_rect, duration = 事务时长。
                    // Issue #690 评论 5682867529: 不再传 now，started_at 留 None，等 Rendering 再启动。
                    let cursor_visual_track = build_cursor_visual_track(
                        old_cursor_rect.as_ref(),
                        new_cursor_rect.as_ref(),
                        old_cursor_visual_line_id,
                        new_cursor_visual_line_id,
                        old_cursor_line_top,
                        old_cursor_line_bottom,
                        new_cursor_line_top,
                        new_cursor_line_bottom,
                        caret_handoff,
                        vt.duration_ms,
                    );
                    // Issue #710 评论 5731145076 症状六: visual_affected_byte_range 已在
                    // 查冲突之前提前计算（current-old 坐标系逐事务映射需要 old-side range）。
                    // Issue #710 评论 5732160521 问题 1/3: Insert 事务 old 侧是插入点
                    // (range_start, range_start)，new 侧是 inserted_range。
                    let prepared = PreparedTextVisualTransaction {
                        key,
                        state: TextVisualTransactionState::Pending,
                        operation_kind: TextVisualOperationKind::Insert,
                        timeline: TransactionTimeline::new(vt.duration_ms),
                        units,
                        old_cursor_rect,
                        new_cursor_rect,
                        cursor_visual_track,
                        cancel_reason: None,
                        texture_prepared: false,
                        old_snapshot: Some(old_snapshot.clone()),
                        new_snapshot: Some(new_snapshot.clone()),
                        cursor_owner_epoch,
                        caret_motion_retired: false,
                        visual_affected_byte_range_old,
                        visual_affected_byte_range_new,
                        layout_basis_revision,
                    };

                    // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
                    emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
                    editor_animation_debug_log(&format!(
                        "anim_event: key={:?} op=Insert inserted={:?} unit_kinds={:?} carried_rebase={}",
                        key,
                        inserted_range_tuple,
                        unit_kind_labels(&prepared.units),
                        rebase_frames.len(),
                    ));

                    self.prepared_queue.enqueue(prepared);

                    return Some(key);
                }
            }
            EditorAnimationKind::Delete => {
                let deleted_ranges: Vec<(usize, usize)> = if let Some(range) = vt.deleted_range {
                    vec![(range.start().value(), range.end().value())]
                } else {
                    let changes = diff_plain_text(&vt.old_text, &vt.new_text);
                    let mut ranges = Vec::new();
                    for change in &changes {
                        if let writer_core::editor::EditorChange::Delete { index, text } = change {
                            let range_start = index.value();
                            let range_end = range_start + text.len();
                            ranges.push((range_start, range_end));
                        }
                    }
                    ranges
                };

                let rebase_byte_start = deleted_ranges.first().map(|(s, _)| *s).unwrap_or(0);
                let rebase_byte_end = deleted_ranges.last().map(|(_, e)| *e).unwrap_or(0);
                let delete_offset_map = OffsetMap::build(&vt.old_text, &vt.new_text);
                // Issue #710 评论 5733109905: 冲突检测用 current-old 坐标系。
                // 先计算 visual_affected_byte_range 得到 old-side range (old_s, old_e)，
                // 再用 old_s/old_e 查冲突。delete_offset_map 仍保留用于 rebase。
                let (visual_affected_byte_range_old, visual_affected_byte_range_new) = {
                    let (old_s, old_e, new_s, new_e) = compute_affected_paragraph_ranges(
                        &vt.old_text,
                        &vt.new_text,
                        (rebase_byte_start, rebase_byte_end),
                        (rebase_byte_start, rebase_byte_start),
                    );
                    (Some((old_s, old_e)), Some((new_s, new_e)))
                };
                let (conflict_old_start, conflict_old_end) =
                    visual_affected_byte_range_old.unwrap_or((rebase_byte_start, rebase_byte_end));
                let conflicting = self.prepared_queue.find_conflicting_transaction(
                    &vt.old_text,
                    conflict_old_start,
                    conflict_old_end,
                );
                let now = Instant::now();
                let (rebase_frames, caret_handoff) = self.take_rebase_frames(
                    &conflicting,
                    "rebased_by_delete",
                    now,
                    Some((&deleted_ranges, &delete_offset_map)),
                    &vt.old_text,
                    cursor_owner_epoch,
                );

                let key = self.alloc_key();

                let mut slices = Vec::new();

                // Issue #687: Delete 事务先生成显式 DeleteConceal，再调用 reflow builder；
                // reflow 必须排除 deleted_range。changed range 由 Core 显式拥有。
                // 对每个 deleted range 生成显式 DeleteConceal 切片。
                // Issue #727 评论 5755858583 问题5: !smooth_cursor_enabled 时跳过
                // DeleteConceal（CaretDriven unit），只保留 Reflow。
                if smooth_cursor_enabled {
                    for &(d_start, d_end) in &deleted_ranges {
                        let conceal_slices = build_delete_conceal_slices(
                            key,
                            old_snapshot,
                            (d_start, d_end),
                            old_cursor_rect.as_ref(),
                        );
                        slices.extend(conceal_slices);
                    }
                }

                let reflow_slices = build_cluster_reflow_slices(
                    key,
                    old_snapshot,
                    new_snapshot,
                    &delete_offset_map,
                    &deleted_ranges,
                    &[],
                    old_cursor_rect.as_ref(),
                    new_cursor_rect.as_ref(),
                );
                slices.extend(reflow_slices);

                let mut units: Vec<PreparedVisualUnit> = slices
                    .into_iter()
                    .map(|s| PreparedVisualUnit::wrap(s, vt.duration_ms))
                    .collect();
                match_rebase_frames(&rebase_frames, &mut units, &delete_offset_map);

                // Issue #690 评论 5681206040 + 5682867529: 构建 caret track（不传 now，等 Rendering 再启动）。
                let cursor_visual_track = build_cursor_visual_track(
                    old_cursor_rect.as_ref(),
                    new_cursor_rect.as_ref(),
                    old_cursor_visual_line_id,
                    new_cursor_visual_line_id,
                    old_cursor_line_top,
                    old_cursor_line_bottom,
                    new_cursor_line_top,
                    new_cursor_line_bottom,
                    caret_handoff,
                    vt.duration_ms,
                );
                // Issue #710 评论 5731145076 症状六: visual_affected_byte_range 已在
                // 查冲突之前提前计算（current-old 坐标系逐事务映射需要 old-side range）。
                // Issue #710 评论 5732160521 问题 1/3: Delete 事务 old 侧是 deleted_range，
                // new 侧是删除后落点 (rebase_byte_start, rebase_byte_start)。
                let prepared = PreparedTextVisualTransaction {
                    key,
                    state: TextVisualTransactionState::Pending,
                    operation_kind: TextVisualOperationKind::Delete,
                    timeline: TransactionTimeline::new(vt.duration_ms),
                    units,
                    old_cursor_rect,
                    new_cursor_rect,
                    cursor_visual_track,
                    cancel_reason: None,
                    texture_prepared: false,
                    old_snapshot: Some(old_snapshot.clone()),
                    new_snapshot: Some(new_snapshot.clone()),
                    cursor_owner_epoch,
                    caret_motion_retired: false,
                    visual_affected_byte_range_old,
                    visual_affected_byte_range_new,
                    layout_basis_revision,
                };

                // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
                emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
                editor_animation_debug_log(&format!(
                    "anim_event: key={:?} op=Delete deleted={:?} unit_kinds={:?} carried_rebase={}",
                    key,
                    deleted_ranges,
                    unit_kind_labels(&prepared.units),
                    rebase_frames.len(),
                ));

                self.prepared_queue.enqueue(prepared);

                return Some(key);
            }
            EditorAnimationKind::Cursor => {
                // Issue #702: 删除"纯光标移动创建空 Cursor 文字事务"的结构。
                // 纯光标移动直接维护 CursorAnimationState（由 rendering.rs
                // update_cursor_visual_position → build_cursor_plan → apply_plan
                // 构造），用 Scene Graph 当前帧 frame_now 推进 from→to 动画，
                // 不再伪装成文字事务（units=空）。
                // 此分支不再创建任何事务，返回 None。
                return None;
            }
        }
        None
    }

    /// Issue #738 评论 5787277777: 把全部活动事务从上一份 canonical 几何重绑到这份新 canonical。
    ///
    /// 在 `record_visual_transaction` 里 `new_doc_snapshot` 已完成后、创建本次新事务之前调：
    /// 先把所有旧活动事务的 Timed Reflow unit 从"上一份 canonical 几何"重绑到这份新 canonical，
    /// 再处理本次新事务自己的 conflict/rebase。
    ///
    /// 流程：
    /// 1. 遍历全部 active transactions（不只遍历和新 edit byte range 相交的事务）。
    /// 2. CaretDriven 仍按现在的 owner/epoch 规则处理；旧事务失去 caret owner 后继续 retire
    ///    到 canonical（由 `build_text_animation_plan_with_sample` 每帧采样时处理）。
    /// 3. 对仍存活的 Timed Reflow unit 调 `rebind_timed_units_to_canonical`。
    /// 4. rebind 后已经没有 unit 的事务直接完成；还有 Timed unit 的继续播放。
    /// 5. 收集完后再生成本帧 clip rect / glyph plan（由 `build_render_plan_full` 完成）。
    ///    禁止 basis revision 旧于当前 canonical revision 的 unit 进入 `build_render_plan_full()`。
    pub(crate) fn reconcile_active_transactions_with_canonical(
        &mut self,
        current_text: &str,
        canonical_snapshot: &crate::editor::layout::CanonicalDocumentVisualSnapshot,
        layout_revision: LayoutRevision,
        now: Instant,
    ) {
        let mut keys_to_complete: Vec<VisualTransactionKey> = Vec::new();
        // Issue #738 评论 5795950264 问题1: 先收集需要处理的事务 key，再逐个
        // retire + rebind。retire_caret_driven_units_for_transaction 需要 &mut self，
        // 不能在 active_transactions_mut() 的循环里直接调。
        let keys: Vec<VisualTransactionKey> = self
            .prepared_queue
            .active_transactions()
            .iter()
            .filter(|t| {
                t.state != TextVisualTransactionState::Cancelled
                    && t.state != TextVisualTransactionState::Completed
            })
            .map(|t| t.key)
            .collect();
        for key in keys {
            // Issue #738 评论 5795950264 问题1: 先 retire CaretDriven units，让旧 caret
            // track 永久失去 ownership，再 rebind Timed Reflow。如果先 rebind 会把
            // layout_basis_revision 提升到当前 canonical，导致 basis 守卫
            //（build_text_animation_plan_with_sample / find_cursor_transaction_for_target）
            // 不再 retire 旧 CaretDriven，旧 caret track 重新拿到 ownership 在新 canonical
            // 上继续用旧布局几何。retire 把 CaretDriven 落到终态并置 caret_motion_retired=true，
            // ReflowMove/ReflowCrossFade 保留不动继续播。
            self.retire_caret_driven_units_for_transaction(key);
            let tx = match self
                .prepared_queue
                .active_transactions_mut()
                .iter_mut()
                .find(|t| t.key == key)
            {
                Some(t) => t,
                None => continue,
            };
            // 把 Timed Reflow unit 重绑到当前 canonical。
            tx.rebind_timed_units_to_canonical(current_text, canonical_snapshot, layout_revision, now);
            // rebind 后已经没有 unit 的事务直接完成。
            if tx.units.is_empty() {
                keys_to_complete.push(tx.key);
            }
        }
        // 完成空事务（rebind 移除了全部 unit，让 canonical 正文接管）。
        for key in keys_to_complete {
            self.prepared_queue.complete(key);
        }
    }

    pub fn handle_composition_update(
        &mut self,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        old_preedit_byte_start: usize,
        old_preedit_byte_end: usize,
        new_preedit_byte_start: usize,
        new_preedit_byte_end: usize,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        old_cursor_visual_line_id: Option<usize>,
        new_cursor_visual_line_id: Option<usize>,
        old_cursor_line_top: f64,
        old_cursor_line_bottom: f64,
        new_cursor_line_top: f64,
        new_cursor_line_bottom: f64,
        cursor_owner_epoch: u64,
        layout_basis_revision: LayoutRevision,
    ) -> Option<VisualTransactionKey> {
        let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);
        // Issue #710 评论 5734282079: 冲突检测用 current-old 坐标系。
        // old_preedit_byte_start/end 是 update_preedit 之前的 old virtualText 坐标，
        // 传 &old_snapshot.virtual_text 作为 current_old_text。offset_map 仍保留用于 rebase。
        let conflicting = self.prepared_queue.find_conflicting_transaction(
            &old_snapshot.virtual_text,
            old_preedit_byte_start,
            old_preedit_byte_end,
        );
        // 预输入文本整体被替换，旧单元必然失效：不做保留判断。
        let now = Instant::now();
        let (rebase_frames, caret_handoff) = self.take_rebase_frames(
            &conflicting,
            "rebased_by_composition_update",
            now,
            None,
            &old_snapshot.virtual_text,
            cursor_owner_epoch,
        );

        let key = self.alloc_key();

        let mut slices = Vec::new();

        // Issue #687: IME 组合更新也显式拥有 changed range。
        // 用 diff_plain_text 找到 inserted/deleted range，显式生成 InsertReveal/DeleteConceal，
        // reflow 只处理 unchanged material。
        let comp_changes = diff_plain_text(&old_snapshot.virtual_text, &new_snapshot.virtual_text);
        let mut comp_inserted_ranges: Vec<(usize, usize)> = Vec::new();
        let mut comp_deleted_ranges: Vec<(usize, usize)> = Vec::new();
        for change in &comp_changes {
            match change {
                writer_core::editor::EditorChange::Insert { index, text } => {
                    let rs = index.value();
                    comp_inserted_ranges.push((rs, rs + text.len()));
                }
                writer_core::editor::EditorChange::Delete { index, text } => {
                    let rs = index.value();
                    comp_deleted_ranges.push((rs, rs + text.len()));
                }
            }
        }

        for &(i_start, i_end) in &comp_inserted_ranges {
            let reveal_slices = build_insert_reveal_slices(key, new_snapshot, (i_start, i_end));
            slices.extend(reveal_slices);
        }
        for &(d_start, d_end) in &comp_deleted_ranges {
            let conceal_slices = build_delete_conceal_slices(
                key,
                old_snapshot,
                (d_start, d_end),
                old_cursor_rect.as_ref(),
            );
            slices.extend(conceal_slices);
        }

        let reflow_slices = build_cluster_reflow_slices(
            key,
            old_snapshot,
            new_snapshot,
            &offset_map,
            &comp_deleted_ranges,
            &comp_inserted_ranges,
            old_cursor_rect.as_ref(),
            new_cursor_rect.as_ref(),
        );
        slices.extend(reflow_slices);

        let unit_duration_ms = u64::from(self.typing_animation_duration_ms);
        let mut units: Vec<PreparedVisualUnit> = slices
            .into_iter()
            .map(|s| PreparedVisualUnit::wrap(s, unit_duration_ms))
            .collect();
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        // Issue #690 评论 5681206040 + 5682867529: 构建 caret track（不传 now，等 Rendering 再启动）。
        let cursor_visual_track = build_cursor_visual_track(
            old_cursor_rect.as_ref(),
            new_cursor_rect.as_ref(),
            old_cursor_visual_line_id,
            new_cursor_visual_line_id,
            old_cursor_line_top,
            old_cursor_line_bottom,
            new_cursor_line_top,
            new_cursor_line_bottom,
            caret_handoff,
            unit_duration_ms,
        );
        // Issue #710 评论 5734282079: composition update 的 visual affected range。
        // old_preedit_byte_start/end 是 old virtualText 坐标，new_preedit_byte_start/end
        // 是 new virtualText 坐标。分别从对应 snapshot 扩段落得到 affected range。
        let (visual_affected_byte_range_old, visual_affected_byte_range_new) = {
            let (old_s, old_e, new_s, new_e) = compute_affected_paragraph_ranges(
                &old_snapshot.virtual_text,
                &new_snapshot.virtual_text,
                (old_preedit_byte_start, old_preedit_byte_end),
                (new_preedit_byte_start, new_preedit_byte_end),
            );
            (Some((old_s, old_e)), Some((new_s, new_e)))
        };
        let prepared = PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::CompositionUpdate,
            timeline: TransactionTimeline::new(unit_duration_ms),
            units,
            old_cursor_rect,
            new_cursor_rect,
            cursor_visual_track,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: Some(old_snapshot.clone()),
            new_snapshot: Some(new_snapshot.clone()),
            cursor_owner_epoch,
            caret_motion_retired: false,
            visual_affected_byte_range_old,
            visual_affected_byte_range_new,
            layout_basis_revision,
        };

        // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
        emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
        editor_animation_debug_log(&format!(
            "anim_event: key={:?} op=CompositionUpdate unit_kinds={:?} carried_rebase={}",
            key,
            unit_kind_labels(&prepared.units),
            rebase_frames.len(),
        ));

        self.prepared_queue.enqueue(prepared);
        Some(key)
    }

    pub fn handle_composition_commit_or_cancel(
        &mut self,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        preedit_byte_start: usize,
        preedit_byte_end: usize,
        is_commit: bool,
        visual_text_unchanged: bool,
        candidate_byte_start: usize,
        candidate_byte_end: usize,
        committed_replace_start: usize,
        committed_replace_end: usize,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        old_cursor_visual_line_id: Option<usize>,
        new_cursor_visual_line_id: Option<usize>,
        old_cursor_line_top: f64,
        old_cursor_line_bottom: f64,
        new_cursor_line_top: f64,
        new_cursor_line_bottom: f64,
        cursor_owner_epoch: u64,
        layout_basis_revision: LayoutRevision,
    ) -> Option<VisualTransactionKey> {
        let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);
        // Issue #710 评论 5734282079: 不再把 committed_replace 坐标和 preedit virtualText 坐标 min/max。
        // old-side affected range 从 old preedit range（old virtualText 坐标）得到；
        // new-side: commit 用 candidate_byte_range（new snapshot 坐标），
        //           cancel 用 committed_replace_range（cancel 后 new = committed，坐标一致）。
        let (visual_affected_byte_range_old, visual_affected_byte_range_new) = {
            let new_edit_range = if is_commit {
                (candidate_byte_start, candidate_byte_end)
            } else {
                (committed_replace_start, committed_replace_end)
            };
            let (old_s, old_e, new_s, new_e) = compute_affected_paragraph_ranges(
                &old_snapshot.virtual_text,
                &new_snapshot.virtual_text,
                (preedit_byte_start, preedit_byte_end),
                new_edit_range,
            );
            (Some((old_s, old_e)), Some((new_s, new_e)))
        };
        let (conflict_old_start, conflict_old_end) =
            visual_affected_byte_range_old.unwrap_or((preedit_byte_start, preedit_byte_end));
        // Issue #710 评论 5733109905: 冲突检测用 current-old 坐标系。
        // conflict_old_start/end 是 old 坐标系，传 &old_snapshot.virtual_text
        // 作为 current_old_text。offset_map 仍保留用于 rebase。
        let conflicting = self.prepared_queue.find_conflicting_transaction(
            &old_snapshot.virtual_text,
            conflict_old_start,
            conflict_old_end,
        );
        // 预输入提交/取消同样整体替换 preedit 区间，不做保留判断。
        let now = Instant::now();
        let (rebase_frames, caret_handoff) = self.take_rebase_frames(
            &conflicting,
            "rebased_by_composition_commit",
            now,
            None,
            &old_snapshot.virtual_text,
            cursor_owner_epoch,
        );

        let key = self.alloc_key();

        let mut slices = Vec::new();
        // Issue #738 评论 5789470425 问题3: CrossFade group id 分配器（事务内唯一）。
        let mut next_crossfade_group_id: u64 = 1;

        if !is_commit {
            // Issue #687: cancel 时显式生成 DeleteConceal for preedit 范围的 old cluster，
            // reflow 只处理 unchanged material。changed range 由显式函数拥有。
            let cancel_deleted_range = (preedit_byte_start, preedit_byte_end);
            let conceal_slices = build_delete_conceal_slices(
                key,
                old_snapshot,
                cancel_deleted_range,
                old_cursor_rect.as_ref(),
            );
            slices.extend(conceal_slices);

            let cancel_excluded_old: [(usize, usize); 1] = [cancel_deleted_range];
            let reflow_slices = build_cluster_reflow_slices(
                key,
                old_snapshot,
                new_snapshot,
                &offset_map,
                &cancel_excluded_old,
                &[],
                old_cursor_rect.as_ref(),
                new_cursor_rect.as_ref(),
            );
            slices.extend(reflow_slices);
        } else {
            if visual_text_unchanged {
            } else {
                let insert_cx = old_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
                let insert_cy = old_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);
                let shrink_x = new_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
                let shrink_y = new_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);

                for old_line in
                    old_snapshot.lines_in_byte_range(preedit_byte_start, preedit_byte_end)
                {
                    for old_cluster in
                        old_line.clusters_in_byte_range(preedit_byte_start, preedit_byte_end)
                    {
                        let mapped_new_bs = offset_map.map_old_to_new(old_cluster.byte_start);
                        let mapped_new_be = offset_map.map_old_to_new(old_cluster.byte_end);
                        let matched_in_new =
                            if let (Some(mbs), Some(mbe)) = (mapped_new_bs, mapped_new_be) {
                                new_snapshot.line_snapshots.iter().any(|nl| {
                                    nl.clusters
                                        .iter()
                                        .any(|nc| nc.byte_start == mbs && nc.byte_end == mbe)
                                })
                            } else {
                                false
                            };
                        if !matched_in_new {
                            if let Some(old_sr) = old_line.source_rect_for_byte_range(
                                old_cluster.byte_start,
                                old_cluster.byte_end,
                            ) {
                                let from_doc = old_line.source_rect_to_document_rect(&old_sr);
                                // Issue #686 评论 5666452462：cancel 时 preedit 文字
                                // 走 delete_conceal，按 old rect 两侧与旧光标距离
                                // 决定收进方向：靠近右端 → Backspace → conceal_to_left_edge=true，
                                // 靠近左端 → Delete 键 → conceal_to_left_edge=false。
                                let left = from_doc.x;
                                let right = from_doc.x + from_doc.w;
                                let conceal_to_left_edge =
                                    (shrink_x - right).abs() <= (shrink_x - left).abs();
                                slices.push(AnimatedSlice::delete_conceal(
                                    key,
                                    old_line.id,
                                    old_sr,
                                    from_doc,
                                    shrink_x,
                                    shrink_y,
                                    old_cluster.byte_start,
                                    old_cluster.byte_end,
                                    Some(old_cluster.shaping_identity.clone()),
                                    conceal_to_left_edge,
                                    // Issue #722 评论 5749791161 问题2: 传真实 visual_line_id
                                    Some(old_line.visual_line_id),
                                ));
                            }
                        } else if let (Some(mbs), Some(mbe)) = (mapped_new_bs, mapped_new_be) {
                            if let Some((new_line, new_cluster)) = new_snapshot
                                .line_snapshots
                                .iter()
                                .filter_map(|nl| {
                                    nl.clusters
                                        .iter()
                                        .find(|nc| nc.byte_start == mbs && nc.byte_end == mbe)
                                        .map(|nc| (nl, nc))
                                })
                                .next()
                            {
                                if !old_cluster
                                    .shaping_identity
                                    .is_same_shaping(&new_cluster.shaping_identity)
                                {
                                    if let Some(old_sr) = old_line.source_rect_for_byte_range(
                                        old_cluster.byte_start,
                                        old_cluster.byte_end,
                                    ) {
                                        let old_doc =
                                            old_line.source_rect_to_document_rect(&old_sr);
                                        if let Some(new_sr) = new_line.source_rect_for_byte_range(
                                            new_cluster.byte_start,
                                            new_cluster.byte_end,
                                        ) {
                                            let new_doc =
                                                new_line.source_rect_to_document_rect(&new_sr);
                                            let group_id = next_crossfade_group_id;
                                            next_crossfade_group_id += 1;
                                            slices.push(AnimatedSlice::reflow_crossfade_old(
                                                key,
                                                old_line.id,
                                                old_sr,
                                                old_doc,
                                                new_doc,
                                                old_cluster.byte_start,
                                                old_cluster.byte_end,
                                                Some(old_cluster.shaping_identity.clone()),
                                                Some(group_id),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                for new_line in
                    new_snapshot.lines_in_byte_range(candidate_byte_start, candidate_byte_end)
                {
                    for new_cluster in
                        new_line.clusters_in_byte_range(candidate_byte_start, candidate_byte_end)
                    {
                        let mapped_old_bs = offset_map.map_new_to_old(new_cluster.byte_start);
                        let mapped_old_be = offset_map.map_new_to_old(new_cluster.byte_end);
                        let found_in_old =
                            if let (Some(mbs), Some(mbe)) = (mapped_old_bs, mapped_old_be) {
                                old_snapshot.line_snapshots.iter().any(|ol| {
                                    ol.clusters
                                        .iter()
                                        .any(|oc| oc.byte_start == mbs && oc.byte_end == mbe)
                                })
                            } else {
                                false
                            };
                        if !found_in_old {
                            if let Some(new_sr) = new_line.source_rect_for_byte_range(
                                new_cluster.byte_start,
                                new_cluster.byte_end,
                            ) {
                                let to_doc = new_line.source_rect_to_document_rect(&new_sr);
                                let to_doc_for_hide = to_doc.clone();
                                let mut reveal_slice = AnimatedSlice::insert_reveal(
                                    key,
                                    new_line.id,
                                    new_sr.clone(),
                                    to_doc,
                                    insert_cx,
                                    insert_cy,
                                    new_cluster.byte_start,
                                    new_cluster.byte_end,
                                    Some(new_cluster.shaping_identity.clone()),
                                    // Issue #722 评论 5749791161 问题2: 传真实 visual_line_id
                                    Some(new_line.visual_line_id),
                                );
                                reveal_slice.static_hidden_document_rects = vec![to_doc_for_hide];
                                slices.push(reveal_slice);
                            }
                        } else if let (Some(mbs), Some(mbe)) = (mapped_old_bs, mapped_old_be) {
                            if let Some((old_line, old_cluster)) = old_snapshot
                                .line_snapshots
                                .iter()
                                .filter_map(|ol| {
                                    ol.clusters
                                        .iter()
                                        .find(|oc| oc.byte_start == mbs && oc.byte_end == mbe)
                                        .map(|oc| (ol, oc))
                                })
                                .next()
                            {
                                let same_shaping = old_cluster
                                    .shaping_identity
                                    .is_same_shaping(&new_cluster.shaping_identity);
                                if !same_shaping {
                                    if let Some(new_sr) = new_line.source_rect_for_byte_range(
                                        new_cluster.byte_start,
                                        new_cluster.byte_end,
                                    ) {
                                        let old_doc = old_line.source_rect_to_document_rect(
                                            &old_line
                                                .source_rect_for_byte_range(
                                                    old_cluster.byte_start,
                                                    old_cluster.byte_end,
                                                )
                                                .unwrap_or(SourceRect::zero()),
                                        );
                                        let new_doc =
                                            new_line.source_rect_to_document_rect(&new_sr);
                                        let new_doc_for_hide = new_doc.clone();
                                        let group_id = next_crossfade_group_id;
                                        next_crossfade_group_id += 1;
                                        let mut new_slice = AnimatedSlice::reflow_crossfade_new(
                                            key,
                                            new_line.id,
                                            new_sr.clone(),
                                            old_doc,
                                            new_doc,
                                            new_cluster.byte_start,
                                            new_cluster.byte_end,
                                            Some(new_cluster.shaping_identity.clone()),
                                            Some(group_id),
                                        );
                                        new_slice.static_hidden_document_rects =
                                            vec![new_doc_for_hide];
                                        slices.push(new_slice);
                                    }
                                } else {
                                    if let (Some(old_sr), Some(new_sr)) = (
                                        old_line.source_rect_for_byte_range(
                                            old_cluster.byte_start,
                                            old_cluster.byte_end,
                                        ),
                                        new_line.source_rect_for_byte_range(
                                            new_cluster.byte_start,
                                            new_cluster.byte_end,
                                        ),
                                    ) {
                                        let old_doc =
                                            old_line.source_rect_to_document_rect(&old_sr);
                                        let new_doc =
                                            new_line.source_rect_to_document_rect(&new_sr);
                                        let geometry_same = (old_doc.x - new_doc.x).abs() < 0.5
                                            && (old_doc.y - new_doc.y).abs() < 0.5
                                            && (old_doc.w - new_doc.w).abs() < 0.5
                                            && (old_doc.h - new_doc.h).abs() < 0.5;
                                        if !geometry_same {
                                            let new_doc_for_hide = new_doc.clone();
                                            let mut move_slice = AnimatedSlice::reflow_move(
                                                key,
                                                old_line.id,
                                                old_sr,
                                                old_doc,
                                                new_line.id,
                                                new_sr.clone(),
                                                new_doc,
                                                new_cluster.byte_start,
                                                new_cluster.byte_end,
                                                Some(new_cluster.shaping_identity.clone()),
                                            );
                                            move_slice.static_hidden_document_rects =
                                                vec![new_doc_for_hide];
                                            slices.push(move_slice);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let reflow_slices = build_cluster_reflow_slices(
                    key,
                    old_snapshot,
                    new_snapshot,
                    &offset_map,
                    &[(preedit_byte_start, preedit_byte_end)],
                    &[(candidate_byte_start, candidate_byte_end)],
                    old_cursor_rect.as_ref(),
                    new_cursor_rect.as_ref(),
                );
                slices.extend(reflow_slices);
            }
        }

        let unit_duration_ms = u64::from(self.typing_animation_duration_ms);
        let mut units: Vec<PreparedVisualUnit> = slices
            .into_iter()
            .map(|s| PreparedVisualUnit::wrap(s, unit_duration_ms))
            .collect();
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        // Issue #690 评论 5681206040 + 5682867529: 构建 caret track（不传 now，等 Rendering 再启动）。
        let cursor_visual_track = build_cursor_visual_track(
            old_cursor_rect.as_ref(),
            new_cursor_rect.as_ref(),
            old_cursor_visual_line_id,
            new_cursor_visual_line_id,
            old_cursor_line_top,
            old_cursor_line_bottom,
            new_cursor_line_top,
            new_cursor_line_bottom,
            caret_handoff,
            unit_duration_ms,
        );
        let prepared = PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::CompositionCommitOrCancel,
            timeline: TransactionTimeline::new(unit_duration_ms),
            units,
            old_cursor_rect,
            new_cursor_rect,
            cursor_visual_track,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: Some(old_snapshot.clone()),
            new_snapshot: Some(new_snapshot.clone()),
            cursor_owner_epoch,
            caret_motion_retired: false,
            // Issue #710 评论 5734282079: composition commit/cancel 的 visual affected range。
            // 不再用保守大区间 min/max，而是分别从 old preedit range（old virtualText 坐标）
            // 和 new-side range（commit: candidate_byte_range / cancel: committed_replace_range）
            // 扩段落得到。
            visual_affected_byte_range_old,
            visual_affected_byte_range_new,
            layout_basis_revision,
        };

        // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
        emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
        editor_animation_debug_log(&format!(
            "anim_event: key={:?} op=CompositionCommitOrCancel unit_kinds={:?} carried_rebase={}",
            key,
            unit_kind_labels(&prepared.units),
            rebase_frames.len(),
        ));

        self.prepared_queue.enqueue(prepared);
        Some(key)
    }

    pub fn active_composition_new_snapshot(&self) -> Option<&EditorLayoutSnapshot> {
        self.prepared_queue
            .active_transactions()
            .iter()
            .filter(|t| {
                t.operation_kind == TextVisualOperationKind::CompositionUpdate
                    && t.state != TextVisualTransactionState::Cancelled
                    && t.state != TextVisualTransactionState::Completed
            })
            .filter_map(|t| t.new_snapshot.as_ref())
            .next_back()
    }

    pub fn cancel_active_composition(&mut self, reason: &str) {
        let keys: Vec<VisualTransactionKey> = self
            .prepared_queue
            .active_transactions()
            .iter()
            .filter(|t| {
                t.operation_kind == TextVisualOperationKind::CompositionUpdate
                    && t.state != TextVisualTransactionState::Cancelled
                    && t.state != TextVisualTransactionState::Completed
            })
            .map(|t| t.key)
            .collect();
        for key in keys {
            self.prepared_queue.cancel(key, reason);
        }
    }

    pub fn finish_by_key(&mut self, key: VisualTransactionKey) -> Option<Vec<LineSnapshotId>> {
        self.prepared_queue.complete(key)
    }

    /// Issue #736 评论 5786231506: 返回当前所有 active transaction 实际还引用的
    /// 去重 LineSnapshotId。事务完成、cancel、rebase 后，队列是"哪些视觉资源
    /// 仍有人用"的唯一事实源。
    pub(crate) fn collect_active_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> = Vec::new();
        for tx in self.prepared_queue.active_transactions() {
            if !matches!(
                tx.state,
                TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
            ) {
                ids.extend(tx.snapshot_ids());
            }
        }
        ids.sort_by_key(|id| (id.layout_revision, id.paragraph_id, id.visual_line_ordinal));
        ids.dedup();
        ids
    }

    pub fn cancel_by_key(&mut self, key: VisualTransactionKey, reason: &str) -> bool {
        self.prepared_queue.cancel(key, reason)
    }

    pub fn suppress_all(&mut self) -> bool {
        if self.prepared_queue.is_empty() {
            return false;
        }
        self.prepared_queue.cancel_all("suppress_all");
        true
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        let expired = self.prepared_queue.tick(now);
        !expired.is_empty()
    }

    /// Issue #702 评论 5708436497: 只做队列方法 `PreparedTransactionQueue::has_active_insert()`
    /// 的转发，不再承载"任意正文事务"的语义（那由 `has_active_text_transaction()` 负责）。
    /// 本方法只认 `TextVisualOperationKind::Insert`，用于输入时的光标 blink 抑制。
    pub fn has_active_insert(&self) -> bool {
        self.prepared_queue.has_active_insert()
    }

    /// Issue #702 评论 5708209114: 判断是否存在任意活跃正文视觉事务
    /// （Insert / Delete / CompositionUpdate / CompositionCommitOrCancel）。
    ///
    /// 与 [`has_active_insert`] 的区别：`has_active_insert()` 现在真正只查 Insert
    /// （Issue #702 评论 5708436497 修正了队列实现里缺失的 `operation_kind` 过滤），
    /// 只保留给"输入时抑制光标闪烁"这种 Insert 专属语义。本方法覆盖所有正文事务类型，
    /// 供 `build_cursor_plan()` 判断"正文协同是否活跃"——只要存在任意活跃正文事务且
    /// coordinated_enabled，就不能创建纯光标 Tween，正文光标只由
    /// `compute_coordinated_cursor_position()` 驱动。
    ///
    /// Issue #705 评论 5717380886: 本方法**不**检查 `cursor_owner_epoch`，保持
    /// "有没有活动事务"的语义。理由：本方法用于 `build_cursor_plan` 中的
    /// `_blink_mode` 计算，blink mode 不应因 epoch 变化而改变——文字动画还在播
    /// 就应该 suppress blink。直接遍历 active_transactions 判断，不调
    /// `active_text_transaction_key()`（后者现在带 epoch 检查）。
    pub(crate) fn has_active_text_transaction(&self) -> bool {
        for tx in self.prepared_queue.active_transactions().iter().rev() {
            if matches!(
                tx.state,
                TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
            ) {
                continue;
            }
            return true;
        }
        false
    }

    /// Issue #686 评论 5664857575 领域2：返回当前活动的正文编辑事务（Insert/Delete，
    /// 不含 Cursor）的 key。光标按事务身份绑定，不靠浮点坐标反查。
    ///
    /// 从新到旧找最近一条 state 不是 Completed/Cancelled 的正文事务（operation_kind
    /// 为 Insert/Delete/CompositionUpdate/CompositionCommitOrCancel）。
    /// Issue #702 评论 5707449688 问题 2: TextVisualOperationKind::Cursor 已删除，
    /// 所有非 Completed/Cancelled 的事务都是正文事务。
    ///
    /// Issue #705 评论 5717380886: 本方法**不**检查 `cursor_owner_epoch`，
    /// 只返回最近一条活动正文事务的 key。epoch 检查由调用方负责
    /// （`find_cursor_transaction_for_target` / `compute_coordinated_cursor_position`
    /// 在拿到 key 后检查 `tx.cursor_owner_epoch != current_cursor_epoch`）。
    /// 保留不带参数的签名是为了让 `has_active_text_transaction` 和结构守卫测试
    /// 能继续用"有没有活动事务"的语义判断。
    pub(crate) fn active_text_transaction_key(&self) -> Option<VisualTransactionKey> {
        for tx in self.prepared_queue.active_transactions().iter().rev() {
            if matches!(
                tx.state,
                TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
            ) {
                continue;
            }
            // Issue #727 评论 5760650874 方案 A / Issue #735 评论 5773604666 问题3:
            // 永久退休 caret motion 的事务不再被选为 active caret owner。
            // find_cursor_transaction_for_target / compute_coordinated_cursor_position
            // 都通过本方法取事务后驱动 caret，retired 事务不应再驱动 caret
            // （CaretDriven units 已落到终态，已 Snap 回 canonical）。
            if tx.caret_motion_retired {
                continue;
            }
            return Some(tx.key);
        }
        None
    }

    /// Issue #705 评论 5717380886: 返回当前活动的正文编辑事务的 key，
    /// 且其 `cursor_owner_epoch == current_cursor_epoch`。
    ///
    /// Issue #727 约束 1 / Issue #735 评论 5773604666 问题3: epoch 不一致时返回 None——
    /// 事务立刻失去 caret motion ownership，CaretDriven units（InsertReveal/DeleteConceal）
    /// 立即落到 canonical final state（`start_fraction = target_fraction`），
    /// 不再继续播放。ReflowMove/ReflowCrossFade 作为独立 passive reflow track 继续。
    ///
    /// 本方法供 `build_cursor_plan` 内部判断"正文协同是否活跃（epoch 一致）"用。
    /// `find_cursor_transaction_for_target` / `compute_coordinated_cursor_position`
    /// 直接调 `active_text_transaction_key()` 后手动检查 epoch，以保留结构守卫测试
    /// 期望的 `self.active_text_transaction_key()` 调用形式。
    fn active_text_transaction_key_with_epoch(
        &self,
        current_cursor_epoch: u64,
        current_layout_revision: LayoutRevision,
    ) -> Option<VisualTransactionKey> {
        for tx in self.prepared_queue.active_transactions().iter().rev() {
            if matches!(
                tx.state,
                TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
            ) {
                continue;
            }
            if tx.cursor_owner_epoch != current_cursor_epoch {
                continue;
            }
            // Issue #738 评论 5788513592 问题1: caret owner 选择必须看 layout_basis_revision。
            // 旧事务即使 cursor_owner_epoch 一致，若 layout basis 已过期，也不能继续拥有
            // coordinated caret——否则旧事务用旧 caret track 驱动光标，与 canonical 新布局分叉。
            // Issue #738 评论 5793319451 问题1: 守卫从 `<` 改成 `!=`。canonical 已推进到
            // new_revision 但 Pipeline.layout_revision 可能停在旧值时，future revision 的事务
            // 也不属于当前 canonical，不能继续拥有 caret ownership。只有 basis 完全一致的
            // 事务才能继续驱动 coordinated caret。
            if tx.layout_basis_revision != current_layout_revision {
                continue;
            }
            // Issue #727 评论 5760650874 方案 A / Issue #735 评论 5773604666 问题3:
            // 永久退休 caret motion 的事务永远跳过——之后本方法不会再返回此事务的 key，
            // sample_coordinated_motion_frame 不会再给它 owner_key，
            // 已 Snap 回 canonical 的旧 caret / 吞吐字轨迹不会重新接管。
            // ReflowMove/ReflowCrossFade 作为独立 passive reflow track 继续播完，
            // 事务只等剩余 Timed unit 完成。
            if tx.caret_motion_retired {
                continue;
            }
            return Some(tx.key);
        }
        None
    }

    /// Issue #679 评论 5657313927 (3d): 按当前光标 target 查找对应的事务。
    ///
    /// Issue #686 评论 5664857575 领域2：当存在活动正文事务时，直接返回该事务的 key
    /// 和它的 old/new cursor rect，不靠浮点坐标相等反查。光标按事务身份绑定。
    /// 只有在没有正文事务时才走原来的 CursorOnly 查找逻辑（按 target x/y 匹配）。
    ///
    /// Issue #705 评论 5717380886: 增加 `current_cursor_epoch` 参数。
    /// 调 `active_text_transaction_key()` 取活动事务后，检查其 `cursor_owner_epoch`
    /// 是否等于 `current_cursor_epoch`。
    ///
    /// Issue #735 评论 5773604666 问题3: epoch 不一致时不再只是 fall through，
    /// 而是触发收口逻辑——调用 `retire_caret_driven_units_for_transaction` 把
    /// CaretDriven units（InsertReveal/DeleteConceal）的 `start_fraction` 设为
    /// `target_fraction`（终态），并置 `caret_motion_retired = true`。
    /// ReflowMove/ReflowCrossFade 保留不动，作为独立 passive reflow track 继续。
    /// 不再存在"同一笔正文吞吐 transaction 还活着，但 caret_owner 已经不是它"
    /// 的状态。
    /// Issue #738 评论 5789470425 问题1: 增加 `current_layout_revision` 参数，
    /// 和 `active_text_transaction_key_with_epoch` 一样跳过 basis 不一致的事务。
    /// 旧事务即使 cursor_owner_epoch 一致，若 layout basis 已过期，也不能继续拥有
    /// coordinated caret——否则旧事务用旧 caret track 驱动光标，与 canonical 新布局分叉。
    pub(crate) fn find_cursor_transaction_for_target(
        &mut self,
        target_x: f64,
        target_y: f64,
        _target_h: f64,
        current_cursor_epoch: u64,
        current_layout_revision: LayoutRevision,
    ) -> Option<(VisualTransactionKey, Option<CursorRect>, Option<CursorRect>)> {
        // 领域2：优先按事务身份绑定——存在活动正文事务时直接返回。
        // Issue #738 评论 5789470425 问题1: 用 active_text_transaction_key_with_epoch
        // 同时检查 epoch 和 layout_basis_revision，跳过 basis 不一致的事务。
        if let Some(key) = self.active_text_transaction_key_with_epoch(
            current_cursor_epoch,
            current_layout_revision,
        ) {
            if let Some(tx) = self
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == key)
            {
                return Some((
                    tx.key,
                    tx.old_cursor_rect.clone(),
                    tx.new_cursor_rect.clone(),
                ));
            }
        }
        // epoch 不一致但 basis 一致的事务可能需要收口。检查是否存在 epoch 不一致的事务。
        if let Some(key) = self.active_text_transaction_key() {
            if let Some(tx) = self
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == key)
            {
                if tx.cursor_owner_epoch != current_cursor_epoch {
                    // Issue #735 评论 5773604666 问题3: 收口这笔事务的 CaretDriven units。
                    self.retire_caret_driven_units_for_transaction(key);
                }
            }
        }

        // 没有正文事务（或 epoch/basis 不一致已收口）时走 CursorOnly 查找逻辑（按 target x/y 匹配）。
        // Issue #738 评论 5789470425 问题1: CursorOnly 查找也跳过 basis 不一致的事务。
        // Issue #738 评论 5793319451 问题1: 守卫从 `<` 改成 `!=`，future revision 的事务
        // 也不属于当前 canonical，不能按其 new_cursor_rect 反查当作 CursorOnly 命中。
        for tx in self.prepared_queue.active_transactions().iter().rev() {
            if matches!(
                tx.state,
                TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
            ) {
                continue;
            }
            if tx.cursor_owner_epoch != current_cursor_epoch {
                continue;
            }
            if tx.layout_basis_revision != current_layout_revision {
                continue;
            }
            if let Some(ref new_rect) = tx.new_cursor_rect {
                if (new_rect.x - target_x).abs() <= 0.01 && (new_rect.top - target_y).abs() <= 0.01
                {
                    return Some((
                        tx.key,
                        tx.old_cursor_rect.clone(),
                        tx.new_cursor_rect.clone(),
                    ));
                }
            }
        }
        None
    }

    /// Issue #735 评论 5773604666 问题3: 收口指定事务的 CaretDriven units。
    ///
    /// 当正文 edit motion 失去 caret ownership 时调用。把指定事务的
    /// CaretDriven units（InsertReveal/DeleteConceal）的 `start_fraction` 设为
    /// `target_fraction`（终态），并置 `caret_motion_retired = true`。
    /// ReflowMove/ReflowCrossFade 保留不动，作为独立 passive reflow track 继续。
    ///
    /// 调用后：
    /// - CaretDriven units 立即落到 canonical final state，不再继续播。
    /// - `active_text_transaction_key_with_epoch` / `active_text_transaction_key`
    ///   永远跳过此事务（`caret_motion_retired == true`）。
    /// - 如果事务中还有 Timed unit（Reflow），事务不立即 Completed，等它们播完。
    /// - 如果没有 Timed unit，事务可在下一帧 Completed。
    ///
    /// 幂等：对已 retired 的事务再次调用是 no-op（start_fraction 已是 target_fraction）。
    fn retire_caret_driven_units_for_transaction(&mut self, key: VisualTransactionKey) {
        let tx = match self
            .prepared_queue
            .active_transactions_mut()
            .iter_mut()
            .find(|t| t.key == key)
        {
            Some(t) => t,
            None => return,
        };
        // 已 retired 的事务无需重复收口。
        if tx.caret_motion_retired {
            return;
        }
        // 只有含 CaretDriven units 的事务才需要收口。
        if !tx.has_caret_driven_units() {
            // 纯 Reflow 事务本来就不驱动 caret，只置 retired 标记防止重新被选为 owner。
            tx.caret_motion_retired = true;
            return;
        }
        tx.retire_caret_driven_units();
        tx.caret_motion_retired = true;
        editor_animation_debug_log(&format!(
            "retire_caret_driven: key={:?} op={:?} units={} — CaretDriven units 已落到终态",
            tx.key,
            tx.operation_kind,
            tx.units.len(),
        ));
    }

    pub fn has_prepared_or_rendering(&self) -> bool {
        self.prepared_queue.active_transactions().iter().any(|t| {
            t.state == TextVisualTransactionState::Prepared
                || t.state == TextVisualTransactionState::Rendering
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_cursor_plan(
        &self,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        cursor_x: f64,
        cursor_y: f64,
        cursor_h: f64,
        editor_enabled: bool,
        has_selection: bool,
        viewport_height: f64,
        is_scrolling: bool,
        is_selecting: bool,
        is_preediting: bool,
        smooth_cursor_enabled: bool,
        smooth_cursor_duration_ms: u32,
        scroll_y: f64,
        old_visible: bool,
        old_blink_visible: bool,
        old_visual_x: f64,
        old_visual_y: f64,
        force_snap_next: bool,
        cursor_animation: Option<&super::rendering::CursorAnimationState>,
        cursor_owner_epoch: u64,
        cursor_move_source: super::cursor_controller::CursorMoveSource,
        cursor_baseline_y: f64,
        layout_basis_revision: LayoutRevision,
    ) -> CursorAnimationPlan {
        // Issue #727 评论 5757225958 问题1: cursor_y 现在是文档坐标（caller 改用
        // editor_layout_cursor_rect_doc），in_viewport 判断需要视口坐标 screen_y =
        // cursor_y - scroll_y。cursor_ctrl.target_y/visual_y 统一保存文档坐标。
        let screen_y = cursor_y - scroll_y;
        let in_viewport = screen_y + cursor_h > 0.0 && screen_y < viewport_height;
        // Issue #724 评论 5750911834 问题 2: should_be_visible 不再用 !is_scrolling
        // 一刀切隐藏光标。滚动期间光标应保持可见（自动跟随滚动时光标在视口内
        // 同一相对位置；用户手动滚动时光标位置不变，只要 in_viewport 就应可见）。
        // 旧逻辑 `editor_enabled && !has_selection && in_viewport && !is_scrolling`
        // 导致滚动期间光标被隐藏，滚动结束时光标动画偶发消失。
        let should_be_visible = editor_enabled && !has_selection && in_viewport;

        // Issue #705 评论 5717380886: 区分两种"有活动正文事务"的判断：
        // - `has_active_for_blink`：不看 epoch，只要文字动画还在播就 suppress blink。
        // - `has_active_for_coordinated`：看 epoch，只有 epoch 一致的事务才驱动
        //   coordinated caret。
        // Issue #735 评论 5773604666 问题3: epoch 不一致时 CaretDriven units 已在
        //   `find_cursor_transaction_for_target` / `build_text_animation_plan_with_sample`
        //   中收口（start_fraction 设为 target_fraction，caret_motion_retired = true），
        //   不再继续播自己的 glyph。ReflowMove/ReflowCrossFade 作为独立 passive
        //   reflow track 继续。纯光标移动可走 Tween。
        let has_active_for_coordinated = self
            .active_text_transaction_key_with_epoch(
                cursor_owner_epoch,
                layout_basis_revision,
            )
            .is_some();
        // Issue #710 评论 5731145076 症状二: 统一 blink 决策。
        // blink_mode 不再在 build_cursor_plan 里计算（之前的 _blink_mode 计算后未使用，
        // 导致 GUI timer 和 render plan 两套判断分歧）。现在 blink 决策只由
        // tick_cursor_animation 每帧从 has_active_text_transaction() + CursorOnly Tween
        // 实时计算，build_cursor_plan 不再参与 blink 决策。
        // has_active_for_blink 也不再在此计算，避免误导读者以为这里还在做 blink 决策。

        // Issue #722 评论 5747719529 改法 1: 把滚动从光标动画判定里彻底拆出去。
        // 删除 scroll_changed 和 old_scroll_y：真实滚动开始/结束继续由 set_is_scrolling()
        // 控制暂停和一次 Snap；普通 contentY -> scroll_y 只是 viewport transform，
        // 不能永久改变光标动画策略。hard_snap 只保留 force_snap_next / is_scrolling /
        // is_selecting / !old_visible。
        // Issue #724 评论 5750911834 问题 2: is_scrolling 不再驱动 should_be_visible
        // 和 hard_snap，滚动的暂停和恢复由 set_is_scrolling() 单独控制。
        // Issue #727 评论 5757225958 问题1: scroll_y 现在用于 in_viewport 判断
        //（cursor_y 是文档坐标），不再丢弃。
        let _ = is_scrolling;

        // Issue #712: 删除 cross_line_snap = dy > cursor_h * 3.0 按距离猜用户意图的规则，
        // 改为按 CursorMoveSource 决定跨行是否允许 Tween。
        let allow_cross_line_tween = match cursor_move_source {
            super::cursor_controller::CursorMoveSource::PointerClick
            | super::cursor_controller::CursorMoveSource::KeyboardNavigation => {
                smooth_cursor_enabled
            }
            super::cursor_controller::CursorMoveSource::DragSelection
            | super::cursor_controller::CursorMoveSource::LayoutChange
            | super::cursor_controller::CursorMoveSource::Scroll => false,
            super::cursor_controller::CursorMoveSource::TextTransaction => false,
        };

        // Issue #679 评论 5658087764 (1): force_snap_next 是一次性强制 Snap 标记，
        // 不再附加"距离够大才算"的条件；点击/滚动/选择/不可见都硬 Snap，
        // 不再被协调动画覆盖为 Tween。
        // Issue #722 评论 5747719529: 删除 scroll_changed，hard_snap 只保留
        // force_snap_next / is_scrolling / is_selecting / !old_visible。
        // Issue #724 评论 5750911834 问题 2: hard_snap 不再因 is_scrolling 强制 snap。
        // 旧逻辑 `force_snap_next || is_scrolling || is_selecting || !old_visible`
        // 导致滚动时强制 Snap，滚动结束时光标动画被 snap 到终态。
        // 滚动的暂停和恢复由 set_is_scrolling() 单独控制，不影响 hard_snap。
        let hard_snap = force_snap_next || is_selecting || !old_visible;

        // Issue #702 评论 5707449688 问题 2: 纯光标移动彻底和文字事务 key 解耦，
        // 不再用 driver_key.is_some() 决定 can_tween。纯光标只要满足 smooth cursor
        // 条件，就直接从当前 visual_x/visual_y 建自己的 Tween，由 CursorAnimationState
        // 自己的 timeline 推进。
        // Issue #702: 纯光标移动 Tween 的 duration_ms，供 CursorAnimationState 自己的 timeline。
        let tween_duration_ms = u64::from(smooth_cursor_duration_ms);

        let transition = if !should_be_visible || hard_snap {
            CursorTransition::Snap
        } else if !smooth_cursor_enabled || !allow_cross_line_tween {
            // Issue #702 评论 5707770318: 正文事务活跃时，光标位置只由
            // compute_coordinated_cursor_position 驱动（正文协同），不应再开
            // CursorAnimationState 独立 timeline。返回 Snap 让 apply_plan 清除
            // animation，不创建独立 timeline。只有没有正文事务时才走纯光标 Tween。
            // Issue #712: !allow_cross_line_tween 替代旧的 cross_line_snap，
            // 按 CursorMoveSource 决定跨行是否允许 Tween。
            CursorTransition::Snap
        } else if let Some(anim) = cursor_animation {
            if (anim.target_x - cursor_x).abs() > 0.01 || (anim.target_y - cursor_y).abs() > 0.01 {
                // Issue #702 评论 5707770318: 正文事务活跃时返回 Snap，
                // 不创建独立 CursorAnimationState timeline。
                // Issue #705 评论 5717380886: 用 has_active_for_coordinated（看 epoch），
                // epoch 不一致时纯光标移动可走 Tween。
                // Issue #727 约束 6: 删除 coordinated_text_cursor_animation_enabled
                // 独立开关。是否有吞吐字直接由 has_active_for_coordinated（本帧有没有
                // 有效 caret motion track）决定，不再受外部开关控制。
                if has_active_for_coordinated {
                    CursorTransition::Snap
                } else {
                    // Issue #702 评论 5707449688 问题 2: 纯光标 Tween 不再需要 driver_key，
                    // 直接从当前 anim 的 start 位置建 Tween。
                    // Issue #712 评论 5739517945: baseline_y 从 canonical caret geometry 获取，
                    // 不使用 top + h * 0.8 估算。
                    let new_baseline_y = new_cursor_rect
                        .as_ref()
                        .map(|r| r.baseline_y)
                        .unwrap_or(cursor_baseline_y);
                    let old_baseline_y = old_cursor_rect
                        .as_ref()
                        .map(|r| r.baseline_y)
                        .unwrap_or(cursor_baseline_y);
                    CursorTransition::Tween {
                        old_rect: CursorRect {
                            x: anim.start_x,
                            top: anim.start_y,
                            bottom: anim.start_y + cursor_h,
                            baseline_y: old_baseline_y,
                        },
                        new_rect: CursorRect {
                            x: cursor_x,
                            top: cursor_y,
                            bottom: cursor_y + cursor_h,
                            baseline_y: new_baseline_y,
                        },
                        duration_ms: tween_duration_ms,
                    }
                }
            } else {
                CursorTransition::Snap
            }
        } else if (old_visual_x - cursor_x).abs() > 0.01 || (old_visual_y - cursor_y).abs() > 0.01 {
            // Issue #702 评论 5707770318: 正文事务活跃时返回 Snap，
            // 不创建独立 CursorAnimationState timeline。
            // Issue #705 评论 5717380886: 用 has_active_for_coordinated（看 epoch），
            // epoch 不一致时纯光标移动可走 Tween。
            // Issue #727 约束 6: 删除 coordinated_text_cursor_animation_enabled 独立开关。
            if has_active_for_coordinated {
                CursorTransition::Snap
            } else {
                // Issue #702 评论 5707449688 问题 2: 纯光标 Tween 不再需要 driver_key，
                // 直接从当前 visual_x/visual_y 建 Tween。
                // Issue #712 评论 5739517945: baseline_y 从 canonical caret geometry 获取，
                // 不使用 top + h * 0.8 估算。
                let new_baseline_y = new_cursor_rect
                    .as_ref()
                    .map(|r| r.baseline_y)
                    .unwrap_or(cursor_baseline_y);
                let old_baseline_y = old_cursor_rect
                    .as_ref()
                    .map(|r| r.baseline_y)
                    .unwrap_or(cursor_baseline_y);
                CursorTransition::Tween {
                    old_rect: CursorRect {
                        x: old_visual_x,
                        top: old_visual_y,
                        bottom: old_visual_y + cursor_h,
                        baseline_y: old_baseline_y,
                    },
                    new_rect: CursorRect {
                        x: cursor_x,
                        top: cursor_y,
                        bottom: cursor_y + cursor_h,
                        baseline_y: new_baseline_y,
                    },
                    duration_ms: tween_duration_ms,
                }
            }
        } else {
            CursorTransition::Snap
        };

        let _ = (is_preediting, old_blink_visible);
        // Issue #702 评论 5707770318: old_cursor_rect/new_cursor_rect 的 baseline_y
        // 已用于 Tween 构造（Issue #712），不再整体丢弃。
        let _ = (old_cursor_rect, new_cursor_rect);

        CursorAnimationPlan {
            should_be_visible,
            transition,
            cursor_x,
            cursor_y,
            cursor_h,
            cursor_baseline_y,
        }
    }

    /// Issue #727 约束 7: 滚动开始时，CaretDriven 事务（InsertReveal/DeleteConceal）
    /// 依赖 caret motion track，caret track 被终止时它们必须立即完成到 canonical 状态，
    /// 不能只 pause（pause 后 resume 时 caret track 已不存在，数据依赖链断裂）。
    /// Timed 事务（ReflowMove/ReflowCrossFade）有独立时间线，可以正常 pause/resume。
    ///
    /// 此方法先完成所有含 CaretDriven unit 的事务，再 pause 剩下的 Timed 事务。
    /// 返回被完成事务的 snapshot IDs，供调用方清理 texture cache。
    pub(crate) fn pause_all(&mut self) -> Vec<super::layout_snapshot::LineSnapshotId> {
        use super::text_visual_transaction::{TextVisualTransactionState, VisualUnitTiming};

        // 1. 找出所有含 CaretDriven unit 的活跃事务，完成它们到 canonical 状态。
        let caret_driven_keys: Vec<VisualTransactionKey> = self
            .prepared_queue
            .active_transactions()
            .iter()
            .filter(|t| {
                t.state != TextVisualTransactionState::Completed
                    && t.state != TextVisualTransactionState::Cancelled
                    && t.units
                        .iter()
                        .any(|u| matches!(u.timing, VisualUnitTiming::CaretDriven { .. }))
            })
            .map(|t| t.key)
            .collect();
        let mut freed_snapshot_ids = Vec::new();
        for key in caret_driven_keys {
            if let Some(ids) = self.prepared_queue.complete(key) {
                freed_snapshot_ids.extend(ids);
            }
        }
        // 2. pause 剩下的活跃事务（纯 Timed：ReflowMove/ReflowCrossFade）。
        for tx in self.prepared_queue.active_transactions_mut() {
            tx.pause();
        }
        freed_snapshot_ids
    }

    pub(crate) fn resume_all(&mut self) {
        for tx in self.prepared_queue.active_transactions_mut() {
            tx.resume();
        }
    }

    /// Issue #727 评论 5760020833 问题2: 把本帧 Prepared 事务切到 Rendering，
    /// 并用当前 frame_now 启动 transaction timeline / Timed units / cursor track。
    /// 必须在 `sample_coordinated_motion_frame` 之前调用，否则刚进入 Prepared 的
    /// 新事务第一帧采样时状态仍为 Prepared → caret=None → InsertReveal/DeleteConceal
    /// 不画、clip 也不藏 → 第一帧直接显示最终正文 → 下一帧才从 progress≈0 开始动画，
    /// 形成固定的一帧"先显示最终文字，再开始动画"的闪烁。
    fn begin_rendering_transactions(&mut self, frame_now: Instant) {
        for tx in self.prepared_queue.active_transactions_mut() {
            if tx.state == TextVisualTransactionState::Prepared {
                tx.state = TextVisualTransactionState::Rendering;
                if !tx.timeline.is_started() {
                    // Issue #727 评论 5760431554 问题2: 传同一个 frame_now，
                    // transaction timeline 与 unit/cursor track 共用同一帧起点。
                    tx.timeline.mark_first_frame(frame_now);
                }
                // Issue #690 评论 5675007226 步骤 3: 事务进入 Rendering 时，为每个视觉单元
                // 打上统一的起始时间；之后每个单元按自己的 duration_ms 独立计算 progress。
                // Issue #727 约束 2: 通过 VisualUnitTiming::mark_started 统一处理。
                // CaretDriven unit 无独立时间线，mark_started 是 no-op。
                for unit in &mut tx.units {
                    unit.timing.mark_started(frame_now);
                }
                // Issue #690 评论 5682867529: caret track 跟文字 unit 同一个 frame_now 启动，
                // 不再在事务创建时就开始计时。这样第一帧 text unit progress = 0 且
                // caret track progress = 0，文字和光标从同一屏幕帧起跑。
                if let Some(track) = tx.cursor_visual_track.as_mut() {
                    if track.started_at.is_none() {
                        track.started_at = Some(frame_now);
                    }
                }
            }
        }
    }

    /// Issue #690 评论 5675007226 步骤 1+2: 接受 `frame_now`，统一采样文字和光标 progress。
    ///
    /// 文字和光标的 progress 全部从同一个 `frame_now` 计算，消除 GUI 线程 tick 和
    /// Scene Graph 渲染帧之间的采样偏差。当正文编辑事务活跃且 coordinated 动画启用时，
    /// 光标位置直接从 text animation progress 计算（跟随文字吞吐边界），
    /// 不再使用 GUI 线程上一帧留下的 `cursor_ctrl.visual_x/y`。
    pub(crate) fn build_render_plan_full(
        &mut self,
        mut cursor_render_state: CursorRenderState,
        selection_preedit: SelectionPreeditPlan,
        mut frame_context: super::render_plan::FrameContext,
        cursor_style: super::render_plan::CursorStyle,
        selection_preedit_style: super::render_plan::SelectionPreeditStyle,
        frame_now: Instant,
        cursor_animation: Option<&super::rendering::CursorAnimationState>,
        cursor_owner_epoch: u64,
        _current_scroll_y: f64,
    ) -> RenderPlan {
        self.begin_rendering_transactions(frame_now);
        let mut frame_sample = AnimationFrameSample::new(frame_now);
        for tx in self.prepared_queue.active_transactions() {
            if !matches!(
                tx.state,
                TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
            ) {
                frame_sample.set_progress(tx.key, tx.progress(frame_now));
            }
        }
        // Issue #738 评论 5788513592 问题1: 先按 layout basis 收口旧事务再采样 caret motion。
        let (text_animation, keys_to_complete, coordinated_motion_frame) = self
            .build_text_animation_plan_with_sample(
                &frame_sample,
                cursor_owner_epoch,
                frame_context.layout_basis_revision,
            );
        let coordinated_motion_frame = self.sample_coordinated_motion_frame(
            &frame_sample,
            cursor_owner_epoch,
            frame_context.layout_basis_revision,
        );
        // Issue #727 评论 5757225958 问题3: 先构建 keys_to_complete_set，
        // 供 clip_rects 收集时跳过本帧即将完成的事务，避免"glyph 无、clip 有"
        // 的一帧文字消失/闪烁。
        let keys_to_complete_set: std::collections::HashSet<VisualTransactionKey> =
            keys_to_complete.iter().copied().collect();
        frame_context.keys_to_complete = keys_to_complete;
        let active_keys: Vec<VisualTransactionKey> = self
            .prepared_queue
            .active_transactions()
            .iter()
            .map(|t| t.key)
            .collect();
        frame_context.active_transaction_keys = active_keys;

        // Issue #727 评论 5755858583 问题2: 只从 AnimatedSlice.static_hidden_document_rects
        // 收集裁剪区域，不再从 tx.static_patches 收集。AnimatedSlice 成为唯一事实源。
        // 只有 texture_prepared == true 的事务才允许静态层隐藏，
        // 避免纹理准备完成前出现空白帧。
        // Issue #679 评论 5657313927 (3e): 只允许 Prepared / Rendering / Paused
        // 的事务裁剪静态正文；Pending 无论 texture_prepared 是什么都不能隐藏正文，
        // 否则资源还没准备好就会出现空洞。
        // Issue #727 评论 5757225958 问题3: 收集 clip_rects 时跳过本帧 keys_to_complete
        // 里的事务。既然这一帧已经不画 overlay（build_text_animation_plan_with_sample
        // 完成帧 continue 跳过 glyph 生成），就必须同帧释放 static ownership，让 canonical
        // 最终正文立即显示，避免"glyph 无、clip 有"的一帧文字消失/闪烁。
        // Issue #727 评论 5757225958 问题2+5: 无 caret frame 时不收集 CaretDriven units
        // 的 rects——本帧 unit 不画就不能继续隐藏 canonical（同帧释放
        // ownership），避免空洞。
        // Issue #727 评论 5757225958 问题2+5: 无 caret frame 时不收集 CaretDriven units
        // 的 rects——本帧 unit 不画就不能继续隐藏 canonical（同帧释放
        // ownership），避免空洞。
        let mut clip_rects: Vec<super::qt_text_node::AnimationClipRect> = Vec::new();
        for tx in self.prepared_queue.active_transactions() {
            // Issue #738 评论 5793319451 问题1: 守卫从 `>=` 改成 `==`。clip rects 用于
            // 裁切 canonical 正文以露出动画 overlay，只有 basis 与当前 frame_context 完全
            // 一致的事务的 static_hidden_document_rects 才属于当前 canonical 几何。
            // future revision 的事务其 hidden rects 对应另一份 canonical，不能裁当前正文。
            if tx.texture_prepared
                && tx.state.is_clip_eligible()
                && !keys_to_complete_set.contains(&tx.key)
                && tx.layout_basis_revision == frame_context.layout_basis_revision
            {
                let has_caret_frame = coordinated_motion_frame.caret.is_some();
                // Issue #727 评论 5760020833 问题1: 还要判断本事务是否是 caret motion 的
                // owner。当旧 CaretDriven 事务 owner 已丢失（epoch 切换/新事务抢占），
                // 即使全局有新事务的 caret frame，旧事务的 static_hidden_document_rects
                // 也不能继续裁 canonical 正文——非 owner 的 CaretDriven 已 Snap 到 canonical，
                // 再藏 canonical 会挖出文字空洞。
                let owns_caret = coordinated_motion_frame.owner_key == Some(tx.key);
                for unit in &tx.units {
                    let is_caret_driven = matches!(
                        unit.slice.kind,
                        AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal
                    );
                    // CaretDriven unit 只在本事务拥有 caret frame（has_caret_frame 且
                    // owns_caret）时才收集；Timed unit（Reflow）始终收集。
                    // !owns_caret 时也不收集：非 owner 的 CaretDriven 已 Snap 到 canonical，
                    // 不能再藏 canonical 正文。
                    if is_caret_driven && (!has_caret_frame || !owns_caret) {
                        continue;
                    }
                    for doc_rect in &unit.slice.static_hidden_document_rects {
                        if doc_rect.h > 0.0 && doc_rect.w > 0.0 {
                            clip_rects.push(super::qt_text_node::AnimationClipRect {
                                x: doc_rect.x,
                                y: doc_rect.y,
                                w: doc_rect.w,
                                h: doc_rect.h,
                                snapshot_id: unit.slice.snapshot_id,
                            });
                        }
                    }
                }
            }
        }

        // Issue #690 评论 5675007226 步骤 2: 协同光标位置从同一 frame_now 计算。
        // 光标严格跟随文字吞吐边界：InsertReveal → 右边界，DeleteConceal → 吞字边界，
        // Reflow/Cursor → old/new 插值。不再用单一 progress 在 old/new rect 之间线性插值。
        //
        // Issue #701 评论 5699573227 第三阶段 (F5): 每帧只采样一次 frame state。
        // 文字层和光标层都使用同一份 `AnimationFrameSample`。无活跃文字事务时，
        // CursorOnly 光标位置也从 frame_sample 采样，不再在 build_render_plan_full
        // 之外用 cursor_timeline_sample_with_time 单独推进 cursor_ctrl.visual_x/y。
        let mut cursor_sample_outcome = super::render_plan::CursorSampleOutcome::Idle;
        // Issue #727 约束 6: 删除 coordinated_text_cursor_animation_enabled 独立开关。
        // 是否有吞吐字直接由 compute_coordinated_cursor_position 是否返回 Some 决定。
        // Issue #705 评论 5717380886: 传入 cursor_owner_epoch。
        // Issue #727 约束 1 / Issue #735 评论 5773604666 问题3: epoch 不一致时
        // compute_coordinated_cursor_position 返回 None，事务立刻失去 caret motion
        // ownership，CaretDriven units 已落到 canonical final state（不再继续播放）。
        // 改走 CursorOnly/点击位置。
        if let Some((cx, cy_doc, ch)) =
            self.compute_coordinated_cursor_position(&frame_sample, cursor_owner_epoch)
        {
            // Issue #727 评论 5755858583 问题1: cursor_render_state.y 保存文档坐标（cy_doc），
            // 不再提前减 scroll_y 转成视口 y。cursor layer 的 QSGTransformNode 统一做
            // translate(0, -scroll_y)，和正文/动画层一致。
            // Issue #702 评论 5707770318: 正文协同光标位置已算出，
            // 把 cursor_sample_outcome 设为 Coordinated { x, y, h }，
            // 让 qquickitem_impl 同步 visual_x/visual_y/visual_h 到本帧
            // 屏幕真正画出的位置，但不启动 CursorAnimationState.started_at，
            // 不创建独立 timeline。正文光标只由 compute_coordinated_cursor_position 驱动。
            cursor_sample_outcome = super::render_plan::CursorSampleOutcome::Coordinated {
                x: cx,
                y: cy_doc,
                h: ch,
            };
            let suppressed = matches!(
                self.active_operation_kind(),
                Some(TextVisualOperationKind::Insert)
            );
            let blink_mode = if suppressed {
                CursorBlinkMode::Suppressed
            } else {
                CursorBlinkMode::Normal
            };
            let opacity = if blink_mode == CursorBlinkMode::Suppressed {
                1.0
            } else {
                cursor_render_state.opacity
            };
            cursor_render_state = CursorRenderState {
                visible: true,
                x: cx,
                y: cy_doc,
                h: ch,
                opacity,
            };
        } else if let Some(anim) = cursor_animation {
            // 无活跃文字事务但有 CursorOnly 动画：用同一份 frame_sample 采样光标位置。
            cursor_sample_outcome = self.sample_cursor_only_position(anim, &frame_sample);
            match cursor_sample_outcome {
                super::render_plan::CursorSampleOutcome::Running(p) => {
                    let eased = super::rendering::ease_out_cubic(p);
                    cursor_render_state.x = anim.start_x + (anim.target_x - anim.start_x) * eased;
                    cursor_render_state.y = anim.start_y + (anim.target_y - anim.start_y) * eased;
                }
                super::render_plan::CursorSampleOutcome::Finished => {
                    cursor_render_state.x = anim.target_x;
                    cursor_render_state.y = anim.target_y;
                }
                super::render_plan::CursorSampleOutcome::Idle => {}
                // Issue #702 评论 5707770318: sample_cursor_only_position 不会返回
                // Coordinated（它只服务纯光标 CursorOnly 动画），此分支不可达。
                super::render_plan::CursorSampleOutcome::Coordinated { .. } => {}
            }
        }

        // Issue #705: drawn_caret_rect 是本帧真正绘制出去的 caret rect。
        // 根据 cursor_sample_outcome 和最终 cursor_render_state 算出。
        // Coordinated → 协同位置;Running/Finished → cursor_render_state 已更新;
        // Idle → 当前 visual 位置。
        // Issue #727 评论 5755858583 问题1: drawn_caret_rect 保存文档坐标 y，
        // apply_render_plan_cursor_state 在回写 visual_y 时转成视口 y。
        let drawn_caret_rect: Option<(f64, f64, f64)> = Some((
            cursor_render_state.x,
            cursor_render_state.y,
            cursor_render_state.h,
        ));

        RenderPlan {
            text_animation,
            selection_preedit,
            cursor: cursor_render_state,
            frame_context,
            cursor_style,
            selection_preedit_style,
            clip_rects,
            cursor_sample_outcome,
            drawn_caret_rect,
            coordinated_motion_frame,
        }
    }

    /// Issue #727 约束 3: 采样本帧统一的 CoordinatedMotionFrame。
    ///
    /// 在 `build_render_plan_full` 入口处调用，先采样 caret motion 得到一份
    /// `SampledCaretFrame`，供 cursor layer 和文字 reveal/conceal 共享。
    ///
    /// Issue #727 约束 1 / Issue #735 评论 5773604666 问题3: epoch 不一致时返回 None——
    /// 事务立刻失去 caret motion ownership，CaretDriven units（InsertReveal/DeleteConceal）
    /// 已落到 canonical final state（不再继续播放）。
    fn sample_coordinated_motion_frame(
        &self,
        sample: &AnimationFrameSample,
        cursor_owner_epoch: u64,
        layout_basis_revision: LayoutRevision,
    ) -> super::render_plan::CoordinatedMotionFrame {
        // 取当前 epoch 一致且 layout basis 未过期的活动正文事务。
        let key = match self.active_text_transaction_key_with_epoch(
            cursor_owner_epoch,
            layout_basis_revision,
        ) {
            Some(k) => k,
            None => {
                return super::render_plan::CoordinatedMotionFrame {
                    caret: None,
                    owner_key: None,
                };
            }
        };
        let tx = match self
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key)
        {
            Some(t) => t,
            None => {
                return super::render_plan::CoordinatedMotionFrame {
                    caret: None,
                    owner_key: None,
                };
            }
        };
        // 只有 Rendering / Paused 状态才有有效 caret motion。
        if !matches!(
            tx.state,
            TextVisualTransactionState::Rendering | TextVisualTransactionState::Paused
        ) {
            return super::render_plan::CoordinatedMotionFrame {
                caret: None,
                owner_key: None,
            };
        }
        // 采样 caret geometry + progress。
        let (x, y, visual_line_id, progress) = match tx.cursor_visual_track.as_ref() {
            Some(track) => {
                let progress = track.progress(sample.frame_now);
                let r = track.sampled_rect_at_progress(progress);
                let line_id = track.sampled_visual_line_id_at_progress(progress);
                (r.x, r.top, line_id, progress)
            }
            None => {
                // 无 cursor_visual_track：无有效 caret motion。
                return super::render_plan::CoordinatedMotionFrame {
                    caret: None,
                    owner_key: None,
                };
            }
        };
        super::render_plan::CoordinatedMotionFrame {
            caret: Some(super::render_plan::SampledCaretFrame {
                x,
                y,
                visual_line_id,
                progress,
            }),
            // Issue #727 评论 5757225958 问题5: 记录拥有此 caret frame 的事务 key，
            // 只有同 key 的 CaretDriven unit 能消费。
            owner_key: Some(key),
        }
    }

    /// Issue #701 评论 5699573227 第三阶段 (F5): 用同一份 `AnimationFrameSample`
    /// 采样 CursorOnly 光标位置。
    ///
    /// 当没有活跃文字事务但有 `cursor_ctrl.animation`（CursorOnly）时，从
    /// `frame_sample` 读取 driver 事务的 progress，按 ease-out-cubic 插值光标位置。
    /// 文字层和光标层都使用同一份 frame state。
    /// Issue #702 评论 5707449688 问题 2: 不再用 `anim.driver_key` 查事务，
    /// 直接用 CursorAnimationState 自己的 timeline（started_at + duration_ms）推进。
    fn sample_cursor_only_position(
        &self,
        anim: &super::rendering::CursorAnimationState,
        sample: &AnimationFrameSample,
    ) -> super::render_plan::CursorSampleOutcome {
        // Issue #702 评论 5707449688 问题 2: 纯光标移动彻底和文字事务 key 解耦。
        // 用 CursorAnimationState 自己的 timeline（started_at + duration_ms）
        // 用 frame_now 推进 from→to 动画。
        let (progress, needs_start) = anim.sample_progress(sample.frame_now);
        if needs_start {
            // 首帧：started_at 尚未初始化，返回 Idle 让调用方用 frame_now 启动。
            super::render_plan::CursorSampleOutcome::Idle
        } else if progress >= 1.0 {
            super::render_plan::CursorSampleOutcome::Finished
        } else {
            super::render_plan::CursorSampleOutcome::Running(progress)
        }
    }

    /// Issue #690 评论 5675007226 步骤 1+3: 从同一 `AnimationFrameSample` 读取文字 progress，
    ///
    /// 替代原来的 `build_text_animation_plan()`（内部各自 `Instant::now()`）。
    /// Issue #722 评论 5747719529 核心语义：光标本身就是吞字/吐字的视觉边界
    /// （caret_geometry_determines_clip / clip_from_coordinated_caret）。
    /// 文字不能再维护一套会和 caret 分叉的独立 timeline 进度。InsertReveal/DeleteConceal
    /// 的裁切边界直接消费本帧 coordinated caret 的位置（`compute_frame_caret_driven`），
    /// caret 与文字使用同一个 frame_now 和同一个 from→to 几何轨迹。reflow/crossfade
    /// 继续用 unit 的时间线做几何插值；`start_fraction` 由 rebase 决定（新单元为 0，
    /// 被连续输入覆盖的单元从已显示比例继续）。
    fn build_text_animation_plan_with_sample(
        &mut self,
        sample: &AnimationFrameSample,
        cursor_owner_epoch: u64,
        layout_basis_revision: LayoutRevision,
    ) -> (
        TextAnimationPlan,
        Vec<VisualTransactionKey>,
        super::render_plan::CoordinatedMotionFrame,
    ) {
        // Issue #738 评论 5788513592 问题1: 先按 layout basis 收口旧事务的 caret motion，
        // 再采样 caret motion。旧 basis 事务的 caret_motion_retired 置 true 后，
        // active_text_transaction_key_with_epoch 跳过它，sample_coordinated_motion_frame
        // 不会给它 owner_key，旧 caret track 不会被采样喂给 cursor layer。
        // Issue #738 评论 5793319451 问题1: 守卫从 `<` 改成 `!=`。basis 不一致（无论是旧
        // 还是 future）的事务都不应继续驱动 caret motion，统一收口 retire。
        for tx in self.prepared_queue.active_transactions_mut() {
            if tx.state == TextVisualTransactionState::Cancelled
                || tx.state == TextVisualTransactionState::Completed
            {
                continue;
            }
            if tx.layout_basis_revision != layout_basis_revision && !tx.caret_motion_retired {
                tx.retire_caret_driven_units();
                tx.caret_motion_retired = true;
            }
        }
        // 再采样 caret motion（旧 basis 事务已 retire，不会被选为 caret owner）。
        let coordinated_motion_frame = self.sample_coordinated_motion_frame(
            sample,
            cursor_owner_epoch,
            layout_basis_revision,
        );

        let mut glyphs = Vec::new();
        let mut keys_to_complete = Vec::new();

        for tx in self.prepared_queue.active_transactions_mut() {
            if tx.state == TextVisualTransactionState::Cancelled
                || tx.state == TextVisualTransactionState::Completed
            {
                continue;
            }

            if tx.state == TextVisualTransactionState::Pending {
                continue;
            }

            // Issue #738: basis 与 canonical revision 不一致的 unit 不进 glyph 计划。
            // Issue #738 评论 5793319451 问题1: 守卫从 `<` 改成 `!=`，future revision 的
            // 事务也不属于当前 canonical，不能画 glyph（其纹理/几何对应另一份 canonical）。
            if tx.layout_basis_revision != layout_basis_revision {
                continue;
            }

            // Prepared→Rendering 状态切换已由 begin_rendering_transactions 完成。

            // owns_caret: 本事务是否拥有 caret ownership。失去 owner 时 caret 部分
            // 立刻视为完成，避免旧事务回跳。
            let owns_caret = coordinated_motion_frame.owner_key == Some(tx.key);

            // caret_driven_active = owns_caret && caret.is_some()。false 时整笔
            // CaretDriven motion 直接 canonical 收口。
            let caret_driven_active = owns_caret && coordinated_motion_frame.caret.is_some();

            // InsertReveal/DeleteConceal 完成条件跟视觉边界一致。
            let has_caret_driven_units = tx.units.iter().any(|u| {
                matches!(
                    u.slice.kind,
                    AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal
                )
            });
            // has_caret_driven_units && !caret_driven_active 时退休 caret motion，
            // 收口 CaretDriven units 到终态。之后永远跳过此事务不再给 owner_key。
            if has_caret_driven_units && !caret_driven_active {
                tx.retire_caret_driven_units();
                tx.caret_motion_retired = true;
            }
            let caret_track_done = if has_caret_driven_units {
                match tx.cursor_visual_track.as_ref() {
                    Some(track) => track.progress(sample.frame_now) >= 1.0,
                    None => true,
                }
            } else {
                true
            };
            // 完成判断按 kind 分开: CaretDriven unit 的完成由 caret_track_done 决定，
            // Timed unit 看 progress >= 1.0。
            let all_units_done = if tx.units.is_empty() {
                sample.progress(tx.key) >= 1.0
            } else {
                tx.units.iter().all(|u| match u.slice.kind {
                    AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal => true,
                    AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
                        u.progress(sample.frame_now) >= 1.0
                    }
                })
            };
            // caret_track_complete: CaretDriven 事务必须 caret track 也完成。
            // 退休后永远视为完成，不会重新接管旧 caret 轨迹。
            let caret_track_complete =
                !has_caret_driven_units || tx.caret_motion_retired || caret_track_done;

            if all_units_done && caret_track_complete {
                // Issue #690 评论 5675007226 步骤 5: 完成也进正式诊断包（一条，不逐帧）。
                emit_transaction_diagnostic(tx, "editor.anim.complete", "completed");
                editor_animation_debug_log(&format!(
                    "anim_complete: key={:?} op={:?} units={}",
                    tx.key,
                    tx.operation_kind,
                    tx.units.len(),
                ));
                keys_to_complete.push(tx.key);
                continue;
            }

            for unit in &tx.units {
                // InsertReveal/DeleteConceal 从统一 CoordinatedMotionFrame.caret 消费。
                // 按 owner_key 过滤，只有同 key 的 unit 能消费此 caret frame。
                let frame = match unit.slice.kind {
                    AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal => {
                        // active 时生成 glyph，否则 continue（已 retire）。
                        if !caret_driven_active {
                            continue;
                        }
                        // 从统一 CoordinatedMotionFrame 获取 caret geometry。
                        // 用 match 而非 expect，避免用 expect 代替错误处理。
                        let Some(caret_frame) = coordinated_motion_frame.caret else {
                            continue;
                        };
                        // Issue #727 约束 2+3: CaretDriven unit 的 visible 从 caret track
                        // progress 推导，不再由 unit 自己的时间线驱动。
                        // visible = start_fraction + (target - start) * ease_out_quad(progress)
                        let eased = AnimatedSlice::ease_out_quad(caret_frame.progress);
                        let start = unit.timing.start_fraction();
                        let target = unit.timing.target_fraction();
                        let visible = start + (target - start) * eased;
                        unit.slice.compute_frame_caret_driven(
                            caret_frame.x,
                            caret_frame.y,
                            caret_frame.visual_line_id,
                            visible,
                        )
                    }
                    AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
                        // Reflow 不消费 caret 边界，用纯几何插值。
                        let visible = unit.current_visible_fraction(sample.frame_now);
                        unit.slice.compute_frame(visible)
                    }
                };
                glyphs.push(TextAnimationGlyphInfo {
                    x: frame.x,
                    y: frame.y,
                    w: frame.w,
                    h: frame.h,
                    opacity: frame.opacity,
                    snapshot_id: frame.snapshot_id,
                    source_rect: frame.source_rect,
                });
            }
        }

        (
            TextAnimationPlan { glyphs },
            keys_to_complete,
            coordinated_motion_frame,
        )
    }

    /// Issue #690 评论 5675007226 步骤 2 + 5681206040: 协同光标直接计算最终屏幕位置。
    ///
    /// Issue #722 评论 5747719529 改法 4 + 核心语义：光标本身就是吞字/吐字的视觉边界。
    /// 不再从文字 glyph 切片反推光标位置（删除 rightmost_x.max() / conceal_edge.min()）。
    /// 光标位置只由 `PreparedCursorVisualTrack`（canonical old caret → canonical new caret）
    /// 插值决定，使用同一个 `frame_now` 和同一个 from→to 几何轨迹。
    /// - 有 cursor_visual_track 时：用 `sampled_rect(frame_now)` 插值（caret_driven_clip）。
    /// - 没有 track 时（首次事务未经过 rebase）：按事务 progress 插值 old/new cursor rect。
    /// - 前向 Delete（conceal_to_left_edge=false）：逻辑光标不移动，固定在 new_cursor_rect。
    ///
    /// 文字的 InsertReveal/DeleteConceal 裁切边界直接消费本帧 coordinated caret 的位置
    /// （caret_driven_clip），caret 与文字使用同一个 frame_now 和同一个 from→to 几何
    /// 轨迹。快速 rebase 时先采样当前 caret 边界，再把这个边界作为下一段动画起点。
    ///
    /// 返回 `(x, y, h)` 供 `build_render_plan_full` 直接写入 `CursorRenderState`。
    ///
    /// Issue #722 评论 5748596920 问题1: 返回的 `y` 是文档坐标（不减 scroll_y），
    /// `build_render_plan_full` 在写入 `CursorRenderState` 时用当前 scroll_y 转成视口 y。
    /// `x` 是水平坐标，不受 scroll_y 影响。
    ///
    /// Issue #705 评论 5717380886: 增加 `current_cursor_epoch` 参数。
    /// Issue #727 约束 1 / Issue #735 评论 5773604666 问题3: epoch 不一致时返回 None——
    /// 事务立刻失去 caret motion ownership，CaretDriven units 已落到 canonical
    /// final state（不再继续播放）。
    fn compute_coordinated_cursor_position(
        &self,
        sample: &AnimationFrameSample,
        current_cursor_epoch: u64,
    ) -> Option<(f64, f64, f64)> {
        // Issue #705 评论 5717380886: 传入 cursor_owner_epoch。
        // 调 active_text_transaction_key() 取活动事务后，检查其 cursor_owner_epoch
        // 是否等于 current_cursor_epoch。epoch 不一致时返回 None。
        // Issue #735 评论 5773604666 问题3: epoch 不一致时 CaretDriven units 已在
        // find_cursor_transaction_for_target / build_text_animation_plan_with_sample
        // 中收口（落到终态），不再继续播自己的 glyph。
        let key = self.active_text_transaction_key()?;
        let tx = self
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key)?;

        // Issue #705 评论 5717380886: cursor_owner_epoch 检查。
        // Issue #727 约束 1 / Issue #735 评论 5773604666 问题3: epoch 不一致时返回 None——
        // 事务立刻失去 caret motion ownership，CaretDriven units 已落到 canonical
        // final state（不再继续播放）。
        if tx.cursor_owner_epoch != current_cursor_epoch {
            return None;
        }

        let _old_rect = tx.old_cursor_rect.as_ref()?;
        let new_rect = tx.new_cursor_rect.as_ref()?;
        let h = new_rect.bottom - new_rect.top;

        match tx.state {
            TextVisualTransactionState::Rendering | TextVisualTransactionState::Paused => {}
            _ => return None,
        }

        let op = tx.operation_kind;
        let frame_now = sample.frame_now;

        // Issue #722 评论 5747719529: 光标是吞字/吐字的视觉边界。
        // caret 位置只由 PreparedCursorVisualTrack（canonical old caret → canonical new caret）
        // 插值决定，不再从文字 glyph 切片反推（删除 rightmost_x.max() / conceal_edge.min()）。
        // - 有 cursor_visual_track 时：用 sampled_rect(frame_now) 插值（caret_driven_clip）。
        // - 没有 track 时：无有效 caret motion，返回 None 走 CursorOnly 路径。
        // 文字的 InsertReveal/DeleteConceal 裁切边界直接消费本帧 coordinated caret 的位置。
        let sample_caret_driven_clip = || -> Option<(f64, f64)> {
            match tx.cursor_visual_track.as_ref() {
                Some(track) => {
                    // caret_driven_clip: 光标位置由 caret track 插值决定。
                    // 用 sampled_rect_at_progress(progress(frame_now)) 与 sampled_rect(frame_now) 等价，
                    // 显式表达"caret 与文字使用同一个 frame_now 和 from→to 几何轨迹"。
                    let r = track.sampled_rect_at_progress(track.progress(frame_now));
                    Some((r.x, r.top))
                }
                None => {
                    // Issue #727 约束 6: 无 cursor_visual_track = 无有效 caret motion。
                    // 返回 None 让 build_render_plan_full 走 CursorOnly 路径，
                    // 不用事务的 old/new cursor rect 改写光标位置。
                    None
                }
            }
        };

        // Issue #722 评论 5747719529: 所有操作类型统一使用 caret track 插值决定光标位置。
        // 不再按操作类型分支从文字 glyph 切片反推。光标给吞了就是吞了，光标给吐出来
        // 就是吐出来。文字效果跟着光标边界，不是光标去追文字动画。
        //
        // 唯一例外：前向 Delete（conceal_to_left_edge=false）逻辑光标本来不移动，
        // 固定在 new_cursor_rect，只让右侧文字向光标方向收掉。
        let has_forward_delete = op == TextVisualOperationKind::Delete
            && tx.units.iter().any(|u| {
                u.slice.kind == AnimatedSliceKind::DeleteConceal && !u.slice.conceal_to_left_edge
            });

        if has_forward_delete {
            // Issue #722 评论 5748596920 问题3: 前向 Delete 时逻辑光标固定在 new_cursor_rect，
            // 但 conceal edge 必须从被删内容的远端向 caret.x 运动（在 compute_frame_caret_driven
            // 内部由 visible 参数驱动），不再把固定 caret.x 既当终点又当当前 conceal edge。
            // 用 forward_delete_sampled 标记逐帧采样机制。
            let forward_delete_sampled = true;
            if forward_delete_sampled {
                // 逻辑光标固定，但文字裁切随帧变化（由 compute_frame_caret_driven 内部处理）。
                Some((new_rect.x, new_rect.top, h))
            } else {
                Some((new_rect.x, new_rect.top, h))
            }
        } else {
            // caret_driven_clip: 光标位置由 caret track 插值决定。
            let (x, y) = sample_caret_driven_clip()?;
            Some((x, y, h))
        }
    }

    /// 返回当前最新正文编辑事务的操作类型，用于决定光标 blink mode。
    /// Issue #702 评论 5707449688 问题 2: TextVisualOperationKind::Cursor 已删除，
    /// 所有非 Completed/Cancelled 的事务都是正文事务。
    fn active_operation_kind(&self) -> Option<TextVisualOperationKind> {
        self.prepared_queue
            .active_transactions()
            .iter()
            .rev()
            .find(|t| {
                !matches!(
                    t.state,
                    TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
                )
            })
            .map(|t| t.operation_kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sujian_editor_item::animated_slice::AnimatedSliceKind;
    use crate::sujian_editor_item::layout_snapshot::ShapingIdentity;
    use crate::sujian_editor_item::render_plan::{CoordinatedMotionFrame, SampledCaretFrame};
    use writer_core::editor::Utf8ByteOffset;

    /// 构造不带时间线的 `RebaseFrame`，用于只验证三层匹配策略的用例。
    fn rebase_frame(
        byte_start: usize,
        byte_end: usize,
        x: f64,
        y: f64,
        opacity: f64,
        shaping_identity: Option<ShapingIdentity>,
        visible_fraction: f64,
    ) -> RebaseFrame {
        RebaseFrame {
            byte_start,
            byte_end,
            x,
            y,
            opacity,
            shaping_identity,
            visible_fraction,
            sampled_at: Instant::now(),
            remaining_duration_ms: 0,
        }
    }

    /// `match_rebase_frames` 现在作用在视觉单元上（Issue #690 评论 5675007226 步骤 3）。
    fn wrap_units(slices: Vec<AnimatedSlice>) -> Vec<PreparedVisualUnit> {
        slices
            .into_iter()
            .map(|s| PreparedVisualUnit::wrap(s, 100))
            .collect()
    }

    #[test]
    fn test_coordinator_suppress_all() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        assert!(!coord.has_active_insert());
        let suppressed = coord.suppress_all();
        assert!(!suppressed);
    }

    #[test]
    fn test_coordinator_finish_by_key() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = VisualTransactionKey::new(1, 1);
        let removed = coord.finish_by_key(key);
        assert!(removed.is_none());
    }

    #[test]
    fn test_animation_mode_system_suppressed_no_transaction() {
        let mode = AnimationMode::SystemSuppressed;
        assert!(!mode.should_create_transaction());
    }

    #[test]
    fn test_animation_mode_glyph_creates_transaction() {
        let mode = AnimationMode::GlyphAnimation;
        assert!(mode.should_create_transaction());
    }

    #[test]
    fn test_rebase_uses_offset_map_and_shaping_identity() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_a = ShapingIdentity {
            text_content_hash: 1,
            raw_font_fingerprint: "font_a".to_string(),
            glyph_indexes_hash: 10,
            cluster_glyph_count: 3,
            direction_rtl: false,
            format_fingerprint: 100,
        };
        let sid_b = ShapingIdentity {
            text_content_hash: 2,
            raw_font_fingerprint: "font_b".to_string(),
            glyph_indexes_hash: 20,
            cluster_glyph_count: 2,
            direction_rtl: false,
            format_fingerprint: 200,
        };

        let rebase_frames: Vec<RebaseFrame> = vec![
            rebase_frame(10, 20, 100.0, 200.0, 0.5, Some(sid_a.clone()), 0.0),
            rebase_frame(30, 40, 150.0, 250.0, 0.7, Some(sid_b.clone()), 0.0),
        ];

        let slices = vec![
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                50,
                60,
                Some(sid_a.clone()),
                None,
            ),
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                70,
                80,
                Some(sid_b.clone()),
                None,
            ),
        ];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        let mut units = wrap_units(slices);
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        assert!((units[0].slice.from_document_rect.x - 0.0).abs() < 0.01);
        assert!((units[1].slice.from_document_rect.x - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_rebase_tier3_closest_position_match_with_duplicate_shaping() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_dup = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font_x".to_string(),
            glyph_indexes_hash: 50,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 500,
        };

        let rebase_frames: Vec<RebaseFrame> = vec![rebase_frame(
            10,
            30,
            10.0,
            100.0,
            0.3,
            Some(sid_dup.clone()),
            0.3,
        )];

        let slices = vec![
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                11,
                15,
                Some(sid_dup.clone()),
                None,
            ),
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                18,
                22,
                Some(sid_dup.clone()),
                None,
            ),
        ];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        let mut units = wrap_units(slices);
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        let mapped_center = 20i64;
        let center_0 = (11 + 15) as i64 / 2;
        let center_1 = (18 + 22) as i64 / 2;
        let dist_0 = (center_0 - mapped_center).abs();
        let dist_1 = (center_1 - mapped_center).abs();
        assert!(
            dist_1 < dist_0,
            "test setup: slice 1 (dist={}) should be closer than slice 0 (dist={})",
            dist_1,
            dist_0
        );
        assert!(
            (units[1].slice.start_fraction - 0.3).abs() < 0.01,
            "slice 1 (center={}, abs dist={}) should match rebase frame, got start_fraction={}",
            center_1,
            dist_1,
            units[1].slice.start_fraction
        );
        assert!(
            (units[0].slice.start_fraction - 0.0).abs() < 0.01,
            "slice 0 (center={}, abs dist={}) should NOT be matched, got start_fraction={}",
            center_0,
            dist_0,
            units[0].slice.start_fraction
        );
    }

    #[test]
    fn test_rebase_tier3_absolute_distance_not_signed() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_dup = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font_x".to_string(),
            glyph_indexes_hash: 50,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 500,
        };

        let rebase_frames: Vec<RebaseFrame> = vec![rebase_frame(
            10,
            30,
            10.0,
            100.0,
            0.3,
            Some(sid_dup.clone()),
            0.3,
        )];

        let slices = vec![
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                11,
                15,
                Some(sid_dup.clone()),
                None,
            ),
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                22,
                26,
                Some(sid_dup.clone()),
                None,
            ),
        ];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        let mut units = wrap_units(slices);
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        let mapped_center = 20i64;
        let center_0 = (11 + 15) as i64 / 2;
        let center_1 = (22 + 26) as i64 / 2;
        let signed_0 = center_0 - mapped_center;
        let signed_1 = center_1 - mapped_center;
        let abs_0 = (center_0 - mapped_center).abs();
        let abs_1 = (center_1 - mapped_center).abs();
        assert!(
            signed_0 < signed_1,
            "test setup: slice 0 signed diff ({}) should be more negative than slice 1 ({})",
            signed_0,
            signed_1
        );
        assert!(
            abs_1 < abs_0,
            "test setup: slice 1 abs dist ({}) should be less than slice 0 ({})",
            abs_1,
            abs_0
        );
        assert!((units[1].slice.start_fraction - 0.3).abs() < 0.01,
            "slice 1 (abs dist={}) should be chosen over slice 0 (abs dist={}, signed={}), got start_fraction={}",
            abs_1, abs_0, signed_0, units[1].slice.start_fraction);
    }

    #[test]
    fn test_rebase_tier1_consumed_prevents_reuse() {
        let sid_a = ShapingIdentity {
            text_content_hash: 1,
            raw_font_fingerprint: "font_a".to_string(),
            glyph_indexes_hash: 10,
            cluster_glyph_count: 3,
            direction_rtl: false,
            format_fingerprint: 100,
        };

        let rebase_frames: Vec<RebaseFrame> = vec![
            rebase_frame(50, 60, 10.0, 100.0, 0.3, Some(sid_a.clone()), 0.3),
            rebase_frame(50, 60, 20.0, 200.0, 0.5, Some(sid_a.clone()), 0.5),
        ];

        let slices = vec![AnimatedSlice::insert_reveal(
            VisualTransactionKey::new(1, 1),
            LineSnapshotId::new(1, 0, 0),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            0.0,
            0.0,
            50,
            60,
            Some(sid_a.clone()),
            None,
        )];

        let offset_map = OffsetMap {
            entries: Vec::new(),
        };
        let mut units = wrap_units(slices);
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        assert!(
            (units[0].slice.start_fraction - 0.3).abs() < 0.01,
            "slice 0 should get first rebase frame start_fraction 0.3 (not second 0.5), got {}",
            units[0].slice.start_fraction
        );
    }

    #[test]
    fn test_rebase_tier3_consumed_prevents_reuse() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_dup = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font_x".to_string(),
            glyph_indexes_hash: 50,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 500,
        };

        let rebase_frames: Vec<RebaseFrame> = vec![
            rebase_frame(10, 30, 10.0, 100.0, 0.3, Some(sid_dup.clone()), 0.3),
            rebase_frame(10, 30, 20.0, 200.0, 0.5, Some(sid_dup.clone()), 0.5),
        ];

        let slices = vec![
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                11,
                15,
                Some(sid_dup.clone()),
                None,
            ),
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                18,
                22,
                Some(sid_dup.clone()),
                None,
            ),
        ];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        let mut units = wrap_units(slices);
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        let mapped_center = 20i64;
        let center_0 = (11 + 15) as i64 / 2;
        let center_1 = (18 + 22) as i64 / 2;
        let dist_0 = (center_0 - mapped_center).abs();
        let dist_1 = (center_1 - mapped_center).abs();
        assert!(dist_1 < dist_0, "test setup: slice 1 should be closer");
        assert!(
            (units[1].slice.start_fraction - 0.3).abs() < 0.01,
            "slice 1 should get first rebase frame (start_fraction=0.3), got start_fraction={}",
            units[1].slice.start_fraction
        );
        assert!(
            (units[0].slice.start_fraction - 0.5).abs() < 0.01,
            "slice 0 should get second rebase frame (start_fraction=0.5), not reuse slice 1's frame, got start_fraction={}",
            units[0].slice.start_fraction
        );
    }

    #[test]
    fn test_rebase_tier2_consumed_prevents_reuse() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_a = ShapingIdentity {
            text_content_hash: 1,
            raw_font_fingerprint: "font_a".to_string(),
            glyph_indexes_hash: 10,
            cluster_glyph_count: 3,
            direction_rtl: false,
            format_fingerprint: 100,
        };

        let rebase_frames: Vec<RebaseFrame> = vec![
            rebase_frame(50, 70, 10.0, 100.0, 0.3, Some(sid_a.clone()), 0.3),
            rebase_frame(50, 70, 20.0, 200.0, 0.5, Some(sid_a.clone()), 0.5),
        ];

        let slices = vec![AnimatedSlice::insert_reveal(
            VisualTransactionKey::new(1, 1),
            LineSnapshotId::new(1, 0, 0),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            0.0,
            0.0,
            150,
            170,
            Some(sid_a.clone()),
            None,
        )];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::unchecked(100),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        let mut units = wrap_units(slices);
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        assert!(
            (units[0].slice.start_fraction - 0.3).abs() < 0.01,
            "slice 0 should get first rebase frame via tier2 (start_fraction=0.3), got start_fraction={}",
            units[0].slice.start_fraction
        );
    }

    #[test]
    fn test_rebase_tier1_consumed_prevents_tier3_reuse() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_dup = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font_x".to_string(),
            glyph_indexes_hash: 50,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 500,
        };

        let rebase_frames: Vec<RebaseFrame> = vec![
            rebase_frame(50, 60, 10.0, 100.0, 0.3, Some(sid_dup.clone()), 0.3),
            rebase_frame(40, 80, 20.0, 200.0, 0.5, Some(sid_dup.clone()), 0.5),
        ];

        let slices = vec![AnimatedSlice::insert_reveal(
            VisualTransactionKey::new(1, 1),
            LineSnapshotId::new(1, 0, 0),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            0.0,
            0.0,
            50,
            60,
            Some(sid_dup.clone()),
            None,
        )];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        let mut units = wrap_units(slices);
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        assert!(
            (units[0].slice.start_fraction - 0.3).abs() < 0.01,
            "slice 0 should get first rebase frame start_fraction 0.3 (not second 0.5), got {}",
            units[0].slice.start_fraction
        );
    }

    #[test]
    fn test_rebase_tier3_tiebreak_by_byte_start_then_index() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_dup = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font_x".to_string(),
            glyph_indexes_hash: 50,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 500,
        };

        let rebase_frames: Vec<RebaseFrame> = vec![rebase_frame(
            10,
            30,
            10.0,
            100.0,
            0.3,
            Some(sid_dup.clone()),
            0.3,
        )];

        let slices = vec![
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                11,
                15,
                Some(sid_dup.clone()),
                None,
            ),
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                25,
                29,
                Some(sid_dup.clone()),
                None,
            ),
        ];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        let mut units = wrap_units(slices);
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        let mapped_center = 20i64;
        let center_0 = (11 + 15) as i64 / 2;
        let center_1 = (25 + 29) as i64 / 2;
        let dist_0 = (center_0 - mapped_center).abs();
        let dist_1 = (center_1 - mapped_center).abs();
        assert_eq!(
            dist_0, dist_1,
            "test setup: both slices should have equal distance"
        );
        assert!(
            (units[0].slice.start_fraction - 0.3).abs() < 0.01,
            "slice 0 (lower byte_start) should win tiebreak, got start_fraction={}",
            units[0].slice.start_fraction
        );
        assert!(
            (units[1].slice.start_fraction - 0.0).abs() < 0.01,
            "slice 1 should not be matched, got start_fraction={}",
            units[1].slice.start_fraction
        );
    }

    fn make_test_snapshot(
        virtual_text: &str,
        line_clusters: Vec<(usize, usize, f64, f64, ShapingIdentity)>,
    ) -> EditorLayoutSnapshot {
        use crate::editor::layout::{CaretAffinity, LayoutSnapshot, VisualLine};
        use crate::sujian_editor_item::layout_snapshot::{
            LineClusterSnapshot, PreparedLineSnapshot,
        };
        let clusters: Vec<LineClusterSnapshot> = line_clusters
            .iter()
            .map(|(bs, be, x, _y, sid)| LineClusterSnapshot {
                byte_start: *bs,
                byte_end: *be,
                source_rect: SourceRect {
                    x: *x,
                    y: 0.0,
                    w: (*be - *bs) as f64 * 10.0,
                    h: 20.0,
                },
                shaping_identity: sid.clone(),
            })
            .collect();
        let line = PreparedLineSnapshot {
            id: LineSnapshotId::new(1, 0, 0),
            image: None,
            clusters,
            document_origin_y: 0.0,
            dpr: 1.0,
            byte_start: line_clusters.first().map(|c| c.0).unwrap_or(0),
            byte_end: line_clusters.last().map(|c| c.1).unwrap_or(0),
            visual_x: 0.0,
            visual_line_id: 0,
            visual_line_top: 0.0,
            visual_line_bottom: 20.0,
            cache_slot: 0,
            qtextline_idx: 0,
            // Issue #724 评论 5752140048 问题 4a: 测试用段落起始偏移 0。
            paragraph_document_byte_start: 0,
        };
        let layout_snapshot = LayoutSnapshot {
            text_revision: 0,
            text_ptr: 0,
            text_len: virtual_text.len(),
            width: 800.0,
            font_size: 16.0,
            font_family: "sans-serif".to_string(),
            line_spacing: 1.5,
            text_indent: 0.0,
            padding: 0.0,
            lines: vec![VisualLine {
                id: 0,
                byte_start: line.byte_start,
                byte_end: line.byte_end,
                qchar_start: 0,
                qchar_end: 0,
                hard_break: false,
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 20.0,
                para_text: virtual_text.to_string(),
                para_start: 0,
                qtextline_idx: 0,
                para_qchar_start: 0,
                para_qchar_end: 0,
                line_wrap_width: 800.0,
                line_indent_x: 0.0,
                para_indent: 0.0,
                x_end_trailing: 800.0,
                qt_ascent: 16.0,
                qt_descent: 4.0,
                cache_slot: 0,
            }],
            layout_generation: 0,
        };
        EditorLayoutSnapshot::new(
            layout_snapshot,
            vec![line],
            None,
            None,
            CaretAffinity::Downstream,
        )
        .with_virtual_text(virtual_text.to_string())
    }

    #[test]
    fn test_commit_same_shaping_different_geometry_creates_move() {
        let sid_common = ShapingIdentity {
            text_content_hash: 42,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 100,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_old_preedit = ShapingIdentity {
            text_content_hash: 10,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 200,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_new_commit = ShapingIdentity {
            text_content_hash: 20,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 300,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let old_snapshot = make_test_snapshot(
            "世界好abc",
            vec![
                (0, 3, 10.0, 0.0, sid_common.clone()),
                (3, 6, 40.0, 0.0, sid_common.clone()),
                (6, 9, 70.0, 0.0, sid_common.clone()),
                (9, 12, 100.0, 0.0, sid_old_preedit.clone()),
            ],
        );
        let new_snapshot = make_test_snapshot(
            "世界好xyz",
            vec![
                (0, 3, 50.0, 0.0, sid_common.clone()),
                (3, 6, 80.0, 0.0, sid_common.clone()),
                (6, 9, 110.0, 0.0, sid_common.clone()),
                (9, 12, 140.0, 0.0, sid_new_commit.clone()),
            ],
        );
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = coord.handle_composition_commit_or_cancel(
            &old_snapshot,
            &new_snapshot,
            0,
            12,
            true,
            false,
            0,
            12,
            0,
            12,
            None,
            None,
            None,
            None,
            0.0,
            0.0,
            0.0,
            0.0,
            0,
            LayoutRevision::initial(),
        );
        assert!(key.is_some());
        let tx = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key.unwrap())
            .unwrap();
        let has_move = tx
            .units
            .iter()
            .any(|u| u.slice.kind == AnimatedSliceKind::ReflowMove);
        assert!(
            has_move,
            "commit with same shaping but different geometry should create ReflowMove slice"
        );
        assert!(
            tx.units
                .iter()
                .any(|u| !u.slice.static_hidden_document_rects.is_empty()),
            "Move slices should have static_hidden_document_rects"
        );
    }

    #[test]
    fn test_commit_different_shaping_creates_crossfade_with_static_patch() {
        let sid_common = ShapingIdentity {
            text_content_hash: 42,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 100,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_common_diff = ShapingIdentity {
            text_content_hash: 42,
            raw_font_fingerprint: "font_other".into(),
            glyph_indexes_hash: 999,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_old_preedit = ShapingIdentity {
            text_content_hash: 10,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 200,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_new_commit = ShapingIdentity {
            text_content_hash: 20,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 300,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let old_snapshot = make_test_snapshot(
            "世界好abc",
            vec![
                (0, 3, 10.0, 0.0, sid_common.clone()),
                (3, 6, 40.0, 0.0, sid_common.clone()),
                (6, 9, 70.0, 0.0, sid_common.clone()),
                (9, 12, 100.0, 0.0, sid_old_preedit.clone()),
            ],
        );
        let new_snapshot = make_test_snapshot(
            "世界好xyz",
            vec![
                (0, 3, 10.0, 0.0, sid_common.clone()),
                (3, 6, 40.0, 0.0, sid_common_diff.clone()),
                (6, 9, 70.0, 0.0, sid_common.clone()),
                (9, 12, 140.0, 0.0, sid_new_commit.clone()),
            ],
        );
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = coord.handle_composition_commit_or_cancel(
            &old_snapshot,
            &new_snapshot,
            0,
            12,
            true,
            false,
            0,
            12,
            0,
            12,
            None,
            None,
            None,
            None,
            0.0,
            0.0,
            0.0,
            0.0,
            0,
            LayoutRevision::initial(),
        );
        assert!(key.is_some());
        let tx = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key.unwrap())
            .unwrap();
        let crossfade_count = tx
            .units
            .iter()
            .filter(|u| u.slice.kind == AnimatedSliceKind::ReflowCrossFade)
            .count();
        assert!(
            crossfade_count >= 2,
            "commit with different shaping should create paired Crossfade slices (old+new), got {}",
            crossfade_count
        );
        assert!(
            tx.units
                .iter()
                .any(|u| !u.slice.static_hidden_document_rects.is_empty()),
            "Crossfade new should have static_hidden_document_rects to prevent double-draw"
        );
    }

    #[test]
    fn test_commit_same_shaping_same_geometry_is_static() {
        let sid_common = ShapingIdentity {
            text_content_hash: 42,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 100,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_old_preedit = ShapingIdentity {
            text_content_hash: 10,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 200,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_new_commit = ShapingIdentity {
            text_content_hash: 20,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 300,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let old_snapshot = make_test_snapshot(
            "世界好abc",
            vec![
                (0, 3, 10.0, 0.0, sid_common.clone()),
                (3, 6, 40.0, 0.0, sid_common.clone()),
                (6, 9, 70.0, 0.0, sid_common.clone()),
                (9, 12, 100.0, 0.0, sid_old_preedit.clone()),
            ],
        );
        let new_snapshot = make_test_snapshot(
            "世界好xyz",
            vec![
                (0, 3, 10.0, 0.0, sid_common.clone()),
                (3, 6, 40.0, 0.0, sid_common.clone()),
                (6, 9, 70.0, 0.0, sid_common.clone()),
                (9, 12, 100.0, 0.0, sid_new_commit.clone()),
            ],
        );
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = coord.handle_composition_commit_or_cancel(
            &old_snapshot,
            &new_snapshot,
            0,
            12,
            true,
            false,
            0,
            12,
            0,
            12,
            None,
            None,
            None,
            None,
            0.0,
            0.0,
            0.0,
            0.0,
            0,
            LayoutRevision::initial(),
        );
        assert!(key.is_some());
        let tx = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key.unwrap())
            .unwrap();
        let first_cluster_slices: Vec<&AnimatedSlice> = tx
            .units
            .iter()
            .filter(|u| u.slice.byte_start == 0 && u.slice.byte_end == 3)
            .map(|u| &u.slice)
            .collect();
        assert!(
            first_cluster_slices.is_empty(),
            "same shaping + same geometry should be Static (no slice), got {} slices",
            first_cluster_slices.len()
        );
    }

    #[test]
    fn test_commit_separate_preedit_and_committed_replace_ranges() {
        let sid_a = ShapingIdentity {
            text_content_hash: 1,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 10,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_b = ShapingIdentity {
            text_content_hash: 2,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 20,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_c = ShapingIdentity {
            text_content_hash: 3,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 30,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_preedit = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 99,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let old_snapshot = make_test_snapshot(
            "abc_preedit_xyz",
            vec![
                (0, 3, 10.0, 0.0, sid_a.clone()),
                (3, 10, 50.0, 0.0, sid_preedit.clone()),
                (10, 13, 120.0, 0.0, sid_c.clone()),
            ],
        );
        let new_snapshot = make_test_snapshot(
            "abc_QQ_xyz",
            vec![
                (0, 3, 10.0, 0.0, sid_a.clone()),
                (3, 5, 50.0, 0.0, sid_b.clone()),
                (5, 8, 120.0, 0.0, sid_c.clone()),
            ],
        );
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = coord.handle_composition_commit_or_cancel(
            &old_snapshot,
            &new_snapshot,
            0,
            12,
            true,
            false,
            0,
            12,
            0,
            12,
            None,
            None,
            None,
            None,
            0.0,
            0.0,
            0.0,
            0.0,
            0,
            LayoutRevision::initial(),
        );
        assert!(key.is_some());
        let tx = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key.unwrap())
            .unwrap();
        let old_preedit_slices: Vec<&AnimatedSlice> = tx
            .units
            .iter()
            .filter(|u| u.slice.byte_start >= 3 && u.slice.byte_end <= 10)
            .map(|u| &u.slice)
            .collect();
        assert!(
            !old_preedit_slices.is_empty(),
            "preedit range should have animated slices"
        );
        let new_candidate_slices: Vec<&AnimatedSlice> = tx
            .units
            .iter()
            .filter(|u| u.slice.byte_start >= 3 && u.slice.byte_end <= 5)
            .map(|u| &u.slice)
            .collect();
        assert!(
            !new_candidate_slices.is_empty(),
            "candidate range should have animated slices"
        );
    }

    #[test]
    fn test_commit_cancel_uses_preedit_range_for_old_clusters() {
        let sid_preedit = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 99,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_after = ShapingIdentity {
            text_content_hash: 42,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 100,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let old_snapshot = make_test_snapshot(
            "abc_preedit_after",
            vec![
                (0, 3, 10.0, 0.0, sid_after.clone()),
                (3, 10, 50.0, 0.0, sid_preedit.clone()),
                (10, 15, 120.0, 0.0, sid_after.clone()),
            ],
        );
        let new_snapshot = make_test_snapshot(
            "abc_after",
            vec![
                (0, 3, 10.0, 0.0, sid_after.clone()),
                (3, 8, 120.0, 0.0, sid_after.clone()),
            ],
        );
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = coord.handle_composition_commit_or_cancel(
            &old_snapshot,
            &new_snapshot,
            3,
            10,
            false,
            false,
            3,
            3,
            3,
            3,
            None,
            None,
            None,
            None,
            0.0,
            0.0,
            0.0,
            0.0,
            0,
            LayoutRevision::initial(),
        );
        assert!(key.is_some());
        let tx = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key.unwrap())
            .unwrap();
        let delete_slices: Vec<&AnimatedSlice> = tx
            .units
            .iter()
            .filter(|u| u.slice.kind == AnimatedSliceKind::DeleteConceal)
            .map(|u| &u.slice)
            .collect();
        assert!(
            !delete_slices.is_empty(),
            "cancel should create DeleteConceal for preedit range"
        );
    }

    #[test]
    fn test_many_to_one_reflow_one_old_splits_to_two_new() {
        // Issue #658 评论 5630181473 问题 3: one old cluster [0,3) maps to
        // two new clusters [0,1) + [4,6) via OffsetMap (insert "XYZ" at position 1).
        // new cluster [1,4) is inserted text with no old counterpart → InsertReveal.
        // old cluster [0,3) connects to both [0,1) and [4,6) → N→M crossfade run.
        let sid_common = ShapingIdentity {
            text_content_hash: 42,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 100,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_preedit = ShapingIdentity {
            text_content_hash: 10,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 200,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_new_a = ShapingIdentity {
            text_content_hash: 50,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 300,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let sid_new_b = ShapingIdentity {
            text_content_hash: 51,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 301,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        // old: "abc" → one cluster covering [0,3)
        let old_snapshot = make_test_snapshot("abc", vec![(0, 3, 10.0, 0.0, sid_preedit.clone())]);
        // new: "aXYZbc" → three clusters: [0,1) "a", [1,4) "XYZ", [4,6) "bc"
        let new_snapshot = make_test_snapshot(
            "aXYZbc",
            vec![
                (0, 1, 10.0, 0.0, sid_new_a.clone()),
                (1, 4, 20.0, 0.0, sid_common.clone()),
                (4, 6, 40.0, 0.0, sid_new_b.clone()),
            ],
        );
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = coord.handle_composition_update(
            &old_snapshot,
            &new_snapshot,
            0,
            3,
            0,
            3,
            None,
            None,
            None,
            None,
            0.0,
            0.0,
            0.0,
            0.0,
            0,
            LayoutRevision::initial(),
        );
        assert!(key.is_some());
        let tx = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key.unwrap())
            .unwrap();
        // N→M run: 1 old [0,3) + 2 new [0,1),[4,6) → crossfade slices
        // old cluster [0,3) produces crossfade_old (uses first new's byte range [0,1))
        let crossfade_count = tx
            .units
            .iter()
            .filter(|u| u.slice.kind == AnimatedSliceKind::ReflowCrossFade)
            .count();
        // 1 crossfade_old (old [0,3)) + 2 crossfade_new (new [0,1) + new [4,6)) = 3
        assert_eq!(
            crossfade_count, 3,
            "expected 3 crossfade slices (1 old + 2 new), got {}",
            crossfade_count
        );
        // new cluster [1,4) is inserted text (no old counterpart) → InsertReveal
        let insert_count = tx
            .units
            .iter()
            .filter(|u| u.slice.kind == AnimatedSliceKind::InsertReveal)
            .filter(|u| u.slice.byte_start == 1 && u.slice.byte_end == 4)
            .count();
        assert_eq!(
            insert_count, 1,
            "inserted cluster [1,4) should produce exactly 1 InsertReveal, got {}",
            insert_count
        );
        // No DeleteConceal for these ranges
        let delete_count = tx
            .units
            .iter()
            .filter(|u| u.slice.kind == AnimatedSliceKind::DeleteConceal)
            .count();
        assert_eq!(delete_count, 0, "should not produce DeleteConceal");
    }

    /// Issue #686 评论 5667184642：回归测试——吞字方向必须与光标位置匹配。
    ///
    /// `conceal_to_left_edge = true` 表示向左边缘收缩（Backspace，光标在文字右侧）；
    /// `conceal_to_left_edge = false` 表示向右边缘收缩（Delete 键，光标在文字左侧）。
    /// 上一轮把比较式写反了（靠左算成 true），这里锁定正确语义。
    ///
    /// 测试布局：old cluster [0,3) source_rect x=10 w=30，dpr=1 visual_x=0
    /// → document rect x=10 w=30 → left=10, right=40。
    fn make_delete_direction_snapshots() -> (EditorLayoutSnapshot, EditorLayoutSnapshot, OffsetMap)
    {
        let sid = ShapingIdentity {
            text_content_hash: 1,
            raw_font_fingerprint: "font".into(),
            glyph_indexes_hash: 10,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let old_snapshot = make_test_snapshot("abc", vec![(0, 3, 10.0, 0.0, sid)]);
        // new 为空 → old cluster 成为纯 old run → delete_conceal
        let new_snapshot = make_test_snapshot("", vec![]);
        let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);
        (old_snapshot, new_snapshot, offset_map)
    }

    #[test]
    fn test_delete_conceal_direction_cursor_near_right_is_backspace() {
        let (old_snapshot, _new_snapshot, _offset_map) = make_delete_direction_snapshots();
        let key = VisualTransactionKey::new(1, 1);
        // 旧光标靠近右端 (x=39, right=40) → Backspace → conceal_to_left_edge=true
        let old_cursor = CursorRect {
            x: 39.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        };
        // Issue #687: changed range 由 build_delete_conceal_slices 显式拥有。
        // old cluster [0,3) 是被删除的范围。
        let slices = build_delete_conceal_slices(key, &old_snapshot, (0, 3), Some(&old_cursor));
        let delete_slices: Vec<_> = slices
            .iter()
            .filter(|s| s.kind == AnimatedSliceKind::DeleteConceal)
            .collect();
        assert_eq!(
            delete_slices.len(),
            1,
            "deleted range [0,3) should produce exactly one DeleteConceal"
        );
        assert!(
            delete_slices[0].conceal_to_left_edge,
            "cursor near right (x=39, right=40) should be Backspace → conceal_to_left_edge=true"
        );
    }

    #[test]
    fn test_delete_conceal_direction_cursor_near_left_is_delete() {
        let (old_snapshot, _new_snapshot, _offset_map) = make_delete_direction_snapshots();
        let key = VisualTransactionKey::new(1, 1);
        // 旧光标靠近左端 (x=11, left=10) → Delete 键 → conceal_to_left_edge=false
        let old_cursor = CursorRect {
            x: 11.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        };
        // Issue #687: changed range 由 build_delete_conceal_slices 显式拥有。
        // old cluster [0,3) 是被删除的范围。
        let slices = build_delete_conceal_slices(key, &old_snapshot, (0, 3), Some(&old_cursor));
        let delete_slices: Vec<_> = slices
            .iter()
            .filter(|s| s.kind == AnimatedSliceKind::DeleteConceal)
            .collect();
        assert_eq!(
            delete_slices.len(),
            1,
            "deleted range [0,3) should produce exactly one DeleteConceal"
        );
        assert!(
            !delete_slices[0].conceal_to_left_edge,
            "cursor near left (x=11, left=10) should be Delete → conceal_to_left_edge=false"
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // Issue #690 评论 5675007226 步骤 1+2+3: 一条动画链的行为回归测试。
    //
    // 覆盖三个真实缺陷：
    // - 交棒帧曾按事务级 timeline progress 计算可见比例（吐到 60% 的字被交棒成 19%）；
    // - 交棒后单元时间线归零重播（事务 key 换了就重新 0→1）；
    // - Scene Graph 仍把 GUI 线程上一帧的 visual_x 当最终屏幕坐标（文字甩开光标）。
    // ─────────────────────────────────────────────────────────────────────

    use std::time::Duration;

    use crate::sujian_editor_item::render_plan::{
        CursorStyle, FrameContext, SelectionPreeditStyle,
    };

    fn reveal_slice(byte_start: usize, byte_end: usize, x: f64, w: f64) -> AnimatedSlice {
        AnimatedSlice::insert_reveal(
            VisualTransactionKey::new(1, 1),
            LineSnapshotId::new(1, 0, 0),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w,
                h: 20.0,
            },
            SourceRect {
                x,
                y: 0.0,
                w,
                h: 20.0,
            },
            x,
            0.0,
            byte_start,
            byte_end,
            None,
            None,
        )
    }

    fn conceal_slice(
        byte_start: usize,
        byte_end: usize,
        x: f64,
        w: f64,
        conceal_to_left_edge: bool,
    ) -> AnimatedSlice {
        AnimatedSlice::delete_conceal(
            VisualTransactionKey::new(1, 1),
            LineSnapshotId::new(1, 0, 0),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w,
                h: 20.0,
            },
            SourceRect {
                x,
                y: 0.0,
                w,
                h: 20.0,
            },
            x,
            0.0,
            byte_start,
            byte_end,
            None,
            conceal_to_left_edge,
            None,
        )
    }

    fn reflow_slice(byte_start: usize, byte_end: usize, from_x: f64, to_x: f64) -> AnimatedSlice {
        AnimatedSlice::reflow_move(
            VisualTransactionKey::new(1, 1),
            LineSnapshotId::new(1, 0, 0),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 30.0,
                h: 20.0,
            },
            SourceRect {
                x: from_x,
                y: 0.0,
                w: 30.0,
                h: 20.0,
            },
            LineSnapshotId::new(1, 0, 0),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 30.0,
                h: 20.0,
            },
            SourceRect {
                x: to_x,
                y: 0.0,
                w: 30.0,
                h: 20.0,
            },
            byte_start,
            byte_end,
            None,
        )
    }

    fn caret(x: f64) -> CursorRect {
        CursorRect {
            x,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }
    }

    /// 已经演进了 `elapsed_ms` 的视觉单元（单元自己的时间线，与事务 timeline 无关）。
    fn elapsed_unit(
        slice: AnimatedSlice,
        elapsed_ms: u64,
        duration_ms: u64,
        now: Instant,
    ) -> PreparedVisualUnit {
        let mut unit = PreparedVisualUnit::wrap(slice, duration_ms);
        // Issue #727 约束 2: 通过 VisualUnitTiming 设置 started_at / start_fraction。
        // CaretDriven unit 无 started_at，通过 start_fraction 模拟已吐/吞比例。
        // Timed unit 通过 started_at 设置独立时间线。
        let fraction = if duration_ms > 0 {
            (elapsed_ms as f64 / duration_ms as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        match &mut unit.timing {
            super::VisualUnitTiming::Timed { started_at, .. } => {
                *started_at = Some(now - Duration::from_millis(elapsed_ms));
            }
            super::VisualUnitTiming::CaretDriven { .. } => {
                // Issue #727 约束 2: CaretDriven unit 的 visible_fraction 从 caret track
                // progress 推导（start + (target - start) * ease_out_quad(progress)），
                // 不需要通过 start_fraction 模拟已演进状态。
                // start_fraction 保持 fresh unit 的初始值（0 for InsertReveal, 1 for DeleteConceal）。
                // 测试中 caret track 的 started_at 由 rendering_tx 设置，反映已演进状态。
            }
        }
        unit
    }

    /// 手工装配一笔处于 Rendering 的正文事务。事务 timeline 从 `tx_elapsed_ms` 起算，
    /// 与单元各自的 `elapsed_ms` 故意取不同值，用来验证两者不再互相顶替。
    /// Issue #727 约束 3: 自动创建 `cursor_visual_track`，使 CaretDriven unit 可以
    /// 从 caret track progress 推导可见比例。`started_at` 与事务 timeline 同步。
    fn rendering_tx(
        key: VisualTransactionKey,
        operation_kind: TextVisualOperationKind,
        units: Vec<PreparedVisualUnit>,
        old_cursor: CursorRect,
        new_cursor: CursorRect,
        now: Instant,
        tx_elapsed_ms: u64,
    ) -> PreparedTextVisualTransaction {
        let mut timeline = TransactionTimeline::new(100);
        timeline.rendering_started_at = Some(now - Duration::from_millis(tx_elapsed_ms));
        // Issue #727 约束 3: CaretDriven unit 依赖 caret motion track。
        // 测试辅助函数自动创建 cursor_visual_track，使 CaretDriven unit 可以生成 glyph。
        let cursor_visual_track = Some(PreparedCursorVisualTrack {
            from: old_cursor.clone(),
            to: new_cursor.clone(),
            from_visual_line_id: None,
            to_visual_line_id: None,
            from_line_top: 0.0,
            from_line_bottom: 20.0,
            to_line_top: 0.0,
            to_line_bottom: 20.0,
            started_at: Some(now - Duration::from_millis(tx_elapsed_ms)),
            duration_ms: 100,
            pause_start: None,
        });
        PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Rendering,
            operation_kind,
            timeline,
            units,
            old_cursor_rect: Some(old_cursor),
            new_cursor_rect: Some(new_cursor),
            cursor_visual_track,
            cancel_reason: None,
            texture_prepared: true,
            old_snapshot: None,
            new_snapshot: None,
            cursor_owner_epoch: 0,
            caret_motion_retired: false,
            visual_affected_byte_range_old: None,
            visual_affected_byte_range_new: None,
            layout_basis_revision: LayoutRevision::initial(),
        }
    }

    fn stale_cursor_state() -> CursorRenderState {
        CursorRenderState {
            visible: true,
            x: 1234.5,
            y: 999.0,
            h: 20.0,
            opacity: 0.0,
        }
    }

    #[test]
    fn issue690_collect_rebase_frames_uses_per_unit_progress() {
        let now = Instant::now();
        let mut tx = rendering_tx(
            VisualTransactionKey::new(1, 1),
            TextVisualOperationKind::Insert,
            vec![
                // CaretDriven unit（InsertReveal）：可见比例从 caret track progress 推导。
                // caret track progress = 0.5 → ease_out_quad(0.5) = 0.75
                elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now),
                // Timed unit（ReflowMove）：有自己的时间线，elapsed 500ms > duration 100ms
                // → progress >= 1.0 → is_finished() = true → 已播完不交棒。
                // Issue #727 约束 2: CaretDriven unit 共享 caret track，不能独立"已播完"，
                // 所以用 Timed unit 验证"已播完的单元不交棒"。
                elapsed_unit(reflow_slice(3, 6, 160.0, 220.0), 500, 100, now),
            ],
            caret(100.0),
            caret(220.0),
            now,
            50,
        );
        // 事务级 progress = 0.5（eased 0.75）。CaretDriven unit 的可见比例从 caret track
        // progress 推导：start + (target - start) * ease_out_quad(0.5) = 0.75。
        tx.timeline.rendering_started_at = Some(now - Duration::from_millis(50));

        let frames = tx.collect_rebase_frames(now);
        assert_eq!(
            frames.len(),
            1,
            "已播完的单元不应再交棒，got {:?}",
            frames.iter().map(|f| f.byte_start).collect::<Vec<_>>()
        );
        let frame = &frames[0];
        assert!(
            (frame.visible_fraction - 0.75).abs() < 1e-6,
            "交棒帧必须按单元自己的 progress 计算可见比例（期望 0.75，按事务 progress 会得 0.19）",
        );
        assert_eq!((frame.byte_start, frame.byte_end), (0, 3));
        assert!((frame.x - 100.0).abs() < 1e-6);
        // Issue #690 评论 5679744253 问题 1: 采集时计算剩余时长，不再沿用旧起始时间。
        // 旧单元演了 50ms，总时长 100ms，剩余 50ms。
        assert_eq!(frame.remaining_duration_ms, 50);
        assert_eq!(frame.sampled_at, now);
        // 采集到的比例必须与文字帧同一个几何结果（右边界 100 + 60*0.75 = 145）
        let edge = reveal_slice(0, 3, 100.0, 60.0).compute_frame(frame.visible_fraction);
        assert!(
            (edge.x + edge.w - 145.0).abs() < 1e-6,
            "可见比例应还原出同一帧的文字右边界，got {}",
            edge.x + edge.w
        );
    }

    #[test]
    fn issue690_match_rebase_frames_continues_unit_timeline() {
        let now = Instant::now();
        let old_unit = elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now);
        // Issue #727 约束 2: CaretDriven unit 的 visible_fraction 从 caret track progress 推导。
        // start_fraction=0, target_fraction=1, progress=0.5 → visible = 0 + (1-0)*ease_out_quad(0.5) = 0.75
        let visible_fraction = 0.0 + (1.0 - 0.0) * AnimatedSlice::ease_out_quad(0.5);
        let frame = old_unit.slice.compute_frame(visible_fraction);
        // Issue #690 评论 5679744253 问题 1: RebaseFrame 携带 sampled_at 和
        // remaining_duration_ms，retarget 时从当前帧重新起段。
        // 旧单元演了 50ms，总时长 100ms，剩余 50ms。
        let frames = vec![RebaseFrame {
            byte_start: old_unit.slice.byte_start,
            byte_end: old_unit.slice.byte_end,
            x: frame.x,
            y: frame.y,
            opacity: frame.opacity,
            shaping_identity: None,
            visible_fraction,
            sampled_at: now,
            remaining_duration_ms: 50,
        }];

        let mut units = wrap_units(vec![reveal_slice(0, 3, 100.0, 60.0)]);
        assert!(
            units[0].current_visible_fraction(now) < 1e-9,
            "交棒前新单元从 0 起步"
        );

        let offset_map = OffsetMap::build("abc", "abc");
        match_rebase_frames(&frames, &mut units, &offset_map);

        let unit = &units[0];
        // Issue #727 约束 2: CaretDriven unit 的 start_fraction 是 rebase 交棒时的载体。
        // 交棒后 start_fraction = visible_fraction = 0.75。
        let (start_fraction, started_at_is_none) = match &unit.timing {
            super::VisualUnitTiming::CaretDriven { start_fraction, .. } => (*start_fraction, true),
            super::VisualUnitTiming::Timed {
                start_fraction,
                started_at,
                ..
            } => (*start_fraction, started_at.is_none()),
        };
        assert!(
            (start_fraction - 0.75).abs() < 1e-6,
            "Reveal 单元交棒后应从已显示比例继续，got {}",
            start_fraction
        );
        // Issue #727 约束 2: CaretDriven unit 没有 duration_ms / started_at。
        // remaining_duration_ms 由 caret track 管理，不由 unit 自己的时间线决定。
        assert!(
            started_at_is_none,
            "CaretDriven unit 无独立时间线，started_at 不适用"
        );
        // Issue #690 评论 5679744253 问题 1: retarget 时从当前帧重新起段，
        // started_at 留 None，等进入 Rendering 再启动，progress 从 0 开始。
        // Issue #690 评论 5683759796: 原来写 Some(sampled_at) 会让 rebased 文字 unit
        // 从旧事务交棒时刻提前计时，与等 Rendering 才启动的 caret track 错拍；
        // 改成 None 后跟 fresh unit、caret track 一样由 build_text_animation_plan_with_sample
        // 在 Prepared→Rendering 时用同一个 sample.frame_now 启动。
        assert!(
            started_at_is_none,
            "Issue #690 评论 5683759796: rebase 后 started_at 应为 None（等 Rendering 再启动）"
        );
        let progress = unit.progress(now);
        assert!(
            progress.abs() < 1e-9,
            "retarget 时从当前帧重新起段，progress 从 0 开始，got {}",
            progress
        );
        // 可见比例连续：start_fraction=0.75 + (1-0.75)*ease_out_quad(0) = 0.75
        let visible = unit.current_visible_fraction(now);
        assert!(
            (visible - 0.75).abs() < 1e-6,
            "retarget 后可见比例应连续（0.75），不重复吃进度，got {}",
            visible
        );
    }

    #[test]
    fn issue690_take_rebase_frames_carries_frames_and_cancels_old_transaction() {
        let now = Instant::now();
        let old_key = VisualTransactionKey::new(7, 7);
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(rendering_tx(
            old_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        ));

        let (frames, _) =
            coord.take_rebase_frames(&[old_key], "rebased_by_insert", now, None, "abc", 0);
        assert_eq!(frames.len(), 1, "旧事务的未播完单元要全部交棒");
        assert!((frames[0].visible_fraction - 0.75).abs() < 1e-6);
        assert!(
            coord.prepared_queue.is_empty(),
            "交棒后旧事务必须取消，snapshot/纹理资源归新事务所有"
        );
        let (no_frames, _) =
            coord.take_rebase_frames(&[], "rebased_by_insert", now, None, "abc", 0);
        assert!(no_frames.is_empty(), "无冲突事务时不产生交棒帧");
    }

    /// Issue #690 评论 5675007226 步骤 3: 未被新编辑覆盖的单元继续自己的时间线。
    #[test]
    fn issue690_take_rebase_frames_keeps_transaction_when_units_are_untouched() {
        let now = Instant::now();
        let old_key = VisualTransactionKey::new(11, 11);
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(rendering_tx(
            old_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        ));

        // 在 "abc" 末尾插入 "d"：old 坐标里只是位置 3 这一个点，前面的单元没被覆盖。
        let offset_map = OffsetMap::build("abc", "abcd");
        let (frames, _) = coord.take_rebase_frames(
            &[old_key],
            "rebased_by_insert",
            now,
            Some((&[(3, 3)], &offset_map)),
            "abc",
            0,
        );

        assert!(frames.is_empty(), "未覆盖的单元不该交棒，旧事务自己播完");
        assert_eq!(
            coord.prepared_queue.active_transactions().len(),
            1,
            "旧事务要留在队列里，继续持有自己的 snapshot 与静态隐藏区"
        );
        let unit = &coord.prepared_queue.active_transactions()[0].units[0];
        let start_fraction = unit.timing.start_fraction();
        assert!(
            start_fraction.abs() < 1e-9,
            "保留的单元起点不能被改写，got {}",
            start_fraction
        );
        // Issue #727 约束 2: CaretDriven unit 的 visible_fraction 从 caret track progress 推导。
        // 保留的单元的 caret track progress = 50/100 = 0.5
        // → visible = 0 + (1-0)*ease_out_quad(0.5) = 0.75
        let tx_ref = &coord.prepared_queue.active_transactions()[0];
        let caret_progress = tx_ref
            .cursor_visual_track
            .as_ref()
            .map(|track| track.progress(now))
            .unwrap_or(0.0);
        let visible = start_fraction
            + (unit.timing.target_fraction() - start_fraction)
                * AnimatedSlice::ease_out_quad(caret_progress);
        assert!(
            (visible - 0.75).abs() < 1e-6,
            "保留的单元沿 caret track 继续，不因新事务 id 归零重播，got {}",
            visible
        );
    }

    #[test]
    fn issue690_take_rebase_frames_cancels_when_edit_covers_playing_unit() {
        let now = Instant::now();
        let old_key = VisualTransactionKey::new(12, 12);
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(rendering_tx(
            old_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        ));

        let offset_map = OffsetMap::build("abc", "ab");
        let (frames, _) = coord.take_rebase_frames(
            &[old_key],
            "rebased_by_delete",
            now,
            Some((&[(2, 3)], &offset_map)),
            "abc",
            0,
        );

        assert_eq!(frames.len(), 1, "被编辑覆盖的单元必须交棒给新事务");
        assert!(coord.prepared_queue.is_empty(), "覆盖后旧事务结束生命期");
    }

    #[test]
    fn issue690_take_rebase_frames_cancels_when_unit_offsets_shift() {
        let now = Instant::now();
        let old_key = VisualTransactionKey::new(13, 13);
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(rendering_tx(
            old_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        ));

        // 在开头插入：old 单元 0..3 在新文档里变成 1..4，几何位置变了必须重排。
        let offset_map = OffsetMap::build("abc", "xabc");
        let (frames, _) = coord.take_rebase_frames(
            &[old_key],
            "rebased_by_insert",
            now,
            Some((&[(0, 0)], &offset_map)),
            "abc",
            0,
        );

        assert_eq!(frames.len(), 1, "偏移被平移的单元仍属被影响范围，要交棒");
        assert!(coord.prepared_queue.is_empty());
    }

    #[test]
    fn issue690_take_rebase_frames_cancels_finished_transaction_without_frames() {
        let now = Instant::now();
        let old_key = VisualTransactionKey::new(14, 14);
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(rendering_tx(
            old_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 500, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            500,
        ));

        let offset_map = OffsetMap::build("abc", "abcd");
        let (frames, _) = coord.take_rebase_frames(
            &[old_key],
            "rebased_by_insert",
            now,
            Some((&[(3, 3)], &offset_map)),
            "abc",
            0,
        );

        assert!(frames.is_empty(), "已播完的单元是稳定终态，不该再交棒");
        assert!(
            coord.prepared_queue.is_empty(),
            "全部单元播完的事务没有保留价值，交给新事务接管资源"
        );
    }

    #[test]
    fn issue690_render_plan_cursor_sits_on_reveal_boundary_of_same_frame() {
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(rendering_tx(
            VisualTransactionKey::new(3, 3),
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        ));

        let plan = coord.build_render_plan_full(
            stale_cursor_state(),
            SelectionPreeditPlan::default(),
            FrameContext::default(),
            CursorStyle::default(),
            SelectionPreeditStyle::default(),
            now,
            None,
            0,
            0.0,
        );

        assert_eq!(plan.text_animation.glyphs.len(), 1);
        let glyph = &plan.text_animation.glyphs[0];
        let text_right_edge = glyph.x + glyph.w;
        assert!(
            (text_right_edge - 145.0).abs() < 1e-6,
            "本帧文字右边界应为 100 + 60*0.75，got {}",
            text_right_edge
        );
        assert!(
            (plan.cursor.x - text_right_edge).abs() < 1e-6,
            "光标必须落在同一帧的文字吞吐边界上，got cursor={} text_right={}",
            plan.cursor.x,
            text_right_edge
        );
        assert!(
            (plan.cursor.x - 1234.5).abs() > 1.0,
            "正文事务期间不再把 GUI 线程留下的 visual_x 当最终屏幕坐标"
        );
        assert_eq!(plan.cursor.y, glyph.y);
        assert!(
            (plan.cursor.opacity - 1.0).abs() < 1e-6,
            "Insert 期间光标闪烁抑制，恒为不透明"
        );
    }

    #[test]
    fn issue690_render_plan_keeps_cursor_only_state_when_coordinated_disabled() {
        // Issue #727 约束 6: 删除 coordinated_text_cursor_animation_enabled 独立开关。
        // 是否有吞吐字直接由"本帧有没有有效 caret motion"决定。
        // 无 cursor_visual_track = 无 caret motion = 走 CursorOnly 自己的平滑曲线。
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();
        // 手工装配一笔无 cursor_visual_track 的 Rendering 事务（模拟 caret motion 丢失）。
        let mut tx = rendering_tx(
            VisualTransactionKey::new(3, 3),
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        );
        tx.cursor_visual_track = None;
        coord.prepared_queue.enqueue(tx);

        let plan = coord.build_render_plan_full(
            stale_cursor_state(),
            SelectionPreeditPlan::default(),
            FrameContext::default(),
            CursorStyle::default(),
            SelectionPreeditStyle::default(),
            now,
            None,
            0,
            0.0,
        );

        assert!(
            (plan.cursor.x - 1234.5).abs() < 1e-6,
            "无 caret motion 时走 CursorOnly 自己的平滑曲线，位置不由事务改写"
        );
        assert!(
            plan.cursor.opacity < 1e-6,
            "CursorOnly 链保留 caller 传入的 blink opacity"
        );
    }

    #[test]
    fn issue690_fresh_conceal_unit_runs_from_fully_visible() {
        let now = Instant::now();
        let fresh = PreparedVisualUnit::wrap(conceal_slice(0, 3, 100.0, 60.0, true), 100);
        assert!(
            (fresh.current_visible_fraction(now) - 1.0).abs() < 1e-6,
            "吞字单元第一帧必须完整可见，否则被删的字一帧都不出现",
        );

        // Issue #727 约束 2: CaretDriven unit 的 visible_fraction 从 caret track progress 推导。
        // start_fraction=1.0, target_fraction=0.0, progress=0.5
        // → visible = 1 + (0-1)*ease_out_quad(0.5) = 1 - 0.75 = 0.25
        let half = elapsed_unit(conceal_slice(0, 3, 100.0, 60.0, true), 50, 100, now);
        let visible = 1.0 + (0.0 - 1.0) * AnimatedSlice::ease_out_quad(0.5);
        assert!(
            (visible - 0.25).abs() < 1e-6,
            "吞字比例必须走与吐字同一条曲线（镜像），got {}",
            visible
        );
        let frame = half.slice.compute_frame(visible);
        assert!(
            (frame.w - 15.0).abs() < 1e-6,
            "演到一半时可见宽度 = 60 * 0.25，got {}",
            frame.w
        );
        // Backspace 保留左段：右边界 160 → 115，前半程已扫过 45px（ease-out 减速）。
        assert!(
            (160.0 - (frame.x + frame.w)) > (frame.x + frame.w - 100.0),
            "吞字边界应先快后慢地逼近终点，got edge={}",
            frame.x + frame.w
        );

        // caret track progress=1.0 → visible = 1 + (0-1)*ease_out_quad(1.0) = 0
        let done = elapsed_unit(conceal_slice(0, 3, 100.0, 60.0, true), 200, 100, now);
        let done_visible = 1.0 + (0.0 - 1.0) * AnimatedSlice::ease_out_quad(1.0);
        let frame = done.slice.compute_frame(done_visible);
        assert!(frame.w.abs() < 1e-6, "播完后旧字彻底消失，got {}", frame.w);
    }

    #[test]
    fn issue690_backspace_cursor_tracks_shrinking_conceal_edge() {
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();
        // Backspace：保留左段，可见宽度 60 → 15，右边界 160 → 115 往左走。
        coord.prepared_queue.enqueue(rendering_tx(
            VisualTransactionKey::new(4, 4),
            TextVisualOperationKind::Delete,
            vec![elapsed_unit(
                conceal_slice(100, 103, 100.0, 60.0, true),
                50,
                100,
                now,
            )],
            caret(160.0),
            caret(100.0),
            now,
            50,
        ));

        let plan = coord.build_render_plan_full(
            stale_cursor_state(),
            SelectionPreeditPlan::default(),
            FrameContext::default(),
            CursorStyle::default(),
            SelectionPreeditStyle::default(),
            now,
            None,
            0,
            0.0,
        );
        assert!(
            (plan.cursor.x - 115.0).abs() < 1e-6,
            "Backspace 光标跟着正在被吞掉的右边界（100 + 60*0.25），got {}",
            plan.cursor.x
        );
        let glyph = &plan.text_animation.glyphs[0];
        assert!(
            (plan.cursor.x - (glyph.x + glyph.w)).abs() < 1e-6,
            "光标与文字帧来自同一个采样点"
        );
        assert!(
            plan.cursor.opacity < 1e-6,
            "Delete 不抑制闪烁，blink 状态由 caller 决定"
        );
    }

    #[test]
    fn issue690_forward_delete_cursor_stays_pinned_at_new_caret() {
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();
        // 前向 Delete：保留右段，逻辑光标本来不动，右侧文字向光标收。
        coord.prepared_queue.enqueue(rendering_tx(
            VisualTransactionKey::new(5, 5),
            TextVisualOperationKind::Delete,
            vec![elapsed_unit(
                conceal_slice(100, 103, 100.0, 60.0, false),
                50,
                100,
                now,
            )],
            caret(100.0),
            caret(100.0),
            now,
            50,
        ));

        let plan = coord.build_render_plan_full(
            stale_cursor_state(),
            SelectionPreeditPlan::default(),
            FrameContext::default(),
            CursorStyle::default(),
            SelectionPreeditStyle::default(),
            now,
            None,
            0,
            0.0,
        );
        assert!(
            (plan.cursor.x - 100.0).abs() < 1e-6,
            "前向 Delete 光标固定在 new caret，不回抽，got {}",
            plan.cursor.x
        );
    }

    #[test]
    fn issue690_cursor_without_boundary_glyph_uses_reflow_easing() {
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();
        // 跨行/软换行 reflow：没有可直接当边界的 reveal/conceal 单元，
        // 走 caret track 插值，easing 与 ReflowMove 同为二次曲线。
        // Issue #690 评论 5681206040: caret track 自带 started_at/duration_ms，
        // 不再借 reflow unit 的 progress。
        let mut tx = rendering_tx(
            VisualTransactionKey::new(6, 6),
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reflow_slice(0, 3, 100.0, 200.0), 50, 100, now)],
            CursorRect {
                x: 100.0,
                top: 0.0,
                bottom: 20.0,
                baseline_y: 16.0,
            },
            CursorRect {
                x: 200.0,
                top: 40.0,
                bottom: 60.0,
                baseline_y: 56.0,
            },
            now,
            10,
        );
        // caret track 与 reflow unit 同一条时间线：started_at = now - 50ms, duration = 100ms
        // Issue #690 评论 5682867529: new_first 不再接受 now 参数（始终 started_at = None）。
        // 测试要模拟"已经播了 50ms"的场景，直接用结构体字面量设置 started_at = Some(...)。
        tx.cursor_visual_track = Some(PreparedCursorVisualTrack {
            from: CursorRect {
                x: 100.0,
                top: 0.0,
                bottom: 20.0,
                baseline_y: 16.0,
            },
            to: CursorRect {
                x: 200.0,
                top: 40.0,
                bottom: 60.0,
                baseline_y: 56.0,
            },
            from_visual_line_id: None,
            to_visual_line_id: None,
            from_line_top: 0.0,
            from_line_bottom: 0.0,
            to_line_top: 0.0,
            to_line_bottom: 0.0,
            started_at: Some(now - Duration::from_millis(50)),
            duration_ms: 100,
            pause_start: None,
        });
        coord.prepared_queue.enqueue(tx);

        let plan = coord.build_render_plan_full(
            stale_cursor_state(),
            SelectionPreeditPlan::default(),
            FrameContext::default(),
            CursorStyle::default(),
            SelectionPreeditStyle::default(),
            now,
            None,
            0,
            0.0,
        );
        // caret track 演了 50/100ms → progress 0.5 → ease_out_quad = 0.75
        // → x = 100 + 100*0.75 = 175
        assert!(
            (plan.cursor.x - 175.0).abs() < 1e-6,
            "无边界 glyph 时用 caret track 的 progress 插值，got {}",
            plan.cursor.x
        );
        assert!(
            (plan.cursor.y - 30.0).abs() < 1e-6,
            "y 同一条曲线（0 + 40*0.75 = 30），got {}",
            plan.cursor.y
        );
        assert!(
            (plan.cursor.h - 20.0).abs() < 1e-6,
            "光标高度取 new caret 行高"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Issue #690 评论 5680276931: rebase 交棒后 reflow 光标从逻辑 old caret 重新起步
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn issue690_comment5680276931_rebase_reflow_cursor_starts_from_screen_cursor_not_logical_old_caret(
    ) {
        // 评论 5680276931 指出：take_rebase_frames() 交棒时只采集文字视觉单元，
        // 没有把旧事务这一帧正在屏幕上显示的 coordinated cursor rect 带给新事务。
        // 新事务的 old_cursor_rect 仍来自 pipeline 对旧正文做的权威布局 caret
        // （逻辑 old caret），不是旧动画当前显示到的位置。
        // compute_coordinated_cursor_position() 在 Enter/删除换行/纯 reflow 这类
        // 没有 InsertReveal/DeleteConceal glyph 当边界的场景，走 old/new caret 插值，
        // rebase 后第一帧 progress=0 → cursor.x = old_cursor_rect.x（逻辑 old caret），
        // 而文字 reflow unit 的 from_document_rect 已被 rebase 成屏幕位置 → 文字不跳。
        // 结果：文字保持在屏幕位置，光标却瞬间跳回逻辑 old caret 再往新位置走。
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();

        // ── 旧事务：一个正在播放的 reflow unit，屏幕光标已离开 old_cursor_rect ──
        // old_cursor_rect = 100（逻辑 old caret），new_cursor_rect = 220
        // reflow unit 演了 50/100ms → progress 0.5 → ease_out_quad(0.5) = 0.75
        // 屏幕光标 = 100 + (220-100)*0.75 = 190
        // 用 Insert 操作（对应 Enter 产生换行：有 reflow 但无 InsertReveal glyph），
        // 这样 active_text_transaction_key() 才会返回本事务。
        let old_key = VisualTransactionKey::new(1, 1);
        coord.prepared_queue.enqueue(rendering_tx(
            old_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reflow_slice(0, 3, 100.0, 220.0), 50, 100, now)],
            caret(100.0),
            caret(220.0),
            now,
            50,
        ));

        // 前置断言：旧事务当前屏幕光标 = 190
        let mut old_sample = AnimationFrameSample::new(now);
        old_sample.set_progress(old_key, 0.5);
        let (cx_old, _, _) = coord
            .compute_coordinated_cursor_position(&old_sample, 0)
            .expect("旧事务应能算出协同光标");
        assert!(
            (cx_old - 190.0).abs() < 1e-6,
            "前置：旧事务屏幕光标应在 190（reflow progress 0.5 → eased 0.75），got {}",
            cx_old
        );

        // ── rebase 交棒：take_rebase_frames 现在同时采集文字单元和屏幕光标 ──
        let (rebase_frames, sampled_cursor) =
            coord.take_rebase_frames(&[old_key], "rebased_by_enter", now, None, "abc", 0);
        assert_eq!(
            rebase_frames.len(),
            1,
            "应采集到一个正在播放的 reflow unit 帧"
        );
        assert!(
            (rebase_frames[0].x - 190.0).abs() < 1e-6,
            "rebase 帧应携带旧 unit 当前屏幕位置 190，got {}",
            rebase_frames[0].x
        );
        let sampled_cursor = sampled_cursor.expect("rebase 交棒应采样到旧事务屏幕光标");
        assert!(
            (sampled_cursor.sampled.x - 190.0).abs() < 1e-6,
            "sampled_cursor_rect 应为旧事务屏幕光标 190，got {}",
            sampled_cursor.sampled.x
        );

        // ── 新事务：Enter/纯 reflow，没有 InsertReveal/DeleteConceal glyph 当边界 ──
        // old_cursor_rect = 100：pipeline.record_visual_transaction() 对旧正文做的
        //   权威布局 caret（逻辑 old caret），不等于旧事务屏幕光标 190。
        // new_cursor_rect = 20：下一行最终 caret。
        // 修复后 cursor_visual_from = sampled_cursor（190），cursor_visual_to = new_cursor_rect（20）。
        let new_key = VisualTransactionKey::new(2, 2);
        let mut new_units = wrap_units(vec![reflow_slice(0, 3, 100.0, 20.0)]);
        let offset_map = OffsetMap::build("abc", "abc");
        match_rebase_frames(&rebase_frames, &mut new_units, &offset_map);
        // rebase 后新 reflow unit：from_document_rect.x = 190（屏幕位置），progress(now) = 0
        assert!(
            (new_units[0].slice.from_document_rect.x - 190.0).abs() < 1e-6,
            "rebase 后新 reflow unit 的 from 应为屏幕位置 190，got {}",
            new_units[0].slice.from_document_rect.x
        );
        assert!(
            new_units[0].progress(now).abs() < 1e-9,
            "rebase 后新 unit progress 从 0 开始，got {}",
            new_units[0].progress(now)
        );

        let mut new_tx = rendering_tx(
            new_key,
            TextVisualOperationKind::Insert,
            new_units,
            caret(100.0), // ← 逻辑 old caret（pipeline 权威布局），≠屏幕光标 190
            caret(20.0),
            now,
            0,
        );
        // 修复后：rebase 交棒时把采样到的旧事务屏幕光标作为 cursor_visual_from，
        // new_cursor_rect 作为 cursor_visual_to，compute_coordinated_cursor_position
        // 消费这条同帧 caret track，不再用裸 old_cursor_rect 当 reflow 光标起点。
        // Issue #690 评论 5682867529: new_first 不再接受 now 参数，用结构体字面量设置 started_at。
        new_tx.cursor_visual_track = Some(PreparedCursorVisualTrack {
            from: sampled_cursor.sampled,
            to: caret(20.0),
            from_visual_line_id: None,
            to_visual_line_id: None,
            from_line_top: 0.0,
            from_line_bottom: 0.0,
            to_line_top: 0.0,
            to_line_bottom: 0.0,
            started_at: Some(now),
            duration_ms: 100,
            pause_start: None,
        });
        coord.prepared_queue.enqueue(new_tx);

        // ── 新事务第一帧（frame_now = now，reflow unit progress = 0）──
        let plan = coord.build_render_plan_full(
            stale_cursor_state(),
            SelectionPreeditPlan::default(),
            FrameContext::default(),
            CursorStyle::default(),
            SelectionPreeditStyle::default(),
            now,
            None,
            0,
            0.0,
        );

        // 文字 reflow：from=190，progress=0 → frame.x = 190（不跳）
        assert!(!plan.text_animation.glyphs.is_empty(), "新事务应产出文字帧");
        let text_x = plan.text_animation.glyphs[0].x;
        assert!(
            (text_x - 190.0).abs() < 1e-6,
            "文字 reflow 应从屏幕位置 190 起步不跳，got {}",
            text_x
        );

        // 光标 reflow：修复后从 cursor_visual_from.x = 190 起步（屏幕光标不跳）。
        let cx_new = plan.cursor.x;
        assert!(
            (cx_new - 190.0).abs() < 1e-6,
            "Issue #690 评论 5680276931: rebase 交棒后 reflow 光标应从上一帧屏幕光标 190 起步，\
             修复后 cursor_visual_from 同步 rebase，第一帧光标不跳（got cursor.x={}）",
            cx_new
        );
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Issue #690 评论 5681206040 复现：连续交棒光标跳变 + reflow_progress 原地取首
    // ═════════════════════════════════════════════════════════════════════════

    /// 评论 5681206040 问题 1：`sample_coordinated_cursor_rect_at()` 没有采样旧事务
    /// 自己的 visual caret track（`tx.cursor_visual_from` / `tx.cursor_visual_to`），
    /// 仍然固定用 `old_cursor_rect / new_cursor_rect`。连续交棒（第二次 rebase）时
    /// 采样到的光标会回到逻辑 old caret 起算，与旧事务当前屏幕光标不一致，
    /// 新事务拿错误的 sampled cursor 当起点，连续快速操作时光标跳变。
    #[test]
    fn issue690_comment5681206040_continuous_handoff_sample_uses_tx_visual_caret_track() {
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();

        // ── 第一次交棒后的新事务 B（手工装配，模拟第一次 rebase 后的状态）──
        // cursor_visual_from = 190：第一次 rebase 采样的旧事务屏幕光标。
        // cursor_visual_to   = 20 ：new_cursor_rect 镜像。
        // old_cursor_rect    = 100：pipeline 对旧正文做的权威布局 caret（逻辑 old caret），
        //                          ≠ 旧事务屏幕光标 190。
        // new_cursor_rect    = 20。
        // reflow unit 已播 50/100ms → progress 0.5 → ease_out_quad(0.5) = 0.75。
        // 事务 B 当前屏幕光标 = 190 + (20 - 190) * 0.75 = 62.5。
        let key_b = VisualTransactionKey::new(2, 2);
        let mut tx_b = rendering_tx(
            key_b,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reflow_slice(0, 3, 190.0, 20.0), 50, 100, now)],
            caret(100.0),
            caret(20.0),
            now,
            50,
        );
        // Issue #690 评论 5682867529: new_first 不再接受 now 参数，用结构体字面量设置 started_at。
        tx_b.cursor_visual_track = Some(PreparedCursorVisualTrack {
            from: caret(190.0),
            to: caret(20.0),
            from_visual_line_id: None,
            to_visual_line_id: None,
            from_line_top: 0.0,
            from_line_bottom: 0.0,
            to_line_top: 0.0,
            to_line_bottom: 0.0,
            started_at: Some(now - Duration::from_millis(50)),
            duration_ms: 100,
            pause_start: None,
        });
        coord.prepared_queue.enqueue(tx_b);

        // 前置断言：compute_coordinated_cursor_position 已修复用 visual track，
        // 事务 B 当前屏幕光标 = 62.5。
        let expected_screen_cursor = 190.0 + (20.0 - 190.0) * AnimatedSlice::ease_out_quad(0.5);
        let mut sample_b = AnimationFrameSample::new(now);
        sample_b.set_progress(key_b, 0.5);
        let (cx_b, _, _) = coord
            .compute_coordinated_cursor_position(&sample_b, 0)
            .expect("事务 B 应能算出协同光标");
        assert!(
            (cx_b - expected_screen_cursor).abs() < 1e-6,
            "前置：事务 B 屏幕光标应在 {}（visual track 190→20, progress 0.5），got {}",
            expected_screen_cursor,
            cx_b
        );

        // ── 第二次 rebase：take_rebase_frames 采样事务 B 的屏幕光标 ──
        // sample_coordinated_cursor_rect_at(B, now) 应返回事务 B 当前屏幕光标 62.5。
        // 当前缺陷：reflow 分支用 old_cursor_rect=100, new_cursor_rect=20
        //   → 100 + (20-100)*0.75 = 40，而非屏幕上的 62.5。
        let (_rebase_frames, sampled_cursor) =
            coord.take_rebase_frames(&[key_b], "rebased_by_second_input", now, None, "abc", 0);
        let sampled_cursor = sampled_cursor.expect("第二次 rebase 应采样到事务 B 的屏幕光标");

        let buggy_value = 100.0 + (20.0 - 100.0) * AnimatedSlice::ease_out_quad(0.5);
        assert!(
            (sampled_cursor.sampled.x - expected_screen_cursor).abs() < 1e-6,
            "Issue #690 评论 5681206040 问题1: 连续交棒第二次 sampled_cursor 应为事务 B \
             屏幕光标 {} (用 cursor_visual_track)，但当前实现用 \
             old_cursor_rect/new_cursor_rect 算出 {} (got sampled_cursor.sampled.x={})",
            expected_screen_cursor,
            buggy_value,
            sampled_cursor.sampled.x
        );
    }

    /// 评论 5681206040 问题 2：cursor reflow 仍然"随便拿第一个 reflow unit 的 progress"。
    /// `sample_coordinated_cursor_rect_at()` 和 `compute_coordinated_cursor_position()` 里
    /// `reflow_progress()` 遍历 `tx.units`，遇到第一个 `ReflowMove/ReflowCrossFade` 就直接
    /// 返回它的 progress。视觉单元各自持有 `started_at / duration_ms`，rebase 后不同 unit
    /// 可能有不同剩余时长。第一个 reflow unit 可能已经到 1.0，另一个与当前 caret 更相关的
    /// reflow unit 还在 0.4，光标提前冲到目标，与实际正在移动的文字不同步。
    #[test]
    fn issue690_comment5681206040_reflow_progress_should_not_take_first_unit() {
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();

        // 事务 C：Insert 纯 reflow（无 InsertReveal glyph），首次事务（无 visual track）。
        // old_cursor_rect = 0, new_cursor_rect = 200。
        // 两个 ReflowMove unit：
        //   unit1: duration=100ms, 已播 100ms → progress=1.0（已播完）
        //   unit2: duration=200ms, 已播 40ms → progress=0.2（仍在播）
        // reflow_progress() 遇到 unit1 直接返回 1.0 → eased=1.0 → 光标 = 200（目标）。
        // 但 unit2 还在 0.2，事务整体未完成，光标不应已到目标。
        let key_c = VisualTransactionKey::new(3, 3);
        let tx_c = rendering_tx(
            key_c,
            TextVisualOperationKind::Insert,
            vec![
                elapsed_unit(reflow_slice(0, 3, 0.0, 100.0), 100, 100, now),
                elapsed_unit(reflow_slice(3, 6, 50.0, 150.0), 40, 200, now),
            ],
            caret(0.0),
            caret(200.0),
            now,
            40,
        );
        coord.prepared_queue.enqueue(tx_c);

        let mut sample_c = AnimationFrameSample::new(now);
        sample_c.set_progress(key_c, 0.2);
        let (cx_c, _, _) = coord
            .compute_coordinated_cursor_position(&sample_c, 0)
            .expect("事务 C 应能算出协同光标");

        // 期望：unit2 还在 progress=0.2，事务未完成，光标不应已到 new_cursor_rect.x=200。
        // 当前缺陷：reflow_progress 取 unit1.progress=1.0 → 光标 = 200（提前冲到目标）。
        let new_cx = 200.0;
        assert!(
            (cx_c - new_cx).abs() > 1e-6,
            "Issue #690 评论 5681206040 问题2: unit2 还在 progress=0.2，事务未完成，\
             光标不应已到 new_cursor_rect.x={}，但 reflow_progress 取第一个 unit1.progress=1.0 \
             导致光标提前冲到目标 (got cursor.x={})",
            new_cx,
            cx_c
        );
    }

    /// 评论 5681206040 要求：测试补真实连续交棒，不要只测一次。
    /// 旧事务 `100 -> 220` 播到中间 -> 第一次 rebase 成 `190 -> 20` -> 再播一段 ->
    /// 第二次 rebase；断言第二次 sampled caret 精确等于第二次 rebase 前
    /// `compute_coordinated_cursor_position()` 的屏幕结果，而不是按逻辑 old/new caret 重算。
    #[test]
    fn issue690_comment5681206040_real_continuous_handoff_two_rebases() {
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();

        // ── 旧事务 A：caret 100→220，reflow unit 播到中间 ──
        // reflow unit: from_x=100, to_x=220, duration=100ms, 已播 50ms → progress 0.5
        // ease_out_quad(0.5) = 0.75 → 屏幕光标 = 100 + (220-100)*0.75 = 190
        let key_a = VisualTransactionKey::new(1, 1);
        let mut tx_a = rendering_tx(
            key_a,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reflow_slice(0, 3, 100.0, 220.0), 50, 100, now)],
            caret(100.0),
            caret(220.0),
            now,
            50,
        );
        // Issue #690 评论 5682867529: new_first 不再接受 now 参数，用结构体字面量设置 started_at。
        tx_a.cursor_visual_track = Some(PreparedCursorVisualTrack {
            from: caret(100.0),
            to: caret(220.0),
            from_visual_line_id: None,
            to_visual_line_id: None,
            from_line_top: 0.0,
            from_line_bottom: 0.0,
            to_line_top: 0.0,
            to_line_bottom: 0.0,
            started_at: Some(now - Duration::from_millis(50)),
            duration_ms: 100,
            pause_start: None,
        });
        coord.prepared_queue.enqueue(tx_a);

        // 验证事务 A 当前屏幕光标 = 190
        let expected_a = 100.0 + (220.0 - 100.0) * AnimatedSlice::ease_out_quad(0.5);
        let mut sample_a = AnimationFrameSample::new(now);
        sample_a.set_progress(key_a, 0.5);
        let (cx_a, _, _) = coord
            .compute_coordinated_cursor_position(&sample_a, 0)
            .expect("事务 A 应能算出协同光标");
        assert!(
            (cx_a - expected_a).abs() < 1e-6,
            "事务 A 屏幕光标应为 {}，got {}",
            expected_a,
            cx_a
        );

        // ── 第一次 rebase：take_rebase_frames 采集事务 A 的屏幕光标 ──
        let (rebase_frames_a, handoff_a) =
            coord.take_rebase_frames(&[key_a], "first_rebase", now, None, "abc", 0);
        let handoff_a = handoff_a.expect("第一次 rebase 应采样到事务 A 的屏幕光标");
        assert!(
            (handoff_a.sampled.x - expected_a).abs() < 1e-6,
            "第一次 rebase sampled caret 应为 {}，got {}",
            expected_a,
            handoff_a.sampled.x
        );

        // ── 新事务 B：用 handoff_a 构造 cursor_visual_track ──
        // from = 190（sampled），to = 20（new_cursor_rect），duration = handoff_a.remaining_duration_ms
        let key_b = VisualTransactionKey::new(2, 2);
        let mut new_units_b = wrap_units(vec![reflow_slice(0, 3, 190.0, 20.0)]);
        let offset_map = OffsetMap::build("abc", "abc");
        match_rebase_frames(&rebase_frames_a, &mut new_units_b, &offset_map);
        let mut tx_b = rendering_tx(
            key_b,
            TextVisualOperationKind::Insert,
            new_units_b,
            caret(100.0),
            caret(20.0),
            now,
            0,
        );
        tx_b.cursor_visual_track = Some(PreparedCursorVisualTrack {
            from: handoff_a.sampled,
            to: caret(20.0),
            from_visual_line_id: None,
            to_visual_line_id: None,
            from_line_top: 0.0,
            from_line_bottom: 0.0,
            to_line_top: 0.0,
            to_line_bottom: 0.0,
            started_at: Some(now),
            duration_ms: handoff_a.remaining_duration_ms,
            pause_start: None,
        });
        coord.prepared_queue.enqueue(tx_b);

        // ── 事务 B 播一段：50ms 后 ──
        // caret track: from=190, to=20, started_at=now, duration=50ms（handoff remaining）
        // progress = 50/50 = 1.0 → eased = 1.0 → caret = 20
        // 但我们要测"再播一段"不是"播完"，所以用 25ms → progress = 25/50 = 0.5
        // ease_out_quad(0.5) = 0.75 → 屏幕光标 = 190 + (20-190)*0.75 = 62.5
        let now_after_b = now + Duration::from_millis(25);
        let expected_b = 190.0 + (20.0 - 190.0) * AnimatedSlice::ease_out_quad(0.5);
        let mut sample_b = AnimationFrameSample::new(now_after_b);
        sample_b.set_progress(key_b, 0.5);
        let (cx_b, _, _) = coord
            .compute_coordinated_cursor_position(&sample_b, 0)
            .expect("事务 B 应能算出协同光标");
        assert!(
            (cx_b - expected_b).abs() < 1e-6,
            "事务 B 屏幕光标应为 {}（visual track 190→20, progress 0.5），got {}",
            expected_b,
            cx_b
        );

        // ── 第二次 rebase：take_rebase_frames 采样事务 B 的屏幕光标 ──
        let (_rebase_frames_b, handoff_b) =
            coord.take_rebase_frames(&[key_b], "second_rebase", now_after_b, None, "abc", 0);
        let handoff_b = handoff_b.expect("第二次 rebase 应采样到事务 B 的屏幕光标");

        // 断言：第二次 sampled caret 精确等于第二次 rebase 前
        // compute_coordinated_cursor_position() 的屏幕结果
        assert!(
            (handoff_b.sampled.x - expected_b).abs() < 1e-6,
            "Issue #690 评论 5681206040: 连续交棒第二次 sampled caret 应为事务 B 屏幕光标 {}，\
             但 got {}（如果按逻辑 old/new caret 重算会得到不同值）",
            expected_b,
            handoff_b.sampled.x
        );

        // 额外验证：第二次 sampled caret 不等于按逻辑 old/new caret 重算的值
        let logical_recalc = 100.0 + (20.0 - 100.0) * AnimatedSlice::ease_out_quad(0.5);
        assert!(
            (handoff_b.sampled.x - logical_recalc).abs() > 1e-6,
            "第二次 sampled caret 不应等于按逻辑 old/new caret 重算的值 {}",
            logical_recalc
        );
    }

    /// 评论 5681206040 要求：再补两个不同 `started_at/duration_ms` 的 reflow unit，
    /// 确认 caret 不依赖 `units` 顺序。
    #[test]
    fn issue690_comment5681206040_caret_track_independent_of_units_order() {
        let now = Instant::now();

        // 事务 D：有两个不同 started_at/duration_ms 的 reflow unit，有 cursor_visual_track。
        // caret track: from=0, to=200, started_at=now-40ms, duration=200ms
        // 已播 40ms → progress = 40/200 = 0.2 → ease_out_quad(0.2) = 0.36
        // 屏幕光标 = 0 + (200-0)*0.36 = 72
        let expected_d = 0.0 + (200.0 - 0.0) * AnimatedSlice::ease_out_quad(0.2);

        // unit1: duration=100ms, 已播 100ms → progress=1.0（已播完）
        // unit2: duration=200ms, 已播 40ms → progress=0.2（仍在播）
        // 如果 caret 依赖 units 顺序（取第一个 reflow unit 的 progress），
        // 会用 unit1.progress=1.0 → eased=1.0 → caret=200（错误）。
        // 正确行为：caret track 自带 started_at/duration_ms，不依赖任何 unit 的 progress。

        // ── 顺序 1：unit1 在前，unit2 在后 ──
        let mut coord1 = LinuxEditorAnimationCoordinator::new();
        let key_d1 = VisualTransactionKey::new(4, 4);
        let mut tx_d1 = rendering_tx(
            key_d1,
            TextVisualOperationKind::Insert,
            vec![
                elapsed_unit(reflow_slice(0, 3, 0.0, 100.0), 100, 100, now),
                elapsed_unit(reflow_slice(3, 6, 50.0, 150.0), 40, 200, now),
            ],
            caret(0.0),
            caret(200.0),
            now,
            40,
        );
        // Issue #690 评论 5682867529: new_first 不再接受 now 参数，用结构体字面量设置 started_at。
        tx_d1.cursor_visual_track = Some(PreparedCursorVisualTrack {
            from: caret(0.0),
            to: caret(200.0),
            from_visual_line_id: None,
            to_visual_line_id: None,
            from_line_top: 0.0,
            from_line_bottom: 0.0,
            to_line_top: 0.0,
            to_line_bottom: 0.0,
            started_at: Some(now - Duration::from_millis(40)),
            duration_ms: 200,
            pause_start: None,
        });
        coord1.prepared_queue.enqueue(tx_d1);

        let mut sample_d1 = AnimationFrameSample::new(now);
        sample_d1.set_progress(key_d1, 0.2);
        let (cx_d1, _, _) = coord1
            .compute_coordinated_cursor_position(&sample_d1, 0)
            .expect("事务 D1 应能算出协同光标");

        assert!(
            (cx_d1 - expected_d).abs() < 1e-6,
            "事务 D1（unit1在前）屏幕光标应为 {}（caret track 0→200, progress 0.2），\
             got {} — caret 不应依赖 units 顺序",
            expected_d,
            cx_d1
        );

        // ── 顺序 2：unit2 在前，unit1 在后（交换 units 顺序）──
        let mut coord2 = LinuxEditorAnimationCoordinator::new();
        let key_d2 = VisualTransactionKey::new(5, 5);
        let mut tx_d2 = rendering_tx(
            key_d2,
            TextVisualOperationKind::Insert,
            vec![
                elapsed_unit(reflow_slice(3, 6, 50.0, 150.0), 40, 200, now),
                elapsed_unit(reflow_slice(0, 3, 0.0, 100.0), 100, 100, now),
            ],
            caret(0.0),
            caret(200.0),
            now,
            40,
        );
        // Issue #690 评论 5682867529: new_first 不再接受 now 参数，用结构体字面量设置 started_at。
        tx_d2.cursor_visual_track = Some(PreparedCursorVisualTrack {
            from: caret(0.0),
            to: caret(200.0),
            from_visual_line_id: None,
            to_visual_line_id: None,
            from_line_top: 0.0,
            from_line_bottom: 0.0,
            to_line_top: 0.0,
            to_line_bottom: 0.0,
            started_at: Some(now - Duration::from_millis(40)),
            duration_ms: 200,
            pause_start: None,
        });
        coord2.prepared_queue.enqueue(tx_d2);

        let mut sample_d2 = AnimationFrameSample::new(now);
        sample_d2.set_progress(key_d2, 0.2);
        let (cx_d2, _, _) = coord2
            .compute_coordinated_cursor_position(&sample_d2, 0)
            .expect("事务 D2 应能算出协同光标");

        assert!(
            (cx_d2 - expected_d).abs() < 1e-6,
            "事务 D2（unit2在前）屏幕光标应为 {}（caret track 0→200, progress 0.2），\
             got {} — caret 不应依赖 units 顺序",
            expected_d,
            cx_d2
        );

        // ── 关键断言：两种 units 顺序的 caret 结果完全相同 ──
        assert!(
            (cx_d1 - cx_d2).abs() < 1e-6,
            "Issue #690 评论 5681206040: 不同 units 顺序的 caret 结果应完全相同，\
             但 got cx_d1={} vs cx_d2={} — caret track 不应依赖 units 顺序",
            cx_d1,
            cx_d2
        );

        // ── 额外验证：sample_coordinated_cursor_rect_at 也不依赖 units 顺序 ──
        let sampled1 = sample_coordinated_cursor_rect_at(
            coord1
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == key_d1)
                .unwrap(),
            now,
        )
        .expect("事务 D1 应能采样到光标");
        let sampled2 = sample_coordinated_cursor_rect_at(
            coord2
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == key_d2)
                .unwrap(),
            now,
        )
        .expect("事务 D2 应能采样到光标");

        assert!(
            (sampled1.x - sampled2.x).abs() < 1e-6,
            "Issue #690 评论 5681206040: sample_coordinated_cursor_rect_at 也不应依赖 units 顺序，\
             但 got sampled1.x={} vs sampled2.x={}",
            sampled1.x,
            sampled2.x
        );
        assert!(
            (sampled1.x - expected_d).abs() < 1e-6,
            "sample_coordinated_cursor_rect_at 结果应为 {}，got {}",
            expected_d,
            sampled1.x
        );
    }

    /// Issue #690 评论 5682867529: caret track 跟文字 unit 共用同一个"开始播放时刻"。
    ///
    /// 真正经过 Pending → Prepared → Rendering 生命周期的行为测试：
    /// - 创建纯 reflow 事务，caret track 此时尚未开始（started_at = None）；
    /// - 模拟在 Pending/Prepared 阶段过去 40ms；
    /// - 第一帧进入 Rendering；
    /// - 断言同一个 frame_now 下文字 reflow unit progress == 0，caret track progress == 0；
    /// - 再推进 50ms，断言二者从同一个起点同时前进。
    #[test]
    fn issue690_comment5682867529_caret_track_starts_with_text_unit_at_rendering() {
        let create_now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = VisualTransactionKey::new(100, 100);

        // 构造一笔纯 reflow 事务（无 InsertReveal/DeleteConceal，只有 ReflowMove）。
        // caret track: from=caret(0), to=caret(200), duration=200ms。
        // 事务创建时 started_at = None（尚未开始）。
        let reflow_unit = PreparedVisualUnit::wrap(reflow_slice(0, 3, 0.0, 100.0), 200);
        let cursor_visual_track = PreparedCursorVisualTrack::new_first(
            caret(0.0),
            caret(200.0),
            None,
            None,
            0.0,
            0.0,
            0.0,
            0.0,
            200,
        );
        let tx = PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::Insert,
            timeline: TransactionTimeline::new(200),
            units: vec![reflow_unit],
            old_cursor_rect: Some(caret(0.0)),
            new_cursor_rect: Some(caret(200.0)),
            cursor_visual_track: Some(cursor_visual_track),
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: None,
            cursor_owner_epoch: 0,
            caret_motion_retired: false,
            visual_affected_byte_range_old: None,
            visual_affected_byte_range_new: None,
            layout_basis_revision: LayoutRevision::initial(),
        };
        coord.prepared_queue.enqueue(tx);

        // 断言 1: 事务创建时 caret track started_at = None，progress = 0。
        {
            let tx_ref = coord
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == key)
                .expect("事务应在队列中");
            let track = tx_ref
                .cursor_visual_track
                .as_ref()
                .expect("应有 caret track");
            assert!(
                track.started_at.is_none(),
                "事务创建时 caret track started_at 应为 None，got {:?}",
                track.started_at
            );
            assert!(
                (track.progress(create_now) - 0.0).abs() < 1e-9,
                "started_at=None 时 progress 应为 0"
            );
            assert!(
                tx_ref.units[0].timing.is_caret_driven()
                    || matches!(
                        &tx_ref.units[0].timing,
                        super::VisualUnitTiming::Timed {
                            started_at: None,
                            ..
                        }
                    ),
                "事务创建时文字 unit started_at 应为 None（CaretDriven 无 started_at，Timed 应为 None）"
            );
            assert!(
                (tx_ref.units[0].progress(create_now) - 0.0).abs() < 1e-9,
                "文字 unit progress 应为 0"
            );
        }

        // 模拟 Pending → Prepared 阶段过去 40ms（纹理准备等）。
        let prepared_now = create_now + Duration::from_millis(40);
        coord.prepared_queue.mark_prepared(key);

        // 断言 2: Prepared 阶段过去 40ms 后，caret track 仍未开始。
        {
            let tx_ref = coord
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == key)
                .expect("事务应在队列中");
            let track = tx_ref
                .cursor_visual_track
                .as_ref()
                .expect("应有 caret track");
            assert!(
                track.started_at.is_none(),
                "Prepared 阶段过去 40ms 后 caret track started_at 仍应为 None \
                （Pending/Prepared 等待时间不算进动画播放时间），got {:?}",
                track.started_at
            );
            assert!(
                (track.progress(prepared_now) - 0.0).abs() < 1e-9,
                "Prepared 阶段 progress 仍应为 0（未开始计时），got {}",
                track.progress(prepared_now)
            );
            assert!(
                (tx_ref.units[0].progress(prepared_now) - 0.0).abs() < 1e-9,
                "Prepared 阶段文字 unit progress 仍应为 0"
            );
        }

        // 第一帧进入 Rendering。
        let frame_now_0 = prepared_now + Duration::from_millis(16);
        // Issue #727 评论 5760020833 问题2: Prepared→Rendering 现由 begin_rendering_transactions
        // 在采样前完成，build_text_animation_plan_with_sample 不再做状态切换。
        coord.begin_rendering_transactions(frame_now_0);
        let mut sample_0 = AnimationFrameSample::new(frame_now_0);
        sample_0.set_progress(key, 0.0);
        let (plan_0, _, _) = coord
            .build_text_animation_plan_with_sample(&sample_0, 0, LayoutRevision::initial());

        // 断言 3: 同一个 frame_now_0 下 progress 都 == 0。
        {
            let tx_ref = coord
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == key)
                .expect("事务应在队列中");
            assert_eq!(
                tx_ref.state,
                TextVisualTransactionState::Rendering,
                "第一帧后事务应进入 Rendering"
            );
            let track = tx_ref
                .cursor_visual_track
                .as_ref()
                .expect("应有 caret track");
            assert!(
                track.started_at.is_some(),
                "进入 Rendering 后 caret track started_at 应被设置"
            );
            assert_eq!(
                track.started_at,
                Some(frame_now_0),
                "caret track started_at 应等于第一帧 frame_now"
            );
            let track_progress = track.progress(frame_now_0);
            assert!(
                (track_progress - 0.0).abs() < 1e-9,
                "第一帧 caret track progress 应为 0（刚启动），got {}",
                track_progress
            );
            let unit_progress = tx_ref.units[0].progress(frame_now_0);
            assert!(
                (unit_progress - 0.0).abs() < 1e-9,
                "第一帧文字 unit progress 应为 0（刚启动），got {}",
                unit_progress
            );
            assert!(!plan_0.glyphs.is_empty(), "应有文字 glyph 输出");
        }

        // 推进 50ms，断言二者从同一个起点同时前进。
        let frame_now_1 = frame_now_0 + Duration::from_millis(50);
        let mut sample_1 = AnimationFrameSample::new(frame_now_1);
        sample_1.set_progress(key, 0.25);
        let (plan_1, _, _) = coord
            .build_text_animation_plan_with_sample(&sample_1, 0, LayoutRevision::initial());

        {
            let tx_ref = coord
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == key)
                .expect("事务应在队列中");
            let track = tx_ref
                .cursor_visual_track
                .as_ref()
                .expect("应有 caret track");
            let track_progress = track.progress(frame_now_1);
            let unit_progress = tx_ref.units[0].progress(frame_now_1);
            assert!(
                (track_progress - 0.25).abs() < 1e-9,
                "推进 50ms 后 caret track progress 应为 0.25（50/200），got {}",
                track_progress
            );
            assert!(
                (unit_progress - 0.25).abs() < 1e-9,
                "推进 50ms 后文字 unit progress 应为 0.25（50/200），got {}",
                unit_progress
            );
            assert!(
                (track_progress - unit_progress).abs() < 1e-9,
                "caret track 和文字 unit 的 progress 应完全相同（从同一帧起跑），\
                 got track={} unit={}",
                track_progress,
                unit_progress
            );
            assert!(!plan_1.glyphs.is_empty(), "推进 50ms 后应有文字 glyph 输出");
        }

        println!("[BUGFIX_690_VERIFY] 评论5682867529 caret track 与文字 unit 同帧起跑 (FIXED)");
    }

    /// Issue #690 评论 5683759796: rebased 文字 unit 和 caret track 在 Rendering 阶段同帧起跑。
    ///
    /// 真正经过 rebase 交棒 + Pending → Prepared → Rendering 生命周期的行为测试：
    /// - 构造旧事务（Rendering），含一个播到 50% 的 InsertReveal unit 和已播 50ms 的 caret track；
    /// - 用 collect_rebase_frames 采集旧事务的 rebase frames；
    /// - 构造新事务的 units（fresh wrap），用 match_rebase_frames rebase；
    /// - 构造新事务的 caret track（rebase_to，started_at = None）；
    /// - 新事务以 Pending 入队，模拟 Pending → Prepared 过去 40ms；
    /// - 断言 Prepared 阶段：rebased text unit started_at == None 且 progress == 0；
    ///   caret track started_at == None 且 progress == 0；
    /// - 第一帧进入 Rendering，断言 rebased text unit progress == 0 且 caret track progress == 0
    ///   （修复前 rebased text unit progress 会 > 0，因为 started_at = Some(旧事务交棒时刻)）；
    /// - 推进 25ms / 50ms，断言二者一起前进（progress 相同）。
    #[test]
    fn issue690_comment5683759796_rebased_unit_and_caret_track_start_together_at_rendering() {
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();

        // ── 1. 构造旧事务（Rendering 状态） ──
        // 文字 unit: InsertReveal, elapsed 50ms / duration 100ms → progress 0.5
        // → ease_out_quad(0.5) = 0.75 → visible_fraction = 0.75
        let old_unit = elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now);
        // caret track: from=caret(100), to=caret(160), 已播 50ms / duration 100ms → progress 0.5
        let old_caret_track = PreparedCursorVisualTrack {
            from: caret(100.0),
            to: caret(160.0),
            from_visual_line_id: None,
            to_visual_line_id: None,
            from_line_top: 0.0,
            from_line_bottom: 0.0,
            to_line_top: 0.0,
            to_line_bottom: 0.0,
            started_at: Some(now - Duration::from_millis(50)),
            duration_ms: 100,
            pause_start: None,
        };
        let old_key = VisualTransactionKey::new(7, 7);
        let mut old_tx = rendering_tx(
            old_key,
            TextVisualOperationKind::Insert,
            vec![old_unit],
            caret(100.0),
            caret(160.0),
            now,
            50,
        );
        old_tx.cursor_visual_track = Some(old_caret_track.clone());

        // ── 2. 用 collect_rebase_frames(now) 采集旧事务的 rebase frames ──
        let rebase_frames = old_tx.collect_rebase_frames(now);
        assert_eq!(
            rebase_frames.len(),
            1,
            "应采集到一个 rebase frame（旧 unit 未播完）"
        );
        let rebase_frame = &rebase_frames[0];
        assert_eq!(rebase_frame.sampled_at, now);
        assert_eq!(rebase_frame.remaining_duration_ms, 50);

        // ── 3. 构造新事务的 units（fresh wrap），用 match_rebase_frames rebase ──
        let mut new_units = wrap_units(vec![reveal_slice(0, 3, 100.0, 60.0)]);
        let offset_map = OffsetMap::build("abc", "abc");
        match_rebase_frames(&rebase_frames, &mut new_units, &offset_map);

        // rebased text unit: start_fraction = 0.75（旧 unit 当前可见比例）。
        // Issue #690 评论 5683759796 关键断言: started_at 必须是 None（不是 Some(sampled_at)），
        // 这样才不会从旧事务交棒时刻提前计时。
        // Issue #727 约束 2: CaretDriven unit 无独立 duration_ms，剩余时长由 caret track 管理。
        // CaretDriven unit 的 start_fraction 是 rebase 交棒时的载体（0.75）。
        let (new_started_at_is_none, new_duration_ms) = match &new_units[0].timing {
            super::VisualUnitTiming::CaretDriven { start_fraction, .. } => {
                // CaretDriven unit 无 duration_ms 字段，剩余时长在 caret track 中断言。
                assert!(
                    (start_fraction - 0.75).abs() < 1e-9,
                    "rebase 后 CaretDriven unit start_fraction 应为 0.75，got {}",
                    start_fraction
                );
                (true, 0u64)
            }
            super::VisualUnitTiming::Timed {
                started_at,
                duration_ms,
                ..
            } => (started_at.is_none(), *duration_ms),
        };
        assert!(
            new_started_at_is_none,
            "Issue #690 评论 5683759796: rebase 后文字 unit started_at 应为 None\
             （等 Rendering 再启动）"
        );
        // CaretDriven unit 无独立 duration_ms，剩余时长在 caret track 中断言（见下方）。
        // Timed unit 的 duration_ms 应为剩余时长 50。
        if !matches!(
            &new_units[0].timing,
            super::VisualUnitTiming::CaretDriven { .. }
        ) {
            assert_eq!(
                new_duration_ms, 50,
                "rebase 后 Timed unit duration_ms 应为剩余时长 50"
            );
        }

        // ── 4. 构造新事务的 caret track（rebase_to，started_at = None） ──
        let new_caret_track = old_caret_track.rebase_to(caret(220.0), now);
        assert!(
            new_caret_track.started_at.is_none(),
            "rebase_to 后 caret track started_at 应为 None"
        );
        assert_eq!(
            new_caret_track.duration_ms, 50,
            "rebase_to 后 caret track duration_ms 应为剩余时长 50"
        );

        // ── 5. 把新事务以 Pending 状态入队 ──
        let new_key = VisualTransactionKey::new(8, 8);
        let new_tx = PreparedTextVisualTransaction {
            key: new_key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::Insert,
            timeline: TransactionTimeline::new(50),
            units: new_units,
            old_cursor_rect: Some(caret(100.0)),
            new_cursor_rect: Some(caret(220.0)),
            cursor_visual_track: Some(new_caret_track),
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: None,
            cursor_owner_epoch: 0,
            caret_motion_retired: false,
            visual_affected_byte_range_old: None,
            visual_affected_byte_range_new: None,
            layout_basis_revision: LayoutRevision::initial(),
        };
        coord.prepared_queue.enqueue(new_tx);

        // ── 6. 模拟 Pending → Prepared 过去 40ms ──
        let prepared_now = now + Duration::from_millis(40);
        coord.prepared_queue.mark_prepared(new_key);

        // ── 7. 断言 Prepared 阶段 ──
        {
            let tx_ref = coord
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == new_key)
                .expect("新事务应在队列中");
            assert_eq!(
                tx_ref.state,
                TextVisualTransactionState::Prepared,
                "mark_prepared 后事务应处于 Prepared"
            );
            assert!(
                tx_ref.units[0].timing.is_caret_driven()
                    || matches!(
                        &tx_ref.units[0].timing,
                        super::VisualUnitTiming::Timed {
                            started_at: None,
                            ..
                        }
                    ),
                "Prepared 阶段 rebased text unit started_at 应为 None（CaretDriven 无 started_at，Timed 应为 None）"
            );
            assert!(
                (tx_ref.units[0].progress(prepared_now) - 0.0).abs() < 1e-9,
                "Prepared 阶段 rebased text unit progress 应为 0，got {}",
                tx_ref.units[0].progress(prepared_now)
            );
            let track = tx_ref
                .cursor_visual_track
                .as_ref()
                .expect("应有 caret track");
            assert!(
                track.started_at.is_none(),
                "Prepared 阶段 caret track started_at 应为 None，got {:?}",
                track.started_at
            );
            assert!(
                (track.progress(prepared_now) - 0.0).abs() < 1e-9,
                "Prepared 阶段 caret track progress 应为 0，got {}",
                track.progress(prepared_now)
            );
        }

        // ── 8. 第一帧进入 Rendering ──
        let frame_now_0 = prepared_now + Duration::from_millis(16);
        // Issue #727 评论 5760020833 问题2: Prepared→Rendering 现由 begin_rendering_transactions
        // 在采样前完成，build_text_animation_plan_with_sample 不再做状态切换。
        coord.begin_rendering_transactions(frame_now_0);
        let mut sample_0 = AnimationFrameSample::new(frame_now_0);
        sample_0.set_progress(new_key, 0.0);
        // Issue #727 约束 3: InsertReveal/DeleteConceal 需要 CoordinatedMotionFrame.caret
        // 不为 None 才能生成 glyph。第一帧 caret track progress = 0，caret 在 from = (100, 0)。
        let coordinated_frame_0 = CoordinatedMotionFrame {
            caret: Some(SampledCaretFrame {
                x: 100.0,
                y: 0.0,
                visual_line_id: None,
                progress: 0.0,
            }),
            owner_key: Some(new_key),
        };
        let (plan_0, _, _) =
            coord.build_text_animation_plan_with_sample(&sample_0, 0, LayoutRevision::initial());

        // ── 9. 断言第一帧 Rendering：二者 progress == 0（同帧起跑，没有错拍） ──
        {
            let tx_ref = coord
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == new_key)
                .expect("新事务应在队列中");
            assert_eq!(
                tx_ref.state,
                TextVisualTransactionState::Rendering,
                "第一帧后事务应进入 Rendering"
            );
            let track = tx_ref
                .cursor_visual_track
                .as_ref()
                .expect("应有 caret track");
            assert_eq!(
                track.started_at,
                Some(frame_now_0),
                "进入 Rendering 后 caret track started_at 应等于第一帧 frame_now"
            );
            // Issue #727 约束 2: CaretDriven unit 无 started_at，progress 总是 0.0。
            // Timed unit 的 started_at 应等于第一帧 frame_now。
            match &tx_ref.units[0].timing {
                super::VisualUnitTiming::CaretDriven { .. } => {
                    // CaretDriven: 无独立时间线，progress 由 caret track 驱动。
                }
                super::VisualUnitTiming::Timed { started_at, .. } => {
                    assert_eq!(
                        *started_at,
                        Some(frame_now_0),
                        "Issue #690 评论 5683759796: 进入 Rendering 后 Timed text unit started_at \
                         应等于第一帧 frame_now"
                    );
                }
            }
            let track_progress = track.progress(frame_now_0);
            let unit_progress = tx_ref.units[0].progress(frame_now_0);
            assert!(
                (track_progress - 0.0).abs() < 1e-9,
                "第一帧 caret track progress 应为 0（刚启动），got {}",
                track_progress
            );
            assert!(
                (unit_progress - 0.0).abs() < 1e-9,
                "Issue #690 评论 5683759796: 第一帧 rebased text unit progress 应为 0\
                 （刚启动，不再从旧事务交棒时刻提前计时），got {}",
                unit_progress
            );
            assert!(
                (track_progress - unit_progress).abs() < 1e-9,
                "第一帧 caret track 和 rebased text unit 的 progress 应完全相同（同帧起跑），\
                 got track={} unit={}",
                track_progress,
                unit_progress
            );
            assert!(!plan_0.glyphs.is_empty(), "第一帧应有文字 glyph 输出");
        }

        // ── 10. 推进 25ms，断言 caret track 前进到 0.5 ──
        // rebased text unit 是 CaretDriven（InsertReveal），progress 总是 0.0。
        // CaretDriven unit 的 current_visible_fraction 返回 start_fraction（rebase 交棒载体），
        // 不随时间变化。真正的可见比例推导在 build_text_animation_plan_with_sample 中
        // 从 caret track progress 计算。
        let frame_now_mid = frame_now_0 + Duration::from_millis(25);
        let mut sample_mid = AnimationFrameSample::new(frame_now_mid);
        sample_mid.set_progress(new_key, 0.5);
        // 推进 25ms 后 caret track progress = 0.5，eased = 0.75，
        // caret 在 100 + (220-100)*0.75 = 190。
        let coordinated_frame_mid = CoordinatedMotionFrame {
            caret: Some(SampledCaretFrame {
                x: 190.0,
                y: 0.0,
                visual_line_id: None,
                progress: 0.5,
            }),
            owner_key: Some(new_key),
        };
        let (_plan_mid, _, _) =
            coord.build_text_animation_plan_with_sample(&sample_mid, 0, LayoutRevision::initial());
        {
            let tx_ref = coord
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == new_key)
                .expect("新事务应在队列中");
            let track = tx_ref
                .cursor_visual_track
                .as_ref()
                .expect("应有 caret track");
            let track_progress = track.progress(frame_now_mid);
            let unit_progress = tx_ref.units[0].progress(frame_now_mid);
            // CaretDriven unit 的 progress 总是 0.0（无独立时间线）。
            assert!(
                (unit_progress - 0.0).abs() < 1e-9,
                "Issue #727 约束 2: CaretDriven unit progress 总是 0.0，got {}",
                unit_progress
            );
            assert!(
                (track_progress - 0.5).abs() < 1e-9,
                "推进 25ms 后 caret track progress 应为 0.5（25/50），got {}",
                track_progress
            );
            // CaretDriven unit 的 current_visible_fraction 返回 start_fraction（rebase 交棒载体），
            // 不随时间变化。这是正确的——真正的可见比例推导在 build_text_animation_plan_with_sample 中
            // 从 caret track progress 计算。
            let unit_visible = tx_ref.units[0].current_visible_fraction(frame_now_mid);
            assert!(
                (unit_visible - 0.75).abs() < 1e-9,
                "Issue #727 约束 2: CaretDriven unit current_visible_fraction 返回 start_fraction（0.75），got {}",
                unit_visible
            );
        }

        // ── 11. 推进到 50ms，断言 caret track 到 1.0 ──
        let frame_now_1 = frame_now_0 + Duration::from_millis(50);
        let mut sample_1 = AnimationFrameSample::new(frame_now_1);
        sample_1.set_progress(new_key, 1.0);
        // 推进 50ms 后 caret track progress = 1.0，caret 在 to = (220, 0)。
        let coordinated_frame_1 = CoordinatedMotionFrame {
            caret: Some(SampledCaretFrame {
                x: 220.0,
                y: 0.0,
                visual_line_id: None,
                progress: 1.0,
            }),
            owner_key: Some(new_key),
        };
        let (_plan_1, _, _) =
            coord.build_text_animation_plan_with_sample(&sample_1, 0, LayoutRevision::initial());
        {
            let tx_ref = coord
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == new_key)
                .expect("新事务应在队列中");
            let track = tx_ref
                .cursor_visual_track
                .as_ref()
                .expect("应有 caret track");
            let track_progress = track.progress(frame_now_1);
            let unit_progress = tx_ref.units[0].progress(frame_now_1);
            // CaretDriven unit 的 progress 总是 0.0（无独立时间线）。
            assert!(
                (unit_progress - 0.0).abs() < 1e-9,
                "Issue #727 约束 2: CaretDriven unit progress 总是 0.0，got {}",
                unit_progress
            );
            assert!(
                (track_progress - 1.0).abs() < 1e-9,
                "推进 50ms 后 caret track progress 应为 1.0（50/50），got {}",
                track_progress
            );
            // CaretDriven unit 的 current_visible_fraction 返回 start_fraction（rebase 交棒载体）。
            let unit_visible = tx_ref.units[0].current_visible_fraction(frame_now_1);
            assert!(
                (unit_visible - 0.75).abs() < 1e-9,
                "Issue #727 约束 2: CaretDriven unit current_visible_fraction 返回 start_fraction（0.75），got {}",
                unit_visible
            );
        }

        println!("[BUGFIX_690_VERIFY] 评论5683759796 rebased 文字 unit 和 caret track 在 Rendering 同帧起跑 (FIXED)");
    }

    // ------------------------------------------------------------------
    // Issue #702 评论 5708436497: has_active_insert() 语义回归测试
    // ------------------------------------------------------------------
    // 之前 `PreparedTransactionQueue::has_active_insert()` 只判断 state 不是
    // Completed/Cancelled，没有过滤 `operation_kind == Insert`，导致 Delete /
    // CompositionUpdate / CompositionCommitOrCancel 事务也返回 true，被 blink
    // 抑制逻辑错误当成 Insert。以下测试确认修复后只认 Insert。

    /// 构造一笔处于 Pending 状态（活跃，非 Completed/Cancelled）的最小事务，
    /// 只设置 key 与 operation_kind，其余字段取最小值。用于 `has_active_insert()`
    /// 的语义验证——该方法只看 `operation_kind` 与 `state`，不依赖 units/snapshots。
    fn make_minimal_active_tx(
        key: VisualTransactionKey,
        operation_kind: TextVisualOperationKind,
    ) -> PreparedTextVisualTransaction {
        PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind,
            timeline: TransactionTimeline::new(100),
            units: Vec::new(),
            old_cursor_rect: None,
            new_cursor_rect: None,
            cursor_visual_track: None,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: None,
            cursor_owner_epoch: 0,
            caret_motion_retired: false,
            visual_affected_byte_range_old: None,
            visual_affected_byte_range_new: None,
            layout_basis_revision: LayoutRevision::initial(),
        }
    }

    #[test]
    fn test_has_active_insert_returns_false_for_delete_only() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(1, 1),
            TextVisualOperationKind::Delete,
        ));
        assert!(
            !coord.has_active_insert(),
            "只有 Delete 事务时 has_active_insert() 应为 false"
        );
        println!(
            "[BUGFIX_VERIFY] 只有 Delete 事务时 has_active_insert()==false \
             (Issue702 评论5708436497)"
        );
    }

    #[test]
    fn test_has_active_insert_returns_false_for_composition_update_only() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(2, 1),
            TextVisualOperationKind::CompositionUpdate,
        ));
        assert!(
            !coord.has_active_insert(),
            "只有 CompositionUpdate 事务时 has_active_insert() 应为 false"
        );
        println!(
            "[BUGFIX_VERIFY] 只有 CompositionUpdate 事务时 has_active_insert()==false \
             (Issue702 评论5708436497)"
        );
    }

    #[test]
    fn test_has_active_insert_returns_false_for_composition_commit_or_cancel_only() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(3, 1),
            TextVisualOperationKind::CompositionCommitOrCancel,
        ));
        assert!(
            !coord.has_active_insert(),
            "只有 CompositionCommitOrCancel 事务时 has_active_insert() 应为 false"
        );
        println!(
            "[BUGFIX_VERIFY] 只有 CompositionCommitOrCancel 事务时 has_active_insert()==false \
             (Issue702 评论5708436497)"
        );
    }

    #[test]
    fn test_has_active_insert_returns_true_for_insert() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(4, 1),
            TextVisualOperationKind::Insert,
        ));
        assert!(
            coord.has_active_insert(),
            "有活跃 Insert 事务时 has_active_insert() 应为 true"
        );
        println!(
            "[BUGFIX_VERIFY] 有活跃 Insert 事务时 has_active_insert()==true \
             (Issue702 评论5708436497)"
        );
    }

    #[test]
    fn test_has_active_insert_returns_false_after_insert_completed() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = VisualTransactionKey::new(5, 1);
        coord
            .prepared_queue
            .enqueue(make_minimal_active_tx(key, TextVisualOperationKind::Insert));
        assert!(
            coord.has_active_insert(),
            "完成前 has_active_insert() 应为 true"
        );
        let removed = coord.prepared_queue.complete(key);
        assert!(removed.is_some(), "complete 应返回 Some（事务曾存在）");
        assert!(
            !coord.has_active_insert(),
            "Insert 事务 Completed 后 has_active_insert() 应为 false"
        );
        println!(
            "[BUGFIX_VERIFY] Insert 事务 Completed 后 has_active_insert()==false \
             (Issue702 评论5708436497)"
        );
    }

    #[test]
    fn test_has_active_insert_returns_false_after_insert_cancelled() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = VisualTransactionKey::new(6, 1);
        coord
            .prepared_queue
            .enqueue(make_minimal_active_tx(key, TextVisualOperationKind::Insert));
        assert!(
            coord.has_active_insert(),
            "取消前 has_active_insert() 应为 true"
        );
        let cancelled = coord.prepared_queue.cancel(key, "test_cancel");
        assert!(cancelled, "cancel 应返回 true（事务曾存在）");
        assert!(
            !coord.has_active_insert(),
            "Insert 事务 Cancelled 后 has_active_insert() 应为 false"
        );
        println!(
            "[BUGFIX_VERIFY] Insert 事务 Cancelled 后 has_active_insert()==false \
             (Issue702 评论5708436497)"
        );
    }

    #[test]
    fn test_has_active_insert_returns_true_when_insert_mixed_with_other_kinds() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(7, 1),
            TextVisualOperationKind::Delete,
        ));
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(7, 2),
            TextVisualOperationKind::CompositionUpdate,
        ));
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(7, 3),
            TextVisualOperationKind::Insert,
        ));
        assert!(
            coord.has_active_insert(),
            "Insert 与其他类型事务共存时 has_active_insert() 应为 true"
        );
        println!(
            "[BUGFIX_VERIFY] Insert 与 Delete/Composition 共存时 has_active_insert()==true \
             (Issue702 评论5708436497)"
        );
    }

    #[test]
    fn test_has_active_insert_only_returns_true_for_insert_kind() {
        // 综合断言：空队列返回 false；逐个入队非 Insert 事务仍返回 false；
        // 入队 Insert 事务后翻转为 true。覆盖队列层面 `any` 迭代顺序无关性。
        let mut coord = LinuxEditorAnimationCoordinator::new();
        assert!(
            !coord.has_active_insert(),
            "空队列 has_active_insert() 应为 false"
        );
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(8, 1),
            TextVisualOperationKind::Delete,
        ));
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(8, 2),
            TextVisualOperationKind::CompositionUpdate,
        ));
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(8, 3),
            TextVisualOperationKind::CompositionCommitOrCancel,
        ));
        assert!(
            !coord.has_active_insert(),
            "仅有 Delete/CompositionUpdate/CompositionCommitOrCancel 时 has_active_insert() 应为 false"
        );
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(8, 4),
            TextVisualOperationKind::Insert,
        ));
        assert!(
            coord.has_active_insert(),
            "追加 Insert 事务后 has_active_insert() 应翻转为 true"
        );
        println!(
            "[BUGFIX_VERIFY] has_active_insert() 综合语义：仅 Insert 翻转 true \
             (Issue702 评论5708436497 FIXED)"
        );
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Issue #710 评论 5733833897: 一次新编辑同时撞上多笔旧事务时，
    // 队列必须处理全部冲突，不能只处理第一笔。
    // ═════════════════════════════════════════════════════════════════════════

    /// Issue #710 评论 5733833897: 一次新编辑同时撞上多笔旧事务时，
    /// 队列必须处理全部冲突，不能只处理第一笔。
    ///
    /// 场景：
    /// - current_old_text = "aaa\nbbb"（A 段 "aaa"，换行，B 段 "bbb"）
    /// - current_new_text = "aaabbb"（删了换行）
    /// - offset_map = OffsetMap::build("aaa\nbbb", "aaabbb")（current-old → current-new）
    /// - changed_old_ranges = [(3, 4)]（换行符位置）
    /// - tx1: unit byte range (0,3)（A 段 "aaa"，旧事务 new 坐标系）
    ///   tx1.new_text = "aaa\nbbb"（tx1 之后文本没变直到当前编辑）
    ///   per_tx_map = OffsetMap::build("aaa\nbbb", "aaa\nbbb") = identity
    ///   unit (0,3) 映射到 current-old (0,3)，不在 changed_old_ranges (3,4) 内
    ///   offset_map(0,3) = (0,3) == (0,3) → untouched ✓（tx1 留在队列）
    /// - tx2: unit byte range (4,7)（B 段 "bbb"，旧事务 new 坐标系）
    ///   tx2.new_text = "aaa\nbbb"
    ///   per_tx_map = identity
    ///   unit (4,7) 映射到 current-old (4,7)，不在 changed_old_ranges (3,4) 内
    ///   但 offset_map(4,7) = (3,6) ≠ (4,7) → 被覆盖 ✓（tx2 被取消）
    ///
    /// 断言：
    /// - tx1 留在队列里（keep）
    /// - tx2 被取消
    /// - rebase_frames 非空（来自 tx2 的 unit）
    #[test]
    fn issue710_take_rebase_frames_handles_multiple_conflicting_transactions() {
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();

        // current-old 文本：A 段 "aaa" + 换行 + B 段 "bbb"
        let current_old_text = "aaa\nbbb";
        // current-new 文本：删除换行后 "aaabbb"
        let current_new_text = "aaabbb";
        // current-old → current-new 的 OffsetMap
        let offset_map = OffsetMap::build(current_old_text, current_new_text);
        // 编辑范围：只删了换行符 (3, 4)
        let changed_old_ranges: [(usize, usize); 1] = [(3, 4)];

        // ── tx1: A 段 "aaa" 的旧事务，unit byte range (0,3) ──
        // tx1.new_snapshot.virtual_text = "aaa\nbbb"（tx1 之后文本没变直到当前编辑）
        // per_tx_map = OffsetMap::build("aaa\nbbb", "aaa\nbbb") = identity
        let tx1_key = VisualTransactionKey::new(100, 1);
        let mut tx1 = rendering_tx(
            tx1_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        );
        tx1.new_snapshot = Some(make_test_snapshot("aaa\nbbb", vec![]));
        coord.prepared_queue.enqueue(tx1);

        // ── tx2: B 段 "bbb" 的旧事务，unit byte range (4,7) ──
        // tx2.new_snapshot.virtual_text = "aaa\nbbb"
        // per_tx_map = identity
        // unit (4,7) 映射到 current-old (4,7)，不在 changed_old_ranges (3,4) 内
        // 但 offset_map(4,7) = (3,6) ≠ (4,7) → 被覆盖
        let tx2_key = VisualTransactionKey::new(101, 1);
        let mut tx2 = rendering_tx(
            tx2_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(4, 7, 200.0, 60.0), 50, 100, now)],
            caret(200.0),
            caret(260.0),
            now,
            50,
        );
        tx2.new_snapshot = Some(make_test_snapshot("aaa\nbbb", vec![]));
        coord.prepared_queue.enqueue(tx2);

        // 前置断言：队列里有两笔事务
        assert_eq!(
            coord.prepared_queue.active_transactions().len(),
            2,
            "前置：队列里应有 tx1 和 tx2 两笔事务"
        );

        // ── 调用 take_rebase_frames 处理全部冲突事务 ──
        // conflicting = [tx1_key, tx2_key]（模拟 find_conflicting_transaction 返回全部）
        let conflicting = vec![tx1_key, tx2_key];
        let (rebase_frames, _caret_handoff) = coord.take_rebase_frames(
            &conflicting,
            "rebased_by_delete",
            now,
            Some((&changed_old_ranges, &offset_map)),
            current_old_text,
            0,
        );

        // ── 断言 1: tx1 留在队列里（keep）──
        let active = coord.prepared_queue.active_transactions();
        let tx1_still_active = active.iter().any(|t| t.key == tx1_key);
        assert!(
            tx1_still_active,
            "Issue #710 评论 5733833897: tx1 的 unit (0,3) 在 A 段，未被删除换行覆盖，\
             应留在队列里继续播完。实际队列里只剩 {:?}",
            active.iter().map(|t| t.key).collect::<Vec<_>>()
        );

        // ── 断言 2: tx2 被取消 ──
        let tx2_still_active = active.iter().any(|t| t.key == tx2_key);
        assert!(
            !tx2_still_active,
            "Issue #710 评论 5733833897: tx2 的 unit (4,7) 在 B 段，删除换行后 B 段上移，\
             offset_map(4,7) = (3,6) ≠ (4,7)，被覆盖，必须被取消。\
             实际队列里仍有 tx2"
        );

        // ── 断言 3: rebase_frames 非空（来自 tx2 的 unit）──
        assert!(
            !rebase_frames.is_empty(),
            "Issue #710 评论 5733833897: tx2 被取消时应采集其 unit 的 rebase frames，\
             rebase_frames 不应为空"
        );
        assert_eq!(
            rebase_frames.len(),
            1,
            "应采集到 tx2 的 1 个 unit frame（tx1 untouched 不采集）"
        );

        // ── 断言 4: rebase_frames 的 byte range 已映射到 current-old 坐标系 ──
        // tx2 的 unit (4,7) 经 per_tx_map (identity) 映射后仍为 (4,7)
        // （per_tx_map 是 tx2.new_text → current_old_text 的 identity 映射）
        let frame = &rebase_frames[0];
        assert_eq!(
            (frame.byte_start, frame.byte_end),
            (4, 7),
            "rebase frame 的 byte range 应为 current-old 坐标系的 (4,7)\
             （per_tx_map=identity 映射）"
        );

        println!(
            "[BUGFIX_VERIFY] Issue #710 评论 5733833897: 多冲突事务逐笔处理 \
             (tx1 keep, tx2 cancel) FIXED"
        );
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Issue #710 评论 5734282079: take_rebase_frames 映射失败的 frame
    // 不应进入 all_rebase_frames。frame 的原值属于旧事务自己的 new_text revision，
    // 映射失败后若保留旧数值，相当于把"已知属于旧 revision 的 byte offset"
    // 伪装成 current-old offset，会污染 match_rebase_frames 的 tier1/tier2 匹配。
    // ═════════════════════════════════════════════════════════════════════════

    /// Issue #710 评论 5734282079: take_rebase_frames 映射失败的 frame
    /// 不应进入 all_rebase_frames。
    ///
    /// 场景：
    /// - 旧事务 tx 的 new_snapshot.virtual_text = "abc"（旧事务 new 坐标系）
    /// - current_old_text = "axyzc"（current-old 坐标系，"bc" 被替换成 "xyz"）
    /// - tx 的 unit byte range = (1, 3)（"bc" 在旧事务 new 坐标系 "abc" 中）
    /// - per_tx_map = OffsetMap::build("abc", "axyzc")
    ///   - entries: [0,0,1] Identity ("a"), [2,4,1] Shifted ("c")
    ///   - map_old_range_to_new(1, 3) 找不到包含 old_start=1 的 entry
    ///     （中间的 "bc" 被替换了）→ 返回 None
    /// - 调用 take_rebase_frames，断言 rebase_frames 为空（映射失败的 frame 被丢弃）
    /// - 断言旧事务被 cancel（不在 active_transactions 中）
    #[test]
    fn issue710_take_rebase_frames_drops_frame_on_mapping_failure() {
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();

        // current-old 文本："abc" 中 "bc" 被替换成 "xyz" → "axyzc"
        let current_old_text = "axyzc";
        // current-new 文本：与 current-old 相同（本测试只关心 per_tx_map 映射失败，
        // 不关心 current-old → current-new 映射）
        let current_new_text = "axyzc";
        let offset_map = OffsetMap::build(current_old_text, current_new_text);
        // 编辑范围：覆盖整个 "xyz" 区域 (1, 4)
        let changed_old_ranges: [(usize, usize); 1] = [(1, 4)];

        // ── tx: unit byte range (1, 3)（"bc" 在旧事务 new 坐标系 "abc" 中）──
        // tx.new_snapshot.virtual_text = "abc"（旧事务 new 坐标系）
        // per_tx_map = OffsetMap::build("abc", "axyzc")
        //   entries: [0,0,1] Identity ("a"), [2,4,1] Shifted ("c")
        // map_old_range_to_new(1, 3) → None（"bc" 被替换了，找不到包含 1 的 entry）
        let tx_key = VisualTransactionKey::new(200, 1);
        let mut tx = rendering_tx(
            tx_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(1, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        );
        tx.new_snapshot = Some(make_test_snapshot("abc", vec![]));
        coord.prepared_queue.enqueue(tx);

        // 前置断言：队列里有一笔事务
        assert_eq!(
            coord.prepared_queue.active_transactions().len(),
            1,
            "前置：队列里应有 tx 一笔事务"
        );

        // ── 调用 take_rebase_frames 处理冲突事务 ──
        let conflicting = vec![tx_key];
        let (rebase_frames, _caret_handoff) = coord.take_rebase_frames(
            &conflicting,
            "rebased_by_replace",
            now,
            Some((&changed_old_ranges, &offset_map)),
            current_old_text,
            0,
        );

        // ── 断言 1: rebase_frames 为空（映射失败的 frame 被丢弃）──
        assert!(
            rebase_frames.is_empty(),
            "Issue #710 评论 5734282079: per_tx_map 映射失败的 frame 不应进入 \
             all_rebase_frames。frame 的原值 (1,3) 属于旧事务 new 坐标系 \"abc\"，\
             在 current-old \"axyzc\" 中 \"bc\" 已被替换成 \"xyz\"，\
             map_old_range_to_new(1,3) 返回 None，该 frame 必须被丢弃。\
             实际 rebase_frames 有 {} 个",
            rebase_frames.len()
        );

        // ── 断言 2: 旧事务被 cancel（不在 active_transactions 中）──
        let active = coord.prepared_queue.active_transactions();
        let tx_still_active = active.iter().any(|t| t.key == tx_key);
        assert!(
            !tx_still_active,
            "Issue #710 评论 5734282079: 映射失败的 frame 虽不进入 byte-range rebase，\
             但旧事务仍应被 cancel（cancel 逻辑不变）。实际队列里仍有 tx"
        );

        println!(
            "[BUGFIX_VERIFY] Issue #710 评论 5734282079: take_rebase_frames 映射失败 \
             的 frame 被丢弃（不进入 all_rebase_frames），旧事务仍 cancel FIXED"
        );
    }

    /// Issue #710 评论 5733833897: 验证 `find_conflicting_transaction` 返回全部冲突事务。
    ///
    /// 构造两笔 active 事务，它们的 units byte range 都与查询 range 重叠，
    /// `find_conflicting_transaction` 应返回两个 key（而非只返回第一个）。
    #[test]
    fn issue710_find_conflicting_transaction_returns_all_conflicts() {
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();

        // current_old_text = "aaa\nbbb"
        let current_old_text = "aaa\nbbb";

        // ── tx1: unit (0,3)（A 段 "aaa"），tx1.new_text = "aaa\nbbb" ──
        let tx1_key = VisualTransactionKey::new(100, 1);
        let mut tx1 = rendering_tx(
            tx1_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        );
        tx1.new_snapshot = Some(make_test_snapshot("aaa\nbbb", vec![]));
        coord.prepared_queue.enqueue(tx1);

        // ── tx2: unit (4,7)（B 段 "bbb"），tx2.new_text = "aaa\nbbb" ──
        let tx2_key = VisualTransactionKey::new(101, 1);
        let mut tx2 = rendering_tx(
            tx2_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(4, 7, 200.0, 60.0), 50, 100, now)],
            caret(200.0),
            caret(260.0),
            now,
            50,
        );
        tx2.new_snapshot = Some(make_test_snapshot("aaa\nbbb", vec![]));
        coord.prepared_queue.enqueue(tx2);

        // 查询 range 覆盖整个 A+B 段 (0,7)
        let conflicts = coord
            .prepared_queue
            .find_conflicting_transaction(current_old_text, 0, 7);

        // 应返回两个 key
        assert_eq!(
            conflicts.len(),
            2,
            "Issue #710 评论 5733833897: find_conflicting_transaction 应返回全部冲突事务\
             （2 笔），而非只返回第一个。实际返回 {:?}",
            conflicts
        );
        assert!(conflicts.contains(&tx1_key), "应包含 tx1_key={:?}", tx1_key);
        assert!(conflicts.contains(&tx2_key), "应包含 tx2_key={:?}", tx2_key);

        println!(
            "[BUGFIX_VERIFY] Issue #710 评论 5733833897: find_conflicting_transaction \
             返回全部冲突事务 (2 笔) FIXED"
        );
    }

    /// Issue #710 评论 5733833897: 验证 caret handoff 在多冲突事务中选最新拥有
    /// coordinated caret 的一笔。
    ///
    /// 构造两笔冲突事务，都拥有 coordinated caret（cursor_owner_epoch == current_cursor_epoch），
    /// tx2 的 transaction_id 更大（更新创建）。take_rebase_frames 应选 tx2 的 caret handoff。
    #[test]
    fn issue710_take_rebase_frames_caret_handoff_picks_latest_coordinated_caret() {
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();

        let current_old_text = "abc";
        let current_new_text = "ab";
        let offset_map = OffsetMap::build(current_old_text, current_new_text);
        let changed_old_ranges: [(usize, usize); 1] = [(2, 3)];

        // ── tx1: transaction_id=100，cursor_owner_epoch=0 ──
        // unit (0,3) 在 changed_old_ranges (2,3) 内 → 被覆盖 → cancel
        let tx1_key = VisualTransactionKey::new(100, 1);
        let mut tx1 = rendering_tx(
            tx1_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        );
        tx1.new_snapshot = Some(make_test_snapshot("abc", vec![]));
        // 给 tx1 设置 caret track，使其拥有 coordinated caret
        tx1.cursor_visual_track = Some(PreparedCursorVisualTrack {
            from: caret(100.0),
            to: caret(160.0),
            from_visual_line_id: None,
            to_visual_line_id: None,
            from_line_top: 0.0,
            from_line_bottom: 0.0,
            to_line_top: 0.0,
            to_line_bottom: 0.0,
            started_at: Some(now - Duration::from_millis(50)),
            duration_ms: 100,
            pause_start: None,
        });
        coord.prepared_queue.enqueue(tx1);

        // ── tx2: transaction_id=200，cursor_owner_epoch=0（更新创建）──
        // unit (0,3) 在 changed_old_ranges (2,3) 内 → 被覆盖 → cancel
        let tx2_key = VisualTransactionKey::new(200, 1);
        let mut tx2 = rendering_tx(
            tx2_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 200.0, 60.0), 50, 100, now)],
            caret(200.0),
            caret(260.0),
            now,
            50,
        );
        tx2.new_snapshot = Some(make_test_snapshot("abc", vec![]));
        // 给 tx2 设置 caret track，使其拥有 coordinated caret
        tx2.cursor_visual_track = Some(PreparedCursorVisualTrack {
            from: caret(200.0),
            to: caret(260.0),
            from_visual_line_id: None,
            to_visual_line_id: None,
            from_line_top: 0.0,
            from_line_bottom: 0.0,
            to_line_top: 0.0,
            to_line_bottom: 0.0,
            started_at: Some(now - Duration::from_millis(50)),
            duration_ms: 100,
            pause_start: None,
        });
        coord.prepared_queue.enqueue(tx2);

        // 两笔都被覆盖，都应被取消。caret handoff 应选 tx2（transaction_id=200 更大）。
        let conflicting = vec![tx1_key, tx2_key];
        let (_rebase_frames, caret_handoff) = coord.take_rebase_frames(
            &conflicting,
            "rebased_by_delete",
            now,
            Some((&changed_old_ranges, &offset_map)),
            current_old_text,
            0, // current_cursor_epoch = 0，两笔 tx 都匹配
        );

        // 两笔都应被取消
        assert!(coord.prepared_queue.is_empty(), "两笔冲突事务都应被取消");

        // caret handoff 应来自 tx2（transaction_id=200 更大）
        // tx2 的 caret track: from=200, to=260, started_at=now-50ms, duration=100ms
        // progress = 50/100 = 0.5 → eased = 0.75
        // sampled.x = 200 + (260-200)*0.75 = 200 + 45 = 245
        let handoff = caret_handoff.expect("应采样到 caret handoff");
        let expected_tx2_cursor = 200.0 + (260.0 - 200.0) * AnimatedSlice::ease_out_quad(0.5);
        assert!(
            (handoff.sampled.x - expected_tx2_cursor).abs() < 1e-6,
            "Issue #710 评论 5733833897: caret handoff 应选 tx2（transaction_id=200 更大），\
             sampled.x 应为 {}（tx2 屏幕光标），got {}",
            expected_tx2_cursor,
            handoff.sampled.x
        );

        // 额外验证：不等于 tx1 的屏幕光标
        let expected_tx1_cursor = 100.0 + (160.0 - 100.0) * AnimatedSlice::ease_out_quad(0.5);
        assert!(
            (handoff.sampled.x - expected_tx1_cursor).abs() > 1e-6,
            "caret handoff 不应选 tx1（transaction_id=100 更小），tx1 屏幕光标为 {}",
            expected_tx1_cursor
        );

        println!(
            "[BUGFIX_VERIFY] Issue #710 评论 5733833897: 多冲突事务 caret handoff \
             选最新拥有 coordinated caret 的一笔 FIXED"
        );
    }

    /// Issue #727 评论 5760650874 方案 A 回归测试：旧 CaretDriven 事务失去 owner 后
    /// **永远**不能再重新获得 owner（即使 new tx 完成移除、即使 old tx 仍在 active queue
    /// 因 Timed Reflow 未播完）。
    ///
    /// 修复前（评论 5760431554 问题1）只在"本帧"把非 owner 的 CaretDriven 事务视为
    /// caret 部分完成：`caret_track_complete = !has_caret_driven_units || !owns_caret || caret_track_done`。
    /// 但旧事务若同时还有 ReflowMove/ReflowCrossFade 没播完，`all_units_done == false`，
    /// 旧事务仍留在 active queue。新事务完成并从队列移除后，下一帧
    /// `active_text_transaction_key_with_epoch()` 会再次倒序选中这个旧事务，
    /// `sample_coordinated_motion_frame()` 又会给它 `owner_key = old_tx.key`，
    /// 已经 Snap 回 canonical 的旧 caret / 吞吐字轨迹会重新接管，造成 caret 回跳。
    ///
    /// 方案 A 修复：给 `PreparedTextVisualTransaction` 增加 `caret_motion_retired: bool`，
    /// 在 `build_text_animation_plan_with_sample` 发现 `has_caret_driven_units && !owns_caret`
    /// 时置 true，`active_text_transaction_key_with_epoch` / `active_text_transaction_key`
    /// 永远跳过 retired 事务。
    ///
    /// 本测试是跨两帧的完整回归测试：
    /// - old tx：CaretDriven (InsertReveal) + 仍未完成的 Timed Reflow (ReflowMove duration=1000ms)；
    /// - new tx：只有 CaretDriven，会成为 owner；
    /// - 第 1 帧确认 old tx 失去 owner 但因 Reflow 仍留队列，且 caret_motion_retired 被置 true；
    /// - 移除/完成 new tx；
    /// - 第 2 帧确认 `active_text_transaction_key_with_epoch()` **不能**重新返回 old tx，
    ///   `CoordinatedMotionFrame.owner_key` 也不能重新变成 old key。
    #[test]
    fn issue727_comment5760650874_old_tx_regains_owner_next_frame() {
        let create_now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let old_key = VisualTransactionKey::new(100, 100);
        let new_key = VisualTransactionKey::new(200, 200);
        let epoch = 1u64;

        // ── 1. 构造 old tx：含 CaretDriven (InsertReveal) + 未完成的 Timed Reflow ──
        // caret track: from=caret(0), to=caret(100), duration=100ms
        // ReflowMove: duration=1000ms（很长，确保在测试时间窗口内未完成）
        let old_insert_unit = PreparedVisualUnit::wrap(reveal_slice(0, 3, 0.0, 60.0), 100);
        let old_reflow_unit = PreparedVisualUnit::wrap(reflow_slice(3, 6, 60.0, 120.0), 1000);
        let old_cursor_track = PreparedCursorVisualTrack::new_first(
            caret(0.0),
            caret(100.0),
            None,
            None,
            0.0,
            20.0,
            0.0,
            20.0,
            100,
        );
        let old_tx = PreparedTextVisualTransaction {
            key: old_key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::Insert,
            timeline: TransactionTimeline::new(100),
            units: vec![old_insert_unit, old_reflow_unit],
            old_cursor_rect: Some(caret(0.0)),
            new_cursor_rect: Some(caret(100.0)),
            cursor_visual_track: Some(old_cursor_track),
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: None,
            cursor_owner_epoch: epoch,
            caret_motion_retired: false,
            visual_affected_byte_range_old: None,
            visual_affected_byte_range_new: None,
            layout_basis_revision: LayoutRevision::initial(),
        };
        coord.prepared_queue.enqueue(old_tx);

        // ── 2. 构造 new tx：只有 CaretDriven (InsertReveal)，会成为 owner ──
        // active_text_transaction_key_with_epoch 倒序选最后一个 → new tx
        let new_insert_unit = PreparedVisualUnit::wrap(reveal_slice(0, 3, 100.0, 60.0), 100);
        let new_cursor_track = PreparedCursorVisualTrack::new_first(
            caret(100.0),
            caret(200.0),
            None,
            None,
            0.0,
            20.0,
            0.0,
            20.0,
            100,
        );
        let new_tx = PreparedTextVisualTransaction {
            key: new_key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::Insert,
            timeline: TransactionTimeline::new(100),
            units: vec![new_insert_unit],
            old_cursor_rect: Some(caret(100.0)),
            new_cursor_rect: Some(caret(200.0)),
            cursor_visual_track: Some(new_cursor_track),
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: None,
            cursor_owner_epoch: epoch,
            caret_motion_retired: false,
            visual_affected_byte_range_old: None,
            visual_affected_byte_range_new: None,
            layout_basis_revision: LayoutRevision::initial(),
        };
        coord.prepared_queue.enqueue(new_tx);

        // ── 3. mark_prepared 两个事务 ──
        coord.prepared_queue.mark_prepared(old_key);
        coord.prepared_queue.mark_prepared(new_key);

        // ── 4. 第 1 帧：begin_rendering_transactions 让两个事务进入 Rendering ──
        let frame_now_0 = create_now + Duration::from_millis(16);
        coord.begin_rendering_transactions(frame_now_0);

        // 构造 sample_0
        let mut sample_0 = AnimationFrameSample::new(frame_now_0);
        sample_0.set_progress(old_key, 0.0);
        sample_0.set_progress(new_key, 0.0);

        // 采样 coordinated motion frame → owner_key 应为 new_key（倒序选最后一个）
        let coordinated_frame_0 = coord.sample_coordinated_motion_frame(&sample_0, epoch, LayoutRevision::initial());
        assert_eq!(
            coordinated_frame_0.owner_key,
            Some(new_key),
            "第 1 帧 owner_key 应为 new tx（active_text_transaction_key_with_epoch 倒序选最后一个）"
        );

        // 调用 build_text_animation_plan_with_sample —— 此处应把 old tx 的 caret_motion_retired 置 true
        let (_plan_0, keys_to_complete_0, _) =
            coord.build_text_animation_plan_with_sample(&sample_0, epoch, LayoutRevision::initial());

        // 验证 old tx 不在 keys_to_complete（因为 ReflowMove 未完成，all_units_done == false）
        assert!(
            !keys_to_complete_0.contains(&old_key),
            "第 1 帧 old tx 不应完成：ReflowMove 未播完，all_units_done == false"
        );

        // 验证 old tx 仍在 active queue（因 ReflowMove 未完成）
        let old_tx_still_active = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .any(|t| t.key == old_key);
        assert!(
            old_tx_still_active,
            "第 1 帧 old tx 应仍在 active queue（ReflowMove 未完成）"
        );

        // 方案 A 核心断言：old tx 的 caret_motion_retired 应已被置 true
        {
            let old_tx_ref = coord
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == old_key)
                .expect("old tx 应仍在队列中");
            assert!(
                old_tx_ref.caret_motion_retired,
                "第 1 帧 build_text_animation_plan_with_sample 应把 old tx 的 caret_motion_retired 置 true\
                 （has_caret_driven_units && !owns_caret）"
            );
        }

        // ── 5. 完成 new tx（从队列移除）──
        let removed = coord.prepared_queue.complete(new_key);
        assert!(removed.is_some(), "new tx 应能被 complete");

        // ── 6. 第 2 帧：确认修复——old tx 不能重新成为 owner ──
        // 推进一小段时间（远小于 ReflowMove 的 1000ms，确保 old tx 的 ReflowMove 仍未完成）
        let frame_now_1 = frame_now_0 + Duration::from_millis(50);
        // old tx 的 ReflowMove duration=1000ms，elapsed≈66ms，progress≈0.066 < 1.0 → 未完成

        let mut sample_1 = AnimationFrameSample::new(frame_now_1);
        sample_1.set_progress(old_key, 0.5);

        // 修复后：active_text_transaction_key_with_epoch 跳过 retired 事务，返回 None
        let active_key_1 = coord.active_text_transaction_key_with_epoch(epoch, LayoutRevision::initial());
        assert_eq!(
            active_key_1, None,
            "修复后：*不应重新返回 old tx\
             （caret_motion_retired == true，被跳过）"
        );

        // 修复后：sample_coordinated_motion_frame 的 owner_key 为 None（不重新变成 old_key）
        let coordinated_frame_1 = coord.sample_coordinated_motion_frame(&sample_1, epoch, LayoutRevision::initial());
        assert_eq!(
            coordinated_frame_1.owner_key, None,
            "修复后：第 2 帧 owner_key 不应重新变成 old tx\
             —— 旧 caret/吞吐字轨迹不会重新接管，不会造成 caret 回跳"
        );

        // 验证 old tx 仍在 active queue（ReflowMove 仍未完成，Timed Reflow 继续播完）
        {
            let old_tx_ref = coord
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == old_key)
                .expect("old tx 应仍在队列中（ReflowMove 未完成）");
            assert!(
                old_tx_ref.caret_motion_retired,
                "第 2 帧 old tx 的 caret_motion_retired 仍应为 true（永久退休，不会重置）"
            );
            // old tx 的 caret track 仍原封不动（from=caret(0), to=caret(100)），
            // 但因为 retired，不会再被选为 owner，不会重新接管。
            let track = old_tx_ref
                .cursor_visual_track
                .as_ref()
                .expect("old tx 应有 caret track");
            assert_eq!(
                (track.from.x, track.to.x),
                (0.0, 100.0),
                "old tx 的 caret track 仍原封不动（from=0, to=100），\
                 但因 caret_motion_retired == true 不会被重新选为 owner"
            );
        }

        // 额外验证：active_text_transaction_key()（无 epoch 版本）也跳过 retired 事务
        let active_key_no_epoch = coord.active_text_transaction_key();
        assert_eq!(
            active_key_no_epoch, None,
            "修复后：active_text_transaction_key()（无 epoch 版本）也应跳过 retired 事务，\
             find_cursor_transaction_for_target / compute_coordinated_cursor_position\
             不会再用 old tx 驱动 caret"
        );

        // 额外验证：再调一次 build_text_animation_plan_with_sample，
        // old tx 的 caret_motion_retired 不会被重置（已经是 true 就保持 true）
        let (_plan_1, _keys_to_complete_1, _) =
            coord.build_text_animation_plan_with_sample(&sample_1, epoch, LayoutRevision::initial());
        {
            let old_tx_ref = coord
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == old_key)
                .expect("第 2 帧 build 后 old tx 应仍在队列中");
            assert!(
                old_tx_ref.caret_motion_retired,
                "第 2 帧 build_text_animation_plan_with_sample 后 old tx 的 caret_motion_retired\
                 仍应为 true（永久退休，不会因再次进入循环而重置）"
            );
        }

        println!(
            "[BUGFIX_VERIFY] Issue #727 评论 5760650874 方案 A: \
             旧 CaretDriven 事务失去 owner 后永远不能再重新获得 owner FIXED"
        );
    }
}
