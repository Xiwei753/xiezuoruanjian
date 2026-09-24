//! Issue #738 评论 5795950264 修复后守卫测试 — 验证 3 个并发视觉事务结构缺陷
//! 已被正确修复。
//!
//! 本测试是源代码静态守卫测试（WHITE_BOX），通过读取源文件并检查代码模式来确认
//! 评论 5795950264 指出的 3 个结构缺陷的修复后正确结构在当前代码中确实存在。
//! 测试 PASS = 修复后结构正确。
//!
//! 3 个问题的修复后正确结构：
//! 1. reconcile_active_transactions_with_canonical 先 retire CaretDriven units 再 rebind
//!    Timed Reflow；rebind_timed_units_to_canonical 有条件提升 layout_basis_revision
//!    （仅当 caret_motion_retired || !has_caret_driven_units 时才提升），防止旧 caret
//!    track 重新拿到 ownership 在新 canonical 上继续用旧布局几何。
//! 2. build_crossfade_split_replacement_units 接住当前帧 opacity（current_opacity 参数），
//!    replacement 的 opacity_from 用 current_opacity 不再用 unit.slice.opacity_from；
//!    current_rect 的 w/h 按 visible 从 from→to 插值，与 compute_frame 一致。
//! 3. New side 因 movement vector 不一致 Split 时，记录 group_has_split 标记，old side
//!    判断加入 !group_has_split 条件，Split 时走 RebindOldSideInPlace 原位 fade-out，
//!    不再追整个 target_union（跨行拉伸）。

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

/// 在 `src` 中定位 `fn_marker`，返回从该处起 `window_size` 字符的函数体窗口。
/// 窗口结束位置回退到最近的 UTF-8 字符边界。
fn function_window(src: &str, fn_marker: &str, window_size: usize) -> String {
    let pos = src
        .find(fn_marker)
        .unwrap_or_else(|| panic!("{} 必须存在", fn_marker));
    let target_end = pos + window_size;
    let window_end = src
        .char_indices()
        .take_while(|(i, _)| *i < target_end)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(src.len())
        .min(src.len());
    src[pos..window_end].to_string()
}

// =========================================================================
// 问题1守卫: reconcile 先 retire CaretDriven 再 rebind，rebind 有条件提升 basis
// =========================================================================

/// 守卫1a: reconcile_active_transactions_with_canonical 中先调
/// retire_caret_driven_units_for_transaction(key) retire CaretDriven units，
/// 再调 rebind_timed_units_to_canonical 重绑 Timed Reflow。retire 调用位置
/// 必须在 rebind 调用之前。
#[test]
fn fix1a_reconcile_retires_caret_driven_before_rebind() {
    let src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let window = function_window(
        &src,
        "fn reconcile_active_transactions_with_canonical",
        6000,
    );

    let retire_marker = "retire_caret_driven_units_for_transaction(key)";
    let rebind_marker = "rebind_timed_units_to_canonical";

    assert!(
        window.contains(retire_marker),
        "修复后 reconcile 应先调 retire_caret_driven_units_for_transaction(key) retire CaretDriven units。"
    );
    assert!(
        window.contains(rebind_marker),
        "修复后 reconcile 应调 rebind_timed_units_to_canonical 重绑 Timed Reflow。"
    );

    let retire_pos = window
        .find(retire_marker)
        .expect("retire marker 已确认存在");
    let rebind_pos = window
        .find(rebind_marker)
        .expect("rebind marker 已确认存在");
    assert!(
        retire_pos < rebind_pos,
        "修复后 retire 调用必须在 rebind 调用之前，否则 rebind 会先提升 layout_basis_revision \
         导致 basis 守卫不再 retire 旧 CaretDriven。"
    );
}

