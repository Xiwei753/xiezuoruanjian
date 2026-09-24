//! Issue #702 评论 5707770318 修复后结构守卫 — 确认正文协同光标与纯光标 Tween
//! 双时间线分叉已消除。
//!
//! WHITE_BOX 结构守卫：验证 issue #702 评论 5707770318 描述的"双时间线分叉"
//! 结构已被修复。这些结构的不存在即证实了缺陷已被消除（修复成功）：
//!
//! 修复点 1：`build_cursor_plan()` 对 Insert/Delete 在有活跃正文事务且
//!   has_active_for_coordinated 时不再返回 `CursorTransition::Tween`，而是返回
//!   `CursorTransition::Snap`（让 apply_plan 清除 animation，不创建独立 timeline）。
//!
//! 修复点 2：`CursorSampleOutcome` 枚举增加了 `Coordinated { x, y, h }` 变体
//!   ——正文协同帧不再落进 Idle。
//!
//! 修复点 3：`qquickitem_impl.rs` 增加了 `Coordinated` 分支，同步 visual_x/y/h
//!   但不启动 `CursorAnimationState.started_at`，并清除残留 animation。
//!
//! 修复点 4：`build_render_plan_full()` 中 `compute_coordinated_cursor_position`
//!   成功时把 `cursor_sample_outcome` 设为 `Coordinated { x, y, h }`。
//!
//! 结果：正文事务活跃时，光标位置只由 compute_coordinated_cursor_position 驱动
//! （正文协同），不创建 CursorAnimationState 独立 timeline；只有纯方向键/
//! Home/End/鼠标点击这类没有正文视觉事务的移动才走 CursorAnimationState 自己
//! 的 timeline。两条路径互斥。

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
/// 用于精确检查某个分支体内的代码片段。
/// 安全处理 UTF-8 字符边界：若-如果 end 落在字符中间，回退到最近的字符边界。
fn window_after(src: &str, anchor: &str, window_chars: usize) -> String {
    let pos = src
        .find(anchor)
        .unwrap_or_else(|| panic!("anchor not found: {}", anchor));
    let end = pos + anchor.len() + window_chars;
    // 找到 <= end 的最大字符边界
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

// =========================================================================
// 修复点 1：build_cursor_plan 对 Insert/Delete 在有活跃正文事务且
// has_active_for_coordinated 时返回 Snap，而不是返回 CursorTransition::Tween
// =========================================================================

#[test]
fn fix1_build_cursor_plan_returns_snap_for_insert_delete_with_active_transaction() {
    let src = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");
    // build_cursor_plan 必须存在
    assert!(
        src.contains("pub(crate) fn build_cursor_plan"),
        "修复点1: build_cursor_plan 必须存在"
    );
    // Issue #727 约束 6: coordinated_enabled 独立开关已删除。
    // 判断条件改为 has_active_for_coordinated。
    // 处 2（anim.target_x 偏移分支）：has_active_for_coordinated → Snap
    // Issue #727: comments between anchor and Snap are ~700 chars, use 1500 to be safe
    let ctx2 = window_after(&src, "(anim.target_x - cursor_x).abs() > 0.01", 1500);
    assert!(
        ctx2.contains("has_active_for_coordinated") && ctx2.contains("CursorTransition::Snap"),
        "修复点1 处2: anim.target_x 偏移分支应有 has_active_for_coordinated 返回 Snap"
    );
    //
    // 处 3（old_visual_x 偏移分支）：has_active_for_coordinated → Snap
    let ctx3 = window_after(&src, "(old_visual_x - cursor_x).abs() > 0.01", 1500);
    assert!(
        ctx3.contains("has_active_for_coordinated") && ctx3.contains("CursorTransition::Snap"),
        "修复点1 处3: old_visual_x 偏移分支应有 has_active_for_coordinated 返回 Snap"
    );
    println!("[BUGFIX_VERIFY] fix1: build_cursor_plan 对 Insert/Delete 在 has_active_for_coordinated 时返回 Snap（不再开独立 Tween timeline）");
}

// =========================================================================
// 修复点 2：CursorSampleOutcome 枚举有 Coordinated 变体
// =========================================================================

#[test]
fn fix2_cursor_sample_outcome_has_coordinated_variant() {
    let src = read_src("src/sujian_editor_item/render_plan.rs");
    // CursorSampleOutcome 枚举必须存在
    assert!(
        src.contains("enum CursorSampleOutcome"),
        "修复点2: CursorSampleOutcome 枚举必须存在"
    );
    // 保留原有变体
    assert!(
        src.contains("Idle"),
        "修复点2: CursorSampleOutcome 应有 Idle 变体"
    );
    assert!(
        src.contains("Running(f64)"),
        "修复点2: CursorSampleOutcome 应有 Running(f64) 变体"
    );
    assert!(
        src.contains("Finished"),
        "修复点2: CursorSampleOutcome 应有 Finished 变体"
    );
    // 新增 Coordinated 变体 —— 正文协同帧不再落进 Idle
    assert!(
        src.contains("Coordinated"),
        "修复点2: CursorSampleOutcome 应有 Coordinated 变体，正文协同帧不再落进 Idle"
    );
    assert!(
        src.contains("Coordinated { x: f64, y: f64, h: f64 }"),
        "修复点2: Coordinated 变体应携带 x/y/h 三元组"
    );
    println!(
        "[BUGFIX_VERIFY] fix2: CursorSampleOutcome 有 Coordinated 变体，正文协同帧不再落进 Idle"
    );
}

// =========================================================================
// 修复点 3：qquickitem_impl.rs 有 Coordinated 分支，不启动 started_at
// =========================================================================

#[test]
fn fix3_coordinated_branch_does_not_start_cursor_animation_state_timeline() {
    let src = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    // match cursor_sample_outcome 必须存在
    assert!(
        src.contains("cursor_sample_outcome"),
        "修复点3: qquickitem_impl 应 match cursor_sample_outcome"
    );
    // Coordinated 分支存在
    let coordinated_marker = "CursorSampleOutcome::Coordinated { x, y, h } =>";
    assert!(
        src.contains(coordinated_marker),
        "修复点3: qquickitem_impl 应有 CursorSampleOutcome::Coordinated 分支"
    );
    // Coordinated 分支内同步 visual_x/visual_y，不启动 started_at
    let coordinated_window = window_after(&src, coordinated_marker, 400);
    assert!(
        coordinated_window.contains("visual_x"),
        "修复点3: Coordinated 分支应同步 visual_x"
    );
    assert!(
        coordinated_window.contains("visual_y"),
        "修复点3: Coordinated 分支应同步 visual_y"
    );
    assert!(
        !coordinated_window.contains("started_at = Some(frame_now)"),
        "修复点3: Coordinated 分支不应启动 CursorAnimationState.started_at"
    );
    assert!(
        coordinated_window.contains("animation = None"),
        "修复点3: Coordinated 分支应清除残留 animation"
    );
    // Idle 分支仍保留（只服务纯光标 CursorOnly 首帧启动）
    assert!(
        src.contains("CursorSampleOutcome::Idle =>"),
        "修复点3: qquickitem_impl 应保留 Idle 分支（纯光标 CursorOnly 首帧启动）"
    );
    println!("[BUGFIX_VERIFY] fix3: qquickitem_impl Coordinated 分支同步 visual_x/y 不启动 started_at，并清除残留 animation");
}

// =========================================================================
// 修复点 4：build_render_plan_full 中 compute_coordinated_cursor_position 成功时
// cursor_sample_outcome 设为 Coordinated
// =========================================================================

#[test]
fn fix4_coordinated_success_sets_cursor_sample_outcome_coordinated() {
    let src = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");
    // build_render_plan_full 必须存在
    assert!(
        src.contains("pub(crate) fn build_render_plan_full"),
        "修复点4: build_render_plan_full 必须存在"
    );
    // cursor_sample_outcome 初始化为 Idle
    // Issue #747: 拆分后路径从 super:: 改为 crate::sujian_editor_item::
    let init_marker =
        "let mut cursor_sample_outcome = crate::sujian_editor_item::render_plan::CursorSampleOutcome::Idle;";
    assert!(
        src.contains(init_marker),
        "修复点4: build_render_plan_full 应初始化 cursor_sample_outcome = Idle"
    );
    // Issue #727 约束 6: compute_coordinated_cursor_position 现在接收 cursor_owner_epoch 参数
    let coord_call = "self.compute_coordinated_cursor_position(&frame_sample, cursor_owner_epoch)";
    assert!(
        src.contains(coord_call),
        "修复点4: build_render_plan_full 应调用 compute_coordinated_cursor_position(&frame_sample, cursor_owner_epoch)"
    );
    // 关键：compute_coordinated_cursor_position 成功分支（Some((cx, cy_doc, ch))）内
    // 应把 cursor_sample_outcome 设为 Coordinated。
    let coord_success_window = window_after(&src, "if let Some((cx, cy_doc, ch))", 1200);
    assert!(
        coord_success_window.contains("cursor_sample_outcome ="),
        "修复点4: compute_coordinated_cursor_position 成功分支应修改 cursor_sample_outcome"
    );
    assert!(
        coord_success_window.contains("CursorSampleOutcome::Coordinated"),
        "修复点4: compute_coordinated_cursor_position 成功分支应把 cursor_sample_outcome 设为 Coordinated"
    );
    // 诊断跟踪 eprintln! 应已移除
    assert!(
        !coord_success_window.contains("BUGFIX_REPRO_TRACE"),
        "修复点4: 诊断跟踪 eprintln! 应已移除"
    );
    assert!(
        !src.contains("BUGFIX_REPRO_TRACE"),
        "修复点4: animation_coordinator.rs 不应再有 BUGFIX_REPRO_TRACE 诊断跟踪"
    );
    println!("[BUGFIX_VERIFY] fix4: compute_coordinated_cursor_position 成功时 cursor_sample_outcome 设为 Coordinated");
}

// =========================================================================
// 综合断言：双时间线分叉已消除（两条路径互斥）
// =========================================================================

#[test]
fn dual_timeline_fork_is_eliminated() {
    let coord = read_src("src/sujian_editor_item/animation/cursor_motion.rs");
    let render_plan_builder = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");
    let render_plan = read_src("src/sujian_editor_item/render_plan.rs");
    let qquick = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    let cursor_ctrl = read_src("src/sujian_editor_item/cursor_controller.rs");

    // Timeline A：正文协同光标（compute_coordinated_cursor_position 用正文文字单元
    // 同一帧进度算屏幕实际显示的光标位置）
    assert!(
        coord.contains("fn compute_coordinated_cursor_position"),
        "Timeline A: compute_coordinated_cursor_position 必须存在"
    );

    // Timeline B：纯光标 Tween（apply_plan 收到 Tween 后创建 CursorAnimationState）
    // —— 只服务没有正文视觉事务的纯光标移动
    assert!(
        cursor_ctrl.contains("CursorTransition::Tween"),
        "Timeline B: cursor_controller.apply_plan 应处理 Tween（纯光标移动）"
    );
    assert!(
        cursor_ctrl.contains("CursorAnimationState"),
        "Timeline B: apply_plan Tween 分支应创建 CursorAnimationState"
    );
    assert!(
        cursor_ctrl.contains("started_at"),
        "Timeline B: CursorAnimationState 应有 started_at 字段"
    );
    assert!(
        cursor_ctrl.contains("duration_ms"),
        "Timeline B: CursorAnimationState 应有 duration_ms 字段"
    );

    // 分叉消除条件 1：CursorSampleOutcome 有 Coordinated 变体
    // → 正文协同帧不再落进 Idle → 不触发 Idle 分支启动独立 timeline
    assert!(
        render_plan.contains("Coordinated"),
        "分叉消除: CursorSampleOutcome 有 Coordinated 变体"
    );
    // 分叉消除条件 2：qquickitem_impl 有 Coordinated 分支
    // → 正文协同帧同步 visual_x/y 但不启动 started_at
    assert!(
        qquick.contains("CursorSampleOutcome::Coordinated { x, y, h } =>"),
        "分叉消除: qquickitem_impl 有 Coordinated 分支"
    );
    // 分叉消除条件 3：build_cursor_plan 在 has_active_for_coordinated 时返回 Snap
    // → apply_plan 收到 Snap 执行 self.animation = None，不创建 CursorAnimationState
    let ctx2 = window_after(&render_plan_builder, "(anim.target_x - cursor_x).abs() > 0.01", 1500);
    assert!(
        ctx2.contains("has_active_for_coordinated") && ctx2.contains("CursorTransition::Snap"),
        "分叉消除: build_cursor_plan 在 has_active_for_coordinated 时返回 Snap"
    );

    println!("[BUGFIX_VERIFY] dual_timeline_fork_eliminated: 正文协同光标（Coordinated 变体）与纯光标 Tween（CursorAnimationState）两条路径互斥");
    println!("[BUGFIX_VERIFY]   正文事务活跃时: build_cursor_plan 返回 Snap → apply_plan 清除 animation → cursor_sample_outcome=Coordinated → 不启动 started_at");
    println!("[BUGFIX_VERIFY]   无正文事务时: build_cursor_plan 返回 Tween → apply_plan 创建 CursorAnimationState → cursor_sample_outcome=Idle/Running/Finished → 纯光标 timeline");
}

// =========================================================================
// 修复点 5（评论 5708209114）：build_cursor_plan 的正文活跃判断必须覆盖
// Insert/Delete/CompositionUpdate/CompositionCommitOrCancel，不能来自
// has_active_insert()（其语义只查 Insert，Delete 路径会漏判）
// =========================================================================

#[test]
fn fix5_build_cursor_plan_active_check_covers_all_text_transactions_not_only_insert() {
    let src = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");
    let coord_src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    assert!(
        src.contains("pub(crate) fn build_cursor_plan"),
        "修复点5: build_cursor_plan 必须存在"
    );
    // 关键守卫：build_cursor_plan 函数体内 has_active 的赋值不能来自 has_active_insert()。
    // Delete 路径上若用 has_active_insert() 判断，可能因语义只查 Insert 而返回 false，
    // 导致 build_cursor_plan 创建纯光标 Tween（CursorAnimationState 独立 timeline），
    // 与 compute_coordinated_cursor_position 驱动的正文协同光标形成双时间线分叉
    // （光标先到、文字后消失）——正是 #702 原本最严重的风险点。
    let plan_start = src
        .find("pub(crate) fn build_cursor_plan")
        .expect("build_cursor_plan must exist");
    let after_plan = &src[plan_start..];
    let plan_body_end = after_plan
        .find("\n    pub(crate) fn ")
        .or_else(|| after_plan.find("\n    pub fn "))
        .unwrap_or(after_plan.len());
    let plan_body = &after_plan[..plan_body_end];
    assert!(
        !plan_body.contains("let has_active = self.has_active_insert();"),
        "修复点5: build_cursor_plan 中 has_active 不能来自 has_active_insert()。\
         has_active_insert() 语义只查 Insert，Delete 路径会漏判导致 has_active=false，\
         重新引入双时间线分叉（光标先到、文字后消失）。\
         应改用 has_active_text_transaction() 或 active_text_transaction_key().is_some()。"
    );
    assert!(
        plan_body.contains("has_active_text_transaction")
            || plan_body.contains("active_text_transaction_key().is_some()"),
        "修复点5: build_cursor_plan 中 has_active 应来自 has_active_text_transaction() \
         或 active_text_transaction_key().is_some()，覆盖 Insert/Delete/IME 全部正文事务类型"
    );
    assert!(
        coord_src.contains("fn has_active_text_transaction"),
        "修复点5: animation coordinator 应定义 has_active_text_transaction 方法"
    );
    println!("[BUGFIX_VERIFY] fix5: build_cursor_plan 正文活跃判断覆盖 Insert/Delete/CompositionUpdate/CompositionCommitOrCancel，不再来自 has_active_insert()");
}
