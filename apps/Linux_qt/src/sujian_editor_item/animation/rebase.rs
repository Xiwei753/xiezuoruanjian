use std::time::Instant;

use writer_core::editor::OffsetMap;

use super::coordinator::LinuxEditorAnimationCoordinator;
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
use crate::sujian_editor_item::editor_animation_transaction_skipped_event;
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;
use crate::sujian_editor_item::AnimationSkipFields;

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
    // Reveal/Conceal 的存活判断：CaretTrack unit 从 cursor track 当前帧取边界，
    // Timed unit 从自己的时间线取 progress。
    let caret_track_progress = tx
        .cursor_visual_track
        .as_ref()
        .map(|track| track.progress(now));
    for unit in &tx.units {
        // Issue #819 评论 5956495850: 协同 InsertReveal/DeleteConceal 的空间边界直接来自
        // 同一笔 cursor track 的当前帧。非协同时才是独立文字 timeline + 独立 smooth cursor。
        // is_caret_driven() 对未 retired 的 CaretTrack unit 返回 true，走 cursor track 边界分支；
        // Timed unit 和已 retired 的 CaretTrack unit 走 else 分支。
        let still_playing = if unit.timing.is_caret_driven() {
            // 协同吞吐字（未 retired 的 CaretTrack）逐帧边界来自 cursor track。
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
    /// Issue #819 评论 5968931455: 旧 caret track 从 sampled frame 之后的剩余吞吐阶段。
    /// 新事务构造 route 时合成 `旧 carried route 剩余段 -> 当前新 route`，
    /// 让 carried CaretTrack unit 消费自己原来的剩余 route，而不是下一笔编辑的 route。
    pub(crate) remaining_ingest_segments:
        Vec<super::transaction::types::CaretTrackSegment>,
    /// Issue #819 评论 5968931455: 旧事务的 stage_id，carried route 剩余段保留这个 id。
    pub(crate) stage_id: super::transaction::types::IngestStageId,
}

/// Issue #819 评论 5956495850 第 4 节：rebase 交棒的完整视觉状态。
///
/// 合成旧的 `(Vec<RebaseFrame>, Option<RebaseCaretHandoff>)` 二元组，
/// 由 `take_rebase_frames` 从 `SampledEditVisualState` 构造。
/// coordinator 的所有 handoff 只接受本类型，不再分别传 frames 和 caret。
///
/// Issue #819 评论 5968240881 问题 1：把 `carried_slices: Vec<SampledSliceFrame>`
/// 替换成 `carried_units: Vec<CarriedVisualUnit>`。`SampledSliceFrame` 只携带
/// 几何，没有完整 ingest 元数据（caret anchor / line mask / ingest line ord /
/// snapshot id /吞吐边界驱动），新事务接管后吞吐边界会从零开始，文字闪一下。
/// `CarriedVisualUnit` 携带完整 `AnimatedSlice` + sampled frame + timing，
/// transaction builder 真正 `units.push()` 到新事务，让旧 slice 继续由新
/// track/新 handoff 收口直到真正终态。
///
/// `carried_snapshot_ids` 显式列出 carried unit 引用的 line snapshot id，
/// 供 `prepare_edit_motion` 在新事务创建之前做 texture retain 时把这些 ids
/// 也算进 active 集合，避免旧事务 cancel 后纹理在新事务接管前被回收。
#[derive(Clone, Debug, Default)]
pub(crate) struct RebaseVisualState {
    pub(crate) rebase_frames: Vec<RebaseFrame>,
    pub(crate) caret_handoff: Option<RebaseCaretHandoff>,
    /// Issue #819 评论 5968240881 问题 1：未匹配到新事务已有 unit、但本帧仍未到达
    /// 终态的旧视觉单元（携带完整 `AnimatedSlice` ingest 元数据 + sampled frame +
    /// timing）。transaction builder 把匹配不上的 carried unit 真正 `units.push()`
    /// 到新事务，不让屏幕上还没吞完的旧 slice 因为新正文里没有那个字就被直接扔掉。
    pub(crate) carried_units: Vec<super::frame_state::CarriedVisualUnit>,
    /// Issue #819 评论 5968240881 问题 1：carried unit 引用的 line snapshot id，
    /// 供 `prepare_edit_motion` 在新事务创建之前做 texture retain 时把这些 ids
    /// 也算进 active 集合，避免旧事务 cancel 后纹理在新事务接管前被回收。
    pub(crate) carried_snapshot_ids:
        Vec<crate::sujian_editor_item::layout_snapshot::LineSnapshotId>,
}

#[derive(Clone, Debug)]
pub(crate) enum PreparedRebaseHandoff {
    Insert {
        visual_state: RebaseVisualState,
        range_start: usize,
        range_end: usize,
        insert_offset_map: OffsetMap,
        visual_affected_byte_range_old: Option<(usize, usize)>,
        visual_affected_byte_range_new: Option<(usize, usize)>,
    },
    Delete {
        visual_state: RebaseVisualState,
        deleted_ranges: Vec<(usize, usize)>,
        delete_offset_map: OffsetMap,
        visual_affected_byte_range_old: Option<(usize, usize)>,
        visual_affected_byte_range_new: Option<(usize, usize)>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedCompositionCommitHandoff {
    pub(crate) visual_state: RebaseVisualState,
    pub(crate) offset_map: OffsetMap,
    pub(crate) visual_affected_byte_range_old: Option<(usize, usize)>,
    pub(crate) visual_affected_byte_range_new: Option<(usize, usize)>,
}

impl LinuxEditorAnimationCoordinator {
    /// Issue #819 评论 5956495850 第 4 节：重写 `take_rebase_frames`。
    ///
    /// 旧事务还活着时调 `sample_transaction_visual_state(tx, now)` 采样当前屏幕帧，
    /// 从 `SampledEditVisualState` 构造 `RebaseVisualState`，再 cancel 旧事务。
    /// 不再自己逐 unit 调 `collect_rebase_frame_for_unit_without_caret`。
    pub(crate) fn take_rebase_frames(
        &mut self,
        conflicting: &[VisualTransactionKey],
        reason: &str,
        now: Instant,
        preserve: Option<(&[(usize, usize)], &OffsetMap)>,
        current_old_text: &str,
        current_cursor_epoch: u64,
    ) -> RebaseVisualState {
        if conflicting.is_empty() {
            return RebaseVisualState::default();
        }
        let mut all_rebase_frames: Vec<RebaseFrame> = Vec::new();
        // Issue #819 评论 5968240881 问题 1：所有冲突事务的未终态、未匹配 unit 汇总。
        // 携带完整 AnimatedSlice ingest 元数据 + sampled frame + timing，让 transaction
        // builder 真正 units.push() 到新事务，而不是只拿几何凑 rebase frame。
        let mut all_carried_units: Vec<super::frame_state::CarriedVisualUnit> = Vec::new();
        let mut all_carried_snapshot_ids: Vec<
            crate::sujian_editor_item::layout_snapshot::LineSnapshotId,
        > = Vec::new();
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
            // 受影响：用 sample_transaction_visual_state 采样当前屏幕帧。
            // Issue #819 评论 5956495850 第 4 节：不再逐 unit 调
            // collect_rebase_frame_for_unit_without_caret，统一走采样入口。
            let sampled = super::sample::sample_transaction_visual_state(tx, now);
            // 消费 sampled 的 transaction_key / layout_basis_revision 用于诊断，
            // 避免 dead_code（这些字段在采样状态中确实有值，供调试时确认帧归属）。
            editor_animation_debug_log(&format!(
                "anim_sample: key={:?} layout_basis={:?} slices={}",
                sampled.transaction_key, sampled.layout_basis_revision, sampled.slices.len(),
            ));
            // Issue #819 评论 5967250411 问题 1：终态过滤改用 slice.is_finished，
            // 不再拿 visible_fraction 猜 CaretTrack 终态。
            // CaretTrack unit 的 visible_fraction 固定 0.0，DeleteConceal 一进入 rebase
            // 就被旧逻辑误判成"已吞完"，文字先消失，光标继续走。
            // is_finished 由 sample_unit_slice_frame 按本帧生命周期明确设置：
            // - Timed unit：unit.timing.progress(now) >= 1.0
            // - CaretTrack unit：caret 不存在或 caret.progress >= 1.0
            // Issue #819 评论 5968240881 问题 1：未终态的 slice 全部收集到 carried_units，
            // 携带完整 AnimatedSlice ingest 元数据 + sampled frame + timing，
            // 由 transaction builder 真正 units.push() 到新事务，
            // 不让屏幕上还没吞完的旧 slice 因为新正文里没有那个字就被直接扔掉。
            let mut frames: Vec<RebaseFrame> = Vec::with_capacity(sampled.slices.len());
            let mut carried: Vec<super::frame_state::CarriedVisualUnit> =
                Vec::with_capacity(sampled.slices.len());
            // Issue #819 评论 5968240881 问题 1：按 byte_start/byte_end/kind 把 sampled slice
            // 关联到 tx.units 里对应的 PreparedVisualUnit，取出完整 AnimatedSlice 和 timing。
            // 匹配不上的 sampled slice 仍构造 carried unit，但 slice/timing 退化为
            // 按 sampled frame 几何重建的 minimal AnimatedSlice + Timed(remaining_duration_ms)。
            for slice in &sampled.slices {
                // 消费 snapshot_side / progress 用于诊断，避免 dead_code。
                editor_animation_debug_log(&format!(
                    "anim_slice: kind={:?} side={:?} progress={:.3}",
                    slice.kind, slice.snapshot_side, slice.progress,
                ));
                if slice.is_finished {
                    continue;
                }
                // 找到 tx.units 里与本 sampled slice 对应的 PreparedVisualUnit。
                // 优先按 byte_start/byte_end + kind 精确匹配；找不到时按 kind +
                // shaping_identity 近似匹配（rebase 后 byte range 可能已变）。
                let matched_unit = tx.units.iter().find(|u| {
                    u.slice.byte_start == slice.byte_start
                        && u.slice.byte_end == slice.byte_end
                        && u.slice.kind == slice.kind
                });
                let carried_unit = match matched_unit {
                    Some(unit) => super::frame_state::CarriedVisualUnit {
                        slice: unit.slice.clone(),
                        sampled_frame: slice.clone(),
                        timing: unit.timing.clone(),
                    },
                    None => {
                        // tx.units 里找不到对应 unit（可能已被 retire/拆分）。
                        // 用 sampled frame 几何重建一个 minimal AnimatedSlice，
                        // timing 退化为 Timed(remaining_duration_ms)。
                        // 这种 carried unit 主要用于诊断连续性，新事务接管后
                        // 由新 track/新 handoff 收口。
                        let rebuilt_slice = rebuild_minimal_animated_slice_from_sampled(slice);
                        let rebuilt_timing = VisualUnitTiming::Timed {
                            started_at: None,
                            duration_ms: slice.remaining_duration_ms.max(1),
                            start_fraction: slice.visible_fraction,
                            target_fraction: match slice.kind {
                                AnimatedSliceKind::DeleteConceal => 0.0,
                                _ => 1.0,
                            },
                        };
                        super::frame_state::CarriedVisualUnit {
                            slice: rebuilt_slice,
                            sampled_frame: slice.clone(),
                            timing: rebuilt_timing,
                        }
                    }
                };
                carried.push(carried_unit);
                frames.push(RebaseFrame {
                    byte_start: slice.byte_start,
                    byte_end: slice.byte_end,
                    x: slice.dest_rect.x,
                    y: slice.dest_rect.y,
                    opacity: slice.opacity,
                    shaping_identity: slice.shaping_identity.clone(),
                    visible_fraction: slice.visible_fraction,
                    sampled_at: now,
                    remaining_duration_ms: slice.remaining_duration_ms,
                });
            }
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
            // Issue #819 评论 5968240881 问题 1：carried units 同样做坐标系映射，
            // 让 transaction builder 能在新事务坐标系里消费它们。映射失败的 carried
            // unit 仍然保留（退化为原 byte range），因为 carried unit 的几何
            //（dest_rect/source_rect/opacity）不依赖 byte range 匹配，它作为额外
            // visual unit 带进新事务后由新 track 收口。
            let mapped_carried: Vec<super::frame_state::CarriedVisualUnit> = carried
                .into_iter()
                .map(|mut unit| {
                    if let Some(ref per_tx_map) = per_tx_map {
                        if let Some((ms, me)) = per_tx_map
                            .map_old_range_to_new(unit.slice.byte_start, unit.slice.byte_end)
                        {
                            unit.slice.byte_start = ms;
                            unit.slice.byte_end = me;
                            unit.sampled_frame.byte_start = ms;
                            unit.sampled_frame.byte_end = me;
                        }
                        // 映射失败：保留原 byte range，几何仍有效。
                    }
                    unit
                })
                .collect();
            // 在 cancel 之前从 sampled.caret 构造 RebaseCaretHandoff 候选。
            // Issue #819 评论 5956495850 第 4 节：caret 帧已由 sample_transaction_visual_state
            // 采好，直接从 SampledCaretFrame 构造 handoff，不再单独调 sample_caret_track_frame。
            let caret_handoff = match (sampled.caret, tx.cursor_visual_track.as_ref()) {
                (Some(caret_frame), Some(track)) => {
                    let sampled = caret_frame.rect;
                    // Issue #722 评论 5749791161: 采样到的行几何从旧 track 的
                    // from_line/to_line 字段中选取。caret_line_id 等于 from 行 id
                    // 时用 from 行几何，等于 to 行 id 时用 to 行几何，否则用 from 行
                    // 几何作 fallback（caret 通常还在过渡中间，偏向 from 行更安全）。
                    let caret_line_id = caret_frame.visual_line_id;
                    let (line_top, line_bottom) = match caret_line_id {
                        Some(id) if Some(id) == track.to_visual_line_id => {
                            (track.to_line_top, track.to_line_bottom)
                        }
                        _ => (track.from_line_top, track.from_line_bottom),
                    };
                    // Issue #819 评论 5968931455: 提取旧 track 从当前 progress 之后的剩余 segments。
                    // 这些 segments 让 carried CaretTrack unit 在新事务里继续消费自己原来的 route，
                    // 而不是被迫消费下一笔编辑的 route。
                    let remaining_ingest_segments = track.remaining_segments_from(
                        caret_frame.progress,
                        track.to, // 旧 track 的终点
                    );
                    Some(RebaseCaretHandoff {
                        sampled,
                        remaining_duration_ms: track.remaining_duration_ms(now).max(1),
                        sampled_visual_line_id: caret_line_id,
                        sampled_line_top: line_top,
                        sampled_line_bottom: line_bottom,
                        remaining_ingest_segments,
                        stage_id: track.stage_id,
                    })
                }
                // Issue #808 评论 5916391891 修改 3: 没有真实 cursor track，就没有 cursor handoff。
                // text-only 事务永远不能生成、保存、交棒插值 cursor；光标直接由 canonical/Snap 接管。
                _ => None,
            };
            caret_handoff_candidates.push((old_key, tx.cursor_owner_epoch, caret_handoff));
            emit_transaction_diagnostic(tx, "editor.anim.rebase", reason);
            all_rebase_frames.extend(mapped_frames);
            // Issue #819 评论 5968240881 问题 1：收集 carried units 引用的 snapshot ids，
            // 供 prepare_edit_motion 在新事务创建之前做 texture retain 时把这些 ids
            // 也算进 active 集合，避免旧事务 cancel 后纹理在新事务接管前被回收。
            for unit in &mapped_carried {
                all_carried_snapshot_ids.push(unit.snapshot_id());
            }
            all_carried_units.extend(mapped_carried);
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
            "anim_rebase: cancelled_keys={:?} reason={} rebase_frames={} carried_cursor={} carried_units={}",
            cancelled_keys,
            reason,
            all_rebase_frames.len(),
            selected_caret_handoff.is_some(),
            all_carried_units.len(),
        ));
        RebaseVisualState {
            rebase_frames: all_rebase_frames,
            caret_handoff: selected_caret_handoff,
            carried_units: all_carried_units,
            carried_snapshot_ids: all_carried_snapshot_ids,
        }
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
        // Issue #756 / Issue #815 评论 6042062633 修改 7: 三个动画开关决定要不要建事务。
        // - coordinated=true：一条 caret 运动轨迹，文字以 caret 当前帧为吞吐边界。
        // - coordinated=false：typing_animation_enabled 只决定文字动画
        //   （Reflow + InsertReveal/DeleteConceal），smooth_cursor_enabled 只决定光标动画
        //   （caret motion track）。两者互相独立，同时为 true 不等于协同。
        let text_animation_enabled = coordinated_animation_enabled || typing_animation_enabled;
        let caret_animation_enabled = coordinated_animation_enabled || smooth_cursor_enabled;
        let inserted_range = vt
            .inserted_range
            .map(|range| (range.start().value(), range.end().value()));
        let skip =
            |cause: &str, old_caret_present: bool, new_caret_present: bool, inserted_range| {
                editor_animation_transaction_skipped_event(&AnimationSkipFields {
                    cause,
                    operation_kind: match vt.kind {
                        EditorAnimationKind::Insert => "Insert",
                        EditorAnimationKind::Delete => "Delete",
                        EditorAnimationKind::Cursor => "Cursor",
                    },
                    typing_animation_enabled,
                    smooth_cursor_enabled,
                    coordinated_animation_enabled,
                    old_caret_present,
                    new_caret_present,
                    inserted_range,
                    unit_kinds: "",
                    cursor_track_present: false,
                    is_scrolling,
                    is_loading,
                    is_applying_format,
                    transaction_id: None,
                    generation: 0,
                });
            };

        if !text_animation_enabled && !caret_animation_enabled {
            return None;
        }

        let mode = AnimationMode::from_context(is_scrolling, is_loading, is_applying_format);
        if is_scrolling || is_loading || is_applying_format || !mode.should_create_transaction() {
            skip(
                "suppressed_by_context",
                old_cursor_rect.is_some(),
                new_cursor_rect.is_some(),
                inserted_range,
            );
            return None;
        }

        // Issue #815 评论 6042062633 修改 7: 协同模式要求 old/new caret 几何都来自对应的
        // old/new canonical snapshot。以前这里是静默 `return None`，诊断包里只剩
        // "有 Delete 没有 Insert"，完全猜不出原因。现在记正式跳过事件。
        let valid_caret_motion_track = old_cursor_rect.is_some() && new_cursor_rect.is_some();
        if coordinated_animation_enabled && !valid_caret_motion_track {
            skip(
                "caret_geometry_missing",
                old_cursor_rect.is_some(),
                new_cursor_rect.is_some(),
                inserted_range,
            );
            return None;
        }

        // Issue #738 评论 5796693007 问题1: 用 if-else 链而不是 match `EditorAnimationKind::Insert =>`，
        // 避免与 `process_transaction` 的测试锚点（`EditorAnimationKind::Insert/Delete/Cursor =>`）
        // 冲突。`process_transaction` 保留原内联 match 结构供 issue687/issue702 白盒测试定位。
        if vt.kind == EditorAnimationKind::Insert {
            match vt.inserted_range {
                Some(range) => {
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
                    // 旧事务还活着，采到的是真实当前帧（文字 Timed unit 与 caret track 独立）。
                    let visual_state = self.take_rebase_frames(
                        &conflicting,
                        "rebased_by_insert",
                        now,
                        Some((&[(range_start, range_start)], &insert_offset_map)),
                        &vt.old_text,
                        cursor_owner_epoch,
                    );
                    Some(PreparedRebaseHandoff::Insert {
                        visual_state,
                        range_start,
                        range_end,
                        insert_offset_map,
                        visual_affected_byte_range_old,
                        visual_affected_byte_range_new,
                    })
                }
                // Issue #815 评论 6042062633 修改 8: Insert 事务没有 inserted_range 时，
                // Core 就没有给出可见插入区间，记正式跳过事件而不是静默丢弃。
                None => {
                    skip(
                        "missing_inserted_range",
                        old_cursor_rect.is_some(),
                        new_cursor_rect.is_some(),
                        None,
                    );
                    None
                }
            }
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
            let visual_state = self.take_rebase_frames(
                &conflicting,
                "rebased_by_delete",
                now,
                Some((&deleted_ranges, &delete_offset_map)),
                &vt.old_text,
                cursor_owner_epoch,
            );
            Some(PreparedRebaseHandoff::Delete {
                visual_state,
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

/// Issue #819 评论 5968240881 问题 1：从 `SampledSliceFrame` 重建一个 minimal `AnimatedSlice`。
///
/// 仅在 `tx.units` 里找不到对应 `PreparedVisualUnit` 时使用（unit 已被 retire/拆分）。
/// 用 sampled frame 的几何（dest_rect / source_rect / opacity / byte range / kind /
/// shaping identity / snapshot id / visual line id）重建一个能被新事务接管的 slice。
/// ingest 元数据（caret anchor / line mask / ingest line ord）退化为按 dest_rect
/// 自身几何算的默认值——这种 carried unit 主要用于诊断连续性，新事务接管后由新
/// track/新 handoff 收口。
fn rebuild_minimal_animated_slice_from_sampled(
    slice: &super::frame_state::SampledSliceFrame,
) -> AnimatedSlice {
    use crate::sujian_editor_item::animated_slice::IngestBoundaryDriver;
    let dest = slice.dest_rect.clone();
    AnimatedSlice {
        kind: slice.kind,
        snapshot_id: slice.snapshot_id,
        source_rect: slice.source_rect.clone(),
        from_document_rect: dest.clone(),
        to_document_rect: dest.clone(),
        opacity_from: slice.opacity,
        opacity_to: slice.opacity,
        scale_from: 1.0,
        scale_to: 1.0,
        byte_start: slice.byte_start,
        byte_end: slice.byte_end,
        shaping_identity: slice.shaping_identity.clone(),
        conceal_to_left_edge: matches!(slice.kind, AnimatedSliceKind::DeleteConceal),
        // 无真实 caret anchor，退化为 dest 左端——新事务接管后由新 track 收口。
        caret_anchor_x: dest.x,
        caret_anchor_y: dest.y,
        // 行级 mask 退化为自身 rect。
        line_mask_left: dest.x,
        line_mask_right: dest.x + dest.w,
        is_caret_line: false,
        ingest_boundary_driver: IngestBoundaryDriver::CaretPosition,
        ingest_boundary_from_x: None,
        ingest_line_ord: None,
        ingest_from_line_ord: None,
        ingest_to_line_ord: None,
        ingest_line_top: None,
        ingest_line_bottom: None,
        visual_line_id: slice.visual_line_id,
        // 起始比例取 sampled frame 的 visible_fraction，让新事务从当前可见比例继续。
        start_fraction: slice.visible_fraction,
        static_hidden_document_rects: Vec::new(),
        crossfade_group_id: None,
        crossfade_side: None,
        reflow_anchors: Vec::new(),
    }
}

#[cfg(test)]
mod tests;
