//! Issue #702 评论 5707770318 仍有效的光标时间线结构守卫。
//!
//! WHITE_BOX 结构守卫：这些结构的不存在即证实了缺陷已被消除（修复成功）。
//!
//! Issue #853 保持光标和正文动画状态独立。光标由单一 CursorController 时间线推进：
//!
//! `apply_plan()` 创建/重基 Tween（progress=0、started_at=None）
//!   → `update_paint_node` 每帧 `self.cursor_ctrl.tick_animation(frame_now);`
//!   → `build_cursor_render_state_for_frame()` 纯读本帧 visual
//!   → `build_render_plan_full()` 纯透传成 `drawn_caret_rect`
//!   → `apply_render_plan_cursor_state()` 回写本帧绘制位置。
//!
//! 推进入口全仓库只有 `CursorController::tick_animation` 一个。

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

/// 在 `src` 中定位 `anchor`，返回 anchor 之后 `window_chars` 字符的窗口。
/// 安全处理 UTF-8 字符边界：若 end 落在字符中间，回退到最近的字符边界。
fn window_after(src: &str, anchor: &str, window_chars: usize) -> String {
    let pos = src
        .find(anchor)
        .unwrap_or_else(|| panic!("anchor not found: {}", anchor));
    let end = pos + anchor.len() + window_chars;
    let safe_end = (0..=end.min(src.len()))
        .rev()
        .find(|&e| src.is_char_boundary(e))
        .unwrap_or(pos);
    if safe_end > pos {
        src[pos..safe_end].to_string()
    } else {
        src[pos..].to_string()
    }
}

/// 取出某个方法从签名到函数体结束（首个 4 空格缩进的 `}`）之间的文本。
fn method_body(src: &str, signature: &str) -> String {
    let start = src
        .find(signature)
        .unwrap_or_else(|| panic!("method `{}` must exist", signature));
    let rest = &src[start..];
    let end = rest.find("\n    }\n").unwrap_or(rest.len());
    rest[..end].to_string()
}

// =========================================================================
// 修复点 1：build_cursor_plan 不再按正文事务决定过渡
// =========================================================================

/// 旧断言（render_plan_builder.rs + `has_active_for_coordinated` 窗口）已失效：
/// `build_cursor_plan` 移到了 `animation/coordinator.rs`，且协同光标开关随 #826 删除。
/// 现在只按「是否需要移动 + 非 hard_snap + duration>0 + 同行或 smooth cursor 开」
/// 决定 Tween，否则 Snap（Snap 会让 `apply_plan` 清掉 animation，不开新 timeline）。
#[test]
fn fix1_build_cursor_plan_no_body_transaction_branch() {
    let src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    assert!(
        src.contains("pub(crate) fn build_cursor_plan"),
        "修复点1: build_cursor_plan 必须存在（已移入 animation/coordinator.rs）"
    );
    let plan = method_body(&src, "pub(crate) fn build_cursor_plan");
    assert!(
        !plan.contains("has_active_for_coordinated"),
        "修复点1: 协同光标开关已删除，不得复活正文事务分支"
    );
    assert!(
        !plan.contains("has_active_insert"),
        "修复点1: 不得再用 has_active_insert() 决定光标过渡（旧双时间线前提）"
    );
    assert!(
        !plan.contains("has_active_text_transaction"),
        "修复点1: 正文事务协同光标已随 #826 删除"
    );
    assert!(
        plan.contains("CursorTransition::Tween") && plan.contains("CursorTransition::Snap"),
        "修复点1: 过渡仍只有 Tween / Snap 两种"
    );
    assert!(
        plan.contains("duration_ms"),
        "修复点1: Tween 必须携带自己的 duration_ms（独立 cursor timeline）"
    );
    println!("[BUGFIX_VERIFY] fix1: build_cursor_plan 无正文事务分支，Tween/Snap + duration_ms");
}

// =========================================================================
// 修复点 2：CursorSampleOutcome 枚举彻底删除（不复活）
// =========================================================================

