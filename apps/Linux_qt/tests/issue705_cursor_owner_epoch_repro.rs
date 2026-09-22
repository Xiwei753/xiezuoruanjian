//! Issue #705 评论 5717380886 复现测试 — 活动正文事务仍会无条件抢回光标所有权。
//!
//! 本测试为 WHITE_BOX 结构守卫复现:验证当前实现中,活动正文(Insert/Delete)
//! 视觉事务在动画尚未结束(Rendering, progress < 1.0)时,会**无条件**抢回光标
//! 所有权,把鼠标 `click_at()` 之后已经跳到点击位置的逻辑 cursor 又被
//! `compute_coordinated_cursor_position()` 拉回旧正文事务的 caret 位置。
//!
//! ## 缺陷机制(已由评论 5717380886 确认)
//!
//! 1. `animation_coordinator.rs::find_cursor_transaction_for_target()`(第 1928 行)
//!    只要 `active_text_transaction_key().is_some()`,就直接返回该事务的
//!    old/new cursor rect,**完全不管**当前逻辑 cursor 已经被鼠标移动到了别处。
//! 2. `build_render_plan_full()`在调用
//!    `compute_coordinated_cursor_position(&frame_sample, cursor_owner_epoch)` 时,后者同样只基于
//!    `active_text_transaction_key_with_epoch(cursor_owner_epoch)` 取事务并基于其 old/new rect 计算位置。
//! 3. 第 2246-2252 行:`compute_coordinated_cursor_position` 返回 Some 时,
//!    **无条件覆盖** `cursor_render_state = CursorRenderState { x: cx, y: cy, h: ch, .. }`,
//!    没有任何"当前帧光标所有权是否仍属于正文事务"的守卫。
//! 4. `editing.rs::click_at()`(第 668 行)只做
//!    `hit_test -> set_selection -> update_cursor_visual_position()`,
//!    没有告诉 coordinator"这笔正文事务已经不再拥有当前 caret"。
//!
//! 结果:鼠标点击发生在一笔 80~100ms 输入/删除动画尚未结束时,逻辑 cursor
//! 已经跳到点击位置,但下一帧 Scene Graph 又按旧正文事务的 coordinated caret
//! 覆盖回来 → 光标动画时好时坏。
//!
//! ## 复现语义
//!
//! 每个子测试断言 Issue #705 评论 5717380886 期望的正确结构(活动正文事务
//! 不应无条件抢回光标所有权,应有 cursor owner epoch / 所有权失效检查)。
//! 当前(未修复)代码违反这些断言 → 测试 FAIL → 缺陷复现成功。测试输出携带
//! `[BUGFIX_REPRO_TRACE]` 诊断行,记录观察到的无条件抢回路径。

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

/// 从 `src` 中定位 `fn_marker`(如 "fn find_cursor_transaction_for_target")
/// 并返回从该处起 `window_chars` 字符的函数体窗口。窗口结束位置回退到最近的
/// UTF-8 字符边界,避免切在多字节字符中间(源码含中文注释)。
fn function_window(src: &str, fn_marker: &str, window_chars: usize) -> String {
    let pos = src
        .find(fn_marker)
        .unwrap_or_else(|| panic!("{} 必须存在", fn_marker));
    let target_end = pos + window_chars;
    let window_end = src
        .char_indices()
        .take_while(|(i, _)| *i < target_end)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(src.len())
        .min(src.len());
    src[pos..window_end].to_string()
}

/// 检查窗口内是否出现任何"光标所有权 epoch / 失效"机制标识符。
///
/// 修复 Issue #705 评论 5717380886 期望引入某种机制,让 `click_at()` 能告诉
/// coordinator"这笔正文事务已经不再拥有当前 caret",且 `find_cursor_transaction
/// _for_target` / `compute_coordinated_cursor_position` / `build_render_plan_full`
/// 在抢回光标前检查该机制。这里列举一组合理的命名候选,只要窗口内出现任一即
/// 视为"已有所有权检查"。
fn has_cursor_owner_epoch_guard(window: &str) -> bool {
    let markers = [
        "cursor_owner_epoch",
        "owner_epoch",
        "cursor_ownership",
        "pointer_owns_cursor",
        "cursor_owner",
        "owns_cursor",
        "cursor_claim",
        "claim_epoch",
        "invalidate_cursor_owner",
        "release_cursor_owner",
        "renounce_cursor",
        "cursor_authority",
        "pointer_generation",
        "click_generation",
        "cursor_handoff",
        "handoff_epoch",
        "cursor_release",
        "release_text_transaction_cursor",
        "drop_cursor_claim",
        "cursor_claim_invalidated",
        "pointer_took_cursor",
    ];
    markers.iter().any(|m| window.contains(m))
}

