use super::*;

use super::pointer_gesture::MoveOutcome;
use super::render_plan::{CursorStyle, SelectionPreeditStyle};
use super::scene_graph_renderer::StaticTextParams;
use std::time::Instant;

cpp::cpp! {{
    #include <qmetaobject_rust.hpp>
    #include <QtQuick/QQuickItem>
    #include <QtQuick/QQuickWindow>
}}

fn qquickitem_window_changed_signal() -> qmetaobject::Signal<fn(*mut std::ffi::c_void)> {
    // SAFETY: QQuickItem::windowChanged has the exact signature void(QQuickWindow *).
    unsafe {
        qmetaobject::Signal::new(cpp::cpp!([] -> qmetaobject::SignalInner as "SignalInner" {
            return &QQuickItem::windowChanged;
        }))
    }
}

fn qquickwindow_after_synchronizing_signal() -> qmetaobject::Signal<fn()> {
    // SAFETY: QQuickWindow::afterSynchronizing has the exact signature void().
    unsafe {
        qmetaobject::Signal::new(cpp::cpp!([] -> qmetaobject::SignalInner as "SignalInner" {
            return &QQuickWindow::afterSynchronizing;
        }))
    }
}

fn qquickwindow_after_frame_end_signal() -> qmetaobject::Signal<fn()> {
    // SAFETY: QQuickWindow::afterFrameEnd has the exact signature void().
    unsafe {
        qmetaobject::Signal::new(cpp::cpp!([] -> qmetaobject::SignalInner as "SignalInner" {
            return &QQuickWindow::afterFrameEnd;
        }))
    }
}

/// Issue #901 第十三轮问题5: QQuickWindow::sceneGraphInvalidated 信号。
/// 在渲染线程触发，场景图被销毁/重建时 emit。用于在渲染线程安全清理
/// g_incremental_para_map 中该 item 的映射条目。
fn qquickwindow_scene_graph_invalidated_signal() -> qmetaobject::Signal<fn()> {
    // SAFETY: QQuickWindow::sceneGraphInvalidated has the exact signature void().
    unsafe {
        qmetaobject::Signal::new(cpp::cpp!([] -> qmetaobject::SignalInner as "SignalInner" {
            return &QQuickWindow::sceneGraphInvalidated;
        }))
    }
}

fn bind_frame_submission_window(
    window_ptr: *mut std::ffi::c_void,
    item_context_ptr: *mut std::ffi::c_void,
    rust_item_ptr: *mut std::ffi::c_void,
    connections: &std::rc::Rc<std::cell::RefCell<Vec<Box<dyn FnMut()>>>>,
    mailbox: &std::sync::Arc<frame_submission::FrameSubmissionMailbox>,
) {
    let window_generation = mailbox.reset_window();
    let mut connections = connections.borrow_mut();
    for disconnect in connections.iter_mut() {
        disconnect();
    }
    connections.clear();
    if window_ptr.is_null() {
        return;
    }

    let sync_mailbox = std::sync::Arc::clone(mailbox);
    let sync_generation = window_generation;
    // SAFETY: window_ptr is QQuickItem::window() (or its windowChanged argument), and
    // both direct callbacks capture only the thread-safe mailbox; neither touches item state.
    let mut after_synchronizing = unsafe {
        qmetaobject::connect(
            window_ptr,
            qquickwindow_after_synchronizing_signal(),
            move || sync_mailbox.after_synchronizing(sync_generation),
        )
    };
    let frame_mailbox = std::sync::Arc::clone(mailbox);
    let frame_generation = window_generation;
    // SAFETY: Same live QQuickWindow pointer and zero-argument signal signature as above.
    let mut after_frame_end = unsafe {
        qmetaobject::connect(
            window_ptr,
            qquickwindow_after_frame_end_signal(),
            move || frame_mailbox.after_frame_end(frame_generation),
        )
    };
    connections.push(Box::new(move || after_synchronizing.disconnect()));
    connections.push(Box::new(move || after_frame_end.disconnect()));

    // Issue #901 第十三轮问题5: 连接 sceneGraphInvalidated 信号。
    // 该信号在渲染线程触发（场景图被销毁/重建时），回调直接在渲染线程执行。
    // 在回调中清理 g_incremental_para_map[item_context_ptr]，与同在渲染线程的
    // update_paint_node 读写互斥安全。
    // item_context_ptr 是 C++ QQuickItem* 指针，用作 g_incremental_para_map 的 key。
    // clear_incremental_para_map_render_thread 只按指针值 erase，不解引用，安全。
    let scene_graph_item_key = item_context_ptr;
    // SAFETY: window_ptr is a live QQuickWindow pointer; sceneGraphInvalidated has
    // void() signature; the callback captures only a raw pointer value (Copy type)
    // and passes it to a C++ function that only uses it as an unordered_map key.
    let mut scene_graph_invalidated = unsafe {
        qmetaobject::connect(
            window_ptr,
            qquickwindow_scene_graph_invalidated_signal(),
            move || {
                qt_text_node::clear_incremental_para_map_render_thread(scene_graph_item_key);
            },
        )
    };
    connections.push(Box::new(move || scene_graph_invalidated.disconnect()));

    // SAFETY: These live Qt pointers come from this item/window binding. The C++ wrapper
    // stores the item as the queued connection context and captures this generation.
    let queued_frame_end = unsafe {
        input::connect_after_frame_end_queued(
            window_ptr,
            item_context_ptr,
            rust_item_ptr,
            window_generation,
        )
    };
    connections.push(Box::new(move || {
        // SAFETY: The opaque handle is returned by connect_after_frame_end_queued and this
        // stored disconnect closure is invoked once on rebind or item destruction.
        unsafe { input::disconnect_after_frame_end_queued(queued_frame_end) }
    }));
}

