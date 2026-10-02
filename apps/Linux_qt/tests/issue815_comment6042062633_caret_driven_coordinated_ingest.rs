//! Issue #815 评论 6042062633 结构守卫 — 吞字/吐字真正由光标驱动的协同动画。
//!
//! WHITE_BOX 验证策略：确定性断言"协同动画退化成两条互不相干的时间线"的缺陷模式
//! 已在代码中消除，而不是靠肉眼回归。
//!
//! 一句话定义（评审收口）：
//! `协同模式 = 一条 caret 运动轨迹 + 文字以 caret 当前帧为吞吐边界 + Reflow 可独立`
//! — 不是 `两条互不相干的时间线，只在起点位置看起来碰巧挨着`。
//!
//! 逐条覆盖评审的九个修改点：
//! 1. `qml/WritingWorkspace.qml` 恢复鼠标左键长按选词；
//! 2. `transaction/timeline.rs` 单元计时分成 `Timed` / `CaretTrack` 两种驱动；
//! 3. `animation/cursor_motion.rs` 光标 track 是唯一采样入口，文字与光标共用；
//! 4. `animation/transaction_builder.rs` 先建 track 再包 unit，协同吞吐字不建独立进度；
//! 5. `animated_slice.rs` 逐帧裁切由当前 caret 帧算出；
//! 6. `animation/render_plan_builder.rs` 每帧每事务只采样一次，完成条件两类分开；
//! 7. `animation/rebase.rs` 不再静默跳过，报正式事件；
//! 8. `editing.rs` / `transaction.rs` / `pipeline.rs` 闭环所有"编辑发生了但没动画"的原因；
//! 9. `animation/composition.rs` IME commit 与普通 Insert 同一规则。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

/// 评审收口定义。代码注释里必须留着这句，避免以后又被改回"两条独立时间线"。
const CLOSING_DEFINITION: &str =
    "一条 caret 运动轨迹 + 文字以 caret 当前帧为吞吐边界 + Reflow 可独立";

// =========================================================================
// 修改点 1：恢复桌面鼠标左键长按选词
// =========================================================================

#[test]
fn issue815_modify1_qml_restores_mouse_left_long_press() {
    let src = read_src("qml/WritingWorkspace.qml");

    let handler_pos = src
        .find("id: touchLongPressHandler")
        .expect("Issue #815: WritingWorkspace.qml 必须保留长按选中 handler");
    let window_start = src[..handler_pos].rfind("TapHandler {").unwrap_or(0);
    // 取到下一个 TapHandler 之前，即本 handler 的完整范围（按字符边界回退）。
    let window_end = src[handler_pos..]
        .find("TapHandler {")
        .map(|i| handler_pos + i)
        .unwrap_or(src.len());
    let window = src[window_start..window_end].to_string();

    assert!(
        window.contains("acceptedButtons: Qt.LeftButton"),
        "Issue #815 评论 6042062633 修改 1: 长按 handler 仍必须只吃左键，右键菜单走独立 handler。"
    );
    for device in [
        "PointerDevice.Mouse",
        "PointerDevice.TouchPad",
        "PointerDevice.TouchScreen",
        "PointerDevice.Stylus",
    ] {
        assert!(
            window.contains(device),
            "Issue #815 评论 6042062633 修改 1: 长按 handler 必须接受 {}，\
             Issue #714 把鼠标排除在外是错的（左键长按与右键菜单是两个不同输入）。",
            device
        );
    }

    // 长按 → 选词手势的现有链路不能被改掉。
    for marker in [
        "begin_selection_gesture",
        "long_press_at",
        "end_selection_gesture_qml",
    ] {
        assert!(
            window.contains(marker),
            "Issue #815 评论 6042062633 修改 1: 长按选中链路 {} 必须保留。",
            marker
        );
    }

    // 右键菜单继续由独立的 Qt.RightButton TapHandler 处理。
    assert!(
        src.contains("acceptedButtons: Qt.RightButton"),
        "Issue #815 评论 6042062633 修改 1: 右键菜单必须继续走独立的 Qt.RightButton handler。"
    );

    // 删掉 Issue #714 的错误论断注释。
    assert!(
        !src.contains("桌面鼠标的长按等同于右键菜单"),
        "Issue #815 评论 6042062633 修改 1: 必须删除「桌面鼠标长按 == 右键菜单」的注释/逻辑，\
         它正是当初把鼠标排除掉的错误理由。"
    );
}