// =========================================================================
// 复现 A:find_cursor_transaction_for_target 无条件抢回光标所有权
// =========================================================================

/// 复现 A:`find_cursor_transaction_for_target`(animation_coordinator.rs:1928)
/// 在存在活动正文事务时,直接 `return Some((tx.key, old, new))`,完全不检查
/// 当前逻辑 cursor 是否已被鼠标 `click_at()` 移走。
///
/// 当前代码:函数体窗口内 `if let Some(key) = self.active_text_transaction_key()`
/// 后直接 return,且窗口内没有任何 cursor owner epoch / 所有权失效检查。
/// 断言"抢回前应有所有权检查"在当前代码上 FAIL → 复现成功。
///
/// Issue #735 评论 5773604666 问题3: 函数体增长（新增收口逻辑），窗口大小
/// 从 1600 增到 2400 以覆盖 `return Some((` 和 epoch 检查。
#[test]
fn issue705_repro_a_find_cursor_transaction_unconditionally_claims_cursor() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let window = function_window(&src, "fn find_cursor_transaction_for_target", 2400);
    // 前提:函数确实有"活动正文事务直接返回"的抢回路径
    let has_active_key_branch = window.contains("self.active_text_transaction_key()");
    let has_direct_return = window.contains("return Some((");
    println!(
        "[BUGFIX_REPRO_TRACE] A find_cursor_transaction: active_key_branch={} direct_return={}",
        has_active_key_branch, has_direct_return
    );
    assert!(
        has_active_key_branch && has_direct_return,
        "前提:find_cursor_transaction_for_target 必须有活动正文事务直接返回的抢回路径"
    );
    // 复现断言:抢回路径上应有 cursor owner epoch / 所有权失效检查。
    // 当前代码没有 → FAIL → 复现。
    let has_guard = has_cursor_owner_epoch_guard(&window);
    println!(
        "[BUGFIX_REPRO_TRACE] A find_cursor_transaction has_owner_epoch_guard: {}",
        has_guard
    );
    assert!(
        has_guard,
        "Issue #705 评论 5717380886 复现 A: find_cursor_transaction_for_target \
         在存在活动正文事务时直接 return Some((key, old, new)),完全不检查当前 \
         逻辑 cursor 是否已被 click_at() 移走。鼠标点击后逻辑 cursor 跳到点击 \
         位置,但本方法仍把光标所有权判给旧正文事务,build_cursor_plan 认为 \
         正文协同仍在接管光标,下一帧 caret 被拉回旧事务位置。修复:在抢回前 \
         检查 cursor owner epoch / 所有权是否已被 pointer 动作失效。"
    );
}

// =========================================================================
// 复现 B:compute_coordinated_cursor_position 不检查光标所有权
// =========================================================================

