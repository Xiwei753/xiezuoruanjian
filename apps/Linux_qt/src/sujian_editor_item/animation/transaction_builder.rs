use writer_core::editor::OffsetMap;

// 仅 `process_transaction` 的单元测试需要下列导入：生产路径已改为
// `build_prepared_transaction` 直接构造、`rebind_timed_units_to_canonical` 重绑，
// 因此这些名字在非测试构建里没有使用者，必须用 `#[cfg(test)]` 收起来，
// 否则 `--bin sujian-linux-qt` 的 clippy（-D warnings）会报 unused import。
#[cfg(test)]
use std::time::Instant;

use super::coordinator::LinuxEditorAnimationCoordinator;
#[cfg(test)]
use crate::editor::layout::compute_affected_paragraph_ranges;
use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};
use crate::sujian_editor_item::animation::cursor_motion::build_cursor_visual_track;
use crate::sujian_editor_item::animation::rebase::{match_rebase_frames, PreparedRebaseHandoff};
use crate::sujian_editor_item::animation::retarget_motion::{
    count_carried_ingest_units, count_ingest_slices, detach_caret_track_to_timed, retarget,
    RetargetPatchKind, RetargetRequest, RetargetStart, RetargetTarget,
};
use crate::sujian_editor_item::animation::transaction::types::IngestStageId;
use crate::sujian_editor_item::animation::{
    PreparedTextVisualTransaction, PreparedVisualUnit, RebaseFrame, TextVisualOperationKind,
    TextVisualTransactionState, TransactionTimeline,
};
#[cfg(test)]
use crate::sujian_editor_item::animation_mode::AnimationMode;
#[cfg(test)]
use crate::sujian_editor_item::edit_motion::{diff_plain_text, EditorAnimationKind};
use crate::sujian_editor_item::edit_motion::{CursorRect, PreparedEditMotion};
use crate::sujian_editor_item::editor_animation_debug_log;
use crate::sujian_editor_item::editor_animation_transaction_skipped_event;
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::layout_snapshot::{
    ClusterInsertRelation, EditorLayoutSnapshot, SourceRect,
};
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;

/// Issue #819 评论 5968931455 问题 2.2: builder 的明确结果。
///
/// 替代 `Option<PreparedTextVisualTransaction>`——builder 跳过事务创建时
/// 必须给出明确的 `EditVisualSkipReason`，不再让调用方猜。
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum BuildTransactionOutcome {
    /// 事务已创建。
    Created(PreparedTextVisualTransaction),
    /// builder 跳过了事务创建，附带具体跳过原因。
    Skipped(super::super::edit_flow::EditVisualSkipReason),
}

#[cfg(test)]
impl BuildTransactionOutcome {
    /// 测试辅助：unwrap 成 Created，panic 时附带 skip reason。
    pub(crate) fn expect_created(self, msg: &str) -> PreparedTextVisualTransaction {
        match self {
            BuildTransactionOutcome::Created(tx) => tx,
            BuildTransactionOutcome::Skipped(reason) => {
                panic!("{}: skipped by {:?}", msg, reason)
            }
        }
    }
}

/// Issue #819 评论 5968931455 问题 2.2: handoff 路径的明确结果。
///
/// 替代 `Option<VisualTransactionKey>`——`create_transaction_from_prepared_handoff`
/// 跳过时必须给出明确的 `EditVisualSkipReason`，不再让 pipeline 猜。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HandoffTransactionOutcome {
    /// 事务已创建，`key` 是真正创建的事务 key。
    Created(VisualTransactionKey),
    /// 跳过了事务创建，附带具体跳过原因。
    Skipped(super::super::edit_flow::EditVisualSkipReason),
}

pub(crate) fn operation_kind_label(kind: TextVisualOperationKind) -> &'static str {
    match kind {
        TextVisualOperationKind::Insert => "Insert",
        TextVisualOperationKind::Delete => "Delete",
        TextVisualOperationKind::Replace => "Replace",
        TextVisualOperationKind::CompositionUpdate => "CompositionUpdate",
        TextVisualOperationKind::CompositionCommitOrCancel => "CompositionCommitOrCancel",
    }
}

pub(crate) fn unit_kind_labels(units: &[PreparedVisualUnit]) -> Vec<String> {
    units
        .iter()
        .map(|u| format!("{:?}", u.slice.kind))
        .collect()
}

