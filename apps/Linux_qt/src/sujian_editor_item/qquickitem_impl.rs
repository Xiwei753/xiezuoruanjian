use super::input_host::is_left_button_event;
use super::input_host::is_left_button_pressed;
use super::*;

use super::pointer_gesture::MoveOutcome;
use super::render_plan::{CursorStyle, FrameContext, SelectionPreeditStyle};
use super::scene_graph_renderer::StaticTextParams;
use std::time::Instant;

impl QQuickItem for SujianEditorItem {
    fn component_complete(&mut self) {
        let obj_ptr = self.get_cpp_object();
        if obj_ptr.is_null() {
            return;
        }
        let item_ptr = self as *mut Self as *mut std::ffi::c_void;
        input::install_event_filter(obj_ptr, item_ptr);
        self.pipeline.clipboard_adapter_mut().set_item_ptr(obj_ptr);
    }

    fn geometry_changed(&mut self, new_geometry: QRectF, old_geometry: QRectF) {
        // 宽度变化需要重新排版 QSGTextNode
        self.invalidate_layout_cache();
        // Issue #738 评论 5789470425 问题1: geometry_changed / layout_property_changed
        // 只负责标记 layout dirty（invalidate_layout_cache）。真正 reconcile 必须放到
        // **新 canonical layout 已经按新 width/font/line_spacing/padding 算完之后**。
        // 不再"先 bump，再拿 previous_canonical_snapshot reconcile"——那是用旧 canonical。
        //
        // 只变高度不推进 layout revision：排版只依赖宽度，高度变化不影响文字布局，
        // 不把正在播的事务全部判旧。只变宽度真正影响排版时才推进并 reconcile。
        let width_changed = (new_geometry.width - old_geometry.width).abs() > 0.5;
        // 先完成新排版（recalculate_content_height_and_emit 内部 ensure_layout_cached
        // 真正按新 width 排版），reconcile 发生在新排版完成之后。
        self.recalculate_content_height_and_emit();
        if width_changed {
            // 新 canonical 已按新 width 算完，用新 canonical reconcile 旧活动事务。
            self.reconcile_after_layout_change();
        }
        self.cursor_ctrl.force_snap_next = true;
        self.cursor_ctrl.last_move_source = cursor_controller::CursorMoveSource::LayoutChange;
        let _ = self.update_cursor_visual_position();
        // request_static_repaint 会在 GUI 线程预计算 snapshot
        self.request_static_repaint();
    }

