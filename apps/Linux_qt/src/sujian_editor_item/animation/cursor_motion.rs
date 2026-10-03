use std::time::Instant;

use super::coordinator::{AnimationFrameSample, LinuxEditorAnimationCoordinator};
use super::rebase::RebaseCaretHandoff;
use crate::sujian_editor_item::animation::transaction::types::{
    CaretTrackSegment, PreparedCursorVisualTrack,
};
use crate::sujian_editor_item::animation::TextVisualTransactionState;
use crate::sujian_editor_item::edit_motion::CursorRect;
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::render_plan::SampledCaretFrame;
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;

pub(crate) fn build_cursor_visual_track(
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
    // Issue #815 评论 5949097065 问题3: 正式的运动路径，由调用方在 slice 建完、
    // `assign_shared_line_masks` 之后按**同侧** slice 几何生成。
    ingest_segments: Vec<CaretTrackSegment>,
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
            segments: ingest_segments,
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
            .map(|mut track| {
                track.set_segments(ingest_segments);
                track
            })
        }
    }
}

/// Issue #815 评论 6042062633 修改 3: 协同动画每帧的**唯一** caret 采样入口。
///
/// 在同一个 `frame_now` 上一次性算出 caret 的完整状态：文档坐标 rect、
/// x、top、当前 visual_line_id、track progress。协同模式下所有消费方都只调它：
/// - [`LinuxEditorAnimationCoordinator::sample_coordinated_motion_frame`]
///   （光标层与文字层共享的 `CoordinatedMotionFrame`）
/// - [`LinuxEditorAnimationCoordinator::compute_coordinated_cursor_position`]
///   （光标层画 caret，不再重新采样 track）
/// - `take_rebase_frames`（连续输入/删除时交棒新 track 的起点）
///
/// 任何一处都不许再自己算一次 `track.progress(now)` + `sampled_rect_at_progress`——
/// 那正是 #815 说的"文字和 caret 各跑一条时间线"的根因。
pub(crate) fn sample_caret_track_frame(
    track: &PreparedCursorVisualTrack,
    frame_now: Instant,
) -> SampledCaretFrame {
    let progress = track.progress(frame_now);
    let rect = track.sampled_rect_at_progress(progress);
    // Issue #815 评论 5949097065 问题3: 本帧的吞吐行与是否处于吞吐段，和 x/y/rect
    // 出自**同一次**路线采样。文字层直接消费这里给出的 ingest_line_ord /
    // is_ingest_segment，不再用 caret.y 猜行序、也不把换位段的对角线 x 当吞吐边界。
    // Issue #815 评论 5950887715: side 也出自同一次采样，文字层据此决定
    // "我这一侧现在该不该动"，避免 old/new 两套行号互相比较。
    let (visual_line_id, ingest_line_ord, is_ingest_segment, ingest_side, ingest_progress) =
        track.sampled_ingest_at_progress(progress);
    SampledCaretFrame {
        x: rect.x,
        y: rect.top,
        visual_line_id,
        progress,
        rect,
        ingest_line_ord,
        is_ingest_segment,
        ingest_side,
        ingest_progress,
    }
}