pub(crate) fn emit_transaction_diagnostic(
    tx: &PreparedTextVisualTransaction,
    event: &str,
    reason: &str,
) {
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

pub(crate) mod edit_spec;
pub(crate) mod ingest_route;

pub(crate) use edit_spec::{
    insert_reveal_not_generated_debug_message, skip_fields, CompositionCommitCrossfadeSpec,
    VisualEditSpec,
};

/// Issue #747 评论 5813540976: 全仓库唯一创建 `PreparedTextVisualTransaction` 的完整入口。
///
/// 接收归一化后的 [`VisualEditSpec`]，内部统一完成 slice 构造、unit wrap、rebase 匹配、
/// cursor track 构建、timeline 初始化。其它模块（含 `composition.rs` 与普通 Insert/Delete
/// 路径）都经由本函数创建事务，从而保证「只允许这里创建 `PreparedTextVisualTransaction`」。
pub(crate) fn build_prepared_transaction(spec: VisualEditSpec) -> BuildTransactionOutcome {
    let mut slices: Vec<AnimatedSlice> = Vec::new();

    // 1a. InsertReveal / DeleteConceal（文字动画）
    //
    // Issue #756: 吞吐字是否存在由 text_animation_enabled 决定（coordinated || typing）。
    //
    // Issue #815 评论 5947443780 问题3: 这里原先写的是 #808 的旧定义
    // （「吞吐字始终用 Timed timing」「文字 progress 不消费 caret frame」），
    // 那套定义已被 #815 推翻。
    //
    // 现在的定义：**协同模式 = 一条 caret 运动轨迹 + 文字以该轨迹当前帧为吞吐边界
    // + Reflow 可独立**。
    // - coordinated=true：InsertReveal/DeleteConceal 走 `VisualUnitTiming::CaretTrack`，
    //   没有自己的 timeline；逐帧边界直接来自本事务 cursor track 的唯一一次采样
    //   （`sample_caret_track_frame`），文字层与光标层消费同一份采样。
    //   Delete 键（caret 固定）额外用 `IngestBoundaryDriver::DeleteForwardBoundary`，
    //   吞字边界自己从被删区间右端朝静止 caret 收拢。
    // - coordinated=false：仍是独立 `Timed`，按「打字动画」设置自己推进
    //   （`ease_out_quad` + `text_duration_ms`）。
    // - ReflowMove/ReflowCrossFade：**始终**独立 `Timed`，协同模式也不接管。
    if spec.text_animation_enabled {
        for &(i_start, i_end) in &spec.inserted_ranges {
            slices.extend(build_insert_reveal_slices(
                spec.key,
                &spec.new_snapshot,
                (i_start, i_end),
                // Issue #808: 传旧 caret 作为吐字遮罩锚点。
                spec.old_cursor_rect.as_ref(),
                // Issue #808 评论 5916391891 修改 4: coordinated 决定是否用 caret 锚点做遮罩。
                spec.coordinated_animation_enabled,
                // Issue #815 评论 5947230558 问题1: 不再传 old snapshot 的
                // `old_cursor_visual_line_id`。锚点行改用 new snapshot 自己的 ingest
                // 起点行序判定（`ingest_from_line_ord == Some(line_ord)`），
                // 避免 soft-wrap 变化后两个 revision 的同数字 line id 指向不同视觉行。
            ));
        }
        for &(d_start, d_end) in &spec.deleted_ranges {
            slices.extend(build_delete_conceal_slices(
                spec.key,
                &spec.old_snapshot,
                (d_start, d_end),
                spec.old_cursor_rect.as_ref(),
                // Issue #808: 传新 caret 作为吞字遮罩锚点。
                spec.new_cursor_rect.as_ref(),
                spec.coordinated_animation_enabled,
                // Issue #815 评论 5947230558 问题1/2: 这里传 **old snapshot 自己的**
                // `old_cursor_visual_line_id`，而不是 new 那一份：
                // - 吞字锚点行用 old 侧的 ingest 终点行序判定；
                // - 吞字起点行序也直接用这个 old line id 精确定位，不再用
                //   caret.top 的 y 几何猜（相邻行 `bottom == top` 时会误命中上一行）。
                spec.old_cursor_visual_line_id,
            ));
        }
    }

    // 1b. Composition commit 特殊 crossfade（preedit→candidate 形变）
    //
    // Issue #756 评论 5821793349: crossfade 文字 unit（DeleteConceal/InsertReveal/
    // ReflowCrossFade/ReflowMove）也受 text_animation_enabled 控制。
    // 非协同 smooth-only commit（coordinated=false + typing=false + smooth=true）只保留
    // cursor_visual_track，tx.units 必须为空，不再生成文字动画。
    // typing-only / typing+smooth / coordinated 模式继续保留 composition crossfade 文字动画。
    if spec.text_animation_enabled {
        if let Some(crossfade) = &spec.composition_commit_crossfade {
            slices.extend(build_composition_commit_crossfade_slices(
                spec.key,
                &spec.old_snapshot,
                &spec.new_snapshot,
                &spec.offset_map,
                crossfade.preedit_byte_start,
                crossfade.preedit_byte_end,
                crossfade.candidate_byte_start,
                crossfade.candidate_byte_end,
                spec.old_cursor_rect.as_ref(),
                spec.new_cursor_rect.as_ref(),
                // Issue #808 评论 5917296533 问题4: Composition 路径统一协同模式参数。
                // coordinated=false 时只走独立文字动画语义。
                //
                // Issue #815 评论 5947443780 问题1: coordinated=true 时这条路径生成的
                // InsertReveal/DeleteConceal 与普通 Insert/Delete 一样是 `CaretTrack`，
                // 所以必须带齐 5 个吞吐元数据字段（行序 + boundary driver + 起点），
                // builder 内部会写入，不靠这里的锚点标记兜底。
                spec.coordinated_animation_enabled,
                // Issue #815 评论 5947230558 问题1: 两侧各传各自的 caret line id，
                // 跨 revision 比大小会把遮罩锚点放到错的行上。
                spec.old_cursor_visual_line_id,
                spec.new_cursor_visual_line_id,
            ));
        }
    }

    // 1c. Reflow（unchanged material）
    //
    // Issue #756: Reflow 是文字动画的一部分，由 text_animation_enabled 决定
    //（coordinated=true 或 typing_animation_enabled=true）。typing 关闭且非协同时
    // 只有光标动画，不生成文字 unit。
    let mut excluded_old: Vec<(usize, usize)> = spec.deleted_ranges.clone();
    let mut excluded_new: Vec<(usize, usize)> = spec.inserted_ranges.clone();
    if let Some(crossfade) = &spec.composition_commit_crossfade {
        excluded_old.push((crossfade.preedit_byte_start, crossfade.preedit_byte_end));
        excluded_new.push((crossfade.candidate_byte_start, crossfade.candidate_byte_end));
    }
    if spec.text_animation_enabled {
        slices.extend(build_cluster_reflow_slices(
            spec.key,
            &spec.old_snapshot,
            &spec.new_snapshot,
            &spec.offset_map,
            &excluded_old,
            &excluded_new,
            spec.old_cursor_rect.as_ref(),
            spec.new_cursor_rect.as_ref(),
            spec.visual_affected_byte_range_old,
            spec.visual_affected_byte_range_new,
        ));
    }

    // 1d. 行级共同 mask 分组
    //
    // Issue #808 评论 5919641249 修改 1: 全部文字 slice 收集完成后，按 visual_line_id
    // 统一写 `line_mask_left/right`。builder 内部各自分过一次组；这里再分一次，保证
    // 跨 builder（普通 Insert/Delete 与 Composition commit）以及跨多个
    // inserted/deleted range 落在同一行的 slice 也共享同一条行级 boundary。
    // 分组只写 mask extent，不动各 slice 自己的 source/document rect。
    // ReflowMove/ReflowCrossFade 不参与分组，保持等于自己 from rect 的默认值。
    assign_shared_line_masks(&mut slices);

    // 1e. caret 吞吐路径（Issue #815 评论 5949097065 问题3）
    //
    // 必须在 `assign_shared_line_masks` **之后**、建 track **之前**：那时每一行的真实
    // 吞吐范围（`line_mask_left/right`）和行几何（`ingest_line_top/bottom`）都已经由
    // **本侧** canonical 写好，路径只能从这些同侧数据生成。
    //
    // 之前把跨软换行当成 `old rect → new rect` 一条直线，那条线是从右上飞到左下的对角线，
    // 文字拿到的是对角线上的某个 x，而不是"这一行上 caret 走到了哪里"。现在改成按行分段：
    // `LayoutHandoff`（跨 layout 换位，不吞吐）/ `IngestLine`（沿某行横扫，那一行的 x 才
    // 是吞吐边界）/ `RowHandoff`（行间换位）。
    //
    // Issue #815 评论 5950887715: 这里原先还带一个
    // `spec.composition_commit_crossfade.is_none()` 的门，把 IME commit 整条排除在
    // 正式 route 之外——那等于绕过 #815 早就规定过的「IME 也走同一套 caret-driven
    // 协同语义」。现在 segment 带 `IngestSnapshotSide`，old / new 行序只在各自
    // side 内比较，IME 的 Mixed 路径也能建出正确的分段路线，所以这道门删掉。
    //
    // Issue #824 评论 5971089641 第 1/3 节：正文运动唯一入口 `retarget(...)`。
    // 起点来自本帧采样的当前屏幕 caret（handoff.sampled）或首条 motion 的逻辑
    // old caret；目标是最新 new_cursor_rect。**不**把旧 route 的剩余段拼进来，
    // 也不按段数均分时长。
    let new_stage_id = IngestStageId(spec.key.transaction_id);
    let ingest_route_segments =
        if spec.caret_animation_enabled && spec.coordinated_animation_enabled {
            let target = spec.new_cursor_rect.map(|caret| RetargetTarget {
                caret,
                visual_line_id: spec.new_cursor_visual_line_id,
            });
            let start = spec
                .visual_state
                .caret_handoff
                .as_ref()
                .map(|handoff| RetargetStart {
                    caret: handoff.sampled,
                    visual_line_id: handoff.sampled_visual_line_id,
                    line_top: handoff.sampled_line_top,
                    line_bottom: handoff.sampled_line_bottom,
                })
                .or_else(|| {
                    spec.old_cursor_rect.map(|caret| RetargetStart {
                        caret,
                        visual_line_id: spec.old_cursor_visual_line_id,
                        line_top: spec.old_cursor_line_top,
                        line_bottom: spec.old_cursor_line_bottom,
                    })
                });
            match (start, target) {
                (Some(start), Some(target)) => {
                    let (new_reveal, new_conceal) = count_ingest_slices(&slices);
                    let (carried_reveal, carried_conceal) =
                        count_carried_ingest_units(&spec.visual_state.carried_units);
                    retarget(RetargetRequest {
                        start,
                        target,
                        slices: &slices,
                        stage_id: new_stage_id,
                        patch_kind: RetargetPatchKind::from_kind(spec.patch_kind),
                        replaced_stage_id: spec
                            .visual_state
                            .caret_handoff
                            .as_ref()
                            .map(|handoff| handoff.stage_id),
                        active_reveal_count: new_reveal + carried_reveal,
                        active_conceal_count: new_conceal + carried_conceal,
                    })
                }
                // 拿不到 old/new caret 之一时返回空：那种事务本来就建不出 track，
                // `build_cursor_visual_track` 返回 None 并由调用点记
                // `editor.anim.transaction_skipped`。
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        };

    // 2. Cursor visual track
    //
    // Issue #756: caret motion track 就是正文编辑期间的光标动画，由
    // caret_animation_enabled 决定（coordinated=true 或 smooth_cursor_enabled=true）。
    // 关闭时本事务不拥有 caret motion，光标位置由 canonical caret 接管（Snap），
    // 不会在用户关掉"平滑光标"后仍然沿 track 滑动。
    //
    // Issue #815 评论 6042062633 修改 4: 这条 track 必须**先于** Wrap units 构造，
    // 因为协同 InsertReveal/DeleteConceal 要用它来决定自己是不是 CaretTrack 驱动。
    let cursor_visual_track = if spec.caret_animation_enabled {
        build_cursor_visual_track(
            spec.old_cursor_rect.as_ref(),
            spec.new_cursor_rect.as_ref(),
            spec.old_cursor_visual_line_id,
            spec.new_cursor_visual_line_id,
            spec.old_cursor_line_top,
            spec.old_cursor_line_bottom,
            spec.new_cursor_line_top,
            spec.new_cursor_line_bottom,
            spec.visual_state.caret_handoff.clone(),
            spec.caret_duration_ms,
            ingest_route_segments,
            new_stage_id,
        )
    } else {
        None
    };

    // Issue #815 评论 6042062633 修改 4: 协同模式必须同时拿到 cursor track。
    // 有吞吐字却拿不到 track 时**不允许**把这两种吞吐字退回独立 Timed —— 那正是
    // "文字在光标附近被切开"而不是"跟着光标吞吐"的根因。这样的事务直接跳过并记
    // `editor.anim.transaction_skipped`，不做静默降级。
    let coordinated_ingest = spec.coordinated_animation_enabled && cursor_visual_track.is_some();

    // 3. Wrap units
    //
    // Issue #815 评论 6042062633 修改 4: 计时驱动分两类。
    // - 协同（`coordinated_ingest`）InsertReveal/DeleteConceal → `VisualUnitTiming::CaretTrack`，
    //   没有自己的 `ease_out_quad + text_duration_ms` progress；逐帧吞吐边界直接来自
    //   上面那条 `cursor_visual_track` 的当前帧。
    // - 非协同 InsertReveal/DeleteConceal → 独立 `Timed`，跟随「打字动画」设置。
    // - ReflowMove/ReflowCrossFade → **始终**独立 `Timed`，协同也不接管，
    //   继续按 `text_duration_ms` 播放。
    let mut units: Vec<PreparedVisualUnit> = slices
        .into_iter()
        .map(|s| match s.kind {
            AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal => {
                PreparedVisualUnit::wrap_with_coordinated(
                    s,
                    spec.text_duration_ms,
                    coordinated_ingest,
                )
                // Issue #819 评论 5968931455: 新事务自己创建的协同吞吐字标记新 stage_id。
                .with_stage_id(new_stage_id)
            }
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
                PreparedVisualUnit::wrap(s, spec.text_duration_ms)
            }
        })
        .collect();

    // 4. Rebase frame 匹配
    match_rebase_frames(
        &spec.visual_state.rebase_frames,
        &mut units,
        &spec.offset_map,
    );

    // Issue #819 评论 5968240881 问题 1：消费 carried_units。
    // carried_units 是上一笔事务 cancel 时本帧仍未到达终态、且在新事务里匹配不到
    // 对应 unit 的旧视觉单元（携带完整 AnimatedSlice ingest 元数据 + sampled frame +
    // timing）。连续 Backspace 时第一笔的 C 的 carried unit 匹配不到第二笔已有 unit，
    // 旧事务又被 cancel，以前直接丢掉导致 C 提前消失。
    //
    // 现在分两步：
    // 1. 先用 carried unit 的 sampled frame 几何作为 RebaseFrame 喂给 match_rebase_frames，
    //    匹配上的新 unit 走 rebase_from_frame 继承几何。
    // 2. 匹配不上的 carried unit 真正 units.push() 到新事务的 units：
    //    - 用 carried unit 的 AnimatedSlice 构造新的 PreparedVisualUnit，
    //      timing 从 carried unit 继承。
    //    - CaretTrack carried unit 从 sampled frame + 新 caret handoff 接着收口。
    //    - Timed carried unit 从 sampled frame + remaining duration 接着播。
    //
    // carried_units 的 byte_start/byte_end 已经在 take_rebase_frames 里映射到
    // current-old 坐标系，这里再用 spec.offset_map 映射到新事务 new 坐标系尝试匹配。
    let carried_unit_count = spec.visual_state.carried_units.len();
    if carried_unit_count > 0 {
        editor_animation_debug_log(&format!(
            "anim_carried: op={:?} carried_units={} (未终态旧 unit 带进新事务，\
             不让屏幕上还没吞完的旧 slice 因新正文没有那个字就被直接扔掉)",
            spec.operation_kind, carried_unit_count,
        ));
        // Step 1: 把 carried units 的 sampled frame 构造成 RebaseFrame 喂给
        // match_rebase_frames 做第一轮匹配。匹配上的新 unit 走 rebase_from_frame
        // 继承几何。用 consumed_carried 跟踪哪些 carried unit 被匹配上了。
        let carried_as_frames: Vec<RebaseFrame> = spec
            .visual_state
            .carried_units
            .iter()
            .map(|unit| RebaseFrame {
                byte_start: unit.slice.byte_start,
                byte_end: unit.slice.byte_end,
                x: unit.sampled_frame.dest_rect.x,
                y: unit.sampled_frame.dest_rect.y,
                opacity: unit.sampled_frame.opacity,
                shaping_identity: unit.slice.shaping_identity.clone(),
                visible_fraction: unit.sampled_frame.visible_fraction,
                sampled_at: std::time::Instant::now(),
                remaining_duration_ms: unit.sampled_frame.remaining_duration_ms,
            })
            .collect();
        let mut consumed_carried: Vec<usize> = Vec::new();
        let mut consumed_units: Vec<usize> = Vec::new();
        for (carried_idx, frame) in carried_as_frames.iter().enumerate() {
            // 协同吞吐字不参与同 byte range 的新 unit 匹配：它会走下面的
            // `detach_caret_track_to_timed`，从当前屏幕帧继续收口到终态，
            // 既不被同 byte range 的新字吸收，也不重新排队旧 route。
            if spec.visual_state.carried_units[carried_idx]
                .timing
                .is_caret_track()
            {
                continue;
            }
            // tier1: 精确 byte range 匹配（未消费的新 unit）。
            let matched = units.iter_mut().enumerate().find(|(unit_idx, nu)| {
                !consumed_units.contains(unit_idx)
                    && nu.slice.byte_start == frame.byte_start
                    && nu.slice.byte_end == frame.byte_end
            });
            if let Some((unit_idx, new_unit)) = matched {
                new_unit.rebase_from_frame(frame);
                consumed_carried.push(carried_idx);
                consumed_units.push(unit_idx);
            }
        }
        // Step 2: 匹配不上的 carried unit 真正 units.push() 到新事务的 units。
        // 用 carried unit 的 AnimatedSlice 构造新的 PreparedVisualUnit，
        // timing 从 carried unit 继承。
        for (carried_idx, unit) in spec.visual_state.carried_units.iter().enumerate() {
            if consumed_carried.contains(&carried_idx) {
                continue;
            }
            // Issue #819 评论 5968240881 问题 1：carried unit 的 AnimatedSlice 已经
            // 在 take_rebase_frames 里映射到 current-old 坐标系。这里再用
            // spec.offset_map 映射到新事务 new 坐标系（如果映射失败保留原 byte range）。
            let mut new_slice = unit.slice.clone();
            if let Some((ms, me)) = spec
                .offset_map
                .map_old_range_to_new(unit.slice.byte_start, unit.slice.byte_end)
            {
                new_slice.byte_start = ms;
                new_slice.byte_end = me;
            }
            // CaretTrack carried unit 从 sampled frame + 新 caret handoff 接着收口；
            // Timed carried unit 从 sampled frame + remaining duration 接着播。
            // 两者都通过 rebase_from_frame 把 sampled frame 几何写入新 slice。
            //
            // Issue #824 评论 5971089641 第 5 节：收缩 carried unit 的职责——
            // 它只保留“本帧还可见的旧 glyph 几何”用于无跳变 retarget；旧
            // CaretTrack timing、旧 stage、旧剩余时长一律不排进下一笔正文动画。
            // 协同吞吐字改挂**本笔 motion 的起点/时长**：从采样到的当前可见比例
            // 出发，在本笔时长内收口到终态。它和本笔 caret track 同起点同时长，
            // 是同一份 motion 时钟，不是历史时间线。
            // Timed carried unit（Reflow）继续按自己的时间线播完。
            //
            // Issue #824 评论 5972388049：这段「当前 CaretTrack 视觉帧 → 脱离 caret
            // 后的 Timed 收口状态」的转换与 PointerClick detach 共用
            // `retarget_motion::detach_caret_track_to_timed`，不再各写一套。
            let carried_is_caret_track = unit.timing.is_caret_track();
            let mut new_unit = PreparedVisualUnit {
                slice: new_slice,
                timing: unit.timing.clone(),
                stage_id: unit.stage_id,
            };
            let detached_timing = if carried_is_caret_track {
                Some(detach_caret_track_to_timed(
                    &mut new_unit.slice,
                    &unit.sampled_frame,
                    // 新事务：等 Rendering 与整条 motion 同帧起跑。
                    spec.text_duration_ms,
                    None,
                ))
            } else {
                None
            };
            let carried_visible_fraction = detached_timing
                .as_ref()
                .map(|timing| timing.start_fraction())
                .unwrap_or(unit.sampled_frame.visible_fraction);
            let carried_frame = RebaseFrame {
                byte_start: unit.slice.byte_start,
                byte_end: unit.slice.byte_end,
                x: unit.sampled_frame.dest_rect.x,
                y: unit.sampled_frame.dest_rect.y,
                opacity: unit.sampled_frame.opacity,
                shaping_identity: unit.slice.shaping_identity.clone(),
                visible_fraction: carried_visible_fraction,
                sampled_at: std::time::Instant::now(),
                remaining_duration_ms: unit.sampled_frame.remaining_duration_ms,
            };
            new_unit.rebase_from_frame(&carried_frame);
            if let Some(timing) = detached_timing {
                // 旧 CaretTrack timing / stage / 剩余时长不排进新动画；
                // carried 吞吐字不再参与 route 的 stage 过滤（stage_id=None）。
                new_unit.timing = timing;
                new_unit.stage_id = None;
            }
            units.push(new_unit);
        }
    }

    // 5. 诊断日志
    editor_animation_debug_log(&format!(
        "anim_spec: op={:?} units={} inserted={} deleted={} rebased={} handoff={} epoch={} \
         text_anim={} caret_anim={} coordinated_anim={}",
        spec.operation_kind,
        units.len(),
        spec.inserted_ranges.len(),
        spec.deleted_ranges.len(),
        spec.visual_state.rebase_frames.len(),
        spec.visual_state.caret_handoff.is_some(),
        spec.cursor_owner_epoch,
        spec.text_animation_enabled,
        spec.caret_animation_enabled,
        spec.coordinated_animation_enabled,
    ));

    // Issue #785 评论 5857451442: 把 InsertReveal 未生成的诊断从 env-gated debug log
    // 升级为正式 editor.anim.* 诊断事件（writer_diagnostics），普通诊断包（不带 debug
    // 环境变量）即可在 zip 中看到，满足"诊断包直接看出原因"的要求。
    // 字段带 inserted range、snapshot revision、相交 line 数、相交 cluster 数、
    // 空白/控制字符跳过数，便于定位是注入链遗漏还是全部 cluster 被当空白跳过。
    //
    // Issue #785 评论 5857873894 修改 5: 正式 Warn 只针对 inserted range 确实含可见字符
    //（非空白、非控制字符）但最终 InsertReveal 为 0 的情况。空格、tab、换行继续正常跳过，
    // 不报异常。如果所有 inserted range 都只含空白/控制字符，则不记 Warn（这是正常跳过）。
    if spec.text_animation_enabled && !spec.inserted_ranges.is_empty() {
        let insert_reveal_count = units
            .iter()
            .filter(|u| u.slice.kind == AnimatedSliceKind::InsertReveal)
            .count();
        if insert_reveal_count == 0 {
            let mut intersecting_lines = 0usize;
            let mut intersecting_clusters = 0usize;
            let mut whitespace_skip_count = 0usize;
            for &(i_start, i_end) in &spec.inserted_ranges {
                for new_line in spec.new_snapshot.line_snapshots.iter() {
                    if new_line.byte_start < i_end && new_line.byte_end > i_start {
                        intersecting_lines += 1;
                    }
                    for new_cluster in new_line.clusters.iter() {
                        if new_cluster.byte_start < i_end && new_cluster.byte_end > i_start {
                            intersecting_clusters += 1;
                            if let Some(text) = spec
                                .new_snapshot
                                .virtual_text
                                .get(new_cluster.byte_start..new_cluster.byte_end)
                            {
                                if text.chars().all(|c| c.is_whitespace() || c.is_control()) {
                                    whitespace_skip_count += 1;
                                }
                            }
                        }
                    }
                }
            }
            // Issue #785 评论 5858151780: 是否属于"可见字符 Insert"直接检查
            // new_snapshot.virtual_text[inserted_range]，不用"相交 cluster 是否全是 whitespace"
            // 反推。Partial cluster 可能同时包含旧可见字符和本次插入的空白，按整个 cluster
            // 判断会把正常空白输入误报成 InsertReveal 丢失。
            let all_inserted_is_whitespace =
                spec.inserted_ranges.iter().all(|&(i_start, i_end)| {
                    let text = spec
                        .new_snapshot
                        .virtual_text
                        .get(i_start..i_end)
                        .unwrap_or("");
                    text.chars().all(|c| c.is_whitespace() || c.is_control())
                });
            if !all_inserted_is_whitespace {
                {
                    use std::collections::BTreeMap;
                    let mut fields = BTreeMap::new();
                    fields.insert(
                        "transaction_id".to_string(),
                        serde_json::json!(spec.key.transaction_id),
                    );
                    fields.insert(
                        "generation".to_string(),
                        serde_json::json!(spec.key.generation),
                    );
                    fields.insert(
                        "operation_kind".to_string(),
                        serde_json::Value::String(
                            operation_kind_label(spec.operation_kind).to_string(),
                        ),
                    );
                    fields.insert(
                        "inserted_ranges".to_string(),
                        serde_json::json!(spec.inserted_ranges),
                    );
                    fields.insert(
                        "snapshot_revision".to_string(),
                        serde_json::json!(spec.new_snapshot.revision.0),
                    );
                    fields.insert(
                        "intersecting_line_count".to_string(),
                        serde_json::json!(intersecting_lines),
                    );
                    fields.insert(
                        "intersecting_cluster_count".to_string(),
                        serde_json::json!(intersecting_clusters),
                    );
                    fields.insert(
                        "whitespace_skip_count".to_string(),
                        serde_json::json!(whitespace_skip_count),
                    );
                    fields.insert(
                        "unit_kinds".to_string(),
                        serde_json::Value::String(unit_kind_labels(&units).join(",")),
                    );
                    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
                        timestamp_ms: chrono::Utc::now().timestamp_millis(),
                        sequence: 0,
                        session_id: String::new(),
                        level: writer_diagnostics::DiagnosticLevel::Warn,
                        origin: writer_diagnostics::DiagnosticOrigin::App,
                        event: "editor.anim.insert_reveal_not_generated".to_string(),
                        target: "editor.anim".to_string(),
                        message: Some(
                            "inserted range has visible chars but InsertReveal count is 0"
                                .to_string(),
                        ),
                        fields,
                    });
                }
                // 保留 env-gated debug log 作为开发时辅助，正式诊断走上面的 writer_diagnostics 事件。
                editor_animation_debug_log(&insert_reveal_not_generated_debug_message(
                    &spec,
                    intersecting_lines,
                    intersecting_clusters,
                    whitespace_skip_count,
                ));
            }
        }
    }

    // Issue #815 评论 6042062633 修改 8: builder 收口时的两个显式跳过点。
    // 这里只暴露 `cause`；真正的实现要求是上面的 `coordinated_ingest`——
    // 协同吞吐字要么拿到 cursor track 由它驱动，要么整笔事务跳过，绝不静默降级。
    let unit_kind_labels = unit_kind_labels(&units);
    let unit_kinds = unit_kind_labels.join(",");
    if spec.coordinated_animation_enabled && cursor_visual_track.is_none() {
        // Issue #815 评论 6042062633 修改 8: 协同模式拿不到 cursor track 时整笔跳过。
        // old/new caret 几何缺失（或 caret 动画被关掉）都会走到这里；这里没有"文字照播、
        // 光标不动"的分支——那正是 #815 要删掉的静默降级。
        editor_animation_transaction_skipped_event(&skip_fields(
            "coordinated_without_cursor_track",
            &spec,
            &unit_kinds,
            cursor_visual_track.is_some(),
            spec.inserted_ranges.first().copied(),
        ));
        return BuildTransactionOutcome::Skipped(
            super::super::edit_flow::EditVisualSkipReason::CursorTrackMissing,
        );
    }

    if units.is_empty()
        && cursor_visual_track.is_none()
        && (spec.text_animation_enabled || spec.caret_animation_enabled)
    {
        // Issue #815 评论 6042062633 修改 8: 调用方要求动画，builder 却既没有 unit
        // 也没有 cursor track —— 这一笔编辑不会有任何动画。
        // 三个动画开关全关时不记事件（正常路径在 rebase 层就已 return None，
        // 根本不会走到 builder）。
        editor_animation_transaction_skipped_event(&skip_fields(
            "empty_units_and_cursor_track",
            &spec,
            &unit_kinds,
            cursor_visual_track.is_some(),
            spec.inserted_ranges.first().copied(),
        ));
        return BuildTransactionOutcome::Skipped(
            super::super::edit_flow::EditVisualSkipReason::BuilderEmptyTransaction,
        );
    }

    // 6. 唯一 PreparedTextVisualTransaction struct literal
    BuildTransactionOutcome::Created(PreparedTextVisualTransaction {
        key: spec.key,
        state: TextVisualTransactionState::Pending,
        operation_kind: spec.operation_kind,
        timeline: TransactionTimeline::new(if spec.text_animation_enabled {
            spec.text_duration_ms
        } else {
            spec.caret_duration_ms
        }),
        units,
        old_cursor_rect: spec.old_cursor_rect,
        new_cursor_rect: spec.new_cursor_rect,
        cursor_visual_track,
        cancel_reason: None,
        texture_prepared: false,
        old_snapshot: Some(spec.old_snapshot),
        new_snapshot: Some(spec.new_snapshot),
        cursor_owner_epoch: spec.cursor_owner_epoch,
        caret_motion_retired: false,
        visual_affected_byte_range_old: spec.visual_affected_byte_range_old,
        visual_affected_byte_range_new: spec.visual_affected_byte_range_new,
        layout_basis_revision: spec.layout_basis_revision,
    })
}