#[test]
fn fix2_cursor_sample_outcome_is_removed() {
    let render_plan = read_src("src/sujian_editor_item/render_plan.rs");
    assert!(
        !render_plan.contains("enum CursorSampleOutcome"),
        "修复点2: CursorSampleOutcome 枚举已删除，不得复活"
    );
    assert!(
        !render_plan.contains("CursorSampleOutcome"),
        "修复点2: render_plan.rs 不得再引用 CursorSampleOutcome"
    );
    assert!(
        !render_plan.contains("Coordinated"),
        "修复点2: 正文协同帧变体 Coordinated 已删除"
    );

    let render_plan_builder = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");
    assert!(
        !render_plan_builder.contains("cursor_sample_outcome"),
        "修复点2: render_plan_builder 不得再维护 cursor_sample_outcome"
    );

    let qquick = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    assert!(
        !qquick.contains("CursorSampleOutcome"),
        "修复点2: qquickitem_impl 不得再 match CursorSampleOutcome"
    );
    assert!(
        !qquick.contains("cursor_sample_outcome"),
        "修复点2: qquickitem_impl 不得再出现 cursor_sample_outcome"
    );
    println!("[BUGFIX_VERIFY] fix2: CursorSampleOutcome 全链删除，正文事务不再驱动 caret progress");
}

// =========================================================================
// 修复点 3：started_at 只能由 tick_animation(frame_now) 初始化
// =========================================================================

/// 旧断言要求 qquickitem_impl 的 `CursorSampleOutcome::Coordinated` 分支**不**
/// 启动 `started_at`。现在没有那条分支了：`started_at` 的唯一初始化点必须是
/// `CursorController::tick_animation`（Issue #826 评论 36 恢复的每帧唯一采样点）。
#[test]
fn fix3_started_at_only_initialized_by_cursor_tick() {
    let qquick = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    assert!(
        !qquick.contains("started_at = Some(frame_now)"),
        "修复点3: qquickitem_impl 不得自己初始化 CursorAnimationState.started_at"
    );
    assert!(
        qquick.contains("self.cursor_ctrl.tick_animation(frame_now);"),
        "修复点3: update_paint_node 必须每帧调一次 cursor_ctrl.tick_animation(frame_now)"
    );

    let ctrl = read_src("src/sujian_editor_item/cursor_controller.rs");
    assert!(
        ctrl.contains("fn tick_animation(&mut self, frame_now: Instant)"),
        "修复点3: cursor_controller 必须有 tick_animation 推进入口"
    );
    let tick = window_after(
        &ctrl,
        "fn tick_animation(&mut self, frame_now: Instant)",
        1200,
    );
    assert!(
        tick.contains("started_at = Some(frame_now)"),
        "修复点3: started_at 必须在 tick_animation 体内用本帧 frame_now 初始化"
    );
    assert!(
        tick.contains("self.update_animation_progress(progress)"),
        "修复点3: progress 消费收敛到 update_animation_progress 单点"
    );
    assert_eq!(
        ctrl.matches("started_at = Some(frame_now)").count(),
        1,
        "修复点3: 全文件只允许 tick_animation 一处初始化 started_at"
    );
    println!("[BUGFIX_VERIFY] fix3: started_at 由 tick_animation(frame_now) 单点初始化");
}

// =========================================================================
// 修复点 4：build_render_plan_full 纯透传 caret，不自己取时间/采样
// =========================================================================

#[test]
fn fix4_build_render_plan_full_is_time_free_pure_passthrough() {
    let src = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");
    assert!(
        src.contains("pub(crate) fn build_render_plan_full"),
        "修复点4: build_render_plan_full 必须存在"
    );
    assert!(
        !src.contains("cursor_sample_outcome"),
        "修复点4: 不得再维护 cursor_sample_outcome"
    );
    assert!(
        !src.contains("BUGFIX_REPRO_TRACE"),
        "修复点4: 诊断跟踪 eprintln! 应已移除"
    );
    let body = method_body(&src, "pub(crate) fn build_render_plan_full");
    assert!(
        !body.contains("Instant::now()"),
        "修复点4: build_render_plan_full 是纯读，不得自己取时钟"
    );
    assert!(
        body.contains("drawn_caret_rect"),
        "修复点4: 光标 caret 纯透传成 drawn_caret_rect（由调用方传入 render state）"
    );
    println!("[BUGFIX_VERIFY] fix4: build_render_plan_full 纯读透传 caret，无时间采样");
}