impl LinuxEditorAnimationCoordinator {
    pub(crate) fn active_text_transaction_key_with_epoch(
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
        if let Some(key) = self
            .active_text_transaction_key_with_epoch(current_cursor_epoch, current_layout_revision)
        {
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
                    // Issue #735 评论 5773604666 问题3 / Issue #785: 退休这笔事务的
                    // cursor motion ownership。Issue #819 评论 5967250411 问题 6：
                    // CaretTrack text unit 没有自己单独的 timeline，逐帧边界来自同一笔
                    // cursor track；epoch 失效后它们由 build_text_animation_plan_with_sample
                    // 收口到终态，不停在半路。Timed unit 按自己时间线继续播完/rebase。
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

    pub(crate) fn sample_coordinated_motion_frame(
        &self,
        sample: &AnimationFrameSample,
        cursor_owner_epoch: u64,
        layout_basis_revision: LayoutRevision,
    ) -> crate::sujian_editor_item::render_plan::CoordinatedMotionFrame {
        // 取当前 epoch 一致且 layout basis 未过期的活动正文事务。
        let key = match self
            .active_text_transaction_key_with_epoch(cursor_owner_epoch, layout_basis_revision)
        {
            Some(k) => k,
            None => {
                return crate::sujian_editor_item::render_plan::CoordinatedMotionFrame {
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
                return crate::sujian_editor_item::render_plan::CoordinatedMotionFrame {
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
            return crate::sujian_editor_item::render_plan::CoordinatedMotionFrame {
                caret: None,
                owner_key: None,
            };
        }
        // Issue #815 评论 6042062633 修改 3: 本帧唯一一次 caret track 采样。
        // 返回值同时喂给光标层和文字吞吐层；文字层不得再自己算一次时间。
        let track = match tx.cursor_visual_track.as_ref() {
            Some(track) => track,
            // 无 cursor_visual_track：无有效 caret motion。
            None => {
                return crate::sujian_editor_item::render_plan::CoordinatedMotionFrame {
                    caret: None,
                    owner_key: None,
                };
            }
        };
        crate::sujian_editor_item::render_plan::CoordinatedMotionFrame {
            caret: Some(sample_caret_track_frame(track, sample.frame_now)),
            // Issue #727 评论 5757225958 问题5 / Issue #815: 记录拥有此 caret frame 的事务 key。
            // 只有同 key 的 CaretTrack unit 能消费这份采样；其它事务的 CaretTrack unit
            // 立刻收口到终态。
            owner_key: Some(key),
        }
    }

    pub(crate) fn sample_cursor_only_position(
        &self,
        anim: &crate::sujian_editor_item::rendering::CursorAnimationState,
        sample: &AnimationFrameSample,
    ) -> crate::sujian_editor_item::render_plan::CursorSampleOutcome {
        // Issue #702 评论 5707449688 问题 2: 纯光标移动彻底和文字事务 key 解耦。
        // 用 CursorAnimationState 自己的 timeline（started_at + duration_ms）
        // 用 frame_now 推进 from→to 动画。
        let (progress, needs_start) = anim.sample_progress(sample.frame_now);
        if needs_start {
            // 首帧：started_at 尚未初始化，返回 Idle 让调用方用 frame_now 启动。
            crate::sujian_editor_item::render_plan::CursorSampleOutcome::Idle
        } else if progress >= 1.0 {
            crate::sujian_editor_item::render_plan::CursorSampleOutcome::Finished
        } else {
            crate::sujian_editor_item::render_plan::CursorSampleOutcome::Running(progress)
        }
    }

    /// Issue #815 评论 6042062633 修改 3: 光标层画 caret 的位置来源。
    ///
    /// 消费 [`CoordinatedMotionFrame`](crate::sujian_editor_item::render_plan::CoordinatedMotionFrame)
    /// 里那一份**已经采样好的** caret frame，不再自己重新采样 track、不再按
    /// `operation_kind` 分叉。本函数只做 ownership / 状态校验：
    /// 这份采样必须属于本事务（`owner_key == tx.key`），且事务处于
    /// Rendering / Paused。
    ///
    /// 返回 `(x, y_doc, h)`，其中 x/top 来自采样帧，h 仍是编辑后 canonical caret 的
    /// 稳定行高（不随 track 中间态插值，避免光标高度抖动）。
    pub(crate) fn compute_coordinated_cursor_position(
        &self,
        current_cursor_epoch: u64,
        motion: &crate::sujian_editor_item::render_plan::CoordinatedMotionFrame,
    ) -> Option<(f64, f64, f64)> {
        let key = *motion.owner_key.as_ref()?;
        let caret = *motion.caret.as_ref()?;
        let tx = self
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key)?;
        // Issue #705 评论 5717380886: epoch 不一致时返回 None——事务立刻失去 caret
        // motion ownership。但这只影响光标层：失去 ownership 的协同吞吐字会由
        // build_text_animation_plan_with_sample 立刻 retire 到终态，
        // 不再停在半路等一条已经不推进的 track。
        if tx.cursor_owner_epoch != current_cursor_epoch {
            return None;
        }
        if !matches!(
            tx.state,
            TextVisualTransactionState::Rendering | TextVisualTransactionState::Paused
        ) {
            return None;
        }
        let new_rect = tx.new_cursor_rect.as_ref()?;
        let h = new_rect.bottom - new_rect.top;
        Some((caret.x, caret.y, h))
    }
}
