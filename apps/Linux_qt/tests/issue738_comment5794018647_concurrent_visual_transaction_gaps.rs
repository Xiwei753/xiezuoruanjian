//! Issue #738 评论 5794018647 修复后守卫测试 — 验证 2 个并发视觉事务剩余几何缺陷
//! 已被正确修复。
//!
//! 本测试是源代码静态守卫测试（WHITE_BOX），通过读取源文件并检查代码模式来确认
//! 评论 5794018647 指出的 2 个缺陷的修复后正确结构在当前代码中确实存在。
//! 测试 PASS = 修复后结构正确。
//!
//! 2 个问题的修复后正确结构：
//! 1. CrossFade group 只管"活不活"（生命周期），每个 unit 用自己的 canonical target
//!    重绑（RebindNewSide / RebindOldSideFollow / RebindOldSideInPlace），不再对所有
//!    old/new unit 都 Rebind(union_to.clone())。
//! 2. ReflowMove 逐 anchor 用自己旧的 from→to 按 visible 采样算 current_rect，
//!    target_rect = 新 canonical cluster doc_rect。movement vector = target - current。
//!    vectors_consistent 时用 RebindMerged 逐 anchor 保留 (current_rect, target_rect)；
//!    不一致时 Split replacement 的 from = current_rect（当前屏幕帧），不再内部插值。

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
// 修复 1a 守卫: CrossFade 不再对所有成员 Rebind(union_to.clone())
// =========================================================================

/// 修复后守卫 1a: rebind_timed_units_to_canonical 中不再有"对所有 old/new unit 都
/// Rebind(union_to.clone())"的统一调用模式。改为 RebindNewSide / RebindOldSideFollow /
/// RebindOldSideInPlace 三个变体分别处理。
#[test]
fn fix1a_crossfade_no_group_union_to_all_members() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 28000);

    // 修复后：存在 RebindNewSide 变体。
    assert!(
        window.contains("RebindNewSide"),
        "修复后应有 RebindNewSide 变体（new side 用自己的 target 重绑）。"
    );
    // 修复后：存在 RebindOldSideFollow 变体。
    assert!(
        window.contains("RebindOldSideFollow"),
        "修复后应有 RebindOldSideFollow 变体（old side 一对一跟随 new target）。"
    );
    // 修复后：存在 RebindOldSideInPlace 变体。
    assert!(
        window.contains("RebindOldSideInPlace"),
        "修复后应有 RebindOldSideInPlace 变体（old side 多对多原位 fade-out）。"
    );
    // 修复后：不再有旧 RebindDecision::Rebind(union_to.clone()) 对 old side 的统一调用。
    assert!(
        !window.contains("RebindDecision::Rebind(union_to.clone())"),
        "修复后不应再有 RebindDecision::Rebind(union_to.clone()) 旧统一重绑调用。"
    );
    // 修复后：不再有旧 RebindDecision::Rebind(SourceRect) 单参数变体定义。
    assert!(
        !window.contains("Rebind(SourceRect)"),
        "修复后 enum RebindDecision 不应再有 Rebind(SourceRect) 旧单参数变体。"
    );
}

// =========================================================================
// 修复 1b 守卫: CrossFade new side 逐 unit 算自己的 target
// =========================================================================

/// 修复后守卫 1b: 存在 RebindNewSide { ... } 结构，且 new side 逐 unit 算自己的
/// target（存在 new_side_rebinds 变量收集每个 unit 自己的 anchor_rebinds + target_union）。
/// Issue #738 评论 5795183758 把 new_side_targets 演进为 new_side_rebinds（逐 anchor
/// 保留 current_rect/target_rect），target_union 仍由 anchor_rebinds 归约得到。
#[test]
fn fix1b_crossfade_new_side_uses_own_target() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 32000);

    // 修复后：存在 new_side_rebinds 变量（逐 unit 收集自己的 anchor_rebinds + target_union）。
    assert!(
        window.contains("new_side_rebinds"),
        "修复后应有 new_side_rebinds 变量收集每个 new side unit 自己的 anchor_rebinds。"
    );
    // 修复后：存在 RebindNewSide { 结构（带字段的花括号变体）。
    assert!(
        window.contains("RebindNewSide {"),
        "修复后应有 RebindNewSide {{ ... }} 结构变体。"
    );
    // 修复后：逐 new unit 算 target_union（该 unit 所有 anchor target 的 union）。
    assert!(
        window.contains("target_union"),
        "修复后应逐 new unit 算 target_union（该 unit 所有 anchor target 的 union）。"
    );
}