    fn mouse_event(&mut self, event: QMouseEvent) -> bool {
        let pos = event.position();
        match event.event_type() {
            qmetaobject::QMouseEventType::MouseButtonPress => {
                // Issue #819 评论 5967250411 问题 4：只有左键 press 才进入 pointer gesture。
                // 右键不进入 pointer gesture（右键菜单由 QML TapHandler acceptedButtons:
                // Qt.RightButton 独立处理）。以前不判断按钮，qt_surface 是 AllButtons，
                // 右键也会进入 pointer_gesture.press()。
                if !is_left_button_event(&event) {
                    return true;
                }
                // Issue #819 评论 5956495850 第 6 节：左键 press 通过状态机驱动。
                // 先 hit_test 得到 hit_index 作为拖选 anchor，再 click_at 设置 cursor。
                // 状态机 press 会清除上一轮手势的残留状态（pointer_drag_selecting /
                // selection_gesture_active），避免上一轮手势的 Snap/隐藏状态污染本次点击。
                let (hit_index, _) = self.hit_test(f64::from(pos.x), f64::from(pos.y));
                self.pointer_gesture
                    .press((pos.x as f32, pos.y as f32), hit_index, Instant::now());
                self.sync_pointer_gesture_flags();
                self.click_at(pos.x as f32, pos.y as f32, false);
                let obj_ptr = self.get_cpp_object();
                input::focus_item(obj_ptr);
                // Issue #819 评论 5967250411 问题 4：左键 press 启动长按 Timer。
                // 不再由 QML TapHandler 接管 pointer event——Timer 只由 Rust property
                // 控制启停，到点调 activate_pointer_long_press。
                self.start_long_press_timer(pos.x as f32, pos.y as f32);
            }
            qmetaobject::QMouseEventType::MouseMove => {
                if is_left_button_pressed(&event) {
                    // Issue #819 评论 5956495850 第 6 节：move 通过状态机驱动。
                    // 状态机决定是否进入/继续 DragSelecting / LongPressSelecting。
                    // selection_gesture_active 决定 render_plan_builder 的 hard_snap，
                    // 不再用 Core has_selection（选区是否存在）代替手势状态。
                    let outcome = self.pointer_gesture.move_pos((pos.x as f32, pos.y as f32));
                    self.sync_pointer_gesture_flags();
                    match outcome {
                        MoveOutcome::StartedDragSelect { .. }
                        | MoveOutcome::ContinueDragSelect { .. }
                        | MoveOutcome::ContinueLongPressSelect { .. } => {
                            // 从 anchor 持续扩选到当前位置。
                            // drag_select_at 内部用 Core selection_anchor() 作为 anchor，
                            // 与状态机 anchor 一致（press 时 click_at 设置
                            // selection(hit_index, hit_index)）。
                            self.drag_select_at(pos.x as f32, pos.y as f32);
                            // Issue #819 评论 5967250411 问题 4：拖选超过阈值，停止长按 Timer。
                            // ContinueLongPressSelect 时长按已激活，Timer 已触发，stop 无副作用。
                            self.stop_long_press_timer();
                        }
                        MoveOutcome::StillPressed | MoveOutcome::Ignored => {
                            // 未超过拖动阈值或无 press，不扩选。
                        }
                    }
                }
            }
            qmetaobject::QMouseEventType::MouseButtonRelease => {
                // Issue #819 评论 5967250411 问题 4：只有左键 release 才进入 pointer gesture。
                if !is_left_button_event(&event) {
                    return true;
                }
                // Issue #819 评论 5956495850 第 6 节：release 走统一的手势结束路径。
                // 状态机 release 清 pointer_drag_selecting / selection_gesture_active，
                // end_selection_gesture 额外告诉 cursor controller 手势结束，保留当前
                // selection head visual rect 供选区收起后恢复光标运动。
                // Issue #819 评论 5967250411 问题 4：release/cancel 的 selection gesture
                // 结束只由 qquickitem_impl 做一次，不再从 QML 调 end_selection_gesture_qml()。
                self.pointer_gesture.release();
                self.sync_pointer_gesture_flags();
                self.end_selection_gesture();
                // Issue #819 评论 5967250411 问题 4：release 停止长按 Timer。
                self.stop_long_press_timer();
            }
            _ => {}
        }
        true
    }

