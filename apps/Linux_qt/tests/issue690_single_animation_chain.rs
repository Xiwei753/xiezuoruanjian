//! Issue #690 评论 5675007226 结构守卫 — Linux Qt 协同动画只剩一条采样链。
//!
//! WHITE_BOX 验证策略：通过读取源文件内容，确定性断言"两套采样 / 两套时钟 / 两套 easing"
//! 的缺陷模式已在代码中消除，而不是靠肉眼回归。覆盖评论的五个步骤：
//! （Issue #826 评论 36：#826 后光标与正文动画彻底解耦，断言已同步到新架构。）
//! 1. `update_paint_node()` 整帧只取一次时间，文字 tick 与光标 tick 共用同一个 frame_now；
//! 2. 光标位置由独立 cursor timeline（CursorAnimationState + tick_animation）推进，
//!    协同 easing / cursor_motion.rs 已删除；
//! 3. 遮罩前沿三层（EditFrontier/Reflow/Shaping）各持 started_at，由 coordinator::tick 推进；
//! 4. 空正文光标不再靠 FrameAnimation 每帧驱动，blink 切换本身请求重绘；
//! 5. 动画诊断进正式诊断包，且不再逐帧/无条件刷 stderr。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

fn linux_qt_root() -> PathBuf {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo");
    PathBuf::from(manifest_dir)
}