// =========================================================================
// 修改点 2：视觉单元计时分成 Timed / CaretTrack 两种驱动
// =========================================================================

#[test]
fn issue815_modify2_visual_unit_timing_has_two_drivers() {
    let src = read_src("src/sujian_editor_item/animation/transaction/timeline.rs");

    let enum_window = function_window(&src, "pub(crate) enum VisualUnitTiming", 1200);
    assert!(
        enum_window.contains("Timed {"),
        "Issue #815 评论 6042062633 修改 2: VisualUnitTiming 必须保留 Timed 驱动\
         （ReflowMove/ReflowCrossFade + 非协同吞吐字）。"
    );
    assert!(
        enum_window.contains("CaretTrack { retired: bool }"),
        "Issue #815 评论 6042062633 修改 2: VisualUnitTiming 必须有 CaretTrack 驱动\
         （协同 InsertReveal/DeleteConceal）。"
    );

    // 协同吞吐字不能自己再拥有一条 ease_out_quad + text_duration_ms 时间线。
    let ctor_window = function_window(&src, "pub(crate) fn default_for_kind_with_coordinated", 900);
    assert!(
        ctor_window.contains("coordinated")
            && ctor_window.contains("AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal"),
        "Issue #815 评论 6042062633 修改 2: 只有协同模式的 InsertReveal/DeleteConceal 才切 CaretTrack，\
         Reflow 必须继续独立 Timed。"
    );
    assert!(
        !ctor_window.contains("ease_out_quad") && !ctor_window.contains("started_at"),
        "Issue #815 评论 6042062633 修改 2: 协同吞吐字的驱动构造里不得再出现自己的 easing/时间线。"
    );

    // retire 语义：失去 caret ownership 后 CaretTrack 单元收口到终态。
    assert!(
        src.contains("pub(crate) fn retire_caret_motion"),
        "Issue #815 评论 6042062633 修改 3: CaretTrack 单元必须有 retire 收口语义，\
         否则事务会等一条已经不推进的 track。"
    );
    assert!(
        src.contains("pub(crate) fn is_caret_track"),
        "Issue #815 评论 6042062633 修改 2/6: 必须能区分 CaretTrack 单元（含已 retire 的）。"
    );
}

// =========================================================================
// 修改点 3：cursor track 是协同运动的唯一采样入口
// =========================================================================

#[test]
fn issue815_modify3_cursor_track_is_the_single_sampling_entry() {
    let src = read_src("src/sujian_editor_item/animation/cursor_motion.rs");

    assert!(
        src.contains("pub(crate) fn sample_caret_track_frame"),
        "Issue #815 评论 6042062633 修改 3: 光标 track 必须有统一采样入口 sample_caret_track_frame，\
         同一个 frame_now 一次给出 caret rect / x / y / visual_line_id / progress。"
    );

    let sample_window = function_window(&src, "pub(crate) fn sample_caret_track_frame", 1500);
    for marker in [
        "sampled_rect_at_progress",
        "sampled_visual_line_id_at_progress",
        "progress(",
    ] {
        assert!(
            sample_window.contains(marker),
            "Issue #815 评论 6042062633 修改 3: 采样入口必须同时给出 rect / 行 id / progress（{}），\
             否则文字层还要自己再算一次时间。",
            marker
        );
    }

    // 光标层不许再自己重新采样一次 track。
    let pos_window = function_window(
        &src,
        "pub(crate) fn compute_coordinated_cursor_position",
        2500,
    );
    assert!(
        pos_window.contains("motion: &crate::sujian_editor_item::render_plan::CoordinatedMotionFrame"),
        "Issue #815 评论 6042062633 修改 3/6: 光标层必须消费文字层已经采好的 CoordinatedMotionFrame，\
         而不是自己再按 frame_now 采样一次 track。"
    );
    assert!(
        !pos_window.contains("sampled_rect_at_progress"),
        "Issue #815 评论 6042062633 修改 3/6: 光标层不得重新采样 track，\
         否则同一帧的文字边界和光标位置可能来自两个不同的时间点。"
    );

    // rebase 交棒必须先采样旧 track 当前帧，不退回逻辑旧 caret。
    let rebase_src = read_src("src/sujian_editor_item/animation/rebase.rs");
    assert!(
        rebase_src.contains("sample_caret_track_frame"),
        "Issue #815 评论 6042062633 修改 3: 快速连续输入/删除交棒时必须先采样旧 cursor track 的当前帧。"
    );
}