    fn update_paint_node(
        &mut self,
        mut node: qmetaobject::scenegraph::SGNode<qmetaobject::scenegraph::ContainerNode>,
    ) -> qmetaobject::scenegraph::SGNode<qmetaobject::scenegraph::ContainerNode> {
        let frame_start = Instant::now();

        // Issue #690 评论 5675007226 步骤 1: 整帧只取一次 Instant::now()。
        // 后续文字 progress、光标 progress、cursor timeline sample 全部从这一个时间点计算，
        // 消除 GUI 线程 FrameAnimation tick 和 Scene Graph 渲染帧之间的采样偏差。
        let frame_now = frame_start;
        self.last_frame_now = Some(frame_now);

        let animation_set_changed = self.tick_text_animations_with_time(frame_now);
        if animation_set_changed {
            self.scene_dirty = true;
        }

        // Issue #710 评论 5732160521 问题 2: 检测 blink 抑制状态的边沿变化，
        // 在边沿处重置 blink 状态，避免输入/光标动画时光标消失。
        // 统一用 current_cursor_blink_mode() == Suppressed 作为判断，覆盖
        // CursorOnly Tween 和正文事务两种 suppress 来源。
        // - false->true（suppressed 开始）：立即 blink_visible = true（光标从可见状态开始），
        //   suppressed 期间 blink 由 Suppressed 模式保持 opacity 固定为 1。
        // - true->false（suppressed 结束）：重置 blink_last_toggle = frame_now /
        //   blink_visible = true，重新开始正常 blink，不继承旧相位。
        let cur_cursor_blink_suppressed = self.current_cursor_blink_mode()
            == super::cursor_animation::CursorBlinkMode::Suppressed;
        if cur_cursor_blink_suppressed != self.prev_cursor_blink_suppressed {
            if cur_cursor_blink_suppressed {
                // false->true: suppressed 开始，光标从可见状态开始。
                self.cursor_ctrl.blink_visible = true;
            } else {
                // true->false: suppressed 结束，重新开始正常 blink，不继承旧相位。
                self.cursor_ctrl.blink_last_toggle = frame_now;
                self.cursor_ctrl.blink_visible = true;
            }
            self.prev_cursor_blink_suppressed = cur_cursor_blink_suppressed;
        }

        let item_ptr = self.get_cpp_object();
        let dpr = if !item_ptr.is_null() {
            renderer::sujian_item_dpr(item_ptr)
        } else {
            1.0
        };

        let vp_h = f64::from(self.current_viewport_height.max(1.0));
        let scroll_y = f64::from(self.current_scroll_y);
        let _content_h = f64::from(self.current_content_height);

        // Issue #658: 静态正文用 QSGTextNode（Qt 6.7+ 公开 API）。
        // needs_relayout 由 layout_dirty 控制：正文/字体/宽度变更时为 true，
        // 滚动时为 false（只更新位移矩阵，不重新排版）。
        // scene_dirty 为 true 时强制重建 Scene Graph 节点（如动画裁剪变化）。
        // Issue #736 评论 5786531280: base_needs_relayout 只是基础值，最终传给
        // static renderer 的 frame_needs_relayout 还需要等 render_plan 构造后
        // 加入 keys_to_complete / keys_to_cancel 条件，保证完成帧同帧回 canonical。
        let base_needs_relayout = self.layout_dirty || self.scene_dirty;
        if self.layout_dirty {
            self.layout_dirty = false;
        }
        if self.scene_dirty {
            self.scene_dirty = false;
        }

        // Issue #677 评论 5653315696: 先取得或创建 editor root。
        // 第一帧旧节点为 null 时，ensure_editor_root 创建新的 QSGTransformNode 根节点，
        // 不再因为 root 为空就整帧跳过渲染。这把"根节点是否为空"的判断从
        // ensure_four_layer_nodes 收口到 ensure_editor_root。
        // Issue #677 评论 5653790560: 不再通过 into_raw / from_raw 做所有权往返，
        // 直接读写 node.raw（SGNode.raw 是 pub 字段）。第一帧 node.raw 为 null 时
        // ensure_editor_root 创建根节点并写回 node.raw；后续渲染统一用 node.raw。
        node.raw = scene_graph::ensure_editor_root(node.raw);
        let editor_root = node.raw;

        if !editor_root.is_null() && !item_ptr.is_null() {
            scene_graph::ensure_four_layer_nodes(editor_root, item_ptr);

            let has_active_txs = !self
                .pipeline
                .animation_coordinator_mut()
                .prepared_queue
                .is_empty();

            if !has_active_txs {
                self.pipeline.texture_cache_mut().clear();
                scene_graph::clear_animation_layer(editor_root, item_ptr);
            }

            // Issue #679 评论 5657313927: 删除 render thread 的第二次 build_cursor_plan()
            // 调用。不再把 cursor_ctrl.visual_x/y 同时当"当前值"和"目标值"传进去。
            // 直接从 GUI 侧已算好的 cursor_ctrl.visual_x/y/visual_h/visible 和
            // blink opacity 填进 RenderPlan 的 CursorRenderState。

            // Issue #701 评论 5699573227 第三阶段 (F5): 每帧只采样一次 frame state。
            // 不再在 build_render_plan_full 之外用 cursor_timeline_sample_with_time
            // 单独推进 cursor_ctrl.visual_x/y。CursorOnly 光标位置采样统一到
            // build_render_plan_full 内部，用同一份 AnimationFrameSample。
            // Issue #707 评论 5725190370: CursorRenderState 构造抽成
            // build_cursor_render_state_for_frame，和 runtime_tests 共用同一份逻辑。
            let cursor_render_state = self.build_cursor_render_state_for_frame();

            // Issue #677 评论 5653944889: render thread 只读 GUI 侧准备好的
            // `PreparedEditorFrame`，不再调用 `build_selection_preedit_plan()` /
            // `layout_snapshot()`，避免进入排版生命周期
            // （`EditorLayout::snapshot()` / `begin_layout_generation()` /
            // `clear_layout_generation()`）。
            // `prepared_frame = None` 时跳过静态正文渲染并请求下一次 GUI 帧准备。
            let prepared_frame = self.prepared_frame.as_ref();
            let selection_preedit = match prepared_frame {
                Some(frame) => frame.selection_preedit.clone(),
                None => render_plan::SelectionPreeditPlan::default(),
            };

            let frame_context = FrameContext {
                active_transaction_keys: Vec::new(),
                keys_to_complete: Vec::new(),
                keys_to_cancel: Vec::new(),
                layout_basis_revision: self.pipeline.layout_revision(),
            };
            let cursor_style = CursorStyle {
                color: self.current_cursor_color.to_string(),
                width: 2.0,
            };
            // Issue #677 评论 5654174714: selection/preedit 颜色是每帧轻量状态，
            // 不进入 PreparedEditorFrame；由 update_paint_node() 读取当前
            // current_selection_color 后构造 SelectionPreeditStyle 传给 RenderPlan。
            // preedit 的透明度（0x1A）由 renderer 在绘制时计算。
            let selection_preedit_style = SelectionPreeditStyle {
                selection_color: self.current_selection_color.to_string(),
            };

            let render_plan = self
                .pipeline
                .animation_coordinator_mut()
                .build_render_plan_full(
                    cursor_render_state,
                    selection_preedit,
                    frame_context,
                    cursor_style,
                    selection_preedit_style,
                    frame_now,
                    self.cursor_ctrl.animation.as_ref(),
                    self.cursor_ctrl.cursor_owner_epoch,
                    scroll_y,
                );

            // Issue #736 评论 5786531280: 在 render_plan 构造之后才计算最终传给
            // static renderer 的 frame_needs_relayout。完成帧（keys_to_complete 非空）
            // 或 cancel 帧（keys_to_cancel 非空）必须同帧重建 static layer，按已经
            // 去掉完成/cancel 事务 clip 的 plan.clip_rects 恢复 canonical 正文，
            // 不等下一帧 scene_dirty。否则完成帧会出现"glyph 已没了、旧 static clip
            // 还在"的一帧空洞。
            let frame_needs_relayout = base_needs_relayout
                || !render_plan.frame_context.keys_to_complete.is_empty()
                || !render_plan.frame_context.keys_to_cancel.is_empty();

            // Issue #658: 静态正文层参数 — 读取 GUI 线程预计算的快照。
            // Issue #677 评论 5653944889: 快照和选区/preedit 几何都来自
            // `PreparedEditorFrame`，render thread 不再自行排版。
            // `prepared_frame = None` 时不排版，请求下一次 GUI 侧准备。
            let has_snapshot = prepared_frame.is_some();
            let static_text = StaticTextParams {
                layout_snapshot: prepared_frame.map(|f| &f.layout_snapshot),
                scroll_y,
                viewport_height: vp_h,
                color: &self.current_text_color.to_string(),
                needs_relayout: frame_needs_relayout,
            };

            // Issue #668 评论 5646458592 问题 1: 接住静态正文 rebuild 的成功/失败结果。
            // rebuild 失败（某个必需 layout 缺失）时不能把本次静态正文更新当成已经完成；
            // 保留 layout_dirty / scene_dirty，下一次 update_paint_node 仍需要继续
            // 处理正确的 snapshot/generation。同时请求下一帧更新，让 GUI 线程
            // prepare_editor_frame 重新排版（snapshot() 会发现
            // cache 无效而分配新 generation 重新排版）。
            let static_rebuild_ok = scene_graph_renderer::render_frame(
                editor_root,
                item_ptr,
                &static_text,
                &render_plan,
                self.pipeline.texture_cache(),
            );

            // Issue #707 评论 5725190370: cursor_sample_outcome 更新 + drawn_caret_rect
            // 回写抽成 apply_render_plan_cursor_state，和 runtime_tests 共用同一份逻辑。
            // 原内联逻辑（Running/Finished/Coordinated/Idle 4 分支 + Issue #705 回写）
            // 移到方法定义处，这里只调一次方法。
            // 放在 render_frame 之后：render_frame 只读 &render_plan 和 &static_text
            // （持有 prepared_frame 引用），不读 cursor_ctrl；回写只改 cursor_ctrl，
            // 不影响 render_frame。原内联代码在 render_frame 之前，因 NLL 能区分
            // cursor_ctrl 和 prepared_frame 字段借用；抽成方法后 &mut self 与
            // prepared_frame 不可变借用冲突，故移到 render_frame 借用结束之后。
            // 语义等价：request_frame_update 的 cursor_ctrl.animation 判断在本调用
            // 之后，看到的仍是回写后的状态。
            self.apply_render_plan_cursor_state(&render_plan, frame_now, scroll_y);

            if !static_rebuild_ok && frame_needs_relayout {
                self.layout_dirty = true;
                self.scene_dirty = true;
                // Issue #677 评论 5653790560: render thread 不再反向排 GUI 线程补建。
                // snapshot/generation 生命周期在 GUI 侧一次收口：这里只保留当前静态
                // 正文节点并记录失败（layout_dirty / scene_dirty 置位），下一帧的
                // snapshot 准备由 GUI 侧的 invalidate_layout_cache() /
                // request_static_repaint() 在正常编辑路径中完成。删除
                // static_snapshot_reprepare_pending 标记和
                // schedule_static_snapshot_reprepare_on_gui_thread 调用，避免
                // render thread -> GUI thread queued 跨线程重排旧链。
            }

            // 没有准备好的 frame 时，跳过静态正文渲染并请求下一次 GUI 帧准备。
            // 放在 render_frame 之后，避免与 static_text 的不可变借用冲突。
            if !has_snapshot && frame_needs_relayout {
                self.request_frame_update();
            }

            for key in &render_plan.frame_context.keys_to_complete {
                // Issue #736 评论 5786231506: 不再 remove_for_transaction，统一在
                // transaction set 变化后用 retain_active_snapshot_ids。
                if let Some(_ids) = self
                    .pipeline
                    .animation_coordinator_mut()
                    .finish_by_key(*key)
                {
                    // 不再在这里释放纹理，统一在下面 retain。
                    // Issue #658 评论 5630650436: GPU texture cache 现在由 AnimationLayerNode
                    // 自身持有，不再需要手动 release。sweep 在每帧 update_animation_layer 时运行。
                }
                editor_animation_debug_log(&format!(
                    "update_paint_node: tid={}, gen={} completed (progress >= 1.0)",
                    key.transaction_id, key.generation
                ));
            }

            let mut transaction_set_changed = false;
            for key in &render_plan.frame_context.keys_to_cancel {
                if self
                    .pipeline
                    .animation_coordinator_mut()
                    .cancel_by_key(*key, "texture_failed")
                {
                    transaction_set_changed = true;
                }
            }

            if !render_plan.frame_context.keys_to_complete.is_empty() || transaction_set_changed {
                self.scene_dirty = true;
                // Issue #736 评论 5786231506: transaction set 变化后，从 coordinator 取
                // 当前全部 active snapshot ids，只释放已经没有任何 active transaction
                // 引用的纹理。rebase/cancel/complete 都不会误删下一笔仍在用的旧快照纹理。
                let active_ids = self
                    .pipeline
                    .animation_coordinator_mut()
                    .collect_active_snapshot_ids();
                self.pipeline
                    .texture_cache_mut()
                    .retain_active_snapshot_ids(&active_ids);
            }

            // Issue #701 评论 5699573227 第三阶段 (F6): 有 active transaction 或光标动画
            // 未结束就持续请求下一帧，直到文字和光标一起结束。
            if self
                .pipeline
                .animation_coordinator_mut()
                .has_prepared_or_rendering()
                || self.cursor_ctrl.animation.is_some()
                || !render_plan.frame_context.keys_to_complete.is_empty()
                || transaction_set_changed
            {
                self.request_frame_update();
            }
        }

        let total_elapsed = frame_start.elapsed();
        if total_elapsed.as_millis() > 4 {
            editor_debug_log(&format!(
                "sujian_update_paint_node: total_ms={}, base_needs_relayout={}, dpr={:.2}",
                total_elapsed.as_millis(),
                base_needs_relayout,
                dpr,
            ));
        }

        // Issue #677 评论 5653790560: node.raw 已在函数开头被 ensure_editor_root 写回，
        // 直接返回 node，不再通过 from_raw(editor_root) 重建 wrapper。
        node
    }
}