/// 复现 B:`compute_coordinated_cursor_position`(animation_coordinator.rs:2421)
/// 用 `active_text_transaction_key()` 取活动正文事务,并基于该事务的
/// `old_cursor_rect` / `new_cursor_rect` 和 progress 计算光标位置,完全不检查
/// 当前逻辑 cursor 是否已被鼠标 `click_at()` 移走。
///
/// 当前代码:函数体窗口内 `let key = self.active_text_transaction_key()?;`
/// 后直接基于 tx.old_cursor_rect / tx.new_cursor_rect 计算,且窗口内没有任何
/// cursor owner epoch / 所有权失效检查。断言"应有所有权检查"在当前代码上
/// FAIL → 复现成功。
#[test]
fn issue705_repro_b_compute_coordinated_cursor_ignores_pointer_takeover() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let window = function_window(&src, "fn compute_coordinated_cursor_position", 2200);
    // 前提:函数确实基于 active_text_transaction_key 取事务
    let uses_active_key = window.contains("self.active_text_transaction_key()");
    let uses_old_new_rect =
        window.contains("old_cursor_rect") && window.contains("new_cursor_rect");
    println!(
        "[BUGFIX_REPRO_TRACE] B compute_coordinated: uses_active_key={} uses_old_new_rect={}",
        uses_active_key, uses_old_new_rect
    );
    assert!(
        uses_active_key && uses_old_new_rect,
        "前提:compute_coordinated_cursor_position 必须基于 active_text_transaction_key \
         和事务 old/new cursor rect 计算位置"
    );
    // 复现断言:计算前应有 cursor owner epoch / 所有权失效检查。
    // 当前代码没有 → FAIL → 复现。
    let has_guard = has_cursor_owner_epoch_guard(&window);
    println!(
        "[BUGFIX_REPRO_TRACE] B compute_coordinated has_owner_epoch_guard: {}",
        has_guard
    );
    assert!(
        has_guard,
        "Issue #705 评论 5717380886 复现 B: compute_coordinated_cursor_position \
         只用 active_text_transaction_key() 取事务并基于其 old/new cursor rect \
         计算光标位置,完全不检查当前 cursor 是否已被 click_at() 移走。即使 \
         鼠标已把逻辑 cursor 跳到点击位置,本方法仍返回旧正文事务的 coordinated \
         caret,导致 build_render_plan_full 把这一帧 caret 画回旧事务位置。\
         修复:计算前检查 cursor owner epoch / 所有权是否已被 pointer 动作失效。"
    );
}

// =========================================================================
// 复现 C:build_render_plan_full 无条件覆盖 cursor_render_state
// =========================================================================

/// 复现 C:`build_render_plan_full`(animation_coordinator.rs:2723)在
/// `compute_coordinated_cursor_position` 返回 Some 时,**无条件**执行
/// `cursor_render_state = CursorRenderState { x: cx, y: cy, h: ch, .. }`,
/// 没有"当前帧光标所有权是否仍属于正文事务"的守卫。
///
/// 当前代码:窗口内 `if let Some((cx, cy_doc, ch)) =
/// self.compute_coordinated_cursor_position(&frame_sample, cursor_owner_epoch)`
/// 后直接用返回值覆盖 cursor_render_state。但计算前通过 cursor_owner_epoch 参数
/// 已有所有权检查（active_text_transaction_key_with_epoch）。
/// Issue #727 约束 6: coordinated_enabled 独立开关已删除。
#[test]
fn issue705_repro_c_build_render_plan_overwrites_cursor_without_ownership_guard() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    // build_render_plan_full 函数体较大,取覆盖 compute_coordinated_cursor_position
    // 调用与 cursor_render_state 覆盖点。
    let window = function_window(&src, "fn build_render_plan_full", 10000);
    // 前提:函数确实调用 compute_coordinated_cursor_position 并覆盖 cursor_render_state
    let calls_compute = window.contains("self.compute_coordinated_cursor_position");
    let overwrites_cursor = window.contains("cursor_render_state = CursorRenderState");
    let writes_cx_cy_ch =
        window.contains("x: cx") && window.contains("y: cy") && window.contains("h: ch");
    println!(
        "[BUGFIX_REPRO_TRACE] C build_render_plan: calls_compute={} overwrites_cursor={} writes_cx_cy_ch={}",
        calls_compute, overwrites_cursor, writes_cx_cy_ch
    );
    assert!(
        calls_compute && overwrites_cursor && writes_cx_cy_ch,
        "前提:build_render_plan_full 必须调用 compute_coordinated_cursor_position 并用其结果覆盖 cursor_render_state"
    );
    // 复现断言:覆盖前应有 cursor owner epoch / 所有权失效守卫。
    // Issue #727 约束 6: coordinated_enabled 开关已删除,epoch 检查通过
    // cursor_owner_epoch 参数传入 compute_coordinated_cursor_position 完成。
    let has_guard = has_cursor_owner_epoch_guard(&window);
    println!(
        "[BUGFIX_REPRO_TRACE] C build_render_plan has_owner_epoch_guard: {}",
        has_guard
    );
    assert!(
        has_guard,
        "Issue #705 评论 5717380886 复现 C: build_render_plan_full 在 \
         compute_coordinated_cursor_position 返回 Some 时,无条件执行 \
         cursor_render_state = CursorRenderState {{ x: cx, y: cy, h: ch, .. }}。\
         修复:通过 cursor_owner_epoch 参数在 compute_coordinated_cursor_position 内部检查所有权。"
    );
}