// =========================================================================
// 修复点 5（评论 5708209114）：build_cursor_plan 不得来自 has_active_insert()
// =========================================================================

/// 旧断言：`has_active` 必须来自 `has_active_text_transaction()` 覆盖全部事务类型。
/// Issue #826 之后正文协同光标整链删除，`build_cursor_plan` 里**不再有任何**正文
/// 活跃判断 —— 旧的 insert-only 判断与它的替代品一起消失，双时间线分叉无从产生。
#[test]
fn fix5_build_cursor_plan_has_no_insert_only_active_check() {
    let src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let plan = method_body(&src, "pub(crate) fn build_cursor_plan");
    assert!(
        !plan.contains("let has_active = self.has_active_insert();"),
        "修复点5: build_cursor_plan 不得用 has_active_insert()（Delete 路径漏判）"
    );
    assert!(
        !plan.contains("has_active_insert"),
        "修复点5: build_cursor_plan 不得保留任何 insert-only 正文活跃判断"
    );
    assert!(
        !plan.contains("has_active_text_transaction"),
        "修复点5: 正文事务协同判断已随协同光标删除（不得留下半套）"
    );
    println!("[BUGFIX_VERIFY] fix5: build_cursor_plan 无正文活跃判断（旧双时间线前提已消失）");
}

// =========================================================================
// 综合断言：双时间线分叉已消除，只剩一条光标 timeline
// =========================================================================

#[test]
fn dual_timeline_fork_is_eliminated() {
    // Timeline A（正文协同光标）：整个模块已删除。
    let motion_path = linux_qt_root().join("src/sujian_editor_item/animation/cursor_motion.rs");
    assert!(
        !motion_path.exists(),
        "Timeline A: cursor_motion.rs 已删除，协同光标不得复活"
    );
    let coord_src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    assert!(
        !coord_src.contains("compute_coordinated_cursor_position"),
        "Timeline A: compute_coordinated_cursor_position 已删除"
    );
    assert!(
        !coord_src.contains("has_active_for_coordinated"),
        "Timeline A: has_active_for_coordinated 已删除"
    );

    // Timeline B：纯光标 Tween（apply_plan 创建 CursorAnimationState 独立 timeline）
    let cursor_ctrl = read_src("src/sujian_editor_item/cursor_controller.rs");
    assert!(
        cursor_ctrl.contains("CursorTransition::Tween"),
        "Timeline B: cursor_controller.apply_plan 应处理 Tween（纯光标移动）"
    );
    assert!(
        cursor_ctrl.contains("CursorAnimationState"),
        "Timeline B: apply_plan Tween 分支应创建 CursorAnimationState"
    );
    assert!(
        cursor_ctrl.contains("started_at") && cursor_ctrl.contains("duration_ms"),
        "Timeline B: CursorAnimationState 自持 started_at / duration_ms"
    );

    // 分叉消除：RenderPlan 不再有 cursor sample 状态机。
    let render_plan = read_src("src/sujian_editor_item/render_plan.rs");
    assert!(
        !render_plan.contains("CursorSampleOutcome") && !render_plan.contains("Coordinated"),
        "分叉消除: render_plan 无 CursorSampleOutcome / Coordinated"
    );

    // 分叉消除：qquickitem_impl 每帧唯一采样点是 tick_animation(frame_now)。
    let qquick = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    assert!(
        qquick.contains("self.cursor_ctrl.tick_animation(frame_now);"),
        "分叉消除: update_paint_node 每帧调用 cursor_ctrl.tick_animation(frame_now)"
    );
    assert!(
        !qquick.contains("CursorSampleOutcome"),
        "分叉消除: qquickitem_impl 无 CursorSampleOutcome 分支"
    );

    println!("[BUGFIX_VERIFY] dual_timeline_fork_eliminated: 只剩一条光标 timeline");
    println!("[BUGFIX_VERIFY]   正文事务不再驱动 caret progress（CursorSampleOutcome 已删）");
    println!("[BUGFIX_VERIFY]   纯光标移动: apply_plan 建 Tween → tick_animation(frame_now) 推进 → 回写 drawn caret");
}