impl SujianEditorItem {
    /// Issue #690 评论 5675007226 步骤 1: 接受统一 `frame_now`，
    /// 替代内部各自 `Instant::now()`。
    pub(crate) fn tick_text_animations_with_time(&mut self, frame_now: Instant) -> bool {
        self.pipeline.animation_coordinator_mut().tick(frame_now)
    }

    /// 构造和 `update_paint_node` 完全一致的 `CursorRenderState`。
    ///
    /// 从 `cursor_ctrl.visual_x/y/h/visible` 和当前 blink mode 算出 opacity。
    /// Issue #707 评论 5725190370: 把这段纯状态逻辑从 `update_paint_node` 抽出，
    /// 供正式渲染路径和 `runtime_tests` 共用，避免测试用 `CursorRenderState::default()`
    /// 冒充光标导致行为偏离生产路径。
    ///
    /// Issue #709 评论 issue-body-709: Insert 活跃期间 blink 保持 suppressed
    /// （opacity 固定为 1），由 Suppressed 模式负责。事务开始/结束的边沿重置
    /// （blink_visible = true / blink_last_toggle = now）在 update_paint_node 的
    /// 边沿检测中处理，确保光标从可见状态开始、事务结束后重新开始正常 blink。
    pub(crate) fn build_cursor_render_state_for_frame(
        &self,
    ) -> super::render_plan::CursorRenderState {
        // Issue #679 评论 5657313927 / #701 评论 5699573227 第三阶段 (F5):
        // 每帧只采样一次 frame state。blink mode 由 current_cursor_blink_mode()
        // 统一决定（Issue #710 评论 5732160521 问题 2），保证 tick/render/opacity/
        // 边沿 reset 四处一致。
        let blink_mode = self.current_cursor_blink_mode();
        super::render_plan::CursorRenderState {
            visible: self.cursor_ctrl.visible,
            x: self.cursor_ctrl.visual_x,
            y: self.cursor_ctrl.visual_y,
            h: self.cursor_ctrl.visual_h,
            opacity: self.cursor_ctrl.cursor_blink_opacity(blink_mode),
        }
    }