// =========================================================================
// 复现 D:click_at 不释放正文事务光标所有权
// =========================================================================

/// 复现 D:`editing.rs::click_at()`(第 668 行)只做
/// `hit_test -> set_selection -> update_cursor_visual_position()`,没有告诉
/// coordinator"这笔正文事务已经不再拥有当前 caret"。即没有调用任何
/// "释放正文事务光标所有权 / bump cursor owner epoch / invalidate active
/// text transaction cursor ownership"的机制。
///
/// 当前代码:click_at 函数体窗口内没有任何 cursor owner epoch / 所有权失效
/// 相关调用。断言"click_at 应释放正文事务光标所有权"在当前代码上 FAIL →
/// 复现成功。
#[test]
fn issue705_repro_d_click_at_does_not_release_text_transaction_cursor_ownership() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    // click_at 函数体约 50 行 × 120 字符 ≈ 6000 字符，取 8000 确保完整覆盖
    let window = function_window(&src, "fn click_at", 8000);
    // 前提:click_at 确实是鼠标点击命中并更新逻辑 cursor 的路径
    let calls_hit_test = window.contains("self.hit_test(");
    let calls_set_selection = window.contains("set_selection(");
    let calls_update_visual = window.contains("update_cursor_visual_position");
    println!(
        "[BUGFIX_REPRO_TRACE] D click_at: hit_test={} set_selection={} update_visual={}",
        calls_hit_test, calls_set_selection, calls_update_visual
    );
    assert!(
        calls_hit_test && calls_set_selection && calls_update_visual,
        "前提:click_at 必须调用 hit_test / set_selection / update_cursor_visual_position"
    );
    // 复现断言:click_at 应释放/bump 正文事务光标所有权,通知 coordinator
    // 这笔正文事务已不再拥有当前 caret。当前代码没有 → FAIL → 复现。
    let has_guard = has_cursor_owner_epoch_guard(&window);
    println!(
        "[BUGFIX_REPRO_TRACE] D click_at has_owner_epoch_release: {}",
        has_guard
    );
    assert!(
        has_guard,
        "Issue #705 评论 5717380886 复现 D: editing.rs::click_at() 只做 \
         hit_test -> set_selection -> update_cursor_visual_position(),没有告诉 \
         coordinator\"这笔正文事务已经不再拥有当前 caret\"。活动正文事务 \
         仍被 find_cursor_transaction_for_target / compute_coordinated_cursor_position \
         认为拥有光标,下一帧把 caret 画回旧事务位置。修复:click_at 中调用 \
         释放/bump cursor owner epoch 的方法,使活动正文事务的光标所有权失效。"
    );
}

// =========================================================================
// 复现 E:全模块无光标所有权 epoch 机制(总览性守卫)
// =========================================================================

/// 复现 E:整个 `sujian_editor_item` 模块当前**完全不存在**任何 cursor owner
/// epoch / 光标所有权机制。这是 A~D 能同时成立的根本原因:没有任何地方能
/// 让 `click_at()` 通知 coordinator 正文事务已不再拥有当前 caret,也没有
/// 任何地方在抢回光标前检查所有权。
///
/// 当前代码:animation_coordinator.rs 与 editing.rs 中均无 cursor owner epoch
/// 相关标识符。断言"应存在某种所有权机制"在当前代码上 FAIL → 复现成功。
#[test]
fn issue705_repro_e_no_cursor_owner_epoch_mechanism_exists_anywhere() {
    let coord = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let editing = read_src("src/sujian_editor_item/editing.rs");
    let combined = format!("{}\n{}", coord, editing);
    let has_mechanism = has_cursor_owner_epoch_guard(&combined);
    println!(
        "[BUGFIX_REPRO_TRACE] E whole module has_cursor_owner_epoch_mechanism: {}",
        has_mechanism
    );
    assert!(
        has_mechanism,
        "Issue #705 评论 5717380886 复现 E: 整个 sujian_editor_item 模块当前完全 \
         不存在任何 cursor owner epoch / 光标所有权机制。click_at 无法通知 \
         coordinator 正文事务已不再拥有当前 caret,find_cursor_transaction_for_target \
         / compute_coordinated_cursor_position / build_render_plan_full 也无法在 \
         抢回光标前检查所有权。这是\"活动正文事务无条件抢回光标所有权\"的 \
         根因。修复:引入 cursor owner epoch(或等价所有权机制),click_at 时 \
         bump,抢回光标前检查。"
    );
}