impl QQuickItem for SujianEditorItem {
    fn component_complete(&mut self) {
        let obj_ptr = self.get_cpp_object();
        if obj_ptr.is_null() {
            return;
        }
        let item_ptr = self as *mut Self as *mut std::ffi::c_void;
        input::install_event_filter(obj_ptr, item_ptr);
        self.pipeline.clipboard_adapter_mut().set_item_ptr(obj_ptr);

        let connections = std::rc::Rc::clone(&self.frame_window_connections);
        let mailbox = std::sync::Arc::clone(&self.frame_submission_mailbox);
        let rust_item = item_ptr;
        let item_context = obj_ptr;
        // SAFETY: QQuickItem::windowChanged's pointer argument is passed as one pointer-sized
        // value by Qt; this slot only uses it to bind that live window's frame signals.
        let mut window_changed_connection = unsafe {
            qmetaobject::connect(
                obj_ptr,
                qquickitem_window_changed_signal(),
                move |window_ptr: &*mut std::ffi::c_void| {
                    bind_frame_submission_window(
                        *window_ptr,
                        item_context,
                        rust_item,
                        &connections,
                        &mailbox,
                    );
                },
            )
        };
        self.window_changed_connection =
            Some(Box::new(move || window_changed_connection.disconnect()));
        bind_frame_submission_window(
            rendering::qquickitem_window(obj_ptr),
            obj_ptr,
            item_ptr,
            &self.frame_window_connections,
            &self.frame_submission_mailbox,
        );
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
        // Issue #904 清理: 宽度变化已由 recalculate_content_height_and_emit() 按新 width
        // 重排，canonical 由 EditorLayout 单一持有；活动中的局部动画随排版失效自然结束，
        // 静态层按新布局绘制，不再有第二份 canonical 需要 reconcile。
        let _ = width_changed;
        self.cursor_ctrl.force_snap_next = true;
        self.cursor_ctrl.last_move_source = cursor_controller::CursorMoveSource::LayoutChange;
        let _ = self.update_cursor_visual_position();
        // request_static_repaint 会在 GUI 线程预计算 snapshot
        self.request_static_repaint();
    }