// 四类动画 slice 的构造按编辑语义拆到 slices.rs，本文件保留事务装配、
// slice 合并与 coordinator 方法。
pub(crate) mod slices;

/// Issue #815 评论 5949097065 问题3: 由**同侧** slice 几何生成 caret 吞吐路径。
///
/// 唯一入口：形状判定、行范围收集、以及 Insert / Delete 两条路径的装配都收在这里，
/// `build_prepared_transaction` 只在 `assign_shared_line_masks` 之后调一次。
///
pub(crate) use slices::{
    assign_shared_line_masks, build_cluster_reflow_slices,
    build_composition_commit_crossfade_slices, build_delete_conceal_slices,
    build_insert_reveal_slices,
};

pub(crate) fn merge_adjacent_slices(slices: Vec<AnimatedSlice>) -> Vec<AnimatedSlice> {
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

fn can_merge(a: &AnimatedSlice, b: &AnimatedSlice) -> bool {
    // 条件 1：相同 kind
    if a.kind != b.kind {
        return false;
    }
    // 条件 2：相同 snapshot_id
    if a.snapshot_id != b.snapshot_id {
        return false;
    }
    match a.kind {
        AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal => {
            // Issue #808 评论 5919641249 修改 1/6: InsertReveal/DeleteConceal 不再用
            // union source_rect 合并成一个大 slice 来表达"行级共同 mask"。
            // union 出来的大矩形会把中间保留（不参与吞吐动画）的字符一起画进动画层，
            // 与 canonical 静态正文重影。共同边界改由 assign_shared_line_masks 写
            // line_mask_left/right 表达：每个 slice 保留自己的 rect，compute_frame
            // 再用行级 boundary 与本 slice rect 求交，所以这里不再合并。
            false
        }
        AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
            // Reflow 仍要求 byte range 相邻，不跨空格合并。
            if a.byte_end != b.byte_start {
                return false;
            }
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

fn merged_byte_range(a: (usize, usize), b: (usize, usize)) -> (usize, usize) {
    (a.0.min(b.0), a.1.max(b.1))
}

fn merge_two(a: &AnimatedSlice, b: &AnimatedSlice) -> AnimatedSlice {
    let (byte_start, byte_end) =
        merged_byte_range((a.byte_start, a.byte_end), (b.byte_start, b.byte_end));
    // Issue #738 评论 5789470425 问题2: 合并 reflow_anchors 列表，不丢子 cluster 身份。
    let merged_anchors: Vec<crate::sujian_editor_item::animated_slice::ReflowAnchor> = a
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
        // 只有 ReflowMove/ReflowCrossFade 会走到这里（InsertReveal/DeleteConceal
        // 已改由 assign_shared_line_masks 表达行级共同 mask）。行级 mask 字段
        // 对 Reflow 不参与 compute_frame，取首个 slice 的值即可。
        line_mask_left: a.line_mask_left,
        line_mask_right: a.line_mask_right,
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
        // Issue #808: 合并后的 slice 取首个 slice 的 caret 锚点。
        // 同一行的相邻 slice 共享同一 caret 锚点（行内插入/删除），取首个即可。
        caret_anchor_x: a.caret_anchor_x,
        caret_anchor_y: a.caret_anchor_y,
        is_caret_line: a.is_caret_line,
        // Issue #815 评论 5946701331: 合并只发生在同一行的 ReflowMove/ReflowCrossFade，
        // 不涉及吞字/吐字边界；吞吐字段原样取首个 slice。
        ingest_boundary_driver: a.ingest_boundary_driver,
        ingest_boundary_from_x: a.ingest_boundary_from_x,
        ingest_line_ord: a.ingest_line_ord,
        ingest_from_line_ord: a.ingest_from_line_ord,
        ingest_to_line_ord: a.ingest_to_line_ord,
        ingest_line_top: a.ingest_line_top,
        ingest_line_bottom: a.ingest_line_bottom,
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

impl LinuxEditorAnimationCoordinator {
    pub(crate) fn create_transaction_from_prepared_handoff(
        &mut self,
        prepared: Option<PreparedRebaseHandoff>,
        vt: &PreparedEditMotion,
        // Issue #756: 文字动画开关（coordinated || typing）与光标动画开关
        // （coordinated || smooth）由调用方按同一份设置算出，两者互相独立。
        text_animation_enabled: bool,
        caret_animation_enabled: bool,
        // Issue #815 评论 6042062633 修改 4: 协同动画显式模式。协同打开时，
        // InsertReveal/DeleteConceal 由本事务的 `cursor_visual_track` 当前帧驱动，
        // 文字层与光标层消费同一次采样；Reflow 仍独立播放。
        coordinated_animation_enabled: bool,
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
    ) -> HandoffTransactionOutcome {
        let Some(handoff) = prepared else {
            return HandoffTransactionOutcome::Skipped(
                super::super::edit_flow::EditVisualSkipReason::BuilderEmptyTransaction,
            );
        };
        let PreparedRebaseHandoff {
            visual_state,
            inserted_ranges,
            deleted_ranges,
            offset_map,
            visual_affected_byte_range_old,
            visual_affected_byte_range_new,
        } = handoff;
        // Issue #824 评论 5971089641 第 2 节：Insert / Delete / Replace 共用同一段
        // 事务装配——正文动画种类只由 patches 的 inserted/deleted 事实决定，
        // 不再有“CompositionCommit 另一套正文动画分类”。
        let operation_kind = match (inserted_ranges.is_empty(), deleted_ranges.is_empty()) {
            (false, true) => TextVisualOperationKind::Insert,
            (true, false) => TextVisualOperationKind::Delete,
            (false, false) => TextVisualOperationKind::Replace,
            (true, true) => {
                return HandoffTransactionOutcome::Skipped(
                    super::super::edit_flow::EditVisualSkipReason::BuilderEmptyTransaction,
                );
            }
        };
        let key = self.alloc_key();
        // Issue #747 评论 5813540976: 只归一化 spec，统一由 build_prepared_transaction 构造。
        // build_prepared_transaction 内部调 build_insert_reveal_slices / build_delete_conceal_slices
        // / build_cluster_reflow_slices / match_rebase_frames / retarget /
        // build_cursor_visual_track 完成全部 slice/unit/track 构造。
        let carried_rebase = visual_state.rebase_frames.len();
        let spec = VisualEditSpec {
            key,
            operation_kind,
            // Issue #824: 正文动画种类来自 patch 事实。
            patch_kind: vt.kind,
            old_snapshot: old_snapshot.clone(),
            new_snapshot: new_snapshot.clone(),
            inserted_ranges: inserted_ranges.clone(),
            deleted_ranges: deleted_ranges.clone(),
            offset_map,
            old_cursor_rect,
            new_cursor_rect,
            old_cursor_visual_line_id,
            new_cursor_visual_line_id,
            old_cursor_line_top,
            old_cursor_line_bottom,
            new_cursor_line_top,
            new_cursor_line_bottom,
            cursor_owner_epoch,
            layout_basis_revision,
            visual_state,
            visual_affected_byte_range_old,
            visual_affected_byte_range_new,
            text_duration_ms: vt.text_duration_ms,
            caret_duration_ms: vt.caret_duration_ms,
            text_animation_enabled,
            caret_animation_enabled,
            coordinated_animation_enabled,
            composition_commit_crossfade: None,
        };
        // Issue #815 评论 6042062633 修改 8: builder 自己收口所有跳过点并记
        // `editor.anim.transaction_skipped`（协同模式拿不到 cursor track、
        // 有可见字符变化却既无 unit 又无 track）。合法的非可见输入
        // （空格/tab/换行）没有 InsertReveal 是正常行为，不记事件。
        // Issue #819 评论 5968931455 问题 2.2: builder 返回 BuildTransactionOutcome，
        // 透传 skip reason，不再用 `?` 吞掉。
        let prepared_tx = match build_prepared_transaction(spec) {
            BuildTransactionOutcome::Created(tx) => tx,
            BuildTransactionOutcome::Skipped(reason) => {
                return HandoffTransactionOutcome::Skipped(reason);
            }
        };

        // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
        emit_transaction_diagnostic(&prepared_tx, "editor.anim.create", "created");
        editor_animation_debug_log(&format!(
            "anim_event: key={:?} op={} inserted={:?} deleted={:?} unit_kinds={:?} carried_rebase={}",
            key,
            operation_kind_label(operation_kind),
            inserted_ranges,
            deleted_ranges,
            unit_kind_labels(&prepared_tx.units),
            carried_rebase,
        ));

        self.prepared_queue.enqueue(prepared_tx);

        HandoffTransactionOutcome::Created(key)
    }

    // process_transaction 只服务本文件的单元测试：正文事务的生产路径已经改成
    // `build_prepared_transaction` 直接构造，rebase 走 `rebind_timed_units_to_canonical`，
    // 没有任何生产调用方。整段 cfg(test)，避免在正常构建里被 dead_code 判死。
    // issue687/issue702 白盒测试按 `EditorAnimationKind::Insert/Delete/Cursor =>`
    // 锚点定位这段内联 match，位置保持在 transaction_builder.rs 不变。
    #[cfg(test)]
    pub fn process_transaction(
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
        // Issue #815 评论 5947443780 问题3: 协同语义 = 一条 caret 运动轨迹 +
        // 文字以该轨迹当前帧为吞吐边界 + Reflow 可独立。
        // - coordinated=true 时：InsertReveal/DeleteConceal 一律是 `CaretTrack`，
        //   没有自己的 progress，逐帧边界来自本事务 cursor track 的唯一一次采样
        //   （`sample_caret_track_frame`）；ReflowMove/ReflowCrossFade 仍是独立 `Timed`。
        //   这里仍要求有效 caret motion（old/new cursor rect 都在）——没有 track 就
        //   没有吞吐边界来源，协同事务必须拒绝而不是退化成文字自己播。
        // - coordinated=false 时：InsertReveal/DeleteConceal 退回独立 `Timed`
        //   （跟随 typing_animation_enabled），smooth_cursor_enabled 只决定光标动画
        //   （caret motion track）。两者互相独立，同时为 true 不等于协同：
        //   只有 coordinated_animation_enabled 才走协同路径。
        let text_animation_enabled = coordinated_animation_enabled || typing_animation_enabled;
        let caret_animation_enabled = coordinated_animation_enabled || smooth_cursor_enabled;
        if (!text_animation_enabled && !caret_animation_enabled)
            || is_scrolling
            || is_loading
            || is_applying_format
        {
            return None;
        }

        // Issue #815 评论 5947443780 问题3: valid_caret_motion_track 检查。
        // - coordinated=true 时：吞吐字是 `CaretTrack`，逐帧边界完全来自 cursor track
        //   的当前帧，所以这笔编辑必须有有效 caret motion，否则不创建事务。
        //   绝不能退化成"文字按自己的 timeline 播、光标不动"。
        // - coordinated=false 时：不把缺少 caret motion 当成"整笔不播"——文字动画（Reflow）
        //   与 cursor track 各自按自己的开关决定（无 caret motion 时只是没有 cursor
        //   track，文字 unit 仍走自己的 Timed 时间线，与 Issue #727 约束 5 一致）。
        let valid_caret_motion_track = old_cursor_rect.is_some() && new_cursor_rect.is_some();
        if coordinated_animation_enabled && !valid_caret_motion_track {
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
                    let visual_state = self.take_rebase_frames(
                        &conflicting,
                        "rebased_by_insert",
                        now,
                        Some((&[(range_start, range_start)], &insert_offset_map)),
                        &vt.old_text,
                        cursor_owner_epoch,
                    );

                    let key = self.alloc_key();
                    let inserted_range_tuple = (range_start, range_end);
                    // Issue #747 评论 5813540976: 只归一化 spec，统一由 build_prepared_transaction 构造。
                    // build_prepared_transaction 内部调 build_cluster_reflow_slices(key, old, new,
                    // offset_map, &[], &[inserted_range_tuple], ...) 排除 inserted_range，
                    // Issue #687: changed range 由 Core 显式拥有。
                    // Issue #756: InsertReveal 生成由 text_animation_enabled + caret_animation_enabled 决定，
                    // 不再把 typing && smooth 当成协同。
                    // Issue #710 评论 5732160521 问题 1/3: Insert 事务 old 侧是插入点
                    // (range_start, range_start)，new 侧是 inserted_range。
                    let carried_rebase = visual_state.rebase_frames.len();
                    let spec = VisualEditSpec {
                        key,
                        operation_kind: TextVisualOperationKind::Insert,
                        patch_kind: vt.kind,
                        old_snapshot: old_snapshot.clone(),
                        new_snapshot: new_snapshot.clone(),
                        inserted_ranges: vec![inserted_range_tuple],
                        deleted_ranges: vec![],
                        offset_map: insert_offset_map,
                        old_cursor_rect,
                        new_cursor_rect,
                        old_cursor_visual_line_id,
                        new_cursor_visual_line_id,
                        old_cursor_line_top,
                        old_cursor_line_bottom,
                        new_cursor_line_top,
                        new_cursor_line_bottom,
                        cursor_owner_epoch,
                        layout_basis_revision,
                        visual_state,
                        visual_affected_byte_range_old,
                        visual_affected_byte_range_new,
                        text_duration_ms: vt.text_duration_ms,
                        caret_duration_ms: vt.caret_duration_ms,
                        text_animation_enabled,
                        caret_animation_enabled,
                        coordinated_animation_enabled,
                        composition_commit_crossfade: None,
                    };
                    // Issue #815: 跳过点已由 builder 自己记正式事件，这里只传播 None。
                    // Issue #819 评论 5968931455 问题 2.2: builder 返回 BuildTransactionOutcome。
                    let prepared = match build_prepared_transaction(spec) {
                        BuildTransactionOutcome::Created(tx) => tx,
                        BuildTransactionOutcome::Skipped(_) => return None,
                    };

                    // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
                    emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
                    editor_animation_debug_log(&format!(
                        "anim_event: key={:?} op=Insert inserted={:?} unit_kinds={:?} carried_rebase={}",
                        key,
                        inserted_range_tuple,
                        unit_kind_labels(&prepared.units),
                        carried_rebase,
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
                let visual_state = self.take_rebase_frames(
                    &conflicting,
                    "rebased_by_delete",
                    now,
                    Some((&deleted_ranges, &delete_offset_map)),
                    &vt.old_text,
                    cursor_owner_epoch,
                );

                let key = self.alloc_key();

                // Issue #747 评论 5813540976: 只归一化 spec，统一由 build_prepared_transaction 构造。
                // build_prepared_transaction 内部调 build_cluster_reflow_slices(key, old, new,
                // offset_map, &deleted_ranges, &[], ...) 排除 deleted_range，
                // Issue #687: changed range 由 Core 显式拥有。
                // Issue #756: DeleteConceal 生成由 text_animation_enabled + caret_animation_enabled 决定，
                // 不再由 smooth_cursor_enabled 单独决定，也不再把 typing && smooth 当成协同。
                // Issue #710 评论 5732160521 问题 1/3: Delete 事务 old 侧是 deleted_range，
                // new 侧是删除后落点 (rebase_byte_start, rebase_byte_start)。
                let carried_rebase = visual_state.rebase_frames.len();
                let spec = VisualEditSpec {
                    key,
                    operation_kind: TextVisualOperationKind::Delete,
                    patch_kind: vt.kind,
                    old_snapshot: old_snapshot.clone(),
                    new_snapshot: new_snapshot.clone(),
                    inserted_ranges: vec![],
                    deleted_ranges: deleted_ranges.clone(),
                    offset_map: delete_offset_map,
                    old_cursor_rect,
                    new_cursor_rect,
                    old_cursor_visual_line_id,
                    new_cursor_visual_line_id,
                    old_cursor_line_top,
                    old_cursor_line_bottom,
                    new_cursor_line_top,
                    new_cursor_line_bottom,
                    cursor_owner_epoch,
                    layout_basis_revision,
                    visual_state,
                    visual_affected_byte_range_old,
                    visual_affected_byte_range_new,
                    text_duration_ms: vt.text_duration_ms,
                    caret_duration_ms: vt.caret_duration_ms,
                    text_animation_enabled,
                    caret_animation_enabled,
                    coordinated_animation_enabled,
                    composition_commit_crossfade: None,
                };
                // Issue #815: 同 Insert 分支。
                // Issue #819 评论 5968931455 问题 2.2: builder 返回 BuildTransactionOutcome。
                let prepared = match build_prepared_transaction(spec) {
                    BuildTransactionOutcome::Created(tx) => tx,
                    BuildTransactionOutcome::Skipped(_) => return None,
                };

                // Issue #690 评论 5675007226 步骤 5: 每笔动画一条紧凑事件进正式诊断包。
                emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
                editor_animation_debug_log(&format!(
                    "anim_event: key={:?} op=Delete deleted={:?} unit_kinds={:?} carried_rebase={}",
                    key,
                    deleted_ranges,
                    unit_kind_labels(&prepared.units),
                    carried_rebase,
                ));

                self.prepared_queue.enqueue(prepared);

                return Some(key);
            }
            EditorAnimationKind::Replace => {
                // Issue #824 评论 5971089641: Replace（patch 同时含 inserted 与
                // deleted，例如 IME 选区替换、粘贴覆盖选区）与 Insert/Delete 走
                // 同一段装配，只是两个 ranges 都非空。全部坐标来自 display patches。
                let inserted_ranges = vt.inserted_ranges.clone();
                let deleted_ranges = vt.deleted_ranges.clone();
                let offset_map = OffsetMap::build(&vt.old_text, &vt.new_text);
                let old_edit_range = (
                    deleted_ranges.first().map(|r| r.0).unwrap_or(0),
                    deleted_ranges.last().map(|r| r.1).unwrap_or(0),
                );
                let new_edit_range = (
                    inserted_ranges.first().map(|r| r.0).unwrap_or(0),
                    inserted_ranges.last().map(|r| r.1).unwrap_or(0),
                );
                let (visual_affected_byte_range_old, visual_affected_byte_range_new) = {
                    let (old_s, old_e, new_s, new_e) = compute_affected_paragraph_ranges(
                        &vt.old_text,
                        &vt.new_text,
                        old_edit_range,
                        new_edit_range,
                    );
                    (Some((old_s, old_e)), Some((new_s, new_e)))
                };
                let (conflict_old_start, conflict_old_end) =
                    visual_affected_byte_range_old.unwrap_or(old_edit_range);
                let conflicting = self.prepared_queue.find_conflicting_transaction(
                    &vt.old_text,
                    conflict_old_start,
                    conflict_old_end,
                );
                let now = Instant::now();
                let visual_state = self.take_rebase_frames(
                    &conflicting,
                    "rebased_by_replace",
                    now,
                    Some((&deleted_ranges, &offset_map)),
                    &vt.old_text,
                    cursor_owner_epoch,
                );
                let key = self.alloc_key();
                let carried_rebase = visual_state.rebase_frames.len();
                let spec = VisualEditSpec {
                    key,
                    operation_kind: TextVisualOperationKind::Replace,
                    patch_kind: vt.kind,
                    old_snapshot: old_snapshot.clone(),
                    new_snapshot: new_snapshot.clone(),
                    inserted_ranges,
                    deleted_ranges,
                    offset_map,
                    old_cursor_rect,
                    new_cursor_rect,
                    old_cursor_visual_line_id,
                    new_cursor_visual_line_id,
                    old_cursor_line_top,
                    old_cursor_line_bottom,
                    new_cursor_line_top,
                    new_cursor_line_bottom,
                    cursor_owner_epoch,
                    layout_basis_revision,
                    visual_state,
                    visual_affected_byte_range_old,
                    visual_affected_byte_range_new,
                    text_duration_ms: vt.text_duration_ms,
                    caret_duration_ms: vt.caret_duration_ms,
                    text_animation_enabled,
                    caret_animation_enabled,
                    coordinated_animation_enabled,
                    composition_commit_crossfade: None,
                };
                let prepared = match build_prepared_transaction(spec) {
                    BuildTransactionOutcome::Created(tx) => tx,
                    BuildTransactionOutcome::Skipped(_) => return None,
                };
                emit_transaction_diagnostic(&prepared, "editor.anim.create", "created");
                editor_animation_debug_log(&format!(
                    "anim_event: key={:?} op=Replace unit_kinds={:?} carried_rebase={}",
                    key,
                    unit_kind_labels(&prepared.units),
                    carried_rebase,
                ));
                self.prepared_queue.enqueue(prepared);
                return Some(key);
            }
            EditorAnimationKind::CursorOnly => {
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
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod handoff_tests;
