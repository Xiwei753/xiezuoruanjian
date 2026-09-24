use std::time::Instant;

use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};
use crate::sujian_editor_item::edit_motion::CursorRect;
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::animation::{
    PreparedCursorVisualTrack, PreparedTextVisualTransaction, TextVisualOperationKind,
    TextVisualTransactionState,
};
use super::coordinator::{AnimationFrameSample, LinuxEditorAnimationCoordinator};
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;
use super::rebase::RebaseCaretHandoff;

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

pub(crate) fn sample_coordinated_cursor_rect_at(
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
                return crate::sujian_editor_item::render_plan::CoordinatedMotionFrame {
                    caret: None,
                    owner_key: None,
                };
            }
        };
        crate::sujian_editor_item::render_plan::CoordinatedMotionFrame {
            caret: Some(crate::sujian_editor_item::render_plan::SampledCaretFrame {
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

    pub(crate) fn compute_coordinated_cursor_position(
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
}