    /// Issue #901 第十三轮问题5: Qt 官方 `QQuickItem::releaseResources()` 在 GUI 线程触发。
    ///
    /// 涉及 render 资源的清理需使用 `QQuickWindow::scheduleRenderJob()`，在渲染线程
    /// 执行清理 job。`g_incremental_para_map` 被 `update_paint_node`（渲染线程）读写，
    /// GUI 线程不能直接 erase（跨线程数据竞争）。
    ///
    /// `schedule_clear_incremental_para_map` 通过 `scheduleRenderJob` 在渲染线程
    /// 安全清理 `g_incremental_para_map[item_ptr]`。erase 只按指针值移除 map 条目，
    /// 不解引用 item_ptr，即使 job 执行时 item 已被销毁也安全。
    fn release_resources(&mut self) {
        let item_ptr = self.get_cpp_object();
        if !item_ptr.is_null() {
            qt_text_node::schedule_clear_incremental_para_map(item_ptr);
        }
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

        self.drain_pointer_frame_submissions();

        // Qt invokes this method during synchronization. Consume only acknowledgments that
        // arrived from afterFrameEnd.
        // Issue #904：帧提交 mailbox 的 texture retain 由唯一的局部动画计划与本帧
        // 静态贡献决定。retain_active_snapshot_ids 的真实 active ids 在下方采样
        // motion_frame 之后计算（需要 overlays 的 snapshot_ids +
        // mailbox 的未确认帧 snapshot_ids）。

        // Pure pointer/keyboard routes remain owned by CursorController. In coordinated
        // TextTransaction mode, build_render_plan_full samples the edit timeline once and
        // applies that same sample to both glyph geometry and the caret.
        self.cursor_ctrl.tick_animation(frame_now);

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

        // Issue #901 第十三轮问题5: 移除 pending_clear_incremental_map 延迟清理机制。
        // 旧机制在 Drop/切章时设置标记，依赖下一次 update_paint_node 消费，
        // 但 Drop 后 item 不再有下一帧，标记永远不会被消费。
        // 新机制通过 release_resources() + sceneGraphInvalidated 在渲染线程安全清理。

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
        // Issue #833：记录重置前的 layout_dirty / scene_dirty 快照，
        // 供"有正文但渲染帧为空"结构化诊断使用（下面会重置这两个标志）。
        let diag_layout_dirty = self.layout_dirty;
        let diag_scene_dirty = self.scene_dirty;
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
            // Issue #904：整个渲染热路径只消费唯一的局部动画计划。
            // 采样一次得到本帧覆盖层/静态孔洞/进度；没有动画计划时直接走静态绘制。
            let frame_now_ms = Self::frame_now_ms(&self.last_frame_now);
            let motion_frame = self
                .pipeline
                .text_motion()
                .map(|motion| motion.sample(frame_now_ms))
                .unwrap_or_default();

            let cursor_render_state = self.build_cursor_render_state_for_frame(&motion_frame);

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
            let canonical_visual_snapshot = self.pipeline.current_layout_snapshot().clone();
            // Issue #904 清理: canonical document snapshot 的唯一权威是 EditorLayout。
            let canonical_document_snapshot = self.editor_layout.last_canonical_snapshot();
            let prepared_frame_matches_canonical =
                match (prepared_frame, canonical_document_snapshot) {
                    (Some(frame), Some(canonical)) => {
                        let layout = &frame.layout_snapshot;
                        let committed_text = self.pipeline.committed_text();
                        layout.text_revision == canonical.text_revision
                            && layout.text_revision == self.pipeline.text_revision()
                            && layout.text_ptr == committed_text.as_ptr() as usize
                            && layout.text_len == committed_text.len()
                            && (layout.width - canonical.width).abs() <= 0.01
                            && (f64::from(layout.font_size) - canonical.font_size).abs() <= 0.01
                            && layout.font_family == canonical.font_family
                            && (f64::from(layout.line_spacing) - canonical.line_spacing).abs()
                                <= 0.01
                            && (f64::from(layout.text_indent) - canonical.text_indent).abs() <= 0.01
                            && (f64::from(layout.padding) - canonical.padding).abs() <= 0.01
                    }
                    _ => false,
                };

            let drawn_caret_rect = if cursor_render_state.visible {
                Some((
                    cursor_render_state.x,
                    cursor_render_state.y,
                    cursor_render_state.h,
                ))
            } else {
                None
            };

            // Issue #904 评论 1/3: 本帧局部动画的静态挖孔是否活跃（有需要从
            // canonical 里挖掉的接管目标）。挖孔只在动画 begin/target-change/end
            // 变化，不随 progress 每帧刷新；状态翻转那一帧必须强制刷新静态裁剪：
            // 开始帧同一帧挖空、结束帧同一帧恢复 canonical。
            let render_plan = super::render_plan::RenderPlan {
                local_text: super::render_plan::LocalTextPlan {
                    overlays: motion_frame.overlays.clone(),
                    static_holes: motion_frame.static_holes.clone(),
                    required_textures: motion_frame.required_textures.clone(),
                    paragraph_y_offsets: motion_frame.paragraph_y_offsets.clone(),
                },
                selection_preedit,
                cursor: cursor_render_state,
                cursor_style,
                selection_preedit_style,
                drawn_caret_rect,
            };

            // Issue #904：局部覆盖层/孔洞的源纹理是否已就绪。未就绪时静态层画完整
            // canonical、动画层不画（两层同一标志对称决策）。
            let animation_resources_ready = render_plan
                .local_text
                .has_animation_resources(self.pipeline.texture_cache());

            // Issue #904 评论 1: 只有资源就绪、本帧真的会挖孔时才算"期望挖孔"。
            // 翻转那一帧强制刷新静态裁剪：开始帧同一帧挖空、结束帧同一帧恢复
            // canonical、资源就绪帧同一帧补挖。
            //
            // Issue #904 复核 5: 这里比较的是"本帧期望"与"上一次**实际提交成功**的
            // 孔洞状态"，并且**不在这里提前记账**。Qt 首帧可能拒绝动画纹理，此时
            // 静态层并未真的挖孔；若提前把期望记成已提交，下一帧就不会补挖孔，
            // 动态图会和完整静态正文叠画。实际状态在 render_frame 返回后更新。
            let desired_holes_active =
                animation_resources_ready && !motion_frame.static_holes.is_empty();
            let clip_state_changed = desired_holes_active != self.prev_local_text_holes_active;

            // Issue #904 复核 12: 段落整体位移的 progress 帧**不再**走静态层重建 ——
            // 每帧只调用 `update_paragraph_motion_transforms` 直接 setMatrix 移动已有
            // QSGTransformNode 子树。只有"位移开始/结束"这一次翻转需要静态层刷新
            // （开始帧把矩阵落到目标 para_y + offset 基准，结束帧清回 para_y）。
            let paragraph_moves_active = !motion_frame.paragraph_y_offsets.is_empty();
            let paragraph_moves_changed =
                paragraph_moves_active != self.prev_paragraph_moves_active;
            self.prev_paragraph_moves_active = paragraph_moves_active;

            let frame_needs_relayout = base_needs_relayout
                || clip_state_changed
                || paragraph_moves_changed
                || !animation_resources_ready
                || !prepared_frame_matches_canonical;

            // Issue #658: 静态正文层参数 — 读取 GUI 线程预计算的快照。
            // Issue #677 评论 5653944889: 快照和选区/preedit 几何都来自
            // `PreparedEditorFrame`，render thread 不再自行排版。
            // `prepared_frame = None` 时不排版，请求下一次 GUI 侧准备。
            // Issue #901 第六轮阻塞5: 传入 height_index 让 scene_graph_renderer
            // 能 O(log n) 定位视口内首末可见段落，不遍历全章 lines。
            // height_index 与 prepared_frame.layout_snapshot 都是不可变借用 self，
            // 可安全共存。
            let has_snapshot = prepared_frame_matches_canonical;
            // Issue #901 第十四轮问题2: 消费 force_full_relayout_pending。
            // true 表示真正全局排版属性变化（字号/字体/行距/缩进/padding/宽度），
            // render_frame 传 force_full_relayout=true，C++ 侧无条件重建所有段落。
            // 普通正文输入不设此标志，由 per-paragraph content_revision 独立判断。
            let force_full_relayout = self.force_full_relayout_pending;
            self.force_full_relayout_pending = false;
            let static_text = StaticTextParams {
                layout_snapshot: prepared_frame
                    .filter(|_| prepared_frame_matches_canonical)
                    .map(|f| &f.layout_snapshot),
                height_index: Some(self.editor_layout.height_index()),
                scroll_y,
                viewport_height: vp_h,
                color: &self.current_text_color.to_string(),
                needs_relayout: frame_needs_relayout,
                force_full_relayout,
                force_static_clip_refresh: clip_state_changed,
                paragraph_y_offsets: &render_plan.local_text.paragraph_y_offsets,
                session_id: self.document_session,
                // Issue #901 第九轮阻塞4: per-item loaded_para_range 替代全局 static。
                loaded_para_range: self.loaded_para_range,
            };

            // Issue #668 评论 5646458592 问题 1: 接住静态正文 rebuild 的成功/失败结果。
            // rebuild 失败（某个必需 layout 缺失）时不能把本次静态正文更新当成已经完成；
            // 保留 layout_dirty / scene_dirty，下一次 update_paint_node 仍需要继续
            // 处理正确的 snapshot/generation。同时请求下一帧更新，让 GUI 线程
            // prepare_editor_frame 重新排版（snapshot() 会发现
            // cache 无效而分配新 generation 重新排版）。
            //
            // Issue #901 第九轮阻塞2：render_frame 返回 FrameRenderOutcome，
            // 包含 static_rebuild_ok 和 new_loaded_para_range。
            let render_outcome = scene_graph_renderer::render_frame(
                editor_root,
                item_ptr,
                &static_text,
                &render_plan,
                self.pipeline.texture_cache(),
            );
            let static_rebuild_ok = render_outcome.static_rebuild_ok;

            // Issue #904 复核 5: 只有本帧静态孔洞与动画层真正提交成功（两层同帧
            // 一致）时，才把"上一帧实际挖了孔"记成真实状态；提交失败时保留上次
            // 真实状态，本帧明确提交无孔洞 canonical 成功时写回 false。下一帧的
            // `clip_state_changed` 因此由**实际提交结果**驱动：Qt 首帧拒绝纹理、
            // 次帧可用时会强制补挖孔，动画结束帧同帧恢复完整 canonical。
            if render_outcome.static_rebuild_ok && render_outcome.animation_layer_ok {
                self.prev_local_text_holes_active = !render_outcome.committed_holes.is_empty();
            }

            // Issue #901 第九轮阻塞4: 更新 per-item loaded_para_range。
            // rebuild 成功时用新范围，失败时保留旧范围（new_loaded_para_range=None）。
            if let Some(new_range) = render_outcome.new_loaded_para_range {
                self.loaded_para_range = Some(new_range);
            }

            // Issue #904 评论 4: 只把**本帧真正提交到 QSG** 的局部片段记为
            // "上一次真正画出的局部图片"（当前矩形、当前遮罩宽度、源 id、段落 id、
            // session/revision），供快速连续输入/删除从当前画面重定向。
            // 计划侧不算数：静态提交失败或源纹理缺失的那一帧没有画出任何动画像素，
            // 此时保留上一帧已画出的记录；动画结束帧清空。
            //
            // 日志只在生命周期关键点各记一次，不逐帧刷屏：
            // 首次真正提交（holes/overlay/committed 个数）、源纹理缺失（具体 id）、
            // 静态提交失败、以及完成交接。
            let motion_identity = self
                .pipeline
                .text_motion()
                .map(|m| (m.session, m.target_revision));
            // Issue #904 复核 7：`motion_frame.finished` 只表示时间采样到了终点，
            // 不等于"终点这一帧已经真正交接出去"。真正完成的定义是：本帧静态层提交
            // 成功、动画层提交成功、且本帧**实际**没有挖孔、也没有动态覆盖层
            // （即静态正文已恢复完整 canonical，旧动态图已清空）。
            let completed_and_committed = motion_frame.finished
                && render_outcome.static_rebuild_ok
                && render_outcome.animation_layer_ok
                && render_outcome.committed_holes.is_empty()
                && render_outcome.committed_local.is_empty();
            if let Some((session, revision)) = motion_identity {
                if completed_and_committed {
                    super::editor_animation_debug_log(&format!(
                        "text_motion complete: session={session} revision={revision} \
                         duration_ms={} pieces={} paragraph_moves={}",
                        self.current_typing_animation_duration_ms,
                        self.pipeline.text_motion().map_or(0, |m| m.slices.len()),
                        self.pipeline
                            .text_motion()
                            .map_or(0, |m| m.paragraph_moves.len()),
                    ));
                    self.last_drawn_local_frame = None;
                    self.last_commit_log_key = None;
                    self.last_missing_log_key = None;
                } else if !animation_resources_ready {
                    // 源纹理缺失：写出具体缺少的 LineSnapshotId，不把它当成
                    // "这次本来就没有动画"。
                    if self.last_missing_log_key != Some((session, revision)) {
                        let missing = render_plan
                            .local_text
                            .missing_textures(self.pipeline.texture_cache());
                        super::editor_animation_debug_log(&format!(
                            "text_motion source missing: session={session} revision={revision} \
                             required={} missing={} ids={missing:?}",
                            render_plan.local_text.required_textures.len(),
                            missing.len(),
                        ));
                        self.last_missing_log_key = Some((session, revision));
                    }
                } else if !static_rebuild_ok {
                    if self.last_missing_log_key != Some((session, revision)) {
                        super::editor_animation_debug_log(&format!(
                            "text_motion static rebuild failed: session={session} revision={revision} \
                             overlays={} holes={}",
                            render_plan.local_text.overlays.len(),
                            render_plan.local_text.static_holes.len(),
                        ));
                        self.last_missing_log_key = Some((session, revision));
                    }
                } else if !render_outcome.animation_layer_ok {
                    // Issue #904 评论 4: 静态层已重建，但 C++ 动画层本帧未能建立
                    // 并绑定有效纹理。此时静态层未挖孔、画完整 canonical，本帧没有
                    // 画出任何动画像素，因此不记录 last_drawn_local_frame。日志沿用
                    // source missing / static rebuild failed 的同一 (session, revision)
                    // 去重键，保持每生命周期一行，不逐帧刷屏。
                    if self.last_missing_log_key != Some((session, revision)) {
                        super::editor_animation_debug_log(&format!(
                            "text_motion animation layer failed: session={session} revision={revision} \
                             overlays={} committed={}",
                            render_plan.local_text.overlays.len(),
                            render_outcome.committed_local.len(),
                        ));
                        self.last_missing_log_key = Some((session, revision));
                    }
                } else if !render_outcome.committed_local.is_empty() {
                    if self.last_commit_log_key != Some((session, revision)) {
                        super::editor_animation_debug_log(&format!(
                            "text_motion committed: session={session} revision={revision} \
                             holes={} overlays={} committed={} paragraph_offsets={}",
                            render_plan.local_text.static_holes.len(),
                            render_plan.local_text.overlays.len(),
                            render_outcome.committed_local.len(),
                            render_plan.local_text.paragraph_y_offsets.len(),
                        ));
                        self.last_commit_log_key = Some((session, revision));
                    }
                    self.last_drawn_local_frame = Some(super::text_motion::LocalDrawnFrame {
                        session,
                        revision,
                        slices: render_outcome.committed_local.clone(),
                    });
                }
            }

            // Issue #901 第九轮阻塞3：渲染成功后生成本帧 resource roots 并 retain。
            // 旧实现在渲染前做 retain，会 sweep 掉本帧即将创建的静态贡献的纹理。
            // 新实现移到渲染后，resource roots 覆盖：
            // (1) 最后确认的局部动画计划与本帧静态片段
            // (2) 所有待 afterFrameEnd 的帧（mailbox.active_snapshot_ids）
            // (3) 当前将渲染的实际静态/动画片段
            // (4) 新 target 中动画尚未出现但即将需要的纹理（视口内行的 line.id）
            {
                let mut active_ids = self.frame_submission_mailbox.active_snapshot_ids();
                // (3a) 当前将渲染的局部动画片段。
                for glyph in &motion_frame.overlays {
                    if !glyph.snapshot_id.is_none() && !active_ids.contains(&glyph.snapshot_id) {
                        active_ids.push(glyph.snapshot_id);
                    }
                }
                // (3c) 动画计划保留的旧/新源纹理（未删字仍需旧局部图片）。
                if let Some(motion) = self.pipeline.text_motion() {
                    for id in motion.source_snapshot_ids() {
                        if !id.is_none() && !active_ids.contains(&id) {
                            active_ids.push(id);
                        }
                    }
                }
                // (3b) 静态正文身份由常驻 QSGTextNode 持有；本帧活跃纹理只来自
                // (3a)/(3c) 局部动画、以及 (4) canonical 视口内行。
                // (1) 动画计划本身已在上方 (3a)/(3c) 覆盖。
                // (4) 新 target 中动画尚未出现但即将需要的纹理：
                // canonical_visual_snapshot 的视口内行的 line.id。
                // 这些行可能不在局部动画计划中（无动画），但下一帧
                // 编辑时可能需要它们的纹理。
                if let Some(snap) = canonical_visual_snapshot.as_ref() {
                    let vp_top = scroll_y - vp_h;
                    let vp_bottom = scroll_y + vp_h * 2.0;
                    for line in &snap.line_snapshots {
                        let line_y = line.visual_line_top;
                        let line_bottom = line.visual_line_bottom;
                        if line_bottom < vp_top || line_y > vp_bottom {
                            continue;
                        }
                        if !line.id.is_none() && !active_ids.contains(&line.id) {
                            active_ids.push(line.id);
                        }
                    }
                }
                self.pipeline.retain_active_snapshot_ids(&active_ids);
            }

            // Issue #904：整帧成功（静态层重建成功且快照与 canonical 一致）后，
            // 只把本帧实际引用的局部源纹理 id 记入提交 mailbox，供
            // active_snapshot_ids / retain 决定哪些 QImage 不能释放。
            if static_rebuild_ok && has_snapshot {
                let mut snapshot_ids: Vec<super::layout_snapshot::LineSnapshotId> = Vec::new();
                for glyph in &motion_frame.overlays {
                    if !glyph.snapshot_id.is_none() && !snapshot_ids.contains(&glyph.snapshot_id) {
                        snapshot_ids.push(glyph.snapshot_id);
                    }
                }
                if let Some(motion) = self.pipeline.text_motion() {
                    for id in motion.source_snapshot_ids() {
                        if !id.is_none() && !snapshot_ids.contains(&id) {
                            snapshot_ids.push(id);
                        }
                    }
                }
                self.frame_submission_mailbox.stage_rendered_frame(
                    self.document_session,
                    self.pipeline
                        .current_layout_snapshot()
                        .as_ref()
                        .map(|s| s.revision)
                        .unwrap_or_else(super::layout_revision::LayoutRevision::initial),
                    snapshot_ids,
                );
            }

            // Issue #707 评论 5725190370: drawn_caret_rect 回写抽成
            // apply_render_plan_cursor_state，和 runtime_tests 共用同一份逻辑。
            // Issue #826 评论 36: 原"Running/Finished/Coordinated/Idle 4 分支"
            // 正文事务驱动的 progress 更新已删除；推进改由帧首
            // cursor_ctrl.tick_animation(frame_now) 做，这里只调一次回写方法。
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
                self.request_frame_update();
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

            // Issue #904：只有一个动画进度。未结束就持续请求下一帧。
            // Issue #904 复核 7：时间到期也不等于终点已交接 —— 只有终点那一帧
            // 真正提交（静态恢复完整 canonical、旧动态图清空）才取走唯一动画；
            // 提交失败时保留 TextMotion 与上一实际帧，下一帧继续推进到成功为止，
            // 不新增第二个完成计时器或兜底动画。
            if motion_frame.needs_next_frame {
                self.request_frame_update();
            } else if motion_frame.finished && self.pipeline.text_motion().is_some() {
                if completed_and_committed {
                    *self.pipeline.text_motion_mut() = None;
                } else {
                    self.request_frame_update();
                }
            }
            if self.cursor_ctrl.animation.is_some() {
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

        // Issue #833：给"有正文但渲染帧为空"加一条结构化诊断，不改变渲染逻辑。
        // 条件：committed_text 非空，且 prepared_frame 为 None 或 item width/height <= 1。
        // 记录 text length、item width/height、viewport_height、scroll_y、
        // content_height、layout_dirty / scene_dirty。
        // 这一条是为了以后直接抓 Scene Graph 边界，不再靠肉眼猜正文到底丢在哪一层。
        //
        // Issue #833 复核：诊断不能进逐帧渲染热路径。正文动画期间 update_paint_node
        // 会连续跑帧，长章节每帧扫描整篇正文（committed.chars().count()）只为一个
        // 异常时才需要的日志，会给高刷动画增加 O(正文长度) 的额外成本。
        // 改成：先用 O(1) 状态判断 empty-frame 条件，只有确实满足时才取 committed text；
        // 先 is_empty()，确认非空后才 chars().count() 并写日志。正常渲染帧不扫描正文。
        {
            let item_w = self.bounding_width();
            let item_h = self.bounding_height();
            let item_vp_h = f64::from(self.current_viewport_height);
            // Issue #833 复核3：item_h 也要进触发条件。QQuickItem 自身 height 掉成 0
            // 但 viewport_height 仍保留上一帧旧值时，原条件漏判，恰好错过最想区分的
            // 几何错位。item_w/item_h/item_vp_h 都是 O(1) 几何状态，frame_empty 成立后
            // 才取 committed_text / chars().count()，热路径成本不变。
            let frame_empty =
                self.prepared_frame.is_none() || item_w <= 1.0 || item_h <= 1.0 || item_vp_h <= 1.0;
            if frame_empty {
                let committed = self.pipeline.committed_text();
                if !committed.is_empty() {
                    let text_bytes = committed.len();
                    let text_chars = committed.chars().count();
                    let item_scroll_y = f64::from(self.current_scroll_y);
                    let item_content_h = f64::from(self.current_content_height);
                    editor_debug_log(&format!(
                        "sujian_empty_frame_diag: text_bytes={}, text_chars={}, item_w={:.1}, item_h={:.1}, viewport_h={:.1}, scroll_y={:.1}, content_h={:.1}, layout_dirty={}, scene_dirty={}",
                        text_bytes,
                        text_chars,
                        item_w,
                        item_h,
                        item_vp_h,
                        item_scroll_y,
                        item_content_h,
                        diag_layout_dirty,
                        diag_scene_dirty,
                    ));
                }
            }
        }

        // Issue #677 评论 5653790560: node.raw 已在函数开头被 ensure_editor_root 写回，
        // 直接返回 node，不再通过 from_raw(editor_root) 重建 wrapper。
        node
    }
}

impl SujianEditorItem {
    pub(crate) fn record_pointer_activity(&mut self, kind: QString, x: f32, y: f32, buttons: i32) {
        let kind = kind.to_string();
        self.pointer_diagnostics.record_qml_activity(
            &kind,
            x,
            y,
            buttons,
            self.pointer_drag_selecting || self.selection_gesture_active,
        );
    }