// =========================================================================
// 修改点 5：逐帧裁切由当前 caret 帧算出
// =========================================================================

#[test]
fn issue815_modify5_ingest_clip_is_driven_by_current_caret_frame() {
    let src = read_src("src/sujian_editor_item/animated_slice.rs");

    assert!(
        src.contains("pub(crate) fn compute_frame_by_caret_ingest"),
        "Issue #815 评论 6042062633 修改 5: 必须新增「按当前 caret 帧算裁切」的入口，\
         不能把协同吞吐换算成独立 0..1 visible fraction。"
    );

    let window = function_window(&src, "pub(crate) fn compute_frame_by_caret_ingest", 2200);
    assert!(
        window.contains("caret_x: f64"),
        "Issue #815 评论 6042062633 修改 5: 本帧边界就是本帧的 caret.x，必须作为参数传进来。"
    );
    assert!(
        window.contains("anchor.min(caret_x)") && window.contains("anchor.max(caret_x)"),
        "Issue #815 评论 6042062633 修改 5: 同一行的另一端必须是构造期锚点\
         （InsertReveal=编辑前 caret，DeleteConceal=删除后最终 caret），并与本帧 caret.x 归一化。"
    );
    assert!(
        !window.contains("ease_out_quad") && !window.contains("* visible"),
        "Issue #815 评论 6042062633 修改 5: 边界不得写成 anchor + extent * text_progress，\
         那正是「看起来像被剪开而不是吞吐」的根因。"
    );

    // 跨软换行/跨段：必须靠行 id 判定 caret 是否已经走过本行。
    assert!(
        window.contains("slice_line > caret_line") && window.contains("slice_line < caret_line"),
        "Issue #815 评论 6042062633 修改 5: 跨行必须用 cursor track 采到的 visual_line_id 判定\
         「还没走到 / 正在这一行 / 已经走过」，绝不能用上一行的 caret.x 裁下一行。"
    );
}

// =========================================================================
// 修改点 6：每帧每事务只采样一次，完成条件两类分开
// =========================================================================

#[test]
fn issue815_modify6_one_caret_sample_per_frame_and_two_class_completion() {
    let src = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");

    let plan_window = function_window(&src, "pub(crate) fn build_render_plan_full", 9000);
    assert!(
        !plan_window.contains("self.sample_coordinated_motion_frame("),
        "Issue #815 评论 6042062633 修改 6: build_render_plan_full 不得再自己采样一次 caret，\
         必须复用 build_text_animation_plan_with_sample 返回的那一份采样。"
    );
    assert!(
        plan_window.contains("build_text_animation_plan_with_sample"),
        "Issue #815 评论 6042062633 修改 6: 文字层必须在同一个函数里完成唯一一次 caret 采样。"
    );
    assert!(
        plan_window.contains(
            "compute_coordinated_cursor_position(cursor_owner_epoch, &coordinated_motion_frame)"
        ),
        "Issue #815 评论 6042062633 修改 6: 光标层必须消费同一份 CoordinatedMotionFrame。"
    );

    let text_window = function_window(
        &src,
        "pub(crate) fn build_text_animation_plan_with_sample",
        9000,
    );
    assert!(
        text_window.contains("self.sample_coordinated_motion_frame("),
        "Issue #815 评论 6042062633 修改 6: 唯一一次 caret 采样在文字层入口，文字与光标共用它的返回值。"
    );

    // 吞吐字按 caret 帧算，只有 Timed unit 才走 current_visible_fraction。
    let glyph_window = text_window
        .find("for unit in &tx.units")
        .map(|i| text_window[i..].to_string())
        .unwrap_or_else(|| panic!("文字层必须有逐 unit 的 glyph 生成循环"));
    assert!(
        glyph_window.contains("is_caret_track()"),
        "Issue #815 评论 6042062633 修改 6: 逐 unit 渲染必须先按 CaretTrack / Timed 分流。"
    );
    assert!(
        glyph_window.contains("compute_frame_by_caret_ingest("),
        "Issue #815 评论 6042062633 修改 6: CaretTrack unit 必须调用按当前 caret 帧算裁切的入口。"
    );
    assert!(
        glyph_window.contains("current_visible_fraction"),
        "Issue #815 评论 6042062633 修改 6: 只有 Timed unit 保留独立 visible fraction 路径。"
    );

    // 完成条件：协同吞吐字随 cursor track 结束，不等一条自己的文字时间线。
    assert!(
        text_window.contains("!u.timing.is_caret_track()"),
        "Issue #815 评论 6042062633 修改 6: 事务完成判断必须把 CaretTrack unit 与 Timed unit 分开，\
         协同吞吐字随 cursor track 结束（它没有第二条进度可等）。"
    );
    assert!(
        text_window.contains("caret_track_done") && text_window.contains("caret_track_complete"),
        "Issue #815 评论 6042062633 修改 6: 事务完成仍必须等待本事务的 cursor track。"
    );

    // 失去 ownership 的事务必须收口 CaretTrack 单元。
    assert!(
        text_window.contains("retire_caret_driven_units()"),
        "Issue #815 评论 6042062633 修改 6: 拿不到本帧 caret 采样的事务必须把 CaretTrack 单元收口到终态。"
    );
}

