//! Linux Qt 文字动画协调器。
//!
//! 主链：
//! ```text
//! Core EditorVisualTransaction
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

use writer_core::editor::{CursorRect, EditorAnimationKind, EditorVisualTransaction, OffsetMap};

use super::animated_slice::{AnimatedSlice, AnimatedSliceKind};
pub(crate) use super::animation_mode::AnimationMode;
pub(crate) use super::cursor_animation::{CursorAnimationPlan, CursorBlinkMode, CursorTransition};
use super::layout_revision::LayoutRevision;
use super::layout_snapshot::{EditorLayoutSnapshot, LineSnapshotId, SourceRect};
pub(crate) use super::render_plan::{
    CursorRenderState, PreeditRange, RenderPlan, SelectionPreeditPlan, SelectionRange,
    TextAnimationGlyphInfo, TextAnimationPlan,
};
use super::static_line_patch::StaticLinePatch;
use super::text_visual_transaction::PreparedVisualUnit;
use super::text_visual_transaction::{
    PreparedCursorVisualTrack, PreparedTextVisualTransaction, PreparedTransactionQueue,
    RebaseFrame, TextVisualOperationKind, TextVisualTransactionState, TransactionTimeline,
};
pub(crate) use super::transaction_key::VisualTransactionKey;

use crate::sujian_editor_item::editor_animation_debug_log;

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
fn conflicting_units_are_untouched(
    tx: &PreparedTextVisualTransaction,
    changed_old_ranges: &[(usize, usize)],
    offset_map: &OffsetMap,
    now: Instant,
) -> bool {
    let mut playing_units = 0usize;
    for unit in &tx.units {
        if unit.progress(now) >= 1.0 {
            continue;
        }
        playing_units += 1;
        let start = unit.slice.byte_start;
        let end = unit.slice.byte_end;
        if changed_old_ranges
            .iter()
            .any(|(cs, ce)| end > *cs && start < *ce)
        {
            return false;
        }
        // 半开区间语义：end 恰为映射条目末端也算完整落在同一区域内。
        // 逐端点查表会在"文本末尾追加"场景返回 None（end == old 长度），
        // 把还在播的单元误判成被影响。
        if offset_map.map_old_range_to_new(start, end) != Some((start, end)) {
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
#[derive(Clone, Debug)]
struct RebaseCaretHandoff {
    sampled: CursorRect,
    remaining_duration_ms: u64,
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
fn build_cursor_visual_track(
    old_cursor_rect: Option<&CursorRect>,
    new_cursor_rect: Option<&CursorRect>,
    handoff: Option<RebaseCaretHandoff>,
    tx_duration_ms: u64,
) -> Option<PreparedCursorVisualTrack> {
    let to = new_cursor_rect?;
    match handoff {
        Some(h) => Some(PreparedCursorVisualTrack {
            from: h.sampled,
            to: to.clone(),
            started_at: None,
            duration_ms: h.remaining_duration_ms,
            pause_start: None,
        }),
        None => {
            let from = old_cursor_rect?;
            Some(PreparedCursorVisualTrack::new_first(
                from.clone(),
                to.clone(),
                tx_duration_ms,
            ))
        }
    }
}

/// Issue #690 评论 5680276931 + 5681206040: 采样旧事务在 `now` 时刻真正显示的
/// coordinated cursor rect。
///
/// 在 `take_rebase_frames` 取消旧事务之前调用，把结果作为新事务纯 reflow 光标动画的
/// 视觉起点（`cursor_visual_track.from`）。采样逻辑与 `compute_coordinated_cursor_position`
/// 的边界选择完全一致，按评论四种场景：
/// - InsertReveal：取这一帧 reveal 边界（frame.x + frame.w）。
/// - Backspace DeleteConceal（conceal_from_left）：取这一帧 conceal 边界（frame.x + frame.w）。
/// - forward Delete（conceal_from_right）：取当前固定 cursor rect（new_rect）。
/// - 纯 reflow（无上述 glyph）：直接 sample `cursor_visual_track`（自带 started_at/duration_ms），
///   不再借任何文字 unit 的 progress，也不再回头使用逻辑 `old_cursor_rect`。
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

    // Issue #690 评论 5681206040: 纯 reflow 光标直接 sample caret track，
    // 不再借第一个 reflow unit 的 progress。track 自带 started_at/duration_ms。
    // 没有 caret track 时（首次事务未经过 rebase），回退到 old/new cursor rect
    // 按事务 progress 插值——与 compute_coordinated_cursor_position 的 fallback 一致。
    let sample_from_track = |track: &PreparedCursorVisualTrack| -> (f64, f64) {
        let r = track.sampled_rect(now);
        (r.x, r.top)
    };

    // 首次事务没有 caret track 时的 fallback：按事务 progress 插值 old/new cursor rect。
    // 与 compute_coordinated_cursor_position 的 sample_reflow fallback 保持一致。
    let sample_reflow_fallback = || -> (f64, f64) {
        let progress = tx.progress(now);
        let eased = AnimatedSlice::ease_out_quad(progress);
        let x = old_rect.x + (new_rect.x - old_rect.x) * eased;
        let y = old_rect.top + (new_rect.top - old_rect.top) * eased;
        (x, y)
    };

    let (cx, cy) = match op {
        TextVisualOperationKind::Insert => {
            let mut rightmost_x: Option<f64> = None;
            let mut cursor_y = new_rect.top;
            for unit in &tx.units {
                if unit.slice.kind != AnimatedSliceKind::InsertReveal {
                    continue;
                }
                let visible = unit.current_visible_fraction(now);
                let frame = unit.slice.compute_frame(visible);
                let edge_x = frame.x + frame.w;
                rightmost_x = Some(match rightmost_x {
                    Some(prev) => prev.max(edge_x),
                    None => edge_x,
                });
                cursor_y = frame.y;
            }
            match rightmost_x {
                Some(x) => (x, cursor_y),
                None => {
                    // 纯 reflow（无 InsertReveal glyph 当边界）：直接 sample caret track。
                    match tx.cursor_visual_track.as_ref() {
                        Some(track) => sample_from_track(track),
                        None => sample_reflow_fallback(),
                    }
                }
            }
        }
        TextVisualOperationKind::Delete => {
            let mut has_conceal_from_right = false;
            let mut conceal_edge: Option<f64> = None;
            let mut cursor_y = new_rect.top;
            for unit in &tx.units {
                if unit.slice.kind != AnimatedSliceKind::DeleteConceal {
                    continue;
                }
                let visible = unit.current_visible_fraction(now);
                let frame = unit.slice.compute_frame(visible);
                if unit.slice.conceal_from_left {
                    let edge = frame.x + frame.w;
                    conceal_edge = Some(match conceal_edge {
                        Some(prev) => prev.min(edge),
                        None => edge,
                    });
                    cursor_y = frame.y;
                } else {
                    has_conceal_from_right = true;
                }
            }
            if let Some(x) = conceal_edge {
                (x, cursor_y)
            } else if has_conceal_from_right {
                (new_rect.x, new_rect.top)
            } else {
                // 跨行 reflow 等没有可直接当边界的 glyph：直接 sample caret track。
                match tx.cursor_visual_track.as_ref() {
                    Some(track) => sample_from_track(track),
                    None => sample_reflow_fallback(),
                }
            }
        }
        _ => {
            // CompositionUpdate / Commit / Cursor 纯 reflow：直接 sample caret track。
            match tx.cursor_visual_track.as_ref() {
                Some(track) => sample_from_track(track),
                None => sample_reflow_fallback(),
            }
        }
    };

    Some(CursorRect {
        x: cx,
        top: cy,
        bottom: cy + h,
        baseline_y: new_rect.baseline_y,
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

/// Issue #658 评论 5630181473 问题 3: 一个 reflow run — 由 byte range 重叠连边
/// 形成的 connected component。
///
/// 包含至少一个 old cluster 和一个 new cluster。run 内的所有成员共享同一动画：
/// - 1 old + 1 new + range 完全对应 + shaping 相同 → geometry 变了才 reflow_move
/// - 1 old + 1 new + shaping 不同 → 一对一 crossfade
/// - 其他情况（1→N / N→1 / N→M）→ old 每个淡出一次，new 每个淡入一次
#[derive(Clone, Debug)]
struct ReflowRun {
    old: Vec<ReflowClusterRef>,
    new: Vec<ReflowClusterRef>,
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
/// `(slices, static_patches)`：InsertReveal 动画切片和 insert 级静态行补丁。
fn build_insert_reveal_slices(
    key: VisualTransactionKey,
    new_snapshot: &EditorLayoutSnapshot,
    inserted_range: (usize, usize),
) -> (Vec<AnimatedSlice>, Vec<StaticLinePatch>) {
    let mut slices = Vec::new();
    let mut managed_new_clusters: Vec<(usize, usize, SourceRect)> = Vec::new();
    let (range_start, range_end) = inserted_range;

    for (line_idx, new_line) in new_snapshot.line_snapshots.iter().enumerate() {
        for (cluster_idx, new_cluster) in new_line.clusters.iter().enumerate() {
            // 只处理落在 inserted_range 内的 cluster
            if new_cluster.byte_start >= range_start && new_cluster.byte_end <= range_end {
                let new_sr = new_cluster.source_rect.clone();
                let new_doc = new_line.source_rect_to_document_rect(&new_sr);
                slices.push(AnimatedSlice::insert_reveal(
                    key,
                    new_line.id,
                    new_sr.clone(),
                    new_doc,
                    0.0,
                    0.0,
                    new_cluster.byte_start,
                    new_cluster.byte_end,
                    Some(new_cluster.shaping_identity.clone()),
                ));
                managed_new_clusters.push((line_idx, cluster_idx, new_sr));
            }
        }
    }

    // 生成 StaticLinePatches：Insert 的新字 sourceRect 必须在静态层隐藏到 Reveal 完成。
    let mut patches_by_line: std::collections::HashMap<usize, Vec<SourceRect>> =
        std::collections::HashMap::new();
    for (line_idx, _cluster_idx, sr) in &managed_new_clusters {
        patches_by_line
            .entry(*line_idx)
            .or_default()
            .push(sr.clone());
    }
    let mut static_patches = Vec::new();
    for (line_idx, hidden_rects) in patches_by_line {
        let new_line = &new_snapshot.line_snapshots[line_idx];
        static_patches.push(StaticLinePatch::insert_patch(
            new_line.id,
            hidden_rects,
            Vec::new(),
            new_line.byte_start,
            new_line.byte_end,
        ));
    }

    let slices = merge_adjacent_slices(slices);
    (slices, static_patches)
}

/// 按 Core 给出的 deleted_range 从 old_snapshot 显式生成 DeleteConceal 切片。
///
/// 只接 `vt.deleted_range + old_snapshot + new_cursor_rect`。按明确删除范围从
/// old snapshot 取纹理，直接生成 `DeleteConceal`。删除后的 canonical new text
/// 可以立即作为背景，旧字只由 overlay 吞掉，因此不生成 StaticLinePatch。
///
/// # 参数
/// - `key`：事务键。
/// - `old_snapshot`：旧布局快照。
/// - `deleted_range`：Core 给出的删除范围 (byte_start, byte_end)。
/// - `new_cursor_rect`：新光标矩形，用于决定吞字方向（conceal_from_left）。
///
/// # 返回
/// `slices`：DeleteConceal 动画切片。
fn build_delete_conceal_slices(
    key: VisualTransactionKey,
    old_snapshot: &EditorLayoutSnapshot,
    deleted_range: (usize, usize),
    new_cursor_rect: Option<&CursorRect>,
) -> Vec<AnimatedSlice> {
    let mut slices = Vec::new();
    let (range_start, range_end) = deleted_range;
    let new_cx = new_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
    let new_cy = new_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);

    for old_line in &old_snapshot.line_snapshots {
        for old_cluster in &old_line.clusters {
            // 只处理落在 deleted_range 内的 cluster
            if old_cluster.byte_start >= range_start && old_cluster.byte_end <= range_end {
                let old_sr = old_cluster.source_rect.clone();
                let old_doc = old_line.source_rect_to_document_rect(&old_sr);
                // 按新光标落在被删文字哪一侧决定保留左段还是右段。
                // 光标靠近右端 → 保留左段（conceal_from_left=true，Backspace 场景）；
                // 光标靠近左端 → 保留右段（conceal_from_left=false，Delete 键场景）。
                let left = old_doc.x;
                let right = old_doc.x + old_doc.w;
                let conceal_from_left = (new_cx - right).abs() <= (new_cx - left).abs();
                slices.push(AnimatedSlice::delete_conceal(
                    key,
                    old_line.id,
                    old_sr,
                    old_doc,
                    new_cx,
                    new_cy,
                    old_cluster.byte_start,
                    old_cluster.byte_end,
                    Some(old_cluster.shaping_identity.clone()),
                    conceal_from_left,
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
/// 两阶段算法：
/// 1. 建关系：遍历所有未 excluded 的 old/new cluster，用 OffsetMap range mapping
///    变到同一逻辑 byte 坐标，只要逻辑范围有重叠就连边，对二分图求 connected components。
/// 2. 按 run 分类：每个 component 按 old/new 成员数量和 shaping 一致性决定动画类型。
///    真正没有任何映射边的 new cluster 才是 insert_reveal；
///    真正没有任何映射边的 old cluster 才是 delete_conceal。
///
/// # 参数
/// - `excluded_old_ranges`：已被 insert/delete 动画接管的 old byte range，跳过不处理。
/// - `excluded_new_ranges`：已被 insert/delete 动画接管的 new byte range，跳过不处理。
/// - `old_cursor_rect`：用于 reflow 中检测到的 insert_reveal 的起始位置。
/// - `new_cursor_rect`：用于 reflow 中检测到的 delete_conceal 的收缩目标。
///
/// # 返回
/// `(slices, static_patches)`：动画切片和 cluster 级静态行补丁。
fn build_cluster_reflow_slices(
    key: VisualTransactionKey,
    old_snapshot: &EditorLayoutSnapshot,
    new_snapshot: &EditorLayoutSnapshot,
    offset_map: &OffsetMap,
    excluded_old_ranges: &[(usize, usize)],
    excluded_new_ranges: &[(usize, usize)],
    old_cursor_rect: Option<&CursorRect>,
    new_cursor_rect: Option<&CursorRect>,
) -> (Vec<AnimatedSlice>, Vec<StaticLinePatch>) {
    let mut slices = Vec::new();
    let mut static_patches = Vec::new();

    // Issue #687: old_cx/old_cy/new_cx/new_cy 不再需要——changed range 由
    // build_insert_reveal_slices / build_delete_conceal_slices 显式拥有，
    // reflow 只处理 unchanged material。
    let _ = (old_cursor_rect, new_cursor_rect);

    // ── 阶段 1：收集所有未 excluded 的 old/new cluster refs ──
    // Issue #658 评论 5630181473 问题 3: 被 excluded 的 old cluster 仍保留在
    // old_refs 中（标记 excluded=true），不参与边构建，但最终在阶段 4 中作为
    // "纯 old" run 生成 delete_conceal。被 excluded 的 new cluster 直接跳过。
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

    // ── 阶段 2：构建二分图边（byte range 重叠）──
    // 被 excluded 的 old cluster 不参与连边，最终作为 "纯 old" run 生成 delete_conceal。
    // Union-Find: 0..old_refs.len() 为 old 节点, old_refs.len().. 为 new 节点
    let n_old = old_refs.len();
    let n_new = new_refs.len();
    let n = n_old + n_new;
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], x: usize) -> usize {
        if parent[x] != x {
            parent[x] = find(parent, parent[x]);
        }
        parent[x]
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[ra] = rb;
        }
    }

    // 连边条件：old cluster 的 byte range 与 new cluster 的 byte range 有逻辑重叠
    // 被 excluded 的 old cluster 跳过（不连边）
    for (oi, oref) in old_refs.iter().enumerate() {
        if old_excluded_flags[oi] {
            continue;
        }
        for (ni, nref) in new_refs.iter().enumerate() {
            // 将 new cluster 的 byte range 映射到 old 坐标系
            let mapped_old_range = offset_map.map_new_range_to_old(nref.byte_start, nref.byte_end);

            let overlaps = if let Some((mos, moe)) = mapped_old_range {
                // 严格半开区间重叠：[a_start, a_end) ∩ [b_start, b_end) ≠ ∅
                // 即 a_start < b_end && b_start < a_end。
                // 共享端点不算重叠（[0,1) 和 [1,2) 只是相邻，不连边）。
                oref.byte_start < moe && mos < oref.byte_end
            } else {
                // new cluster 跨越映射边界 — 逐端点回退检查
                let start_mapped = offset_map.map_new_to_old(nref.byte_start);
                let last_byte = if nref.byte_end > 0 {
                    nref.byte_end - 1
                } else {
                    0
                };
                let end_mapped = offset_map.map_new_to_old(last_byte);
                if let (Some(ms), Some(me)) = (start_mapped, end_mapped) {
                    (ms >= oref.byte_start && ms < oref.byte_end)
                        || (me >= oref.byte_start && me < oref.byte_end)
                        || (ms <= oref.byte_start && me >= oref.byte_end)
                } else {
                    false
                }
            };

            if overlaps {
                union(&mut parent, oi, n_old + ni);
            }
        }
    }

    // ── 阶段 3：提取 connected components → ReflowRuns ──
    let mut component_map: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::new();
    let mut runs: Vec<ReflowRun> = Vec::new();

    for oi in 0..n_old {
        let root = find(&mut parent, oi);
        let comp_idx = *component_map.entry(root).or_insert_with(|| {
            let idx = runs.len();
            runs.push(ReflowRun {
                old: Vec::new(),
                new: Vec::new(),
            });
            idx
        });
        runs[comp_idx].old.push(old_refs[oi].clone());
    }
    for ni in 0..n_new {
        let root = find(&mut parent, n_old + ni);
        let comp_idx = *component_map.entry(root).or_insert_with(|| {
            let idx = runs.len();
            runs.push(ReflowRun {
                old: Vec::new(),
                new: Vec::new(),
            });
            idx
        });
        runs[comp_idx].new.push(new_refs[ni].clone());
    }

    // ── 阶段 4：按 run 分类生成动画切片 ──
    // 跟踪已被 run 接管的 new cluster，用于生成 StaticLinePatch
    let mut run_managed_new_clusters: Vec<(usize, usize, SourceRect)> = Vec::new();

    for run in &runs {
        if run.old.is_empty() && run.new.is_empty() {
            continue;
        }

        let run_old = &run.old;
        let run_new = &run.new;

        // Issue #687: 纯 new（无 old 对应）和纯 old（无 new 对应）的 run 不再由
        // reflow 推断 InsertReveal / DeleteConceal。changed range 所有权由
        // build_insert_reveal_slices / build_delete_conceal_slices 显式拥有。
        // reflow 只处理 unchanged material（ReflowMove / ReflowCrossFade）。
        // 纯 new / 纯 old run 理论上不应出现（changed range 已被 excluded），
        // 若因边界情况出现则直接跳过，不生成切片。
        if run_old.is_empty() || run_new.is_empty() {
            continue;
        }

        // 1 old + 1 new：精确匹配
        if run_old.len() == 1 && run_new.len() == 1 {
            let oref = &run_old[0];
            let nref = &run_new[0];
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
                    slices.push(AnimatedSlice::reflow_move(
                        key,
                        old_line.id,
                        old_sr,
                        old_doc,
                        new_line.id,
                        new_sr.clone(),
                        new_doc,
                        new_cluster.byte_start,
                        new_cluster.byte_end,
                        Some(old_cluster.shaping_identity.clone()),
                    ));
                    run_managed_new_clusters.push((nref.line_idx, nref.cluster_idx, new_sr));
                }
            } else {
                // shaping 改变：old 淡出 + new 淡入
                slices.push(AnimatedSlice::reflow_crossfade_old(
                    key,
                    old_line.id,
                    old_sr,
                    old_doc.clone(),
                    new_doc.clone(),
                    new_cluster.byte_start,
                    new_cluster.byte_end,
                ));
                slices.push(AnimatedSlice::reflow_crossfade_new(
                    key,
                    new_line.id,
                    new_sr.clone(),
                    old_doc,
                    new_doc,
                    new_cluster.byte_start,
                    new_cluster.byte_end,
                ));
                run_managed_new_clusters.push((nref.line_idx, nref.cluster_idx, new_sr));
            }
            continue;
        }

        // N→M（多 old ↔ 多 new）：无法可靠一一对应时，每个成员独立动画。
        // old 每个在自己的 old_doc 原位 fade-out（from == to == old_doc，不移动只淡出）。
        // new 每个在自己的 new_doc 原位 fade-in（from == to == new_doc，不移动只淡入）。
        // 不再拿第一个 cluster 当 run 锚点——避免整组文字往一个 cluster 上聚拢。
        for oref in run_old {
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
            ));
        }

        for nref in run_new {
            let new_line = &new_snapshot.line_snapshots[nref.line_idx];
            let new_cluster = &new_line.clusters[nref.cluster_idx];
            let new_sr = new_cluster.source_rect.clone();
            let new_doc = new_line.source_rect_to_document_rect(&new_sr);

            slices.push(AnimatedSlice::reflow_crossfade_new(
                key,
                new_line.id,
                new_sr.clone(),
                new_doc.clone(),
                new_doc,
                new_cluster.byte_start,
                new_cluster.byte_end,
            ));
            run_managed_new_clusters.push((nref.line_idx, nref.cluster_idx, new_sr));
        }
    }

    // ── 阶段 5：生成 StaticLinePatches ──
    // 按 line_idx 分组，只为有被接管 cluster 的行生成 patch
    let mut patches_by_line: std::collections::HashMap<usize, Vec<SourceRect>> =
        std::collections::HashMap::new();
    for (line_idx, _cluster_idx, sr) in &run_managed_new_clusters {
        patches_by_line
            .entry(*line_idx)
            .or_default()
            .push(sr.clone());
    }
    for (line_idx, hidden_rects) in patches_by_line {
        let new_line = &new_snapshot.line_snapshots[line_idx];
        static_patches.push(StaticLinePatch::reflow_patch(
            new_line.id,
            hidden_rects,
            Vec::new(),
            new_line.byte_start,
            new_line.byte_end,
        ));
    }

    let slices = merge_adjacent_slices(slices);
    (slices, static_patches)
}

/// 合并相邻同类型、同方向、同快照的动画切片为 run，避免一个字一个 slice。
///
/// 合并条件（全部满足才合并）：
/// 1. 相同 `kind`（AnimatedSliceKind）
/// 2. 相同 `snapshot_id`（来自同一行快照）
/// 3. 相邻 byte range：`slice[i].byte_end == slice[i+1].byte_start`
/// 4. 同方向：
///    - InsertReveal：`from_document_rect` 的 y 相同（同一行吐字）
///    - DeleteConceal：`from_document_rect` 的 y 相同（同一行吞字）且 `conceal_from_left` 相同
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
            // 同一行吞字且同方向：from_document_rect 的 y 相同，conceal_from_left 相同
            (a.from_document_rect.y - b.from_document_rect.y).abs() < 0.5
                && a.conceal_from_left == b.conceal_from_left
        }
        AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
            // 移动向量相同：dx = to.x - from.x, dy = to.y - from.y
            let a_dx = a.to_document_rect.x - a.from_document_rect.x;
            let a_dy = a.to_document_rect.y - a.from_document_rect.y;
            let b_dx = b.to_document_rect.x - b.from_document_rect.x;
            let b_dy = b.to_document_rect.y - b.from_document_rect.y;
            (a_dx - b_dx).abs() < 0.5 && (a_dy - b_dy).abs() < 0.5
        }
    }
}

/// 合并两个 slice 为一个 run。
fn merge_two(a: &AnimatedSlice, b: &AnimatedSlice) -> AnimatedSlice {
    AnimatedSlice {
        kind: a.kind,
        snapshot_id: a.snapshot_id,
        source_rect: bounding_box(&a.source_rect, &b.source_rect),
        from_document_rect: bounding_box(&a.from_document_rect, &b.from_document_rect),
        to_document_rect: bounding_box(&a.to_document_rect, &b.to_document_rect),
        opacity_from: a.opacity_from,
        opacity_to: a.opacity_to,
        scale_from: a.scale_from,
        scale_to: a.scale_to,
        byte_start: a.byte_start.min(b.byte_start),
        byte_end: a.byte_end.max(b.byte_end),
        shaping_identity: a.shaping_identity.clone(),
        conceal_from_left: a.conceal_from_left,
        start_fraction: a.start_fraction.min(b.start_fraction),
    }
}

/// 计算两个 SourceRect 的 bounding box（取最小 x/y 和最大 right/bottom）。
fn bounding_box(a: &SourceRect, b: &SourceRect) -> SourceRect {
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
    layout_revision: LayoutRevision,
    /// 打字/预输入动画时长（毫秒）。本地生成的事务不来自 core 的
    /// `EditorVisualTransaction`，因此在此持有该视觉配置，与 core 把
    /// `duration_ms` 放进 visual transaction 结构体的设计方向一致。
    typing_animation_duration_ms: u32,
    /// 光标平滑移动动画时长（毫秒）。
    cursor_animation_duration_ms: u32,
}

impl LinuxEditorAnimationCoordinator {
    pub fn new() -> Self {
        Self {
            next_key_id: 1,
            prepared_queue: PreparedTransactionQueue::new(),
            layout_revision: LayoutRevision::initial(),
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
    fn take_rebase_frames(
        &mut self,
        conflicting: Option<VisualTransactionKey>,
        reason: &str,
        now: Instant,
        preserve: Option<(&[(usize, usize)], &OffsetMap)>,
    ) -> (Vec<RebaseFrame>, Option<RebaseCaretHandoff>) {
        let Some(old_key) = conflicting else {
            return (Vec::new(), None);
        };
        let untouched = match preserve {
            Some((changed_old_ranges, offset_map)) => self
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|tx| tx.key == old_key)
                .map(|tx| conflicting_units_are_untouched(tx, changed_old_ranges, offset_map, now))
                .unwrap_or(false),
            None => false,
        };
        if untouched {
            if let Some(tx) = self
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|tx| tx.key == old_key)
            {
                emit_transaction_diagnostic(tx, "editor.anim.keep", "units_untouched");
            }
            editor_animation_debug_log(&format!(
                "anim_keep: key={:?} reason={} (units outside changed range keep playing)",
                old_key, reason,
            ));
            return (Vec::new(), None);
        }
        let (frames, caret_handoff) = match self
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|tx| tx.key == old_key)
        {
            Some(tx) => {
                let frames = tx.collect_rebase_frames(now);
                // Issue #690 评论 5680276931 + 5681206040: 在取消旧事务之前，用同一个
                // now 采样旧事务这一帧正在屏幕上显示的 coordinated cursor rect，并取旧
                // caret track 的剩余时长，一起带给新事务。新事务用 sampled caret 当
                // caret track.from，用剩余时长当 caret track.duration_ms，不再借任何
                // 文字 unit 的 progress。
                let sampled_cursor = sample_coordinated_cursor_rect_at(tx, now);
                let caret_handoff = match (sampled_cursor, tx.cursor_visual_track.as_ref()) {
                    (Some(sampled), Some(track)) => Some(RebaseCaretHandoff {
                        sampled,
                        remaining_duration_ms: track.remaining_duration_ms(now).max(1),
                    }),
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
                        })
                    }
                    (None, _) => None,
                };
                emit_transaction_diagnostic(tx, "editor.anim.rebase", reason);
                (frames, caret_handoff)
            }
            None => (Vec::new(), None),
        };
        self.prepared_queue.cancel(old_key, "rebased");
        editor_animation_debug_log(&format!(
            "anim_rebase: old_key={:?} reason={} carried_units={} carried_cursor={}",
            old_key,
            reason,
            frames.len(),
            caret_handoff.is_some(),
        ));
        (frames, caret_handoff)
    }

    pub fn process_transaction(
        &mut self,
        vt: &EditorVisualTransaction,
        typing_animation_enabled: bool,
        is_scrolling: bool,
        is_loading: bool,
        is_applying_format: bool,
        is_applying_settings: bool,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        cursor_owner_epoch: u64,
    ) -> Option<VisualTransactionKey> {
        if !typing_animation_enabled
            || is_scrolling
            || is_loading
            || is_applying_format
            || is_applying_settings
        {
            return None;
        }

        let mode = AnimationMode::from_core(vt.animation_mode);
        if !mode.should_create_transaction() {
            return None;
        }

        let new_revision = LayoutRevision::next();

        match vt.kind {
            EditorAnimationKind::Insert => {
                if let Some(range) = vt.inserted_range {
                    let range_start = range.start().value();
                    let range_end = range.end().value();
                    let insert_offset_map = OffsetMap::build(&vt.old_text, &vt.new_text);
                    let conflicting = self
                        .prepared_queue
                        .find_conflicting_transaction(range_start, range_end);
                    // 纯插入在 old 文档里就是 range_start 这一个位置点。
                    let now = Instant::now();
                    let (rebase_frames, caret_handoff) = self.take_rebase_frames(
                        conflicting,
                        "rebased_by_insert",
                        now,
                        Some((&[(range_start, range_start)], &insert_offset_map)),
                    );

                    let key = self.alloc_key();
                    let mut slices = Vec::new();
                    let mut static_patches = Vec::new();

                    // Issue #687: Insert 事务先生成显式 InsertReveal，再调用 reflow builder；
                    // reflow 必须排除 inserted_range。changed range 由 Core 显式拥有。
                    let inserted_range_tuple = (range_start, range_end);
                    let (reveal_slices, reveal_patches) =
                        build_insert_reveal_slices(key, new_snapshot, inserted_range_tuple);
                    slices.extend(reveal_slices);
                    static_patches.extend(reveal_patches);

                    let (reflow_slices, reflow_patches) = build_cluster_reflow_slices(
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
                    static_patches.extend(reflow_patches);

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
                        caret_handoff,
                        vt.duration_ms,
                    );
                    let prepared = PreparedTextVisualTransaction {
                        key,
                        state: TextVisualTransactionState::Pending,
                        operation_kind: TextVisualOperationKind::Insert,
                        timeline: TransactionTimeline::new(vt.duration_ms),
                        units,
                        static_patches,
                        old_cursor_rect,
                        new_cursor_rect,
                        cursor_visual_track,
                        cancel_reason: None,
                        texture_prepared: false,
                        old_snapshot: Some(old_snapshot.clone()),
                        new_snapshot: Some(new_snapshot.clone()),
                        cursor_owner_epoch,
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

                    self.layout_revision = new_revision;
                    self.prepared_queue.enqueue(prepared);

                    return Some(key);
                }
            }
            EditorAnimationKind::Delete => {
                let deleted_ranges: Vec<(usize, usize)> = if let Some(range) = vt.deleted_range {
                    vec![(range.start().value(), range.end().value())]
                } else {
                    let changes = writer_core::editor::diff_plain_text(&vt.old_text, &vt.new_text);
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
                let conflicting = self
                    .prepared_queue
                    .find_conflicting_transaction(rebase_byte_start, rebase_byte_end);
                let now = Instant::now();
                let (rebase_frames, caret_handoff) = self.take_rebase_frames(
                    conflicting,
                    "rebased_by_delete",
                    now,
                    Some((&deleted_ranges, &delete_offset_map)),
                );

                let key = self.alloc_key();
                let new_revision = LayoutRevision::next();

                let mut slices = Vec::new();
                let mut static_patches = Vec::new();

                // Issue #687: Delete 事务先生成显式 DeleteConceal，再调用 reflow builder；
                // reflow 必须排除 deleted_range。changed range 由 Core 显式拥有。
                // 对每个 deleted range 生成显式 DeleteConceal 切片。
                for &(d_start, d_end) in &deleted_ranges {
                    let conceal_slices = build_delete_conceal_slices(
                        key,
                        old_snapshot,
                        (d_start, d_end),
                        new_cursor_rect.as_ref(),
                    );
                    slices.extend(conceal_slices);
                }

                let (reflow_slices, reflow_patches) = build_cluster_reflow_slices(
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
                static_patches.extend(reflow_patches);

                let mut units: Vec<PreparedVisualUnit> = slices
                    .into_iter()
                    .map(|s| PreparedVisualUnit::wrap(s, vt.duration_ms))
                    .collect();
                match_rebase_frames(&rebase_frames, &mut units, &delete_offset_map);

                // Issue #690 评论 5681206040 + 5682867529: 构建 caret track（不传 now，等 Rendering 再启动）。
                let cursor_visual_track = build_cursor_visual_track(
                    old_cursor_rect.as_ref(),
                    new_cursor_rect.as_ref(),
                    caret_handoff,
                    vt.duration_ms,
                );
                let prepared = PreparedTextVisualTransaction {
                    key,
                    state: TextVisualTransactionState::Pending,
                    operation_kind: TextVisualOperationKind::Delete,
                    timeline: TransactionTimeline::new(vt.duration_ms),
                    units,
                    static_patches,
                    old_cursor_rect,
                    new_cursor_rect,
                    cursor_visual_track,
                    cancel_reason: None,
                    texture_prepared: false,
                    old_snapshot: Some(old_snapshot.clone()),
                    new_snapshot: Some(new_snapshot.clone()),
                    cursor_owner_epoch,
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

                self.layout_revision = new_revision;
                self.prepared_queue.enqueue(prepared);

                return Some(key);
            }
            EditorAnimationKind::Cursor => {
                // Issue #702: 删除"纯光标移动创建空 Cursor 文字事务"的结构。
                // 纯光标移动直接维护 CursorAnimationState（由 rendering.rs
                // update_cursor_visual_position → build_cursor_plan → apply_plan
                // 构造），用 Scene Graph 当前帧 frame_now 推进 from→to 动画，
                // 不再伪装成文字事务（units=空, static_patches=空）。
                // 此分支不再创建任何事务，返回 None。
                return None;
            }
        }
        None
    }

    pub fn handle_composition_update(
        &mut self,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        composition_byte_start: usize,
        composition_byte_end: usize,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        cursor_owner_epoch: u64,
    ) -> Option<VisualTransactionKey> {
        let conflicting = self
            .prepared_queue
            .find_conflicting_transaction(composition_byte_start, composition_byte_end);
        // 预输入文本整体被替换，旧单元必然失效：不做保留判断。
        let now = Instant::now();
        let (rebase_frames, caret_handoff) =
            self.take_rebase_frames(conflicting, "rebased_by_composition_update", now, None);

        let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);

        let key = self.alloc_key();
        let new_revision = LayoutRevision::next();

        let mut slices = Vec::new();
        let mut static_patches = Vec::new();

        // Issue #687: IME 组合更新也显式拥有 changed range。
        // 用 diff_plain_text 找到 inserted/deleted range，显式生成 InsertReveal/DeleteConceal，
        // reflow 只处理 unchanged material。
        let comp_changes = writer_core::editor::diff_plain_text(
            &old_snapshot.virtual_text,
            &new_snapshot.virtual_text,
        );
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
            let (reveal_slices, reveal_patches) =
                build_insert_reveal_slices(key, new_snapshot, (i_start, i_end));
            slices.extend(reveal_slices);
            static_patches.extend(reveal_patches);
        }
        for &(d_start, d_end) in &comp_deleted_ranges {
            let conceal_slices = build_delete_conceal_slices(
                key,
                old_snapshot,
                (d_start, d_end),
                new_cursor_rect.as_ref(),
            );
            slices.extend(conceal_slices);
        }

        let (reflow_slices, reflow_patches) = build_cluster_reflow_slices(
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
        static_patches.extend(reflow_patches);

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
            caret_handoff,
            unit_duration_ms,
        );
        let prepared = PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::CompositionUpdate,
            timeline: TransactionTimeline::new(unit_duration_ms),
            units,
            static_patches,
            old_cursor_rect,
            new_cursor_rect,
            cursor_visual_track,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: Some(old_snapshot.clone()),
            new_snapshot: Some(new_snapshot.clone()),
            cursor_owner_epoch,
        };

        // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
        emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
        editor_animation_debug_log(&format!(
            "anim_event: key={:?} op=CompositionUpdate unit_kinds={:?} carried_rebase={}",
            key,
            unit_kind_labels(&prepared.units),
            rebase_frames.len(),
        ));

        self.layout_revision = new_revision;
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
        cursor_owner_epoch: u64,
    ) -> Option<VisualTransactionKey> {
        let conflict_start = committed_replace_start.min(preedit_byte_start);
        let conflict_end = committed_replace_end.max(preedit_byte_end);
        let conflicting = self
            .prepared_queue
            .find_conflicting_transaction(conflict_start, conflict_end);
        // 预输入提交/取消同样整体替换 preedit 区间，不做保留判断。
        let now = Instant::now();
        let (rebase_frames, caret_handoff) =
            self.take_rebase_frames(conflicting, "rebased_by_composition_commit", now, None);

        let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);

        let key = self.alloc_key();
        let new_revision = LayoutRevision::next();

        let mut slices = Vec::new();
        let mut static_patches = Vec::new();

        if !is_commit {
            // Issue #687: cancel 时显式生成 DeleteConceal for preedit 范围的 old cluster，
            // reflow 只处理 unchanged material。changed range 由显式函数拥有。
            let cancel_deleted_range = (preedit_byte_start, preedit_byte_end);
            let conceal_slices = build_delete_conceal_slices(
                key,
                old_snapshot,
                cancel_deleted_range,
                new_cursor_rect.as_ref(),
            );
            slices.extend(conceal_slices);

            let cancel_excluded_old: [(usize, usize); 1] = [cancel_deleted_range];
            let (reflow_slices, reflow_patches) = build_cluster_reflow_slices(
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
            static_patches.extend(reflow_patches);
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
                                // 走 delete_conceal，按 old rect 两侧与新光标距离
                                // 决定收进方向：靠近右端 → 保留左段（Backspace），
                                // 靠近左端 → 保留右段（Delete 键）。
                                let left = from_doc.x;
                                let right = from_doc.x + from_doc.w;
                                let conceal_from_left =
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
                                    conceal_from_left,
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
                                            slices.push(AnimatedSlice::reflow_crossfade_old(
                                                key,
                                                old_line.id,
                                                old_sr,
                                                old_doc,
                                                new_doc,
                                                old_cluster.byte_start,
                                                old_cluster.byte_end,
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
                                slices.push(AnimatedSlice::insert_reveal(
                                    key,
                                    new_line.id,
                                    new_sr.clone(),
                                    to_doc,
                                    insert_cx,
                                    insert_cy,
                                    new_cluster.byte_start,
                                    new_cluster.byte_end,
                                    Some(new_cluster.shaping_identity.clone()),
                                ));
                                static_patches.push(StaticLinePatch::insert_patch(
                                    new_line.id,
                                    vec![new_sr],
                                    Vec::new(),
                                    new_cluster.byte_start,
                                    new_cluster.byte_end,
                                ));
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
                                        slices.push(AnimatedSlice::reflow_crossfade_new(
                                            key,
                                            new_line.id,
                                            new_sr.clone(),
                                            old_doc,
                                            new_doc,
                                            new_cluster.byte_start,
                                            new_cluster.byte_end,
                                        ));
                                        static_patches.push(StaticLinePatch::insert_patch(
                                            new_line.id,
                                            vec![new_sr],
                                            Vec::new(),
                                            new_cluster.byte_start,
                                            new_cluster.byte_end,
                                        ));
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
                                            slices.push(AnimatedSlice::reflow_move(
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
                                            ));
                                            static_patches.push(StaticLinePatch::insert_patch(
                                                new_line.id,
                                                vec![new_sr],
                                                Vec::new(),
                                                new_cluster.byte_start,
                                                new_cluster.byte_end,
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let (reflow_slices, reflow_patches) = build_cluster_reflow_slices(
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
                static_patches.extend(reflow_patches);
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
            caret_handoff,
            unit_duration_ms,
        );
        let prepared = PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::CompositionCommitOrCancel,
            timeline: TransactionTimeline::new(unit_duration_ms),
            units,
            static_patches,
            old_cursor_rect,
            new_cursor_rect,
            cursor_visual_track,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: Some(old_snapshot.clone()),
            new_snapshot: Some(new_snapshot.clone()),
            cursor_owner_epoch,
        };

        // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
        emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
        editor_animation_debug_log(&format!(
            "anim_event: key={:?} op=CompositionCommitOrCancel unit_kinds={:?} carried_rebase={}",
            key,
            unit_kind_labels(&prepared.units),
            rebase_frames.len(),
        ));

        self.layout_revision = new_revision;
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
            return Some(tx.key);
        }
        None
    }

    /// Issue #705 评论 5717380886: 返回当前活动的正文编辑事务的 key，
    /// 且其 `cursor_owner_epoch == current_cursor_epoch`。
    ///
    /// epoch 不一致时返回 None——文字事务继续播自己的 glyph/reflow
    /// （不清除事务），但不再驱动 caret。
    ///
    /// 本方法供 `build_cursor_plan` 内部判断"正文协同是否活跃（epoch 一致）"用。
    /// `find_cursor_transaction_for_target` / `compute_coordinated_cursor_position`
    /// 直接调 `active_text_transaction_key()` 后手动检查 epoch，以保留结构守卫测试
    /// 期望的 `self.active_text_transaction_key()` 调用形式。
    fn active_text_transaction_key_with_epoch(
        &self,
        current_cursor_epoch: u64,
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
    /// 是否等于 `current_cursor_epoch`。epoch 不一致时不返回该事务（文字事务继续
    /// 播自己的 glyph/reflow，但不再驱动 caret）。
    pub(crate) fn find_cursor_transaction_for_target(
        &self,
        target_x: f64,
        target_y: f64,
        _target_h: f64,
        current_cursor_epoch: u64,
    ) -> Option<(VisualTransactionKey, Option<CursorRect>, Option<CursorRect>)> {
        // 领域2：优先按事务身份绑定——存在活动正文事务时直接返回。
        if let Some(key) = self.active_text_transaction_key() {
            if let Some(tx) = self
                .prepared_queue
                .active_transactions()
                .iter()
                .find(|t| t.key == key)
            {
                // Issue #705 评论 5717380886: cursor_owner_epoch 检查。
                // epoch 不一致时跳过——文字事务继续播自己的 glyph/reflow，
                // 但不再驱动 caret。
                if tx.cursor_owner_epoch != current_cursor_epoch {
                    // epoch 不一致，fall through 到 CursorOnly 查找逻辑。
                } else {
                    return Some((
                        tx.key,
                        tx.old_cursor_rect.clone(),
                        tx.new_cursor_rect.clone(),
                    ));
                }
            }
        }

        // 没有正文事务（或 epoch 不一致）时走 CursorOnly 查找逻辑（按 target x/y 匹配）。
        // Issue #705 评论 5717380886: CursorOnly 查找也跳过 epoch 不一致的事务，
        // 因为这些事务的 new_cursor_rect 已不再代表当前 caret 目标。
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
        coordinated_enabled: bool,
        scroll_y: f64,
        old_scroll_y: f64,
        old_visible: bool,
        old_blink_visible: bool,
        old_visual_x: f64,
        old_visual_y: f64,
        force_snap_next: bool,
        cursor_animation: Option<&super::rendering::CursorAnimationState>,
        cursor_owner_epoch: u64,
    ) -> CursorAnimationPlan {
        let in_viewport = cursor_y + cursor_h > 0.0 && cursor_y < viewport_height;
        let should_be_visible = editor_enabled && !has_selection && in_viewport && !is_scrolling;

        // Issue #705 评论 5717380886: 区分两种"有活动正文事务"的判断：
        // - `has_active_for_blink`：不看 epoch，只要文字动画还在播就 suppress blink。
        // - `has_active_for_coordinated`：看 epoch，只有 epoch 一致的事务才驱动
        //   coordinated caret。epoch 不一致时文字事务继续播自己的 glyph/reflow，
        //   但不再驱动 caret，纯光标移动可走 Tween。
        let has_active_for_blink = self.has_active_text_transaction();
        let has_active_for_coordinated = self
            .active_text_transaction_key_with_epoch(cursor_owner_epoch)
            .is_some();
        // Issue #679 评论 5657313927: blink_mode 不再固化进 CursorAnimationPlan,
        // 由 tick_cursor_animation 每帧从 has_active_text_transaction() 实时计算。
        // Issue #702 评论 5708209114: has_active 覆盖所有正文事务类型
        // （Insert/Delete/CompositionUpdate/CompositionCommitOrCancel），
        // 不再用 has_active_insert()，避免 Delete 路径漏判导致双时间线分叉。
        // Issue #705 评论 5717380886: blink 用 has_active_for_blink（不看 epoch），
        // 文字动画还在播就 suppress blink。
        let _blink_mode = if coordinated_enabled && has_active_for_blink {
            CursorBlinkMode::Suppressed
        } else {
            CursorBlinkMode::Normal
        };

        let scroll_changed = (old_scroll_y - scroll_y).abs() > 0.01;

        let dy = (cursor_y - old_visual_y).abs();

        let cross_line_snap = dy > cursor_h * 3.0;

        // Issue #679 评论 5658087764 (1): force_snap_next 是一次性强制 Snap 标记，
        // 不再附加"距离够大才算"的条件；点击/滚动/选择/不可见/滚动变化都硬 Snap，
        // 不再被协调动画覆盖为 Tween。
        let hard_snap =
            force_snap_next || is_scrolling || is_selecting || !old_visible || scroll_changed;

        // Issue #702 评论 5707449688 问题 2: 纯光标移动彻底和文字事务 key 解耦，
        // 不再用 driver_key.is_some() 决定 can_tween。纯光标只要满足 smooth cursor
        // 条件，就直接从当前 visual_x/visual_y 建自己的 Tween，由 CursorAnimationState
        // 自己的 timeline 推进。
        // Issue #702: 纯光标移动 Tween 的 duration_ms，供 CursorAnimationState 自己的 timeline。
        let tween_duration_ms = u64::from(smooth_cursor_duration_ms);

        let transition = if !should_be_visible || hard_snap {
            CursorTransition::Snap
        } else if !smooth_cursor_enabled || cross_line_snap {
            // Issue #702 评论 5707770318: 正文事务活跃时，光标位置只由
            // compute_coordinated_cursor_position 驱动（正文协同），不应再开
            // CursorAnimationState 独立 timeline。返回 Snap 让 apply_plan 清除
            // animation，不创建独立 timeline。只有没有正文事务时才走纯光标 Tween。
            // 此分支（!smooth_cursor_enabled || cross_line_snap）原本就对非协同
            // 情况返回 Snap；现在协调情况也返回 Snap，因此统一返回 Snap。
            CursorTransition::Snap
        } else if let Some(anim) = cursor_animation {
            if (anim.target_x - cursor_x).abs() > 0.01 || (anim.target_y - cursor_y).abs() > 0.01 {
                // Issue #702 评论 5707770318: 正文事务活跃时返回 Snap，
                // 不创建独立 CursorAnimationState timeline。
                // Issue #705 评论 5717380886: 用 has_active_for_coordinated（看 epoch），
                // epoch 不一致时纯光标移动可走 Tween。
                if coordinated_enabled && has_active_for_coordinated {
                    CursorTransition::Snap
                } else {
                    // Issue #702 评论 5707449688 问题 2: 纯光标 Tween 不再需要 driver_key，
                    // 直接从当前 anim 的 start 位置建 Tween。
                    CursorTransition::Tween {
                        old_rect: CursorRect {
                            x: anim.start_x,
                            top: anim.start_y,
                            bottom: anim.start_y + cursor_h,
                            baseline_y: anim.start_y + cursor_h * 0.8,
                        },
                        new_rect: CursorRect {
                            x: cursor_x,
                            top: cursor_y,
                            bottom: cursor_y + cursor_h,
                            baseline_y: cursor_y + cursor_h * 0.8,
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
            if coordinated_enabled && has_active_for_coordinated {
                CursorTransition::Snap
            } else {
                // Issue #702 评论 5707449688 问题 2: 纯光标 Tween 不再需要 driver_key，
                // 直接从当前 visual_x/visual_y 建 Tween。
                CursorTransition::Tween {
                    old_rect: CursorRect {
                        x: old_visual_x,
                        top: old_visual_y,
                        bottom: old_visual_y + cursor_h,
                        baseline_y: old_visual_y + cursor_h * 0.8,
                    },
                    new_rect: CursorRect {
                        x: cursor_x,
                        top: cursor_y,
                        bottom: cursor_y + cursor_h,
                        baseline_y: cursor_y + cursor_h * 0.8,
                    },
                    duration_ms: tween_duration_ms,
                }
            }
        } else {
            CursorTransition::Snap
        };

        let _ = (is_preediting, old_blink_visible);
        // Issue #702 评论 5707770318: old_cursor_rect/new_cursor_rect 不再用于
        // build_cursor_plan 的 Tween 构造（正文协同时返回 Snap，纯光标 Tween 从
        // anim.start_x/start_y 或 visual_x/visual_y 建）。保留参数以维持调用方契约。
        let _ = (old_cursor_rect, new_cursor_rect);

        CursorAnimationPlan {
            should_be_visible,
            transition,
            cursor_x,
            cursor_y,
            cursor_h,
        }
    }

    pub(crate) fn pause_all(&mut self) {
        for tx in self.prepared_queue.active_transactions_mut() {
            tx.pause();
        }
    }

    pub(crate) fn resume_all(&mut self) {
        for tx in self.prepared_queue.active_transactions_mut() {
            tx.resume();
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
        coordinated_enabled: bool,
        cursor_animation: Option<&super::rendering::CursorAnimationState>,
        cursor_owner_epoch: u64,
    ) -> RenderPlan {
        // Issue #690 评论 5675007226 步骤 1: 本帧统一采样一次，后续文字与光标 progress
        // 都从同一个 `AnimationFrameSample` 读取，消除 GUI tick 与 Scene Graph 渲染帧之间的偏差。
        let mut frame_sample = AnimationFrameSample::new(frame_now);
        for tx in self.prepared_queue.active_transactions() {
            if !matches!(
                tx.state,
                TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
            ) {
                frame_sample.set_progress(tx.key, tx.progress(frame_now));
            }
        }
        let (text_animation, keys_to_complete) =
            self.build_text_animation_plan_with_sample(&frame_sample);
        frame_context.keys_to_complete = keys_to_complete;
        let active_keys: Vec<VisualTransactionKey> = self
            .prepared_queue
            .active_transactions()
            .iter()
            .map(|t| t.key)
            .collect();
        frame_context.active_transaction_keys = active_keys;

        // Issue #658: 收集已准备好的 static_patches 供静态正文裁剪。
        // 只有 texture_prepared == true 的事务才允许静态层隐藏，
        // 避免纹理准备完成前出现空白帧。
        // 同时将 hidden_source_rects 通过 source_rect_to_document_rect()
        // 转换为 doc_hidden_rects，供 QSGClipNode 直接使用文档逻辑坐标。
        // Issue #679 评论 5657313927 (3e): 只允许 Prepared / Rendering / Paused
        // 的事务裁剪静态正文；Pending 无论 texture_prepared 是什么都不能隐藏正文，
        // 否则资源还没准备好就会出现空洞。
        let mut static_patches = Vec::new();
        for tx in self.prepared_queue.active_transactions() {
            if tx.texture_prepared
                && matches!(
                    tx.state,
                    TextVisualTransactionState::Prepared
                        | TextVisualTransactionState::Rendering
                        | TextVisualTransactionState::Paused
                )
            {
                for mut patch in tx.static_patches.iter().cloned() {
                    // 查找对应行快照，将 hidden_source_rects 转换为文档坐标
                    if !patch.hidden_source_rects.is_empty() && patch.doc_hidden_rects.is_empty() {
                        if let Some(ref new_snapshot) = tx.new_snapshot {
                            if let Some(line_snap) = new_snapshot
                                .line_snapshots
                                .iter()
                                .find(|ls| ls.id == patch.snapshot_id)
                            {
                                patch.doc_hidden_rects = patch
                                    .hidden_source_rects
                                    .iter()
                                    .map(|sr| line_snap.source_rect_to_document_rect(sr))
                                    .collect();
                            }
                        }
                    }
                    static_patches.push(patch);
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
        if coordinated_enabled {
            // Issue #705 评论 5717380886: 传入 cursor_owner_epoch。
            // epoch 不一致时 compute_coordinated_cursor_position 返回 None，
            // 文字事务继续播自己的 glyph/reflow（不清除事务），但不再驱动 caret，
            // 改走 CursorOnly/点击位置。
            // 原调用形式 self.compute_coordinated_cursor_position(&frame_sample)
            // 现增加 cursor_owner_epoch 参数。
            if let Some((cx, cy, ch)) =
                self.compute_coordinated_cursor_position(&frame_sample, cursor_owner_epoch)
            {
                // Issue #702 评论 5707770318: 正文协同光标位置已算出，
                // 把 cursor_sample_outcome 设为 Coordinated { x, y, h }，
                // 让 qquickitem_impl 同步 visual_x/visual_y/visual_h 到本帧
                // 屏幕真正画出的位置，但不启动 CursorAnimationState.started_at，
                // 不创建独立 timeline。正文光标只由 compute_coordinated_cursor_position 驱动。
                cursor_sample_outcome = super::render_plan::CursorSampleOutcome::Coordinated {
                    x: cx,
                    y: cy,
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
                    y: cy,
                    h: ch,
                    opacity,
                };
            } else if let Some(anim) = cursor_animation {
                // 无活跃文字事务但有 CursorOnly 动画：用同一份 frame_sample 采样光标位置。
                cursor_sample_outcome = self.sample_cursor_only_position(anim, &frame_sample);
                match cursor_sample_outcome {
                    super::render_plan::CursorSampleOutcome::Running(p) => {
                        let eased = super::rendering::ease_out_cubic(p);
                        cursor_render_state.x =
                            anim.start_x + (anim.target_x - anim.start_x) * eased;
                        cursor_render_state.y =
                            anim.start_y + (anim.target_y - anim.start_y) * eased;
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
        }

        // Issue #705: drawn_caret_rect 是本帧真正绘制出去的 caret rect。
        // 根据 cursor_sample_outcome 和最终 cursor_render_state 算出。
        // Coordinated → 协同位置;Running/Finished → cursor_render_state 已更新;
        // Idle → 当前 visual 位置。
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
            static_patches,
            cursor_sample_outcome,
            drawn_caret_rect,
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
    /// 替代原来的 `build_text_animation_plan()`（内部各自 `Instant::now()`）。每个视觉单元
    /// 拥有自己的 `started_at` / `duration_ms`，从自己的时间线计算 per-unit progress；
    /// `start_fraction` 由 rebase 决定（新单元为 0，被连续输入覆盖的单元从已显示比例继续）。
    fn build_text_animation_plan_with_sample(
        &mut self,
        sample: &AnimationFrameSample,
    ) -> (TextAnimationPlan, Vec<VisualTransactionKey>) {
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

            if tx.state == TextVisualTransactionState::Prepared {
                tx.state = TextVisualTransactionState::Rendering;
                if !tx.timeline.is_started() {
                    tx.timeline.mark_first_frame();
                }
                // Issue #690 评论 5675007226 步骤 3: 事务进入 Rendering 时，为每个视觉单元
                // 打上统一的起始时间；之后每个单元按自己的 duration_ms 独立计算 progress。
                for unit in &mut tx.units {
                    if unit.started_at.is_none() {
                        unit.started_at = Some(sample.frame_now);
                    }
                }
                // Issue #690 评论 5682867529: caret track 跟文字 unit 同一个 frame_now 启动，
                // 不再在事务创建时就开始计时。这样第一帧 text unit progress = 0 且
                // caret track progress = 0，文字和光标从同一屏幕帧起跑。
                if let Some(track) = tx.cursor_visual_track.as_mut() {
                    if track.started_at.is_none() {
                        track.started_at = Some(sample.frame_now);
                    }
                }
            }

            let all_units_done = if tx.units.is_empty() {
                sample.progress(tx.key) >= 1.0
            } else {
                tx.units.iter().all(|u| u.progress(sample.frame_now) >= 1.0)
            };

            if all_units_done {
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
                // 单元生命期 + 单元视觉窗口，唯一公式（与协同光标完全一致）。
                let visible = unit.current_visible_fraction(sample.frame_now);
                let frame = unit.slice.compute_frame(visible);
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

        (TextAnimationPlan { glyphs }, keys_to_complete)
    }

    /// Issue #690 评论 5675007226 步骤 2 + 5681206040: 协同光标直接计算最终屏幕位置。
    ///
    /// 光标严格跟随文字吞吐边界，不再在 old/new cursor rect 之间用 progress 插值：
    /// - InsertReveal：光标 x = 本帧所有 reveal 单元的最右可见边界（frame.x + frame.w）。
    /// - DeleteConceal (Backspace, conceal_from_left)：光标跟 frame.x + frame.w 往左走，
    ///   旧字正好被光标"吞掉"。
    /// - DeleteConceal (forward Delete, !conceal_from_left)：逻辑光标不移动，
    ///   固定在 new_cursor_rect.x。
    /// - Reflow / Cursor / Enter：直接 sample `cursor_visual_track`（自带
    ///   started_at/duration_ms），不再借任何文字 unit 的 progress。
    ///
    /// 返回 `(x, y, h)` 供 `build_render_plan_full` 直接写入 `CursorRenderState`。
    ///
    /// Issue #705 评论 5717380886: 增加 `current_cursor_epoch` 参数。epoch 不一致时
    /// 返回 None——文字事务继续播自己的 glyph/reflow（不清除事务），但不再驱动 caret。
    fn compute_coordinated_cursor_position(
        &self,
        sample: &AnimationFrameSample,
        current_cursor_epoch: u64,
    ) -> Option<(f64, f64, f64)> {
        // Issue #705 评论 5717380886: 传入 cursor_owner_epoch。
        // 调 active_text_transaction_key() 取活动事务后，检查其 cursor_owner_epoch
        // 是否等于 current_cursor_epoch。epoch 不一致时返回 None，
        // 文字事务继续播自己的 glyph/reflow，但不再驱动 caret。
        let key = self.active_text_transaction_key()?;
        let tx = self
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key)?;

        // Issue #705 评论 5717380886: cursor_owner_epoch 检查。
        // epoch 不一致时返回 None——文字事务继续播自己的 glyph/reflow，
        // 但不再驱动 caret。
        if tx.cursor_owner_epoch != current_cursor_epoch {
            return None;
        }

        let old_rect = tx.old_cursor_rect.as_ref()?;
        let new_rect = tx.new_cursor_rect.as_ref()?;
        let h = new_rect.bottom - new_rect.top;

        match tx.state {
            TextVisualTransactionState::Rendering | TextVisualTransactionState::Paused => {}
            _ => return None,
        }

        let op = tx.operation_kind;
        let frame_now = sample.frame_now;

        // Issue #690 评论 5681206040: 纯 reflow 光标直接 sample caret track，
        // 不再借第一个 reflow unit 的 progress。track 自带 started_at/duration_ms。
        // 没有 caret track 时（首次事务未经过 rebase，或 CursorOnly），回退到
        // old/new cursor rect 从事务 progress 插值——保持首次事务原语义。
        let sample_reflow = || -> Option<(f64, f64)> {
            match tx.cursor_visual_track.as_ref() {
                Some(track) => {
                    let r = track.sampled_rect(frame_now);
                    Some((r.x, r.top))
                }
                None => {
                    // 首次事务没有 caret track：用 old/new cursor rect 按事务 progress 插值。
                    let progress = tx.progress(frame_now);
                    let eased = AnimatedSlice::ease_out_quad(progress);
                    let x = old_rect.x + (new_rect.x - old_rect.x) * eased;
                    let y = old_rect.top + (new_rect.top - old_rect.top) * eased;
                    Some((x, y))
                }
            }
        };

        match op {
            TextVisualOperationKind::Insert => {
                let mut rightmost_x: Option<f64> = None;
                let mut cursor_y = new_rect.top;
                for unit in &tx.units {
                    if unit.slice.kind != AnimatedSliceKind::InsertReveal {
                        continue;
                    }
                    // 与文字帧同一个函数、同一个 frame_now：光标边界 == 本帧 reveal 边界。
                    let visible = unit.current_visible_fraction(frame_now);
                    let frame = unit.slice.compute_frame(visible);
                    let edge_x = frame.x + frame.w;
                    rightmost_x = Some(match rightmost_x {
                        Some(prev) => prev.max(edge_x),
                        None => edge_x,
                    });
                    cursor_y = frame.y;
                }
                match rightmost_x {
                    Some(x) => Some((x, cursor_y, h)),
                    None => {
                        // 纯 reflow（无 InsertReveal glyph 当边界）：直接 sample caret track。
                        let (x, y) = sample_reflow()?;
                        Some((x, y, h))
                    }
                }
            }
            TextVisualOperationKind::Delete => {
                let mut has_conceal_from_right = false;
                let mut conceal_edge: Option<f64> = None;
                let mut cursor_y = new_rect.top;
                // Issue #702: 记录 DeleteConceal unit 的可见进度，供 fallback
                // 让 caret track 跟随文字 unit 的同一帧基准，而非 caret track
                // 自己的 timeline，消除"光标先完成、旧字晚消失"的错拍。
                let mut delete_unit_progress: Option<f64> = None;

                for unit in &tx.units {
                    if unit.slice.kind != AnimatedSliceKind::DeleteConceal {
                        continue;
                    }
                    // 与文字帧同一个函数、同一个 frame_now：光标边界 == 本帧 conceal 边界。
                    let visible = unit.current_visible_fraction(frame_now);
                    let frame = unit.slice.compute_frame(visible);

                    if unit.slice.conceal_from_left {
                        let edge = frame.x + frame.w;
                        conceal_edge = Some(match conceal_edge {
                            Some(prev) => prev.min(edge),
                            None => edge,
                        });
                        cursor_y = frame.y;
                    } else {
                        has_conceal_from_right = true;
                    }
                    // 记算 unit 的 progress（与 visible 同一帧基准），供 fallback 使用。
                    let unit_progress = unit.progress(frame_now);
                    delete_unit_progress = Some(match delete_unit_progress {
                        Some(prev) => prev.min(unit_progress),
                        None => unit_progress,
                    });
                }

                if let Some(x) = conceal_edge {
                    // Backspace：光标带着旧字往左吞。
                    Some((x, cursor_y, h))
                } else if has_conceal_from_right {
                    // 前向 Delete：逻辑光标本来不移动，固定在 new_cursor_rect，
                    // 只让右侧文字向光标方向收掉。
                    Some((new_rect.x, new_rect.top, h))
                } else if let Some(unit_progress) = delete_unit_progress {
                    // Issue #702: 有 DeleteConceal unit 但没有可直接当边界的 glyph
                    // （跨行 reflow 等）。caret track 跟随文字 unit 的可见进度
                    // （同一帧基准），而非 caret track 自己的 timeline，消除错拍。
                    if let Some(track) = tx.cursor_visual_track.as_ref() {
                        let r = track.sampled_rect_at_progress(unit_progress);
                        Some((r.x, r.top, h))
                    } else {
                        let eased = AnimatedSlice::ease_out_quad(unit_progress);
                        let x = old_rect.x + (new_rect.x - old_rect.x) * eased;
                        let y = old_rect.top + (new_rect.top - old_rect.top) * eased;
                        Some((x, y, h))
                    }
                } else {
                    // 没有 DeleteConceal unit：直接 sample caret track。
                    let (x, y) = sample_reflow()?;
                    Some((x, y, h))
                }
            }
            _ => {
                // CompositionUpdate/Commit/Cursor 纯 reflow：直接 sample caret track。
                let (x, y) = sample_reflow()?;
                Some((x, y, h))
            }
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
        EditorLayoutSnapshot::new(layout_snapshot, vec![line], None, CaretAffinity::Downstream)
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
            0,
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
            !tx.static_patches.is_empty(),
            "Move slices should have corresponding StaticLinePatch::insert_patch"
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
            0,
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
            !tx.static_patches.is_empty(),
            "Crossfade new should have StaticLinePatch::insert_patch to prevent double-draw"
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
            0,
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
            3,
            10,
            true,
            false,
            3,
            5,
            3,
            10,
            None,
            None,
            0,
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
            0,
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
        let key = coord.handle_composition_update(&old_snapshot, &new_snapshot, 0, 3, None, None, 0);
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
    /// `conceal_from_left = true` 表示保留左段（Backspace，光标在文字右侧）；
    /// `conceal_from_left = false` 表示保留右段（Delete 键，光标在文字左侧）。
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
        // 新光标靠近右端 (x=39, right=40) → Backspace → conceal_from_left=true
        let new_cursor = CursorRect {
            x: 39.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        };
        // Issue #687: changed range 由 build_delete_conceal_slices 显式拥有。
        // old cluster [0,3) 是被删除的范围。
        let slices = build_delete_conceal_slices(key, &old_snapshot, (0, 3), Some(&new_cursor));
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
            delete_slices[0].conceal_from_left,
            "cursor near right (x=39, right=40) should be Backspace → conceal_from_left=true"
        );
    }

    #[test]
    fn test_delete_conceal_direction_cursor_near_left_is_delete() {
        let (old_snapshot, _new_snapshot, _offset_map) = make_delete_direction_snapshots();
        let key = VisualTransactionKey::new(1, 1);
        // 新光标靠近左端 (x=11, left=10) → Delete 键 → conceal_from_left=false
        let new_cursor = CursorRect {
            x: 11.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        };
        // Issue #687: changed range 由 build_delete_conceal_slices 显式拥有。
        // old cluster [0,3) 是被删除的范围。
        let slices = build_delete_conceal_slices(key, &old_snapshot, (0, 3), Some(&new_cursor));
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
            !delete_slices[0].conceal_from_left,
            "cursor near left (x=11, left=10) should be Delete → conceal_from_left=false"
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
        )
    }

    fn conceal_slice(
        byte_start: usize,
        byte_end: usize,
        x: f64,
        w: f64,
        conceal_from_left: bool,
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
            conceal_from_left,
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
        unit.started_at = Some(now - Duration::from_millis(elapsed_ms));
        unit
    }

    /// 手工装配一笔处于 Rendering 的正文事务。事务 timeline 从 `tx_elapsed_ms` 起算，
    /// 与单元各自的 `elapsed_ms` 故意取不同值，用来验证两者不再互相顶替。
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
        PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Rendering,
            operation_kind,
            timeline,
            units,
            static_patches: Vec::new(),
            old_cursor_rect: Some(old_cursor),
            new_cursor_rect: Some(new_cursor),
            cursor_visual_track: None,
            cancel_reason: None,
            texture_prepared: true,
            old_snapshot: None,
            new_snapshot: None,
            // Issue #705 评论 5717380886: 测试辅助函数默认 epoch=0，
            // 与 CursorController::new() 的初始 epoch 一致。
            cursor_owner_epoch: 0,
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
                // 单元自己的时间线：50/100ms → progress 0.5 → 可见比例 ease_out_quad(0.5)=0.75
                elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now),
                // 已播完的单元是稳定终态，不再交棒
                elapsed_unit(reveal_slice(3, 6, 160.0, 60.0), 500, 100, now),
            ],
            caret(100.0),
            caret(220.0),
            now,
            10,
        );
        // 事务级 progress = 0.1（eased 0.19）。旧实现按它一刀切采集，会把吐到 75% 的字
        // 交棒成 19%，视觉上跳回一半。
        tx.timeline.rendering_started_at = Some(now - Duration::from_millis(10));

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
        let visible_fraction = old_unit.current_visible_fraction(now);
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
        assert!(
            (unit.start_fraction - 0.75).abs() < 1e-6,
            "Reveal 单元交棒后应从已显示比例继续，got {}",
            unit.start_fraction
        );
        assert_eq!(
            unit.duration_ms, 50,
            "单元时长用剩余时长，不被新事务 wrap 的默认时长覆盖"
        );
        // Issue #690 评论 5679744253 问题 1: retarget 时从当前帧重新起段，
        // started_at 留 None，等进入 Rendering 再启动，progress 从 0 开始。
        // Issue #690 评论 5683759796: 原来写 Some(sampled_at) 会让 rebased 文字 unit
        // 从旧事务交棒时刻提前计时，与等 Rendering 才启动的 caret track 错拍；
        // 改成 None 后跟 fresh unit、caret track 一样由 build_text_animation_plan_with_sample
        // 在 Prepared→Rendering 时用同一个 sample.frame_now 启动。
        assert!(
            unit.started_at.is_none(),
            "Issue #690 评论 5683759796: rebase 后 started_at 应为 None（等 Rendering 再启动），\
             got {:?}",
            unit.started_at
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

        let (frames, _) = coord.take_rebase_frames(Some(old_key), "rebased_by_insert", now, None);
        assert_eq!(frames.len(), 1, "旧事务的未播完单元要全部交棒");
        assert!((frames[0].visible_fraction - 0.75).abs() < 1e-6);
        assert!(
            coord.prepared_queue.is_empty(),
            "交棒后旧事务必须取消，snapshot/纹理资源归新事务所有"
        );
        let (no_frames, _) = coord.take_rebase_frames(None, "rebased_by_insert", now, None);
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
            Some(old_key),
            "rebased_by_insert",
            now,
            Some((&[(3, 3)], &offset_map)),
        );

        assert!(frames.is_empty(), "未覆盖的单元不该交棒，旧事务自己播完");
        assert_eq!(
            coord.prepared_queue.active_transactions().len(),
            1,
            "旧事务要留在队列里，继续持有自己的 snapshot 与静态隐藏区"
        );
        let unit = &coord.prepared_queue.active_transactions()[0].units[0];
        assert!(
            unit.start_fraction.abs() < 1e-9,
            "保留的单元起点不能被改写，got {}",
            unit.start_fraction
        );
        assert!(
            (unit.current_visible_fraction(now) - 0.75).abs() < 1e-6,
            "保留的单元沿自己的 started_at 继续，不因新事务 id 归零重播"
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
            Some(old_key),
            "rebased_by_delete",
            now,
            Some((&[(2, 3)], &offset_map)),
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
            Some(old_key),
            "rebased_by_insert",
            now,
            Some((&[(0, 0)], &offset_map)),
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
            Some(old_key),
            "rebased_by_insert",
            now,
            Some((&[(3, 3)], &offset_map)),
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
            true,
            None,
            0,
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
            false,
            None,
            0,
        );

        assert!(
            (plan.cursor.x - 1234.5).abs() < 1e-6,
            "关闭协同动画时走 CursorOnly 自己的平滑曲线，位置不由事务改写"
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

        // 同一条 ease-out 曲线镜像到 1→0：eased(0.5)=0.75 → 还剩 0.25。
        let half = elapsed_unit(conceal_slice(0, 3, 100.0, 60.0, true), 50, 100, now);
        let visible = half.current_visible_fraction(now);
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

        let done = elapsed_unit(conceal_slice(0, 3, 100.0, 60.0, true), 200, 100, now);
        let frame = done.slice.compute_frame(done.current_visible_fraction(now));
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
            true,
            None,
            0,
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
            true,
            None,
            0,
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
            true,
            None,
            0,
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
            coord.take_rebase_frames(Some(old_key), "rebased_by_enter", now, None);
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
            true,
            None,
            0,
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
            coord.take_rebase_frames(Some(key_b), "rebased_by_second_input", now, None);
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
            coord.take_rebase_frames(Some(key_a), "first_rebase", now, None);
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
            coord.take_rebase_frames(Some(key_b), "second_rebase", now_after_b, None);
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
        let cursor_visual_track =
            PreparedCursorVisualTrack::new_first(caret(0.0), caret(200.0), 200);
        let tx = PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::Insert,
            timeline: TransactionTimeline::new(200),
            units: vec![reflow_unit],
            static_patches: Vec::new(),
            old_cursor_rect: Some(caret(0.0)),
            new_cursor_rect: Some(caret(200.0)),
            cursor_visual_track: Some(cursor_visual_track),
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: None,
            cursor_owner_epoch: 0,
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
                tx_ref.units[0].started_at.is_none(),
                "事务创建时文字 unit started_at 应为 None"
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
        let mut sample_0 = AnimationFrameSample::new(frame_now_0);
        sample_0.set_progress(key, 0.0);
        let (plan_0, _) = coord.build_text_animation_plan_with_sample(&sample_0);

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
        let (plan_1, _) = coord.build_text_animation_plan_with_sample(&sample_1);

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

        // rebased text unit: start_fraction = 0.75（旧 unit 当前可见比例），
        // duration_ms = 50（剩余时长）。
        // Issue #690 评论 5683759796 关键断言: started_at 必须是 None（不是 Some(sampled_at)），
        // 这样才不会从旧事务交棒时刻提前计时。
        assert!(
            new_units[0].started_at.is_none(),
            "Issue #690 评论 5683759796: rebase 后文字 unit started_at 应为 None\
             （等 Rendering 再启动），got {:?}",
            new_units[0].started_at
        );
        assert_eq!(
            new_units[0].duration_ms, 50,
            "rebase 后文字 unit duration_ms 应为剩余时长 50"
        );

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
            static_patches: Vec::new(),
            old_cursor_rect: Some(caret(100.0)),
            new_cursor_rect: Some(caret(220.0)),
            cursor_visual_track: Some(new_caret_track),
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: None,
            cursor_owner_epoch: 0,
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
                tx_ref.units[0].started_at.is_none(),
                "Prepared 阶段 rebased text unit started_at 应为 None，got {:?}",
                tx_ref.units[0].started_at
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
        let mut sample_0 = AnimationFrameSample::new(frame_now_0);
        sample_0.set_progress(new_key, 0.0);
        let (plan_0, _) = coord.build_text_animation_plan_with_sample(&sample_0);

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
            assert_eq!(
                tx_ref.units[0].started_at,
                Some(frame_now_0),
                "Issue #690 评论 5683759796: 进入 Rendering 后 rebased text unit started_at \
                 应等于第一帧 frame_now（由 build_text_animation_plan_with_sample 设置），\
                 got {:?}",
                tx_ref.units[0].started_at
            );
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

        // ── 10. 推进 25ms，断言二者一起前进到 0.5 ──
        // rebased text unit duration_ms = 50（剩余时长），caret track duration_ms = 50（剩余时长）。
        // 推进 25ms 后二者 progress 都应到 0.5。
        let frame_now_mid = frame_now_0 + Duration::from_millis(25);
        let mut sample_mid = AnimationFrameSample::new(frame_now_mid);
        sample_mid.set_progress(new_key, 0.5);
        let (_plan_mid, _) = coord.build_text_animation_plan_with_sample(&sample_mid);
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
            assert!(
                (track_progress - 0.5).abs() < 1e-9,
                "推进 25ms 后 caret track progress 应为 0.5（25/50），got {}",
                track_progress
            );
            assert!(
                (unit_progress - 0.5).abs() < 1e-9,
                "推进 25ms 后 rebased text unit progress 应为 0.5（25/50），got {}",
                unit_progress
            );
            assert!(
                (track_progress - unit_progress).abs() < 1e-9,
                "推进 25ms 后 caret track 和 rebased text unit 的 progress 应完全相同，\
                 got track={} unit={}",
                track_progress,
                unit_progress
            );
        }

        // ── 11. 推进到 50ms，断言二者一起到 1.0 ──
        let frame_now_1 = frame_now_0 + Duration::from_millis(50);
        let mut sample_1 = AnimationFrameSample::new(frame_now_1);
        sample_1.set_progress(new_key, 1.0);
        let (_plan_1, _) = coord.build_text_animation_plan_with_sample(&sample_1);
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
            assert!(
                (track_progress - 1.0).abs() < 1e-9,
                "推进 50ms 后 caret track progress 应为 1.0（50/50），got {}",
                track_progress
            );
            assert!(
                (unit_progress - 1.0).abs() < 1e-9,
                "推进 50ms 后 rebased text unit progress 应为 1.0（50/50），got {}",
                unit_progress
            );
            assert!(
                (track_progress - unit_progress).abs() < 1e-9,
                "推进 50ms 后 caret track 和 rebased text unit 的 progress 应完全相同，\
                 got track={} unit={}",
                track_progress,
                unit_progress
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
            static_patches: Vec::new(),
            old_cursor_rect: None,
            new_cursor_rect: None,
            cursor_visual_track: None,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: None,
            cursor_owner_epoch: 0,
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
}