    pub(crate) fn record_pointer_window_activity(
        &mut self,
        kind: i32,
        window_x: f64,
        window_y: f64,
        editor_x: f64,
        editor_y: f64,
        button: i32,
        buttons: i32,
        modifiers: i32,
        inside_editor: bool,
        active_focus: bool,
        qt_timestamp: u64,
        wheel_pixel_x: i32,
        wheel_pixel_y: i32,
        wheel_angle_x: i32,
        wheel_angle_y: i32,
    ) {
        self.pointer_diagnostics.record_window_activity(
            kind,
            window_x,
            window_y,
            editor_x,
            editor_y,
            button,
            buttons,
            modifiers,
            inside_editor,
            active_focus,
            qt_timestamp,
            wheel_pixel_x,
            wheel_pixel_y,
            wheel_angle_x,
            wheel_angle_y,
        );
    }

    pub(crate) fn flush_pointer_move_activity(&mut self) {
        let sequence = self.pointer_diagnostics.active_sequence();
        self.pointer_diagnostics.flush_moves(sequence);
    }

    pub(crate) fn finish_pointer_sequence(&mut self) {
        self.pointer_diagnostics.close_sequence();
    }

    pub(crate) fn pointer_press(&mut self, x: f32, y: f32) {
        self.pointer_diagnostics.ensure_active_sequence();
        let (hit_index, _) = self.hit_test(f64::from(x), f64::from(y));
        self.pointer_gesture
            .press((x, y), hit_index, Instant::now());
        self.sync_pointer_gesture_flags();
        self.click_at(x, y, false);
    }