// =========================================================================
// 修改点 4：builder 先建 track 再包 unit
// =========================================================================

#[test]
fn issue815_modify4_builder_marks_ingest_as_caret_track_after_track_exists() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder.rs");

    let track_pos = src
        .find("build_cursor_visual_track(")
        .expect("Issue #815 评论 6042062633 修改 4: builder 必须建 cursor_visual_track");
    let wrap_pos = src
        .find("PreparedVisualUnit::wrap_with_coordinated(")
        .expect("Issue #815 评论 6042062633 修改 4: InsertReveal/DeleteConceal 必须走 wrap_with_coordinated");
    let rebase_pos = src
        .find("match_rebase_frames(")
        .expect("Issue #815 评论 6042062633 修改 4: builder 仍要做 rebase 帧交棒");

    assert!(
        track_pos < wrap_pos,
        "Issue #815 评论 6042062633 修改 4: 必须先建 cursor_visual_track 再包 unit，\
         否则 wrap_with_coordinated 无从知道这笔事务到底有没有 caret 轨迹可依附。"
    );
    assert!(
        wrap_pos < rebase_pos,
        "Issue #815 评论 6042062633 修改 4: rebase 帧交棒仍在包 unit 之后。"
    );

    let window = function_window(
        &src,
        "let coordinated_ingest = spec.coordinated_animation_enabled",
        300,
    );
    assert!(
        window.contains("cursor_visual_track.is_some()"),
        "Issue #815 评论 6042062633 修改 4: 协同吞吐字的判定条件是「协同开启且这笔事务确实有 cursor track」，\
         不是只看设置开关。"
    );

    // 被删掉的根因注释/实现不得复活。
    for stale in [
        "InsertReveal/DeleteConceal 的文字 progress 只来自文字自己的 timeline",
        "coordinated 不再改 timing",
    ] {
        assert!(
            !src.contains(stale),
            "Issue #815 评论 6042062633 修改 4: 「{}」正是根因描述，必须删除。",
            stale
        );
    }

    // 协同却没有 track 时不允许入队，必须记正式事件。
    let skip_window = function_window(&src, "coordinated_without_cursor_track", 900);
    assert!(
        skip_window.contains("editor_animation_transaction_skipped_event"),
        "Issue #815 评论 6042062633 修改 7/8: 协同模式下没有 cursor track 时必须记正式跳过事件，\
         不允许退化成「文字自己播、光标不动」。"
    );
}

// =========================================================================
// 修改点 7/8/9：所有跳过点都有正式事件，输入路径闭环
// =========================================================================

