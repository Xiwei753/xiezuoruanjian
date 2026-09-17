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
//! 2. `build_render_plan_full()`(第 2138 行)在 `coordinated_enabled` 时调用
//!    `compute_coordinated_cursor_position()`(第 2421 行),后者同样只基于
//!    `active_text_transaction_key()` 取事务并基于其 old/new rect 计算位置。
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
#[test]
fn issue705_repro_a_find_cursor_transaction_unconditionally_claims_cursor() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let window = function_window(
        &src,
        "fn find_cursor_transaction_for_target",
        1600,
    );
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
    let window = function_window(
        &src,
        "fn compute_coordinated_cursor_position",
        2200,
    );
    // 前提:函数确实基于 active_text_transaction_key 取事务
    let uses_active_key = window.contains("self.active_text_transaction_key()");
    let uses_old_new_rect = window.contains("old_cursor_rect") && window.contains("new_cursor_rect");
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

/// 复现 C:`build_render_plan_full`(animation_coordinator.rs:2138)在
/// `coordinated_enabled` 且 `compute_coordinated_cursor_position` 返回 Some 时,
/// **无条件**执行 `cursor_render_state = CursorRenderState { x: cx, y: cy, h: ch, .. }`
/// (第 2246-2252 行),没有"当前帧光标所有权是否仍属于正文事务"的守卫。
///
/// 当前代码:窗口内 `if coordinated_enabled { if let Some((cx, cy, ch)) =
/// self.compute_coordinated_cursor_position(...) { cursor_render_state = \
/// CursorRenderState { ... x: cx, y: cy, h: ch ... } } }`,且窗口内没有任何
/// cursor owner epoch / 所有权失效检查。断言"覆盖前应有所有权守卫"在当前
/// 代码上 FAIL → 复现成功。
#[test]
fn issue705_repro_c_build_render_plan_overwrites_cursor_without_ownership_guard() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    // build_render_plan_full 函数体较大,取从 coordinated_enabled 分支起的窗口,
    // 覆盖 compute_coordinated_cursor_position 调用与 cursor_render_state 覆盖点。
    let window = function_window(
        &src,
        "fn build_render_plan_full",
        10000,
    );
    // 前提:函数确实有 coordinated_enabled 分支并覆盖 cursor_render_state
    let has_coordinated_branch = window.contains("if coordinated_enabled");
    let calls_compute = window.contains("self.compute_coordinated_cursor_position");
    let overwrites_cursor = window.contains("cursor_render_state = CursorRenderState");
    let writes_cx_cy_ch = window.contains("x: cx")
        && window.contains("y: cy")
        && window.contains("h: ch");
    println!(
        "[BUGFIX_REPRO_TRACE] C build_render_plan: coordinated_branch={} calls_compute={} overwrites_cursor={} writes_cx_cy_ch={}",
        has_coordinated_branch, calls_compute, overwrites_cursor, writes_cx_cy_ch
    );
    assert!(
        has_coordinated_branch && calls_compute && overwrites_cursor && writes_cx_cy_ch,
        "前提:build_render_plan_full 必须有 coordinated_enabled 分支调用 \
         compute_coordinated_cursor_position 并用其结果覆盖 cursor_render_state"
    );
    // 复现断言:覆盖前应有 cursor owner epoch / 所有权失效守卫。
    // 当前代码没有 → FAIL → 复现。
    let has_guard = has_cursor_owner_epoch_guard(&window);
    println!(
        "[BUGFIX_REPRO_TRACE] C build_render_plan has_owner_epoch_guard: {}",
        has_guard
    );
    assert!(
        has_guard,
        "Issue #705 评论 5717380886 复现 C: build_render_plan_full 在 coordinated_enabled \
         且 compute_coordinated_cursor_position 返回 Some 时,无条件执行 \
         cursor_render_state = CursorRenderState {{ x: cx, y: cy, h: ch, .. }}。\
         没有任何\"当前帧光标所有权是否仍属于正文事务\"的守卫。鼠标 click_at() \
         之后逻辑 cursor 已跳到点击位置,但本帧 Scene Graph 仍按旧正文事务的 \
         coordinated caret 覆盖 cursor_render_state,把这一帧 caret 画回旧事务 \
         位置。修复:覆盖前检查 cursor owner epoch / 所有权是否已被 pointer \
         动作失效,失效则不覆盖、改走 CursorOnly/点击位置。"
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
    let window = function_window(&src, "fn click_at", 1200);
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