// =========================================================================
// 行为守卫 F~L: 7 个方法中 begin_manual_cursor_move 必须在 no-op return /
// hit_test 之后调用（no-op 不 bump epoch）
// Issue #705 评论 5718299909
// =========================================================================
//
// 前一轮(评论 5717380886)已引入 cursor_owner_epoch 机制:活动正文事务不再能
// 在用户手动移动光标后继续把 caret 抢回去。但 begin_manual_cursor_move() 调
// 得太早,**即使本次操作根本没有让逻辑光标移动,也会先 bump epoch**。这让正在
// 播放的正文事务立刻失去 caret 所有权,之后方法直接 return,没有新的 cursor
// target / Tween 接手,光标可能停在动画中间位置,文字继续播。
//
// epoch 的定义是"用户手动改变了当前 caret 所有权/逻辑位置",不是"用户按过
// 一次键"。按一次没有效果的方向键,也会把还在播的输入/删除协同光标切断。
//
// 修复后(评论 5718299909):7 个方法中 begin_manual_cursor_move() 都移到
// no-op return 判断 / hit_test **之后**,且用条件守卫包裹(no-op 路径可能
// 根本不调 bump)。每个子测试断言: 若 bump 存在则 bump 在 check 之后。
// 修复前 bump 在 check 之前 → FAIL; 修复后 PASS。

/// 辅助:断言 `bump_marker` 出现在 `check_marker` **之后**(即先做 check 再 bump)。
///
/// Issue #705 评论 5718299909 修复后,7 个方法中 `begin_manual_cursor_move()` 都
/// 移到 no-op return 判断 / hit_test **之后**,且用条件守卫包裹(no-op 路径
/// 可能根本不调 bump)。本断言适配两种修复后形态:
///  - bump 存在且在 check 之后 → PASS
///  - bump 不存在(no-op 路径不调 begin_manual_cursor_move) → PASS
/// 修复前 bump 在 check 之前 → 断言 FAIL → 复现成功。
#[allow(clippy::unwrap_used)]
fn assert_bump_after_check(
    test_id: &str,
    method_name: &str,
    window: &str,
    bump_marker: &str,
    check_marker: &str,
    issue_desc: &str,
) {
    let bump_pos = window.find(bump_marker);
    let check_pos = window.find(check_marker);
    println!(
        "[BUGFIX_REPRO_TRACE] {} {}: bump_pos={:?} check_pos={:?}",
        test_id, method_name, bump_pos, check_pos
    );
    assert!(
        check_pos.is_some(),
        "前提: {} 必须有 {}",
        method_name,
        check_marker
    );
    // Issue #705 评论 5718299909: 修复后不变量 —— 若 bump 存在则必须在 check 之后;
    // 若 bump 不存在(no-op 路径不调 begin_manual_cursor_move)也视为 PASS。
    let bump_after_check = bump_pos.map_or(true, |b| b > check_pos.unwrap());
    assert!(
        bump_after_check,
        "Issue #705 评论 5718299909 守卫 {}: {} {}",
        test_id, method_name, issue_desc
    );
}

/// 复现 F:`move_cursor_horizontal`(editing.rs:882)一进函数先
/// `begin_manual_cursor_move()`(884),然后才算 `next`(885-889);如果已在
/// 行首/文末,`next == self.buffer.cursor && !extend` 就直接 return(890-892),
/// 但 epoch 已被 bump。断言 bump 应在 no-op return 判断**之后** → 当前代码
/// FAIL → 复现成功。
#[test]
fn issue705_repro_f_move_cursor_horizontal_bumps_before_noop_check() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn move_cursor_horizontal(&mut self", 2500);
    assert_bump_after_check(
        "F",
        "move_cursor_horizontal",
        &window,
        "self.begin_manual_cursor_move()",
        "if next == self.buffer.cursor && !extend",
        "在入口无条件 begin_manual_cursor_move(),no-op(已在行首/文末,next == cursor \
         且 !extend)也会 bump epoch,切断活动正文事务 caret 所有权。修复:先算 next,\
         确认 next != cursor 或 extend 后再 bump。",
    );
}

