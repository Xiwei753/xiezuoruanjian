//! Issue #690 评论 5675007226 结构守卫 — Linux Qt 协同动画只剩一条采样链。
//!
//! WHITE_BOX 验证策略：通过读取源文件内容，确定性断言"两套采样 / 两套时钟 / 两套 easing"
//! 的缺陷模式已在代码中消除，而不是靠肉眼回归。覆盖评论的五个步骤：
//! 1. `update_paint_node()` 整帧只取一次时间，文字与光标共用 `AnimationFrameSample`；
//! 2. 光标落在文字吞吐边界上，Reveal/Conceal/Reflow/协同光标共用同一条二次 easing；
//! 3. 视觉单元自持生命期，交棒时带上当前可见比例与单元时间线；
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
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    assert!(
        src.contains("pub(crate) struct AnimationFrameSample"),
        "步骤1: 必须存在纯数据的 AnimationFrameSample"
    );
    let text_plan = method_body(&src, "fn build_text_animation_plan_with_sample(");
    assert!(
        text_plan.contains("sample: &AnimationFrameSample"),
        "步骤1: 文字 plan 由帧采样构造，不再自己取时间"
    );
    assert!(
        !text_plan.contains("Instant::now()"),
        "步骤1: 文字 plan 构造路径内不得再次采样时间"
    );
    assert!(
        text_plan.contains("unit.current_visible_fraction(sample.frame_now)"),
        "步骤1: 文字帧的可见比例来自单元时间线 + 本帧采样点"
    );
    let cursor = method_body(&src, "fn compute_coordinated_cursor_position(");
    assert!(
        !cursor.contains("Instant::now()"),
        "步骤1: 协同光标不得再独立采样时间（否则仍是两套时钟）"
    );
    assert!(
        cursor.contains("sample.frame_now"),
        "步骤1: 协同光标与文字共用同一个采样点"
    );
    let render_plan = method_body(&src, "fn build_render_plan_full(");
    assert!(
        render_plan.contains("self.compute_coordinated_cursor_position(&frame_sample)"),
        "步骤1: 最终 CursorRenderState 在 build_render_plan_full 内由同一 sample 算出"
    );
    println!("[BUGFIX_690_VERIFY] 步骤1 文字与光标共用帧采样 (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 步骤 2: 光标 = 文字吞吐边界，且只有一条协同 easing
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue690_cursor_sits_on_text_reveal_and_conceal_boundary() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let cursor = method_body(&src, "fn compute_coordinated_cursor_position(");
    assert!(
        cursor.contains("frame.x + frame.w"),
        "步骤2: InsertReveal / Backspace 光标必须取本帧 reveal-conceal 右边界"
    );
    assert!(
        cursor.contains("new_rect.x"),
        "步骤2: 前向 Delete 光标固定在 new_cursor_rect.x，不回抽"
    );
    assert!(
        cursor.contains("AnimatedSlice::ease_out_quad"),
        "步骤2: 无边界 glyph 时按与 ReflowMove 相同的曲线插值 old/new caret"
    );
    println!("[BUGFIX_690_VERIFY] 步骤2 光标跟随吞吐边界 (FIXED)");
}