    /// Issue #707 评论 5725190370: 把 `update_paint_node` 里"根据 RenderPlan 更新
    /// `cursor_ctrl`"的纯状态逻辑抽成方法，供正式渲染路径和 `runtime_tests` 共用。
    ///
    /// 包含两段逻辑：
    /// 1. `cursor_sample_outcome` 的 4 分支 match（Running/Finished/Coordinated/Idle）
    ///    对 `cursor_ctrl` 的更新；
    /// 2. Issue #705: `drawn_caret_rect` 回写 `cursor_ctrl.visual_x/visual_y/visual_h`。
    ///
    /// 抽出后 `update_paint_node` 和测试用同一份回写代码，不再有"测试不调正式回写"
    /// 的缺口。
    pub(crate) fn apply_render_plan_cursor_state(
        &mut self,
        render_plan: &super::render_plan::RenderPlan,
        frame_now: std::time::Instant,
        scroll_y: f64,
    ) {
        use super::render_plan::CursorSampleOutcome;
        // Issue #701 评论 5699573227 第三阶段 (F5): 用 build_render_plan_full 内部
        // 同一份 frame_sample 采样的结果推进 cursor_ctrl.visual_x/y。
        // 文字层和光标层都使用同一份 frame state。
        // Issue #702: 纯光标移动不再依赖空 Cursor 文字事务。CursorAnimationState
        // 拥有自己的 timeline（started_at + duration_ms），首帧 started_at 为 None
        // 时用 frame_now 启动，之后每帧用 frame_now 推进 from→to 动画。
        // Issue #727 评论 5755858583 问题1: Coordinated/drawn_caret_rect 的 y 是文档坐标，
        // visual_y 现在也统一保存文档坐标（与 cursor_ctrl.target_y 一致），
        // 不再减 scroll_y。QML/IME 边界方法在返回前减 current_scroll_y 转视口坐标。
        match render_plan.cursor_sample_outcome {
            CursorSampleOutcome::Running(p) => {
                self.cursor_ctrl.update_animation_progress(p);
            }
            CursorSampleOutcome::Finished => {
                self.cursor_ctrl.finish_animation_to_target();
            }
            // Issue #702 评论 5707770318: 正文协同光标帧。
            // 把 cursor_ctrl.visual_x/visual_y/visual_h 同步为本帧真正画出的位置，
            // 不启动 CursorAnimationState.started_at（不创建独立 timeline）。
            // 同时清除残留的纯光标 animation，因为正文协同模式下不应有独立 timeline。
            // Issue #727 评论 5757225958 问题1: visual_y 保存文档坐标，不再减 scroll_y。
            CursorSampleOutcome::Coordinated { x, y, h } => {
                self.cursor_ctrl.visual_x = x;
                self.cursor_ctrl.visual_y = y;
                if h > 0.0 {
                    self.cursor_ctrl.visual_h = h;
                }
                self.cursor_ctrl.animation = None;
            }
            CursorSampleOutcome::Idle => {
                // Issue #702: 纯光标动画首帧启动 started_at。
                // 此分支现在只在"没有正文事务且没有 CursorOnly 动画"时到达。
                if let Some(ref mut anim) = self.cursor_ctrl.animation {
                    if anim.started_at.is_none() {
                        anim.started_at = Some(frame_now);
                    }
                }
            }
        }

        // Issue #705: 每帧生成 RenderPlan 后,把 cursor_ctrl.visual_x/
        // visual_y/visual_h 同步成 drawn_caret_rect(本帧真正绘制出去
        // 的 caret rect)。下一次输入、删除、鼠标点击创建新事务时,
        // 只允许从这个"上一帧真正画出来的位置" rebase。
        // cursor_ctrl.target_x/target_y 只表示逻辑目标,不被拿来当
        // 当前屏幕位置。
        // Issue #727 评论 5757225958 问题1: drawn_caret_rect 的 y 是文档坐标，
        // visual_y 现在也统一保存文档坐标，不再减 scroll_y。
        let _ = scroll_y;
        if let Some((cx, cy, ch)) = render_plan.drawn_caret_rect {
            self.cursor_ctrl.visual_x = cx;
            self.cursor_ctrl.visual_y = cy;
            if ch > 0.0 {
                self.cursor_ctrl.visual_h = ch;
            }
        }
    }
}