    pub(crate) fn pointer_move(&mut self, x: f32, y: f32) {
        let outcome = self.pointer_gesture.move_pos((x, y));
        self.sync_pointer_gesture_flags();
        match outcome {
            MoveOutcome::StartedDragSelect { .. }
            | MoveOutcome::ContinueDragSelect { .. }
            | MoveOutcome::ContinueLongPressSelect { .. } => self.drag_select_at(x, y),
            MoveOutcome::StillPressed | MoveOutcome::Ignored => {}
        }
    }

    pub(crate) fn pointer_release(&mut self) {
        self.pointer_gesture.release();
        self.sync_pointer_gesture_flags();
        self.end_selection_gesture();
        self.pointer_diagnostics.close_sequence();
    }

    pub(crate) fn pointer_cancel(&mut self) {
        self.pointer_gesture.cancel();
        self.sync_pointer_gesture_flags();
        self.end_selection_gesture();
        self.pointer_diagnostics.close_sequence();
        self.request_static_repaint();
    }

    /// Issue #904：从唯一的局部动画计划采样帧构造 `CursorRenderState`。
    ///
    /// 正文协同输入时，位移/吐字进行中光标从 `motion_frame.caret_sample`
    /// （局部覆盖层右边界）获取；没有正文动画时直接用 `cursor_ctrl` 的实时位置。
    pub(crate) fn build_cursor_render_state_for_frame(
        &self,
        motion_frame: &super::text_motion::TextMotionFrame,
    ) -> super::render_plan::CursorRenderState {
        // Issue #679 评论 5657313927 / #701 评论 5699573227 第三阶段 (F5):
        // 每帧只采样一次 frame state。blink mode 由 current_cursor_blink_mode()
        // 统一决定（Issue #710 评论 5732160521 问题 2），保证 tick/render/opacity/
        // 边沿 reset 四处一致。
        let blink_mode = self.current_cursor_blink_mode();

        // Issue #904 复核三: 协同模式是完整模式——只要解析出的策略共享时间线、
        // 本帧确有 text_motion、且是正文事务（TextTransaction），光标就用正文动画
        // 的同一进度采样；不再额外要求原始 typing && smooth 同时为 true。
        // 纯点击/选区不被正文动画接管，仍走 cursor_ctrl 自己的 Snap/Tween。
        let coordinated_cursor = self.effective_animation().shared_timeline
            && self.pipeline.text_motion().is_some()
            && self.cursor_ctrl.last_move_source
                == super::cursor_controller::CursorMoveSource::TextTransaction;
        let (caret_x, caret_y) = match motion_frame.caret_sample {
            Some((x, y)) if coordinated_cursor => (x, y),
            _ => (self.cursor_ctrl.visual_x, self.cursor_ctrl.visual_y),
        };

        super::render_plan::CursorRenderState {
            visible: self.cursor_ctrl.visible,
            x: caret_x,
            y: caret_y,
            h: self.cursor_ctrl.visual_h,
            baseline_y: self.cursor_ctrl.visual_baseline_y,
            opacity: self.cursor_ctrl.cursor_blink_opacity(blink_mode),
        }
    }