#[test]
fn issue815_modify7_rebase_reports_skip_instead_of_silently_returning_none() {
    let src = read_src("src/sujian_editor_item/animation/rebase.rs");

    assert!(
        !src.contains("if coordinated_animation_enabled && !valid_caret_motion_track {\n            return None;"),
        "Issue #815 评论 6042062633 修改 7: 「协同但缺 caret 几何」不能再静默 return None，\
         这正是诊断包里只剩 Delete、没有 Insert 的原因。"
    );

    let window = function_window(&src, "pub(crate) fn prepare_rebase_handoff_for_edit", 9000);
    assert!(
        window.contains("editor_animation_transaction_skipped_event"),
        "Issue #815 评论 6042062633 修改 7: rebase handoff 的跳过点必须记正式事件。"
    );
    for cause in [
        "suppressed_by_context",
        "caret_geometry_missing",
        "missing_inserted_range",
    ] {
        assert!(
            window.contains(cause),
            "Issue #815 评论 6042062633 修改 7: 必须报出跳过原因 {}，不能吞掉原因。",
            cause
        );
    }
}

#[test]
fn issue815_modify8_input_path_close_the_loop_with_formal_events() {
    let skip_fields = function_window(
        &read_src("src/sujian_editor_item/mod.rs"),
        "pub(crate) struct AnimationSkipFields",
        2000,
    );
    for field in [
        "cause",
        "operation_kind",
        "typing_animation_enabled",
        "smooth_cursor_enabled",
        "coordinated_animation_enabled",
        "old_caret_present",
        "new_caret_present",
        "inserted_range",
        "unit_kinds",
        "cursor_track_present",
        "is_scrolling",
        "is_loading",
        "is_applying_format",
    ] {
        assert!(
            skip_fields.contains(field),
            "Issue #815 评论 6042062633 修改 8: 跳过事件必须带字段 {}，日志只暴露 cause 不足以定位问题。",
            field
        );
    }
    assert!(
        read_src("src/sujian_editor_item/mod.rs")
            .contains("event: \"editor.anim.transaction_skipped\".to_string()"),
        "Issue #815 评论 6042062633 修改 7/8: 正式事件名必须是 editor.anim.transaction_skipped。"
    );

    // 输入路径上的每个跳过点都要有正式事件。
    let cases: &[(&str, &[&str])] = &[
        (
            "src/sujian_editor_item/pipeline.rs",
            &[
                "suppressed_by_context",
                "stale_current_canonical",
                "canonical_invariant_failure",
            ],
        ),
        (
            "src/sujian_editor_item/transaction.rs",
            &["suppressed_by_scrolling"],
        ),
        (
            "src/sujian_editor_item/editing.rs",
            &[
                "composition_commit_old_snapshot_unavailable",
                "composition_commit_new_snapshot_invariant_failure",
            ],
        ),
    ];
    for (path, causes) in cases {
        let src = read_src(path);
        assert!(
            src.contains("editor_animation_transaction_skipped_event"),
            "Issue #815 评论 6042062633 修改 8: {} 必须用正式跳过事件解释「编辑发生了但没有动画」。",
            path
        );
        for cause in *causes {
            assert!(
                src.contains(cause),
                "Issue #815 评论 6042062633 修改 8: {} 必须报出 {}。",
                path,
                cause
            );
        }
    }
}

#[test]
fn issue815_modify9_ime_commit_uses_the_same_caret_driven_rule() {
    let src = read_src("src/sujian_editor_item/animation/composition.rs");

    assert!(
        !src.contains("prepared.cursor_visual_track.is_none()"),
        "Issue #815 评论 6042062633 修改 9: IME commit/update 不得再单独用「只有 debug log」的 gate 拦掉，\
         必须和普通 Insert 走同一条规则（builder 统一判定 + 正式事件）。"
    );
    for fn_marker in [
        "pub fn handle_composition_update",
        "pub fn handle_composition_commit_or_cancel",
    ] {
        let window = function_window(&src, fn_marker, 9000);
        assert!(
            window.contains("build_prepared_transaction(spec)?"),
            "Issue #815 评论 6042062633 修改 9: {} 必须直接消费 builder 的判定结果。",
            fn_marker
        );
    }
}

/// 评审收口定义必须留在代码注释里，防止语义再次漂移。
#[test]
fn issue815_closing_definition_is_preserved_in_comments() {
    let files = [
        "src/sujian_editor_item/animation/transaction/timeline.rs",
        "src/sujian_editor_item/animation/transaction/types.rs",
    ];
    for path in files {
        assert!(
            read_src(path).contains(CLOSING_DEFINITION),
            "Issue #815: {} 的注释里必须保留评审收口定义「{}」，\
             否则「两条互不相干的时间线」的错误语义会再漂回来。",
            path,
            CLOSING_DEFINITION
        );
    }
}