impl SujianEditorItem {
    /// Issue #819 评论 5956495850 第 6 节：把状态机的两个布尔值同步回
    /// `self.pointer_drag_selecting` / `self.selection_gesture_active`。
    ///
    /// render_plan_builder / rendering 仍读 `self.selection_gesture_active` 字段
    /// （保留字段不破坏它们），但字段的值由状态机维护。每次状态机 transition
    /// 后调此方法同步。
    pub(crate) fn sync_pointer_gesture_flags(&mut self) {
        self.pointer_drag_selecting = self.pointer_gesture.pointer_drag_selecting();
        self.selection_gesture_active = self.pointer_gesture.selection_gesture_active();
    }

    /// Issue #819 评论 5967250411 问题 4：长按 Timer 启动。
    ///
    /// 由 `mouse_event` 左键 Press 调用。设置 `long_press_timer_active = true` 和
    /// 待处理位置 x/y，QML Timer.running 绑定该 property 自动启动。Timer 到点调
    /// `activate_pointer_long_press`。不再由 QML TapHandler 接管 pointer event。
    fn start_long_press_timer(&mut self, x: f32, y: f32) {
        self.long_press_pending_x = x;
        self.long_press_pending_y = y;
        self.set_long_press_timer_active(true);
    }

    /// Issue #819 评论 5967250411 问题 4：长按 Timer 停止。
    ///
    /// 由 `mouse_event` Release / Move 超阈值调用。设置 `long_press_timer_active = false`，
    /// QML Timer.running 绑定该 property 自动停止。
    fn stop_long_press_timer(&mut self) {
        self.set_long_press_timer_active(false);
    }