    /// Issue #707 评论 5725190370: 把 `update_paint_node` 里"根据 RenderPlan 更新
    /// `cursor_ctrl`"的纯状态逻辑抽成方法，供正式渲染路径和 `runtime_tests` 共用。
    ///
    /// 包含两段逻辑：
    /// 1. Issue #705: `drawn_caret_rect` 回写 `cursor_ctrl.visual_x/visual_y/visual_h`
    ///    （只回写"本帧实际绘制位置"，不负责推进 progress）；
    /// 2. 同上回写后，下一笔 Tween 才能从"上一帧真正画出来的位置" rebase。
    ///
    /// Issue #826 评论 36：旧的"正文事务驱动 caret progress" 4 分支 match
    /// （Running/Finished/Coordinated/Idle）已彻底删除，对应枚举也不复存在；
    /// 视觉光标 Tween 的推进只由 `cursor_ctrl.tick_animation(frame_now)` 在
    /// `update_paint_node` 每帧做一次。`apply_plan()` 只负责创建/重基 Tween，
    /// 真正的推进不是它。
    ///
    /// 抽出后 `update_paint_node` 和测试用同一份回写代码，不再有"测试不调正式回写"
    /// 的缺口。
    pub(crate) fn apply_render_plan_cursor_state(
        &mut self,
        render_plan: &super::render_plan::RenderPlan,
        _frame_now: std::time::Instant,
        scroll_y: f64,
    ) {
        // Issue #826 评论 36: 光标与文字动画完全解耦。
        //
        // 视觉光标 Tween 由 `cursor_ctrl` 自己的 timeline 推进：`rendering.rs`
        // 的 `apply_plan` 只负责**创建/重基**（progress=0、started_at=None），
        // 真正的推进是 `tick_animation(frame_now)` —— `update_paint_node` 在
        // 整帧唯一的 frame_now 上每帧调一次，先推进 visual，再
        // `build_cursor_render_state_for_frame()` 读本帧位置。这里只把本帧
        // RenderPlan 的 caret 位置同步回 visual_x / visual_y / visual_h；这表示
        // Scene Graph 的计划位置，不代表 Qt 已提交或屏幕已显示。

        // Issue #705: 每帧生成 RenderPlan 后,把 cursor_ctrl.visual_x/
        // visual_y/visual_h 同步成 drawn_caret_rect（本帧计划的 caret rect）。
        // 下一次输入、删除、鼠标点击创建新事务时,从这个上一帧计划位置 rebase。
        // cursor_ctrl.target_x/target_y 只表示逻辑目标,不被拿来当
        // 当前屏幕位置。
        // Issue #727 评论 5757225958 问题1: drawn_caret_rect 的 y 是文档坐标，
        // visual_y 现在也统一保存文档坐标，不再减 scroll_y。
        let drawn_caret_rect = render_plan.drawn_caret_rect;
        if let Some((cx, cy, ch)) = drawn_caret_rect {
            self.cursor_ctrl.visual_x = cx;
            self.cursor_ctrl.visual_y = cy;
            if ch > 0.0 {
                self.cursor_ctrl.visual_h = ch;
            }
            self.cursor_ctrl.visual_baseline_y = render_plan.cursor.baseline_y;
        }

        let animation_active = self.cursor_ctrl.animation.is_some();
        if let Some((pointer_sequence, phase)) =
            self.pointer_diagnostics.next_frame_phase(animation_active)
        {
            let caret = render_plan.cursor;
            let submission = self.frame_submission_mailbox.stage_pointer_frame(
                frame_submission::PointerFrameSubmission {
                    pointer_sequence,
                    phase,
                    caret_present: drawn_caret_rect.is_some(),
                    drawn_caret_x: drawn_caret_rect.map(|rect| rect.0),
                    drawn_caret_y: drawn_caret_rect.map(|rect| rect.1),
                    drawn_viewport_y: drawn_caret_rect.map(|rect| rect.1 - scroll_y),
                    visible: caret.visible,
                    opacity: caret.opacity,
                    h: caret.h,
                    scroll_y,
                    effective_visible: caret.visible && caret.opacity > 0.0 && caret.h > 0.0,
                    animation_active,
                },
            );
            self.pointer_diagnostics.record_caret_submitted(submission);
        }
    }

