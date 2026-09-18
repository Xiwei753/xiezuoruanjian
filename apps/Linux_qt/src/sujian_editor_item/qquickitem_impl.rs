use super::input_host::is_left_button_pressed;
use super::*;

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

    fn geometry_changed(&mut self, _new_geometry: QRectF, _old_geometry: QRectF) {
        // 宽度变化需要重新排版 QSGTextNode
        self.invalidate_layout_cache();
        self.recalculate_content_height_and_emit();
        self.cursor_ctrl.force_snap_next = true;
        let _ = self.update_cursor_visual_position();
        // request_static_repaint 会在 GUI 线程预计算 snapshot
        self.request_static_repaint();
    }

    fn mouse_event(&mut self, event: QMouseEvent) -> bool {
        let pos = event.position();
        match event.event_type() {
            qmetaobject::QMouseEventType::MouseButtonPress => {
                self.click_at(pos.x as f32, pos.y as f32, false);
                let obj_ptr = self.get_cpp_object();
                input::focus_item(obj_ptr);
            }
            qmetaobject::QMouseEventType::MouseMove => {
                if is_left_button_pressed(&event) {
                    self.drag_select_at(pos.x as f32, pos.y as f32);
                }
            }
            qmetaobject::QMouseEventType::MouseButtonRelease => {}
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

        // Issue #709 评论 issue-body-709: 检测正文协同事务（has_active_insert）的边沿变化，
        // 在边沿处重置 blink 状态，避免输入时光标消失。
        // - false->true（insert 开始）：立即 blink_visible = true（光标从可见状态开始），
        //   Insert 活跃期间 blink 由 build_cursor_render_state_for_frame 的 Suppressed 模式
        //   保持 suppressed（opacity 固定为 1）。
        // - true->false（insert 结束）：重置 blink_last_toggle = frame_now / blink_visible = true，
        //   重新开始正常 blink，不继承事务开始前碰巧为 false 的旧相位。
        // 与 build_cursor_render_state_for_frame 用同一个 has_active_insert 判断，保持一致。
        let cur_has_active_insert = self
            .pipeline
            .animation_coordinator()
            .has_active_insert();
        if cur_has_active_insert != self.prev_has_active_insert {
            if cur_has_active_insert {
                // false->true: insert 开始，光标从可见状态开始。
                self.cursor_ctrl.blink_visible = true;
            } else {
                // true->false: insert 结束，重新开始正常 blink，不继承旧相位。
                self.cursor_ctrl.blink_last_toggle = frame_now;
                self.cursor_ctrl.blink_visible = true;
            }
            self.prev_has_active_insert = cur_has_active_insert;
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
        let needs_relayout = self.layout_dirty || self.scene_dirty;
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
                None => animation_coordinator::SelectionPreeditPlan::default(),
            };

            let frame_context = FrameContext {
                active_transaction_keys: Vec::new(),
                keys_to_complete: Vec::new(),
                keys_to_cancel: Vec::new(),
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
                    self.current_coordinated_text_cursor_animation_enabled,
                    self.cursor_ctrl.animation.as_ref(),
                    self.cursor_ctrl.cursor_owner_epoch,
                );

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
                needs_relayout,
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
            self.apply_render_plan_cursor_state(&render_plan, frame_now);

            if !static_rebuild_ok && needs_relayout {
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
            if !has_snapshot && needs_relayout {
                self.request_frame_update();
            }

            for key in &render_plan.frame_context.keys_to_complete {
                if let Some(ids) = self
                    .pipeline
                    .animation_coordinator_mut()
                    .finish_by_key(*key)
                {
                    self.pipeline
                        .texture_cache_mut()
                        .remove_for_transaction(&ids);
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
                "sujian_update_paint_node: total_ms={}, needs_relayout={}, dpr={:.2}",
                total_elapsed.as_millis(),
                needs_relayout,
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
        // 每帧只采样一次 frame state。blink mode 由"是否有活动正文 insert 事务"
        // 决定：有则压制闪烁（Suppressed），无则正常闪烁（Normal）。
        // `has_active_insert` 是 `&self` 方法，这里用不可变 `animation_coordinator()`
        // 访问，避免要求 `&mut self`。
        let blink_mode = if self.current_coordinated_text_cursor_animation_enabled
            && self.pipeline.animation_coordinator().has_active_insert()
        {
            super::cursor_animation::CursorBlinkMode::Suppressed
        } else {
            super::cursor_animation::CursorBlinkMode::Normal
        };
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
    ) {
        use super::render_plan::CursorSampleOutcome;
        // Issue #701 评论 5699573227 第三阶段 (F5): 用 build_render_plan_full 内部
        // 同一份 frame_sample 采样的结果推进 cursor_ctrl.visual_x/y。
        // 文字层和光标层都使用同一份 frame state。
        // Issue #702: 纯光标移动不再依赖空 Cursor 文字事务。CursorAnimationState
        // 拥有自己的 timeline（started_at + duration_ms），首帧 started_at 为 None
        // 时用 frame_now 启动，之后每帧用 frame_now 推进 from→to 动画。
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
        if let Some((cx, cy, ch)) = render_plan.drawn_caret_rect {
            self.cursor_ctrl.visual_x = cx;
            self.cursor_ctrl.visual_y = cy;
            if ch > 0.0 {
                self.cursor_ctrl.visual_h = ch;
            }
        }
    }
}