    pub(crate) fn long_press_timer_active(&self) -> bool {
        self.long_press_timer_active
    }

    pub(crate) fn set_long_press_timer_active(&mut self, v: bool) {
        if self.long_press_timer_active != v {
            self.long_press_timer_active = v;
            let obj_ptr = self.get_cpp_object();
            if !obj_ptr.is_null() {
                self.long_press_timer_changed();
            }
        }
    }

    pub(crate) fn long_press_pending_x(&self) -> f32 {
        self.long_press_pending_x
    }

    pub(crate) fn set_long_press_pending_x(&mut self, v: f32) {
        self.long_press_pending_x = v;
    }

    pub(crate) fn long_press_pending_y(&self) -> f32 {
        self.long_press_pending_y
    }

    pub(crate) fn set_long_press_pending_y(&mut self, v: f32) {
        self.long_press_pending_y = v;
    }

    /// Issue #819 评论 5956495850 第 6 节：QML Timer 长按到点时调用。
    ///
    /// 调状态机 `activate_long_press`，若成功激活（返回 true）则调
    /// `long_press_at` 选词。左键长按只负责选择，不弹菜单
    /// （菜单只由右键 TapHandler 触发）。
    ///
    /// 与旧 `begin_selection_gesture + long_press_at` 链路的区别：
    /// - 旧链路：QML onLongPressed 调 begin_selection_gesture（置
    ///   selection_gesture_active=true）+ long_press_at（选词 + 弹菜单）。
    /// - 新链路：QML Timer 到点调 activate_pointer_long_press，内部状态机
    ///   activate_long_press（置 selection_gesture_active=true +
    ///   pointer_drag_selecting=true）+ long_press_at（只选词，不弹菜单）。
    pub(crate) fn activate_pointer_long_press(&mut self, x: f32, y: f32) {
        let activated = self.pointer_gesture.activate_long_press();
        self.sync_pointer_gesture_flags();
        if activated {
            // 只在成功激活时选词。已在 DragSelecting 时忽略（拖选优先于长按）。
            self.long_press_at(x, y);
        }
    }