// =========================================================================
// 修复 1c 守卫: CrossFade old side 多对多原位 fade-out
// =========================================================================

/// 修复后守卫 1c: 存在 RebindOldSideInPlace 变体，且多对多时 old side 原位 fade-out
///（不跟随 group union）。
#[test]
fn fix1c_crossfade_old_side_inplace_for_many_to_many() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 28000);

    // 修复后：存在 RebindOldSideInPlace 变体。
    assert!(
        window.contains("RebindOldSideInPlace"),
        "修复后应有 RebindOldSideInPlace 变体（多对多 old side 原位 fade-out）。"
    );
    // 修复后：多对多判断条件存在（old_indices.len() == 1 && new_indices.len() == 1）。
    assert!(
        window.contains("group.old_indices.len() == 1 && group.new_indices.len() == 1"),
        "修复后应有一对一判断 group.old_indices.len() == 1 && group.new_indices.len() == 1。"
    );
    // 修复后：多对多分支用 RebindOldSideInPlace（else 分支）。
    // 定位一对一判断，确认其后有 else 分支用 RebindOldSideInPlace。
    let one_to_one_pos = window
        .find("group.old_indices.len() == 1 && group.new_indices.len() == 1")
        .expect("一对一判断必须存在");
    let after_branch = &window[one_to_one_pos..];
    assert!(
        after_branch.contains("RebindOldSideInPlace"),
        "修复后多对多 else 分支应用 RebindOldSideInPlace 原位 fade-out。"
    );
}

// =========================================================================
// 修复 2a 守卫: Reflow anchor current_rect 从旧 basis 采样
// =========================================================================

/// 修复后守卫 2a: ReflowMove 循环中用 anchor 旧的 from→to 按 visible 采样算
/// current_rect（存在 current_rect 和 anchor.to_document_rect 在采样表达式中），
/// movement vector 用 target - current。
#[test]
fn fix2a_reflow_anchor_current_rect_from_old_basis() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 28000);

    // 修复后：存在 current_rect 变量。
    assert!(
        window.contains("let current_rect = SourceRect"),
        "修复后应有 let current_rect = SourceRect 变量（anchor 旧 from→to 采样）。"
    );
    // 修复后：current_rect 采样表达式中用 anchor.to_document_rect（旧 basis 的 to）。
    assert!(
        window.contains("anchor.to_document_rect.x - anchor.from_document_rect.x"),
        "修复后 current_rect 采样应用 anchor.to_document_rect.x - anchor.from_document_rect.x（旧 from→to）。"
    );
    // 修复后：存在 anchor_rebinds 变量收集 (current_rect, target_rect)。
    assert!(
        window.contains("anchor_rebinds"),
        "修复后应有 anchor_rebinds 变量收集 (current_rect, target_rect)。"
    );
    // 修复后：movement vector 用 target - current（不是 to - from）。
    assert!(
        window.contains("(target.x - current.x, target.y - current.y)"),
        "修复后 movement vector 应用 (target.x - current.x, target.y - current.y)（target - current）。"
    );
    // 修复后：不再用旧 (to.x - from.x, to.y - from.y) movement vector。
    assert!(
        !window.contains("(to.x - from.x, to.y - from.y)"),
        "修复后不应再用 (to.x - from.x, to.y - from.y) 旧 movement vector。"
    );
}

// =========================================================================
// 修复 2b 守卫: RebindMerged 保留逐 anchor 几何
// =========================================================================