#[test]
fn issue690_single_collaborative_easing_function() {
    let slice_src = read_src("src/sujian_editor_item/animated_slice.rs");
    assert_eq!(
        slice_src.matches("powi(2)").count(),
        1,
        "步骤2: 二次曲线只能定义一次（AnimatedSlice::ease_out_quad）"
    );
    let compute_frame = method_body(&slice_src, "pub fn compute_frame(");
    assert!(
        !compute_frame.contains("powi(") && !compute_frame.contains("ease_out_quad("),
        "步骤2: compute_frame 只做线性插值，easing 不得重复施加"
    );
    let coord_src = read_src("src/sujian_editor_item/animation_coordinator.rs");
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
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let frame_start = src
        .find("pub(crate) struct RebaseFrame")
        .expect("步骤3: 必须存在 RebaseFrame");
    // Issue #690 评论 5679744253 问题 1: 截取长度加大以包含新字段 sampled_at /
    // remaining_duration_ms（它们在结构体末尾，旧 900 字符不够）。
    // 使用安全的字符边界截取，避免落在 UTF-8 多字节字符中间。
    let raw_end = frame_start + 1100;
    let safe_end = src.ceil_char_boundary(raw_end);
    let frame_def = &src[frame_start..safe_end];
    for field in [
        "visible_fraction",
        "sampled_at",
        "remaining_duration_ms",
        "shaping_identity",
    ] {
        assert!(
            frame_def.contains(field),
            "步骤3: RebaseFrame 必须携带 `{}`，retarget 从当前帧重新起段",
            field
        );
    }
    let collect = method_body(&src, "pub fn collect_rebase_frames(");
    assert!(
        collect.contains("unit.progress(now) < 1.0"),
        "步骤3: 采集按单元进度过滤已完成单元"
    );
    assert!(
        collect.contains("unit.current_visible_fraction(now)"),
        "步骤3: 可见比例按单元自己的时间线算，不用事务级 progress 一刀切"
    );
    // Issue #690 评论 5679744253 问题 1: retarget 时 started_at 重置到当前帧，
    // duration 用剩余时长，不沿用旧时间线。
    let rebase = method_body(&src, "pub fn rebase_from_frame(");
    assert!(
        rebase.contains("frame.sampled_at") && rebase.contains("frame.remaining_duration_ms"),
        "步骤3: retarget 时 started_at 重置到当前帧，duration 用剩余时长，不沿用旧时间线"
    );
    assert!(
        src.contains("struct PreparedVisualUnit"),
        "步骤3: 视觉单元必须拥有自己的动画生命期"
    );
    let wrap = method_body(&src, "pub fn wrap(");
    assert!(
        wrap.contains("initial_fraction_for_kind"),
        "步骤3: 新单元起点比例由动画类型决定（Conceal 起手完整可见）"
    );
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
    assert!(
        body.contains("active_text_transaction_key"),
        "步骤2: update_paint_node 必须判断是否有活跃正文事务"
    );
    assert!(
        body.contains("cursor_timeline_sample_with_time"),
        "步骤2: CursorOnly 位置由 frame_now 采样驱动，不再只靠 blink Timer"
    );
    assert!(
        body.contains("CursorTimelineSample::Running"),
        "步骤2: CursorOnly 采样到 Running progress 时推进 visual_x/y"
    );
    println!("[BUGFIX_690_VERIFY] 步骤2 CursorOnly 帧驱动 (FIXED)");
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
    let src = read_src("src/editor/layout.rs");
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
    let coord = read_src("src/sujian_editor_item/animation_coordinator.rs");
    for event in [
        "\"editor.anim.create\"",
        "\"editor.anim.rebase\"",
        "\"editor.anim.keep\"",
        "\"editor.anim.complete\"",
    ] {
        assert!(coord.contains(event), "步骤5: 缺少生命周期事件 {}", event);
    }
    assert!(
        coord.contains("fn emit_transaction_diagnostic("),
        "步骤5: 各生命周期点共用一个紧凑事件构造器"
    );
    assert!(
        coord.contains("fn conflicting_units_are_untouched("),
        "步骤3: 只有真正被新编辑覆盖的单元才结束/替换，未覆盖的走 keep 分支"
    );
    println!("[BUGFIX_690_VERIFY] 步骤5 生命周期诊断事件 (FIXED)");
}

#[test]
fn issue690_no_unconditional_stderr_animation_spam() {
    // 逐帧/无条件 eprintln 会淹没诊断包；只允许 env 控制的 debug log。
    let coord = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let non_test = match coord.find("\n#[cfg(test)]") {
        Some(idx) => &coord[..idx],
        None => &coord[..],
    };
    assert!(
        !non_test.contains("eprintln!("),
        "步骤5: 协调器生产路径不再用 eprintln 刷动画日志"
    );
    assert!(
        !non_test.contains("[BUGFIX_687]"),
        "步骤5: 历史临时验证输出已清理，诊断改走 editor.anim.* 事件"
    );
    println!("[BUGFIX_690_VERIFY] 步骤5 stderr 残留清理 (FIXED)");
}