    /// Issue #810 评论 5932233052 问题3: 触屏/手写笔长按 selection gesture 生命周期入口。
    ///
    /// 保留供 QML 兼容调用，但内部改成通过状态机驱动：等价于在当前 press 窗口内
    /// 激活长按。Issue #819 评论 5956495850 第 7 节接线后，QML Timer 改调
    /// `activate_pointer_long_press`，此方法仅作向后兼容入口。
    pub(crate) fn begin_selection_gesture(&mut self) {
        // 状态机 activate_long_press 在 Pressed 状态下会置
        // selection_gesture_active=true / pointer_drag_selecting=true。
        // 若当前不在 Pressed（如 QML 在没有 press 的情况下调），则 no-op。
        let _ = self.pointer_gesture.activate_long_press();
        self.sync_pointer_gesture_flags();
    }

    /// Issue #810 评论 5932233052 问题3: 统一的选择手势结束路径。
    ///
    /// 由 `mouse_event` 的 `MouseButtonRelease` 调用，替代旧的
    /// `self.pointer_drag_selecting = false`。负责：
    /// 1. 清 `pointer_drag_selecting` / `selection_gesture_active`
    /// 2. 告诉 cursor controller 手势结束，保留当前 selection head 的 visual rect，
    ///    供选区收起后从该位置恢复 Tween（而非 Snap 瞬移）。
    ///
    /// 与 Core `has_selection` 的关系：本方法不改变 Core 选区状态（选区仍存在），
    /// 只结束平台手势状态。选区收起由后续的 click_at / 键盘导航 / delete 等操作触发，
    /// 那时 `selection_gesture_active` 已为 false，build_cursor_plan 不再强制 Snap，
    /// 从 `selection_head_rect`（= visual_x/visual_y）建 Tween 到新 cursor 位置。
    ///
    /// Issue #819 评论 5956495850 第 6 节：状态机 flag 同步由调用方
    /// （mouse_event release 分支）在调本方法前完成。本方法只负责 cursor_ctrl 收尾。
    fn end_selection_gesture(&mut self) {
        // 记录当前 selection head 的 visual rect。
        // 手势结束时光标因 has_selection 隐藏（should_be_visible=false），
        // visual_x/visual_y 是最后一次拖选的 cursor 位置（selection head）。
        // 选区收起后从此位置恢复 Tween。
        self.cursor_ctrl.record_selection_head_rect();
    }

    /// Issue #810 评论 5932233052 问题3: QML 暴露的选择手势结束入口。
    ///
    /// QML TapHandler onPressedChanged / onCanceled 在触屏/手写笔长按释放时调用，
    /// 委托到私有 `end_selection_gesture`，与鼠标 MouseButtonRelease 走统一路径。
    ///
    /// Issue #819 评论 5956495850 第 6 节：同时调状态机 cancel，确保状态机回到 Idle。
    pub(crate) fn end_selection_gesture_qml(&mut self) {
        self.pointer_gesture.cancel();
        self.sync_pointer_gesture_flags();
        self.end_selection_gesture();
    }
}