/// 修复后守卫 2b: 存在 RebindMerged 变体，且应用阶段逐 anchor 更新为自己的
/// (current_rect, target_rect)（通过 anchor_rebinds.get(k) 取每个 anchor 自己的
/// rebind），不把总 frame/总 target 写给所有 anchor。
#[test]
fn fix2b_rebind_merged_preserves_per_anchor_geometry() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 28000);

    // 修复后：存在 RebindMerged 变体。
    assert!(
        window.contains("RebindMerged"),
        "修复后应有 RebindMerged 变体（vectors_consistent 时整体重绑）。"
    );
    // 修复后：RebindMerged 携带 anchor_rebinds 字段。
    assert!(
        window.contains("anchor_rebinds: Vec<(SourceRect, SourceRect)>"),
        "修复后 RebindMerged 应携带 anchor_rebinds: Vec<(SourceRect, SourceRect)> 字段。"
    );
    // 修复后：应用阶段逐 anchor 通过 anchor_rebinds.get(k) 取自己的 rebind。
    assert!(
        window.contains("anchor_rebinds.get(k)"),
        "修复后应用阶段应通过 anchor_rebinds.get(k) 逐 anchor 取自己的 (current_rect, target_rect)。"
    );
    // 修复后：逐 anchor 更新为 target_rect.clone()（不是统一 new_to）。
    assert!(
        window.contains("anchor.to_document_rect = target_rect.clone()"),
        "修复后 RebindMerged 应用阶段应逐 anchor 更新 anchor.to_document_rect = target_rect.clone()。"
    );
    // 修复后：逐 anchor 更新 from 为 current_rect.clone()。
    assert!(
        window.contains("anchor.from_document_rect = current_rect.clone()"),
        "修复后 RebindMerged 应用阶段应逐 anchor 更新 anchor.from_document_rect = current_rect.clone()。"
    );
}

// =========================================================================
// 修复 2c 守卫: Split replacement 的 from 直接用 current_rect
// =========================================================================

/// 修复后守卫 2c: build_split_replacement_units 中 replacement 的 from 直接用
/// current_rect（anchor_rebinds 的第一项），不再内部算
/// from_rect.x + (to_rect.x - from_rect.x) * current_visible 插值。
#[test]
fn fix2c_split_replacement_from_is_current_frame() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn build_split_replacement_units", 5000);

    // 修复后：签名参数名为 anchor_rebinds（不是 anchor_targets）。
    assert!(
        window.contains("anchor_rebinds: &[(SourceRect, SourceRect)]"),
        "修复后 build_split_replacement_units 签名参数应为 anchor_rebinds: &[(SourceRect, SourceRect)]。"
    );
    // 修复后：解构为 (current_rect, target_rect)。
    assert!(
        window.contains("let (current_rect, target_rect) = &anchor_rebinds[anchor_idx]"),
        "修复后应解构 let (current_rect, target_rect) = &anchor_rebinds[anchor_idx]。"
    );
    // 修复后：from_document_rect 直接用 current_rect.clone()。
    assert!(
        window.contains("from_document_rect: current_rect.clone()"),
        "修复后 replacement 的 from_document_rect 应直接用 current_rect.clone()（当前屏幕帧）。"
    );
    // 修复后：不再有内部 from_rect.x + (to_rect.x - from_rect.x) * current_visible 插值代码。
    assert!(
        !window.contains("from_rect.x + (to_rect.x - from_rect.x) * current_visible"),
        "修复后不应再有 from_rect.x + (to_rect.x - from_rect.x) * current_visible 内部插值代码。"
    );
    // 修复后：不再有 from_rect / to_rect 变量名（已改为 current_rect / target_rect）。
    assert!(
        !window.contains("let (from_rect, to_rect) = &anchor_targets[anchor_idx]"),
        "修复后不应再有 let (from_rect, to_rect) = &anchor_targets[anchor_idx] 旧解构。"
    );
}