    pub(crate) fn drain_pointer_frame_submissions(&mut self) {
        // Issue #904：不再有整章 PresentedScene 需要提交到 GUI 侧状态机；
        // 只清空已提交的视觉帧队列，指针帧仍交给诊断消费。
        let _ = self.frame_submission_mailbox.take_submitted_frames();

        let submitted = self
            .frame_submission_mailbox
            .take_submitted_pointer_frames();
        for frame in submitted {
            self.pointer_diagnostics.record_frame_end(frame);
        }
    }

    pub(crate) fn drain_pointer_frame_submissions_for_generation(
        &mut self,
        expected_generation: u64,
    ) {
        if self.frame_submission_mailbox.window_generation() != expected_generation {
            return;
        }
        self.drain_pointer_frame_submissions();
    }
}

#[no_mangle]
extern "C" fn sujian_drain_pointer_frame_submissions(
    rust_item: *mut std::ffi::c_void,
    window_generation: u64,
) {
    if rust_item.is_null() {
        return;
    }
    let _ = std::panic::catch_unwind(move || {
        // SAFETY: The queued Qt connection uses the QQuickItem QObject as its context and
        // Qt::QueuedConnection, so this callback runs on the item's GUI thread while it lives.
        let item = unsafe { &mut *(rust_item as *mut SujianEditorItem) };
        item.drain_pointer_frame_submissions_for_generation(window_generation);
    });
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

    /// Issue #898：QML MouseArea 的 onPressAndHold 调用。
    ///
    /// 调状态机 `activate_long_press`，若成功激活（返回 true）则调
    /// `long_press_at` 选词。左键长按只负责选择，不弹菜单
    /// （菜单只由右键 TapHandler 触发）。
    ///
    /// 状态机负责选择手势，右键菜单仍由右键 TapHandler 单独触发。
    pub(crate) fn activate_pointer_long_press(&mut self, x: f32, y: f32) {
        let activated = self.pointer_gesture.activate_long_press();
        self.sync_pointer_gesture_flags();
        if activated {
            // 只在成功激活时选词。已在 DragSelecting 时忽略（拖选优先于长按）。
            self.long_press_at(x, y);
        }
    }

    /// Issue #810 评论 5932233052 问题3: 统一的选择手势结束路径。
    ///
    /// 由 QML pointer release/cancel 入口调用，负责：
    /// 1. 清 `pointer_drag_selecting` / `selection_gesture_active`
    /// 2. 告诉 cursor controller 手势结束，保留当前 selection head 的 visual rect，
    ///    供选区收起后从该位置恢复 Tween（而非 Snap 瞬移）。
    ///
    /// 与 Core `has_selection` 的关系：本方法不改变 Core 选区状态（选区仍存在），
    /// 只结束平台手势状态。选区收起由后续的 click_at / 键盘导航 / delete 等操作触发，
    /// 那时 `selection_gesture_active` 已为 false，build_cursor_plan 不再强制 Snap，
    /// 从 `selection_head_rect`（= visual_x/visual_y）建 Tween 到新 cursor 位置。
    ///
    /// 状态机 flag 同步由调用方在调本方法前完成。本方法只负责 cursor_ctrl 收尾。
    fn end_selection_gesture(&mut self) {
        // 记录当前 selection head 的 visual rect。
        // 手势结束时光标因 has_selection 隐藏（should_be_visible=false），
        // visual_x/visual_y 是最后一次拖选的 cursor 位置（selection head）。
        // 选区收起后从此位置恢复 Tween。
        self.cursor_ctrl.record_selection_head_rect();
    }
}

// Issue #904：静态正文的绘制身份只由唯一的局部动画计划与常驻 QSGTextNode 决定，
// qquickitem_impl 不再需要前置的"谁被动画接管"推测逻辑。