fn read_src(rel: &str) -> String {
    let path = linux_qt_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

/// 取出某个方法从签名到函数体结束（首个 4 空格缩进的 `}`）之间的文本。
fn method_body(src: &str, signature: &str) -> String {
    let start = src
        .find(signature)
        .unwrap_or_else(|| panic!("method `{}` must exist", signature));
    let rest = &src[start..];
    let end = rest
        .find("\n    }\n")
        .unwrap_or_else(|| panic!("method `{}` body end not found", signature));
    rest[..end].to_string()
}

// ─────────────────────────────────────────────────────────────────────────
// 步骤 1: 一帧只有一个采样时间点，文字与光标共用
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue690_update_paint_node_samples_clock_once_per_frame() {
    let src = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    let body = method_body(&src, "fn update_paint_node(");
    assert!(
        body.contains("let frame_now = frame_start;"),
        "步骤1: 本帧统一时间点必须来自函数入口的 frame_start"
    );
    assert_eq!(
        body.matches("Instant::now();").count(),
        1,
        "步骤1: update_paint_node 整帧只允许一次时间采样，不再各自 now()。\
         出现次数={}",
        body.matches("Instant::now();").count()
    );
    assert!(
        body.contains("tick_text_animations_with_time(frame_now)"),
        "步骤1: GUI 侧文字动画 tick 必须吃同一个 frame_now"
    );
    assert!(
        body.contains("self.last_frame_now = Some(frame_now);"),
        "步骤1: frame_now 必须留给 CursorOnly 链复用，避免第二次采样"
    );
    let build_call = &src[src
        .find("build_render_plan_full(")
        .expect("步骤1: update_paint_node 必须调用 build_render_plan_full")..];
    assert!(
        build_call.contains("frame_now,"),
        "步骤1: build_render_plan_full 必须接收本帧统一采样点"
    );
    println!("[BUGFIX_690_VERIFY] 步骤1 单帧单次采样 (FIXED)");
}

#[test]
fn issue690_frame_sample_drives_text_and_cursor() {
    // Issue #826 评论 36: 旧的 `AnimationFrameSample` + `compute_coordinated_cursor_position`
    // 协同采样链已随「光标与正文动画彻底解耦」删除（`animation/cursor_motion.rs`
    // 已不存在）。验证意图不变 —— 文字与光标仍然只有**一套时钟**：
    // 同一个 `frame_now`、整帧只有一次 `Instant::now()`、RenderPlanBuilder 纯读、
    // 光标 timeline 只有一个推进入口。
    let src = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    let body = method_body(&src, "fn update_paint_node(");
    assert_eq!(
        body.matches("Instant::now();").count(),
        1,
        "步骤1: update_paint_node 整帧只允许采样一次 Instant::now()"
    );
    let text_tick = body
        .find("self.tick_text_animations_with_time(frame_now)")
        .expect("步骤1: 文字动画 tick 必须吃同一个 frame_now");
    let cursor_tick = body
        .find("self.cursor_ctrl.tick_animation(frame_now);")
        .expect("步骤1: 光标 cursor timeline 必须吃同一个 frame_now");
    let state_pos = body
        .find("self.build_cursor_render_state_for_frame()")
        .expect("步骤1: 必须在推进之后才读本帧光标位置");
    assert!(
        text_tick < cursor_tick && cursor_tick < state_pos,
        "步骤1: 顺序必须是 文字 tick -> 光标 tick -> build_cursor_render_state_for_frame"
    );
    let render_src = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");
    let render_plan = method_body(&render_src, "fn build_render_plan_full(");
    assert!(
        !render_plan.contains("Instant::now()"),
        "步骤1: RenderPlanBuilder 仍只是纯读采样器，不得自己取时间（否则又是两套时钟）"
    );
    let ctrl_src = read_src("src/sujian_editor_item/cursor_controller.rs");
    assert!(
        ctrl_src.contains("pub(crate) fn tick_animation(&mut self, frame_now: Instant) -> bool"),
        "步骤1: cursor timeline 必须有唯一推进入口 tick_animation(frame_now)"
    );
    assert!(
        ctrl_src.contains("self.update_animation_progress(progress)"),
        "步骤1: progress 消费统一走 update_animation_progress（生产只留一个推进入口）"
    );
    println!("[BUGFIX_690_VERIFY] 步骤1 文字与光标共用一套时钟 (#826 评论36 FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 步骤 2: 光标 = 文字吞吐边界，且只有一条协同 easing
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue690_cursor_sits_on_text_reveal_and_conceal_boundary() {
    // Issue #826 评论 36: 旧的 cursor_motion.rs（compute_coordinated_cursor_position /
    // sample_caret_track_frame，正文事务协同光标那条采样链）已随「光标与正文动画彻底
    // 解耦」整文件删除，不得复活。现在光标是完全独立的 cursor timeline：位置只由
    // CursorAnimationState 自己的 start/target 插值得出，推进入口只有 tick_animation。
    let motion_path = linux_qt_root().join("src/sujian_editor_item/animation/cursor_motion.rs");
    assert!(
        !motion_path.exists(),
        "步骤2: 协调光标模块 cursor_motion.rs 已删除，不得复活正文事务协同光标采样链"
    );
    let rendering_src = read_src("src/sujian_editor_item/rendering.rs");
    let cursor = method_body(&rendering_src, "pub fn current_position(&self)");
    // Issue #722 评论 5747719529 修正的语义在新架构下由「视觉 caret 追逻辑 caret」承担：
    // 插值只发生在 CursorAnimationState 内部，不再从文字 glyph 切片反推光标位置。
    assert!(
        cursor.contains("start_x") && cursor.contains("target_x"),
        "步骤2: 光标位置由 CursorAnimationState 的 start/target 插值决定"
    );
    assert!(
        cursor.contains("ease_out_cubic"),
        "步骤2: 光标插值收敛在 CursorAnimationState::current_position，不得多处重复插值"
    );
    let ctrl_src = read_src("src/sujian_editor_item/cursor_controller.rs");
    assert!(
        ctrl_src.contains("fn tick_animation(&mut self, frame_now: Instant)"),
        "步骤2: 光标位置只能由 tick_animation(frame_now) 每帧推进"
    );
    println!("[BUGFIX_690_VERIFY] 步骤2 光标跟随吞吐边界 (FIXED)");
}

#[test]
fn issue690_single_collaborative_easing_function() {
    // Issue #826 评论 36: 旧 animated_slice.rs（ease_out_quad 协同曲线）已删除，
    // 协同 easing 不得复活；「一套 easing」的守卫改成对现存文件的断言。
    let slice_path = linux_qt_root().join("src/sujian_editor_item/animated_slice.rs");
    assert!(
        !slice_path.exists(),
        "步骤2: animated_slice.rs 已删除，协同二次曲线不得复活"
    );
    let coord_src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    assert!(
        !coord_src.contains("1.0 - (1.0 - "),
        "步骤2: 协调器内不得再内联各自的 easing 公式"
    );
    // CursorOnly（方向键/点选平滑）保留自己的三次曲线，不被协同链复用。
    let rendering_src = read_src("src/sujian_editor_item/rendering.rs");
    assert!(
        rendering_src.contains("powi(3i32)"),
        "步骤2: CursorOnly 的平滑曲线保持独立，不并入协同曲线"
    );
    println!("[BUGFIX_690_VERIFY] 步骤2 单条协同 easing (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 步骤 3: 视觉单元自持生命期，交棒带当前可见比例
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue690_rebase_frame_carries_visible_fraction_and_unit_timeline() {
    // Issue #826 评论 36: 旧 animation/transaction/ 模块（RebaseFrame 交棒续播、
    // 单元时间线 rebase_from_frame）已整目录删除，双时间线交棒不得复活。
    // 新架构：遮罩前沿三层（EditFrontier / ReflowMotion / ShapingTransition）
    // 各自持有 started_at，由 coordinator::tick(frame_now) 用同一帧时钟统一推进，
    // 完成与否按自己的 is_finished(now) 判定，不共享事务级 progress。
    let transaction_dir = linux_qt_root().join("src/sujian_editor_item/animation/transaction");
    assert!(
        !transaction_dir.exists(),
        "步骤3: animation/transaction 模块已删除，RebaseFrame 双时间线交棒不得复活"
    );
    let coord_src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    assert!(
        coord_src.contains("fn tick(&mut self, frame_now: Instant)"),
        "步骤3: 三层动画必须由 coordinator::tick(frame_now) 一个入口统一推进"
    );
    for rel in [
        "src/sujian_editor_item/animation/edit_frontier.rs",
        "src/sujian_editor_item/animation/reflow_motion.rs",
        "src/sujian_editor_item/animation/shaping_transition.rs",
    ] {
        let layer_src = read_src(rel);
        assert!(
            layer_src.contains("fn is_finished(&self, now: Instant)"),
            "步骤3: {} 必须按自己的 started_at(now) 判断完成，不用事务级 progress",
            rel
        );
    }
    println!("[BUGFIX_690_VERIFY] 步骤3 单元生命期 + 交棒续播 (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 步骤 4: 空正文光标 —— blink 自己请求重绘，QML 不再逐帧驱动
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue690_blink_change_requests_frame_update() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let body = method_body(&src, "pub(crate) fn tick_cursor_animation(");
    assert!(
        body.contains("blink_changed"),
        "步骤4: blink_changed 必须触发更新，否则空正文光标停在不可见"
    );
    let blink_idx = body
        .find("let blink_changed")
        .expect("步骤4: 必须仍有 blink tick");
    let after_blink = &body[blink_idx..];
    assert!(
        after_blink.contains("self.request_frame_update()"),
        "步骤4: blink 切换后要显式请求 Scene Graph 重绘"
    );
    println!("[BUGFIX_690_VERIFY] 步骤4 blink 触发重绘 (FIXED)");
}

#[test]
fn issue690_cursor_only_driven_by_frame_now_not_blink_timer() {
    let src = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    let body = method_body(&src, "fn update_paint_node(");
    // Issue #826 评论 36: 视觉光标改成完全独立的 cursor timeline，由
    // update_paint_node 在整帧唯一的 frame_now 上推进一次（不走 blink Timer，
    // 也不再是 build_render_plan_full 内部的 cursor_sample_outcome 采样）。
    assert!(
        body.contains("self.cursor_ctrl.tick_animation(frame_now);"),
        "步骤2: update_paint_node 必须用 frame_now 推进 cursor timeline（唯一采样点）"
    );
    assert!(
        body.contains("build_render_plan_full"),
        "步骤2: update_paint_node 仍统一走 build_render_plan_full 产出 RenderPlan"
    );
    // 推进只在帧首；回写方法只同步本帧实际绘制的 caret，不负责 progress。
    let apply_body = method_body(&src, "fn apply_render_plan_cursor_state(");
    assert!(
        !apply_body.contains("CursorSampleOutcome"),
        "步骤2: 旧的 CursorSampleOutcome（正文事务驱动 caret progress）不得复活"
    );
    assert!(
        !apply_body.contains("update_animation_progress("),
        "步骤2: 回写方法不得负责推进 progress（推进只在 tick_animation）"
    );
    assert!(
        apply_body.contains("drawn_caret_rect"),
        "步骤2: 回写仍要把本帧实际绘制的 caret 同步回 visual_x/visual_y"
    );
    let ctrl_src = read_src("src/sujian_editor_item/cursor_controller.rs");
    assert!(
        ctrl_src.contains("fn tick_animation(&mut self, frame_now: Instant)"),
        "步骤2: controller 必须提供 tick_animation(frame_now) 作为推进入口"
    );
    println!("[BUGFIX_690_VERIFY] 步骤2 CursorOnly 帧驱动 (#826 评论36 FIXED)");
}

#[test]
fn issue690_qml_uses_low_frequency_blink_timer_not_frame_animation() {
    let qml = read_src("qml/WritingWorkspace.qml");
    assert!(
        !qml.contains("FrameAnimation {"),
        "步骤4: 写作区不再有每帧 FrameAnimation 驱动光标动画"
    );
    let timer_id = qml
        .find("id: cursorBlinkTimer")
        .expect("步骤4: 必须保留低频 blink Timer");
    let block_start = qml[..timer_id]
        .rfind("Timer {")
        .expect("步骤4: blink 更新必须由 Timer 触发");
    let window = &qml[block_start..timer_id + 400];
    assert!(
        window.contains("tick_cursor_animation()"),
        "步骤4: blink 由低频 Timer 显式请求帧更新"
    );
    assert!(
        !window.contains("FrameAnimation"),
        "步骤4: 空闲闪烁不挂在每帧回调上"
    );
    println!("[BUGFIX_690_VERIFY] 步骤4 QML 低频 blink Timer (FIXED)");
}

#[test]
fn issue690_empty_document_uses_layout_fallback_not_fake_glyph() {
    // Issue #748: layout.rs 拆分为 layout/ 子目录，caret_rect 在 hit_test.rs。
    let src = read_src("src/editor/layout/hit_test.rs");
    // `EditorLayout::caret_rect` 是薄封装转发；要断言的是自由函数实现。
    let start = src
        .find("pub fn caret_rect(\n    snapshot")
        .expect("步骤4: 必须存在 caret_rect 实现");
    let rest = &src[start..];
    let end = rest.find("\n}\n").expect("步骤4: caret_rect 实现结束位置") + 3;
    let body = &rest[..end];
    assert!(
        body.contains("or_else(|| snapshot.lines.last())"),
        "步骤4: 空正文要靠 caret_rect 兜底行，不靠注入假字符"
    );
    assert!(
        !src.contains("\\u{200b}") && !src.contains('\u{200b}'),
        "步骤4: 排版源码里不得出现零宽假字符"
    );
    println!("[BUGFIX_690_VERIFY] 步骤4 空正文兜底行 (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 步骤 5: 每个动画一条紧凑诊断事件进正式诊断包
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue690_animation_lifecycle_events_go_to_diagnostics_logger() {
    let mod_src = read_src("src/sujian_editor_item/mod.rs");
    assert!(
        mod_src.contains("writer_diagnostics::record_event"),
        "步骤5: 动画事件必须写进正式诊断包"
    );
    // Issue #747/826: 旧 transaction_builder.rs / rebase.rs（editor.anim.create /
    // rebase / keep）随 animation/transaction 整体删除；生命周期事件收敛到遮罩前沿
    // 三层的紧凑诊断，仍走 writer_diagnostics::record_event 正式诊断包。
    let root = linux_qt_root();
    assert!(
        !root
            .join("src/sujian_editor_item/animation/transaction_builder.rs")
            .exists(),
        "步骤5: transaction_builder.rs 已删除，生命周期事件不得回到旧事务链"
    );
    assert!(
        !root
            .join("src/sujian_editor_item/animation/rebase.rs")
            .exists(),
        "步骤5: rebase.rs 已删除"
    );
    let coord_src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    assert!(
        coord_src.contains("record_frontier_diagnostic"),
        "步骤5: 遮罩前沿生命周期必须走统一的紧凑事件构造器"
    );
    assert!(
        coord_src.contains("\"editor.anim.frontier\""),
        "步骤5: 缺少生命周期事件 editor.anim.frontier"
    );
    let frontier_src = read_src("src/sujian_editor_item/animation/edit_frontier.rs");
    assert!(
        frontier_src.contains("\"editor.anim.frontier.identity_breakdown\""),
        "步骤5: 缺少 identity_breakdown 诊断事件"
    );
    assert!(
        mod_src.contains("\"editor.anim.transaction_skipped\""),
        "步骤5: 缺少 editor.anim.transaction_skipped 事件"
    );
    println!("[BUGFIX_690_VERIFY] 步骤5 生命周期诊断事件 (FIXED)");
}

#[test]
fn issue690_no_unconditional_stderr_animation_spam() {
    // 逐帧/无条件 eprintln 会淹没诊断包；只允许 env 控制的 debug log。
    // Issue #826 评论 36: 旧 transaction/ / cursor_motion.rs / rebase.rs /
    // transaction_builder.rs 已删除，改为只扫描仍然存在的动画文件，跳过已删文件。
    let animation_files = [
        "src/sujian_editor_item/animation/coordinator.rs",
        "src/sujian_editor_item/animation/composition.rs",
        "src/sujian_editor_item/animation/cursor_motion.rs",
        "src/sujian_editor_item/animation/rebase.rs",
        "src/sujian_editor_item/animation/render_plan_builder.rs",
        "src/sujian_editor_item/animation/transaction_builder.rs",
        "src/sujian_editor_item/animation/transaction/types.rs",
        "src/sujian_editor_item/animation/transaction/timeline.rs",
        "src/sujian_editor_item/animation/transaction/rebind.rs",
        "src/sujian_editor_item/animation/transaction/queue.rs",
    ];
    let mut checked = 0usize;
    for file in &animation_files {
        if !linux_qt_root().join(file).exists() {
            // 旧动画模块已被 #826 删除，没有可扫描的生产路径。
            continue;
        }
        checked += 1;
        let coord = read_src(file);
        let non_test = match coord.find("\n#[cfg(test)]") {
            Some(idx) => &coord[..idx],
            None => &coord[..],
        };
        assert!(
            !non_test.contains("eprintln!("),
            "步骤5: {} 生产路径不再用 eprintln 刷动画日志",
            file
        );
        assert!(
            !non_test.contains("[BUGFIX_687]"),
            "步骤5: {} 历史临时验证输出已清理，诊断改走 editor.anim.* 事件",
            file
        );
    }
    assert!(
        checked >= 3,
        "步骤5: 至少应扫描到仍存在的动画文件（实际 {} 个）",
        checked
    );
    println!("[BUGFIX_690_VERIFY] 步骤5 stderr 残留清理 (FIXED)");
}