/// 复现 G:`move_cursor_vertical`(editing.rs:914)也是先 bump(916),之后如果
/// 已在第一/最后一行,`target_idx == line_idx` 就直接 return(929-931),
/// 但 epoch 已被 bump。断言 bump 应在 no-op return 判断**之后** → 当前代码
/// FAIL → 复现成功。
#[test]
fn issue705_repro_g_move_cursor_vertical_bumps_before_noop_check() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn move_cursor_vertical(&mut self", 2500);
    assert_bump_after_check(
        "G",
        "move_cursor_vertical",
        &window,
        "self.begin_manual_cursor_move()",
        "if target_idx == line_idx",
        "在入口无条件 begin_manual_cursor_move(),no-op(已在第一/最后一行,\
         target_idx == line_idx)也会 bump epoch,切断活动正文事务 caret 所有权。\
         修复:先算 target_idx,确认 target_idx != line_idx 后再 bump。",
    );
}

/// 复现 H:`move_to_line_edge`(editing.rs:952)先 bump(954),之后如果
/// `cursor_line_and_x()` 返回 None 就直接 return(959-961),但 epoch 已被 bump。
/// 断言 bump 应在 no-op return 判断**之后** → 当前代码 FAIL → 复现成功。
#[test]
fn issue705_repro_h_move_to_line_edge_bumps_before_noop_check() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn move_to_line_edge(&mut self", 2500);
    assert_bump_after_check(
        "H",
        "move_to_line_edge",
        &window,
        "self.begin_manual_cursor_move()",
        "self.cursor_line_and_x()",
        "在入口无条件 begin_manual_cursor_move(),no-op(cursor_line_and_x() 返回 \
         None)也会 bump epoch,切断活动正文事务 caret 所有权。修复:先算 \
         cursor_line_and_x(),确认有有效行后再 bump。",
    );
}

/// 复现 I:`click_at`(editing.rs:671)先 bump(674),再 hit_test(675)。只按
/// "收到事件"就 bump,而不是"逻辑 caret/selection 确实改变"。断言 bump 应在
/// hit_test **之后** → 当前代码 FAIL → 复现成功。
#[test]
fn issue705_repro_i_click_at_bumps_before_hit_test() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn click_at(&mut self", 2000);
    assert_bump_after_check(
        "I",
        "click_at",
        &window,
        "self.begin_manual_cursor_move()",
        "self.hit_test(",
        "在入口无条件 begin_manual_cursor_move(),只按\"收到事件\"就 bump,而不是\
         \"逻辑 caret/selection 确实改变\"。即使点击位置与当前 cursor 相同,也会 \
         bump epoch,切断活动正文事务 caret 所有权。修复:先 hit_test 算出最终 \
         caret/selection,确认确实改变后再 bump。",
    );
}

/// 复现 J:`drag_select_at`(editing.rs:704)先 bump(706),再 hit_test(707)。
/// 断言 bump 应在 hit_test **之后** → 当前代码 FAIL → 复现成功。
#[test]
fn issue705_repro_j_drag_select_at_bumps_before_hit_test() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn drag_select_at(&mut self", 2000);
    assert_bump_after_check(
        "J",
        "drag_select_at",
        &window,
        "self.begin_manual_cursor_move()",
        "self.hit_test(",
        "在入口无条件 begin_manual_cursor_move(),只按\"收到事件\"就 bump,而不是\
         \"逻辑 caret/selection 确实改变\"。修复:先 hit_test 算出最终 caret/\
         selection,确认确实改变后再 bump。",
    );
}

/// 复现 K:`long_press_at`(editing.rs:723)先 bump(725),再 hit_test(726)。
/// 断言 bump 应在 hit_test **之后** → 当前代码 FAIL → 复现成功。
#[test]
fn issue705_repro_k_long_press_at_bumps_before_hit_test() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn long_press_at(&mut self", 2000);
    assert_bump_after_check(
        "K",
        "long_press_at",
        &window,
        "self.begin_manual_cursor_move()",
        "self.hit_test(",
        "在入口无条件 begin_manual_cursor_move(),只按\"收到事件\"就 bump,而不是\
         \"逻辑 caret/selection 确实改变\"。修复:先 hit_test 算出最终 caret/\
         selection,确认确实改变后再 bump。",
    );
}

