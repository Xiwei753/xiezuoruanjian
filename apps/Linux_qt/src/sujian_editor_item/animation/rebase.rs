use std::time::Instant;

use writer_core::editor::OffsetMap;

use super::coordinator::LinuxEditorAnimationCoordinator;
use super::cursor_motion::sample_coordinated_cursor_rect_at;
use super::transaction_builder::emit_transaction_diagnostic;
use crate::editor::layout::compute_affected_paragraph_ranges;
use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};
use crate::sujian_editor_item::animation::{
    PreparedTextVisualTransaction, PreparedVisualUnit, RebaseFrame, VisualUnitTiming,
};
use crate::sujian_editor_item::animation_mode::AnimationMode;
use crate::sujian_editor_item::edit_motion::{
    diff_plain_text, CursorRect, EditorAnimationKind, PreparedEditMotion,
};
use crate::sujian_editor_item::editor_animation_debug_log;
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;

pub(crate) fn match_rebase_frames(
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

pub(crate) fn conflicting_units_are_untouched(
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
        // Issue #756: 按 timing 判断是否 caret-driven。
        // - CaretDriven（coordinated=true 吞吐字）：visible 从 caret track progress 推导。
        // - Timed（Reflow + coordinated=false typing-driven 吞吐字）：看 unit progress。
        let still_playing = if unit.timing.is_caret_driven() {
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
        } else {
            // Timed unit（Reflow / typing-driven 吞吐字）看 unit progress。
            unit.progress(now) < 1.0
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

#[derive(Clone, Debug)]
pub(crate) struct RebaseCaretHandoff {
    pub(crate) sampled: CursorRect,
    pub(crate) remaining_duration_ms: u64,
    pub(crate) sampled_visual_line_id: Option<usize>,
    pub(crate) sampled_line_top: f64,
    pub(crate) sampled_line_bottom: f64,
}

#[derive(Clone, Debug)]
pub(crate) enum PreparedRebaseHandoff {
    Insert {
        rebase_frames: Vec<RebaseFrame>,
        caret_handoff: Option<RebaseCaretHandoff>,
        range_start: usize,
        range_end: usize,
        insert_offset_map: OffsetMap,
        visual_affected_byte_range_old: Option<(usize, usize)>,
        visual_affected_byte_range_new: Option<(usize, usize)>,
    },
    Delete {
        rebase_frames: Vec<RebaseFrame>,
        caret_handoff: Option<RebaseCaretHandoff>,
        deleted_ranges: Vec<(usize, usize)>,
        delete_offset_map: OffsetMap,
        visual_affected_byte_range_old: Option<(usize, usize)>,
        visual_affected_byte_range_new: Option<(usize, usize)>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedCompositionCommitHandoff {
    pub(crate) rebase_frames: Vec<RebaseFrame>,
    pub(crate) caret_handoff: Option<RebaseCaretHandoff>,
    pub(crate) offset_map: OffsetMap,
    pub(crate) visual_affected_byte_range_old: Option<(usize, usize)>,
    pub(crate) visual_affected_byte_range_new: Option<(usize, usize)>,
}

pub(crate) fn collect_rebase_frame_for_unit_without_caret(
    unit: &PreparedVisualUnit,
    caret_track_progress: Option<f64>,
    caret_remaining_ms: u64,
    now: Instant,
) -> Option<RebaseFrame> {
    // Issue #756: 按 timing 判断 visible_fraction 推导方式。
    // - CaretDriven（coordinated=true 吞吐字）：从 caret track progress 推导。
    // - Timed（Reflow + coordinated=false typing-driven 吞吐字）：从自己的时间线算。
    let visible_fraction = if unit.timing.is_caret_driven() {
        // Issue #727 约束 2+3: CaretDriven unit 的 visible 从 caret track progress 推导。
        // visible = start_fraction + (target - start) * ease_out_quad(progress)
        let progress = caret_track_progress.unwrap_or(0.0);
        let eased = AnimatedSlice::ease_out_quad(progress);
        let start = unit.timing.start_fraction();
        let target = unit.timing.target_fraction();
        start + (target - start) * eased
    } else {
        unit.current_visible_fraction(now)
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

impl LinuxEditorAnimationCoordinator {
    pub(crate) fn take_rebase_frames(
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

    pub(crate) fn prepare_rebase_handoff_for_edit(
        &mut self,
        vt: &PreparedEditMotion,
        typing_animation_enabled: bool,
        smooth_cursor_enabled: bool,
        coordinated_animation_enabled: bool,
        is_scrolling: bool,
        is_loading: bool,
        is_applying_format: bool,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        cursor_owner_epoch: u64,
        now: Instant,
    ) -> Option<PreparedRebaseHandoff> {
        // Issue #756: 删除把"两个独立开关同时开启"等价成"协同动画"的逻辑。
        // - coordinated=true 时：走协同路径，文字与光标绑死，要求有效 caret motion。
        // - coordinated=false 时：typing_animation_enabled 只决定文字动画
        //   （Reflow + InsertReveal/DeleteConceal），smooth_cursor_enabled 只决定光标动画
        //   （caret motion track）。两者互相独立，同时为 true 不等于协同。
        let text_animation_enabled = coordinated_animation_enabled || typing_animation_enabled;
        let caret_animation_enabled = coordinated_animation_enabled || smooth_cursor_enabled;
        if (!text_animation_enabled && !caret_animation_enabled)
            || is_scrolling
            || is_loading
            || is_applying_format
        {
            return None;
        }

        // Issue #756: valid_caret_motion_track 检查。
        // - coordinated=true 时：文字和光标绑死，必须有有效 caret motion，否则不创建事务
        //   （文字动画也不启动）。
        // - coordinated=false 时：缺少 caret motion 只意味着没有 caret track / 没有
        //   CaretDriven units（Issue #727 约束 5），文字动画（Reflow）照常播放。
        let valid_caret_motion_track = old_cursor_rect.is_some() && new_cursor_rect.is_some();
        if coordinated_animation_enabled && !valid_caret_motion_track {
            return None;
        }

        let mode = AnimationMode::from_context(is_scrolling, is_loading, is_applying_format);
        if !mode.should_create_transaction() {
            return None;
        }

        // Issue #738 评论 5796693007 问题1: 用 if-else 链而不是 match `EditorAnimationKind::Insert =>`，
        // 避免与 `process_transaction` 的测试锚点（`EditorAnimationKind::Insert/Delete/Cursor =>`）
        // 冲突。`process_transaction` 保留原内联 match 结构供 issue687/issue702 白盒测试定位。
        if vt.kind == EditorAnimationKind::Insert {
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
                // Issue #738 评论 5796693007 问题1: 用外层传入的统一 now 采样，
                // 旧事务还活着，采到的是真实当前帧（CaretDriven 还没被推到终态）。
                let (rebase_frames, caret_handoff) = self.take_rebase_frames(
                    &conflicting,
                    "rebased_by_insert",
                    now,
                    Some((&[(range_start, range_start)], &insert_offset_map)),
                    &vt.old_text,
                    cursor_owner_epoch,
                );
                return Some(PreparedRebaseHandoff::Insert {
                    rebase_frames,
                    caret_handoff,
                    range_start,
                    range_end,
                    insert_offset_map,
                    visual_affected_byte_range_old,
                    visual_affected_byte_range_new,
                });
            }
            None
        } else if vt.kind == EditorAnimationKind::Delete {
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
            // Issue #738 评论 5796693007 问题1: 用外层传入的统一 now 采样。
            let (rebase_frames, caret_handoff) = self.take_rebase_frames(
                &conflicting,
                "rebased_by_delete",
                now,
                Some((&deleted_ranges, &delete_offset_map)),
                &vt.old_text,
                cursor_owner_epoch,
            );
            Some(PreparedRebaseHandoff::Delete {
                rebase_frames,
                caret_handoff,
                deleted_ranges,
                delete_offset_map,
                visual_affected_byte_range_old,
                visual_affected_byte_range_new,
            })
        } else {
            // Issue #702: 纯光标移动不创建文字事务（Cursor 分支）。
            // 纯光标移动直接维护 CursorAnimationState（由 rendering.rs
            // update_cursor_visual_position → build_cursor_plan → apply_plan
            // 构造），用 Scene Graph 当前帧 frame_now 推进 from→to 动画，
            // 不再伪装成文字事务（units=空）。
            // 此分支不创建任何事务，返回 None。
            None
        }
    }
}

#[cfg(test)]
mod tests;
