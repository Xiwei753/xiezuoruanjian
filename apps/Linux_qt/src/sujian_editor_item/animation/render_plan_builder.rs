use std::time::Instant;

use super::coordinator::{AnimationFrameSample, LinuxEditorAnimationCoordinator};
use super::transaction_builder::emit_transaction_diagnostic;
use crate::sujian_editor_item::animated_slice::AnimatedSlice;
use crate::sujian_editor_item::animation::{TextVisualOperationKind, TextVisualTransactionState};
use crate::sujian_editor_item::cursor_animation::{
    CursorAnimationPlan, CursorBlinkMode, CursorTransition,
};
use crate::sujian_editor_item::edit_motion::CursorRect;
use crate::sujian_editor_item::editor_animation_debug_log;
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::render_plan::{
    CursorRenderState, RenderPlan, SelectionPreeditPlan, TextAnimationGlyphInfo, TextAnimationPlan,
};
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;

impl LinuxEditorAnimationCoordinator {
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
        cursor_animation: Option<&crate::sujian_editor_item::rendering::CursorAnimationState>,
        cursor_owner_epoch: u64,
        cursor_move_source: crate::sujian_editor_item::cursor_controller::CursorMoveSource,
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
            .active_text_transaction_key_with_epoch(cursor_owner_epoch, layout_basis_revision)
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
            crate::sujian_editor_item::cursor_controller::CursorMoveSource::PointerClick
            | crate::sujian_editor_item::cursor_controller::CursorMoveSource::KeyboardNavigation => {
                smooth_cursor_enabled
            }
            crate::sujian_editor_item::cursor_controller::CursorMoveSource::DragSelection
            | crate::sujian_editor_item::cursor_controller::CursorMoveSource::LayoutChange
            | crate::sujian_editor_item::cursor_controller::CursorMoveSource::Scroll => false,
            crate::sujian_editor_item::cursor_controller::CursorMoveSource::TextTransaction => {
                false
            }
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

    pub(crate) fn begin_rendering_transactions(&mut self, frame_now: Instant) {
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

    pub(crate) fn build_render_plan_full(
        &mut self,
        mut cursor_render_state: CursorRenderState,
        selection_preedit: SelectionPreeditPlan,
        mut frame_context: crate::sujian_editor_item::render_plan::FrameContext,
        cursor_style: crate::sujian_editor_item::render_plan::CursorStyle,
        selection_preedit_style: crate::sujian_editor_item::render_plan::SelectionPreeditStyle,
        frame_now: Instant,
        cursor_animation: Option<&crate::sujian_editor_item::rendering::CursorAnimationState>,
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
        let (text_animation, keys_to_complete, _coordinated_motion_frame) = self
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
        let mut clip_rects: Vec<crate::sujian_editor_item::qt_text_node::AnimationClipRect> =
            Vec::new();
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
                    // Issue #756: 按 timing 判断 caret-driven（coordinated=true 吞吐字）。
                    let is_caret_driven = unit.timing.is_caret_driven();
                    // CaretDriven unit 只在拥有 caret frame 时收集；Timed 始终收集。
                    if is_caret_driven && (!has_caret_frame || !owns_caret) {
                        continue;
                    }
                    for doc_rect in &unit.slice.static_hidden_document_rects {
                        if doc_rect.h > 0.0 && doc_rect.w > 0.0 {
                            clip_rects.push(
                                crate::sujian_editor_item::qt_text_node::AnimationClipRect {
                                    x: doc_rect.x,
                                    y: doc_rect.y,
                                    w: doc_rect.w,
                                    h: doc_rect.h,
                                    snapshot_id: unit.slice.snapshot_id,
                                },
                            );
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
        let mut cursor_sample_outcome =
            crate::sujian_editor_item::render_plan::CursorSampleOutcome::Idle;
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
            cursor_sample_outcome =
                crate::sujian_editor_item::render_plan::CursorSampleOutcome::Coordinated {
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
                crate::sujian_editor_item::render_plan::CursorSampleOutcome::Running(p) => {
                    let eased = crate::sujian_editor_item::rendering::ease_out_cubic(p);
                    cursor_render_state.x = anim.start_x + (anim.target_x - anim.start_x) * eased;
                    cursor_render_state.y = anim.start_y + (anim.target_y - anim.start_y) * eased;
                }
                crate::sujian_editor_item::render_plan::CursorSampleOutcome::Finished => {
                    cursor_render_state.x = anim.target_x;
                    cursor_render_state.y = anim.target_y;
                }
                crate::sujian_editor_item::render_plan::CursorSampleOutcome::Idle => {}
                // Issue #702 评论 5707770318: sample_cursor_only_position 不会返回
                // Coordinated（它只服务纯光标 CursorOnly 动画），此分支不可达。
                crate::sujian_editor_item::render_plan::CursorSampleOutcome::Coordinated {
                    ..
                } => {}
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

    pub(crate) fn build_text_animation_plan_with_sample(
        &mut self,
        sample: &AnimationFrameSample,
        cursor_owner_epoch: u64,
        layout_basis_revision: LayoutRevision,
    ) -> (
        TextAnimationPlan,
        Vec<VisualTransactionKey>,
        crate::sujian_editor_item::render_plan::CoordinatedMotionFrame,
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
        let coordinated_motion_frame =
            self.sample_coordinated_motion_frame(sample, cursor_owner_epoch, layout_basis_revision);

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
            // Issue #756: 按 timing 判断是否 caret-driven。coordinated=false 的吞吐字
            // 是 Timed（typing-driven），不参与 caret motion retire 逻辑。
            let has_caret_driven_units = tx.units.iter().any(|u| u.timing.is_caret_driven());
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
            // Timed unit（Reflow + typing-driven 吞吐字）看 progress >= 1.0。
            let all_units_done = if tx.units.is_empty() {
                sample.progress(tx.key) >= 1.0
            } else {
                tx.units.iter().all(|u| {
                    if u.timing.is_caret_driven() {
                        true
                    } else {
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
                // Issue #756: 按 timing 区分 caret-driven 和 timed 吞吐字。
                // - CaretDriven（coordinated=true 的 InsertReveal/DeleteConceal）：从统一
                //   CoordinatedMotionFrame.caret 消费，按 owner_key 过滤。
                // - Timed（Reflow + coordinated=false 的 typing-driven 吞吐字）：用自己
                //   的时间线算 visible，走 compute_frame(visible)，不消费 caret frame。
                let frame = if unit.timing.is_caret_driven() {
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
                } else {
                    // Timed unit（Reflow / typing-driven 吞吐字）：用自己的时间线算 visible。
                    let visible = unit.current_visible_fraction(sample.frame_now);
                    unit.slice.compute_frame(visible)
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
}

#[cfg(test)]
mod tests;