/// 守卫1b: rebind_timed_units_to_canonical 中 layout_basis_revision 的提升被包在
/// `self.caret_motion_retired || !self.has_caret_driven_units()` 条件里，不再无条件提升。
/// 具体断言：窗口内 `self.layout_basis_revision = current_layout_revision;` 出现次数
/// 应等于 `self.caret_motion_retired || !self.has_caret_driven_units()` 出现次数
///（即每次提升都有条件守卫）。
#[test]
fn fix1b_rebind_conditionally_promotes_layout_basis_revision() {
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 30000);

    let condition = "self.caret_motion_retired || !self.has_caret_driven_units()";
    let promotion = "self.layout_basis_revision = current_layout_revision;";

    assert!(
        window.contains(condition),
        "修复后 rebind 应有条件判断 self.caret_motion_retired || !self.has_caret_driven_units()。"
    );

    let condition_count = window.matches(condition).count();
    let promotion_count = window.matches(promotion).count();
    assert!(
        condition_count > 0,
        "修复后 rebind 中条件守卫应至少出现一次。"
    );
    assert_eq!(
        condition_count, promotion_count,
        "修复后每次 layout_basis_revision 提升都应有条件守卫：条件出现 {} 次但提升出现 {} 次。",
        condition_count, promotion_count
    );
}

// =========================================================================
// 问题2守卫: CrossFade Split 接住当前 opacity，current_rect w/h 按 visible 插值
// =========================================================================

/// 守卫2a: build_crossfade_split_replacement_units 新增 current_opacity: f64 参数，
/// replacement 的 opacity_from 用 current_opacity，不再用 unit.slice.opacity_from。
#[test]
fn fix2a_crossfade_split_replacement_uses_current_opacity() {
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    let window = function_window(&src, "fn build_crossfade_split_replacement_units", 6000);

    assert!(
        window.contains("current_opacity: f64"),
        "修复后 build_crossfade_split_replacement_units 应有 current_opacity: f64 参数。"
    );
    assert!(
        window.contains("opacity_from: current_opacity"),
        "修复后 replacement 的 opacity_from 应为 current_opacity，接住当前帧透明度。"
    );
    assert!(
        !window.contains("opacity_from: unit.slice.opacity_from"),
        "修复后不应再用 opacity_from: unit.slice.opacity_from（旧值 New side=0.0 会导致拆分第一帧闪）。"
    );
}

/// 守卫2b: rebind_timed_units_to_canonical 中 current_rect 的 w/h 按 visible 从
/// from→to 插值（与 ReflowCrossFade::compute_frame 一致），不再直接取 to_document_rect.w/h。
/// 检查插值表达式 `(anchor.to_document_rect.w - anchor.from_document_rect.w)` 存在。
#[test]
fn fix2b_current_rect_wh_interpolates_by_visible() {
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 30000);

    assert!(
        window.contains("(anchor.to_document_rect.w - anchor.from_document_rect.w)"),
        "修复后 current_rect 的 w 应按 visible 插值：包含 (anchor.to_document_rect.w - anchor.from_document_rect.w) 表达式。"
    );
    assert!(
        window.contains("(anchor.to_document_rect.h - anchor.from_document_rect.h)"),
        "修复后 current_rect 的 h 应按 visible 插值：包含 (anchor.to_document_rect.h - anchor.from_document_rect.h) 表达式。"
    );
}

/// 守卫2c: 调用 build_crossfade_split_replacement_units 前用
/// compute_frame(split_visible).opacity 算出当前帧透明度传入。
#[test]
fn fix2c_crossfade_split_computes_current_opacity_before_call() {
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 30000);

    assert!(
        window.contains("compute_frame(split_visible).opacity"),
        "修复后调用 build_crossfade_split_replacement_units 前应算 compute_frame(split_visible).opacity 接住当前帧透明度。"
    );
}

// =========================================================================
// 问题3守卫: New side Split 时 old side 原位 fade-out 不追 target_union
// =========================================================================

/// 守卫3: rebind_timed_units_to_canonical 中记录 group_has_split 标记，
/// Split 时设 group_has_split = true，old side 判断加入 !group_has_split 条件，
/// Split 时走 RebindOldSideInPlace 原位 fade-out。
#[test]
fn fix3_old_side_inplace_when_new_side_splits() {
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 30000);

    assert!(
        window.contains("group_has_split"),
        "修复后应有 group_has_split 变量记录本 group 是否发生 Split。"
    );
    assert!(
        window.contains("group_has_split = true"),
        "修复后 Split 决策时应设 group_has_split = true 标记。"
    );
    assert!(
        window.contains("!group_has_split && group.old_indices.len() == 1 && group.new_indices.len() == 1"),
        "修复后 old side 判断应加入 !group_has_split 条件，Split 时走原位 fade-out 不追 target_union。"
    );
}