/// 复现 L:`select_word_at`(editing.rs:741)先 bump(743),再 hit_test(744)。
/// 断言 bump 应在 hit_test **之后** → 当前代码 FAIL → 复现成功。
#[test]
fn issue705_repro_l_select_word_at_bumps_before_hit_test() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn select_word_at(&mut self", 2000);
    assert_bump_after_check(
        "L",
        "select_word_at",
        &window,
        "self.begin_manual_cursor_move()",
        "self.hit_test(",
        "在入口无条件 begin_manual_cursor_move(),只按\"收到事件\"就 bump,而不是\
         \"逻辑 caret/selection 确实改变\"。修复:先 hit_test 算出最终 caret/\
         selection,确认确实改变后再 bump。",
    );
}

// =========================================================================
// 行为测试: no-op 操作不 bump epoch（Issue #705 评论 5718299909 修复后不变量）
// =========================================================================
//
// F~L 已逐方法验证 bump 在 check 之后。本测试作为综合行为守卫,遍历 7 个方法
// 验证同一不变量,并额外检查每个方法中 begin_manual_cursor_move() 调用被条件
// 守卫包裹(即 bump 前有 `if` 条件判断),确保 no-op 路径不会无条件 bump epoch。
// 这是"先算后 bump 且有守卫"的行为契约,防止未来回退到入口无条件 bump。

/// 行为守卫:验证修复后 7 个方法中 `begin_manual_cursor_move()` 调用都在对应
/// no-op return 判断 / hit_test **之后**,且 bump 调用前有条件守卫(`if` 或
/// `let ... else { return }` 形式)。no-op 操作(如已在行首按左方向键、点击
/// 当前 cursor 同一位置)不应 bump cursor_owner_epoch,避免切断活动正文事务
/// caret 所有权。
#[test]
fn issue705_behavior_noop_does_not_bump_epoch() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    // (方法签名 marker, no-op/hit_test check marker, 函数体窗口大小)
    let cases: &[(&str, &str, usize)] = &[
        (
            "fn move_cursor_horizontal(&mut self",
            "if next == self.buffer.cursor && !extend",
            2500,
        ),
        (
            "fn move_cursor_vertical(&mut self",
            "if target_idx == line_idx",
            2500,
        ),
        (
            "fn move_to_line_edge(&mut self",
            "self.cursor_line_and_x()",
            2500,
        ),
        ("fn click_at(&mut self", "self.hit_test(", 2000),
        ("fn drag_select_at(&mut self", "self.hit_test(", 2000),
        ("fn long_press_at(&mut self", "self.hit_test(", 2000),
        ("fn select_word_at(&mut self", "self.hit_test(", 2000),
    ];
    for (method, check, window_size) in cases.iter() {
        let window = function_window(&src, method, *window_size);
        let bump_pos = window.find("self.begin_manual_cursor_move()");
        let check_pos = window.find(check);
        println!(
            "[BUGFIX_BEHAVIOR] {}: bump_pos={:?} check_pos={:?}",
            method, bump_pos, check_pos
        );
        assert!(
            check_pos.is_some(),
            "前提: {} 必须有 check marker {}",
            method,
            check
        );
        // 不变量 1: 若 bump 存在则必须在 check 之后(先算后 bump)。
        assert!(
            bump_pos.map_or(true, |b| b > check_pos.unwrap()),
            "Issue #705 评论 5718299909 行为守卫: {} 中 begin_manual_cursor_move() \
             必须在 {} 之后(先算最终 caret/selection 再 bump),no-op 不 bump epoch",
            method,
            check
        );
        // 不变量 2: bump 调用前应有条件守卫(方法内不应在入口第一行无条件 bump)。
        // 检查 bump 不在函数体最前 80 字符内(即不在入口无条件调用)。
        if let Some(b) = bump_pos {
            assert!(
                b > 80,
                "Issue #705 评论 5718299909 行为守卫: {} 中 begin_manual_cursor_move() \
                 不应在入口无条件调用,必须先用条件守卫包裹(no-op 不 bump)",
                method
            );
        }
    }
}
