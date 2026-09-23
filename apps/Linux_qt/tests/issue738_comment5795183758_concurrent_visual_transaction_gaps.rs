//! Issue #738 评论 5795183758 修复后守卫测试 — 验证 3 个并发视觉事务残留缺陷
//! 已被正确修复。
//!
//! 本测试是源代码静态守卫测试（WHITE_BOX），通过读取源文件并检查代码模式来确认
//! 评论 5795183758 指出的 3 个缺陷的修复后正确结构在当前代码中确实存在。
//! 测试 PASS = 修复后结构正确。
//!
//! 3 个问题的修复后正确结构：
//! 1. build_split_replacement_units 中 replacement 的 reflow_anchors[0] 与 slice
//!    同一 layout basis：先 clone anchor 再显式覆盖 from/to 为 current_rect/target_rect，
//!    不再用旧 basis 的 anchor.clone()。
//! 2. CrossFade 重绑（apply_old_side_rebind + RebindNewSide）采样当前 frame 后写
//!    unit.slice.opacity_from = frame.opacity，不再从旧 opacity_from 重新开始。
//! 3. RebindNewSide 携带逐 anchor 的 (current_rect, target_rect) 列表
//!    （anchor_rebinds: Vec<(SourceRect, SourceRect)>），应用时逐 anchor 设置各自
//!    from/to，不再把 merged unit 的总 frame/总 target 写给所有 anchors。
//!    movement vector 不一致时用 build_crossfade_split_replacement_units 拆成多个
//!    CrossFade units，保留 crossfade_group_id/crossfade_side。

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
// 修复 1 守卫: build_split_replacement_units 的 reflow_anchors[0] 与 slice 同 basis
// =========================================================================

/// 修复后守卫 1: build_split_replacement_units 中 replacement 的 reflow_anchors[0]
/// 的 from_document_rect/to_document_rect 被显式覆盖成 current_rect/target_rect，
/// 不再用旧 basis 的 anchor.clone()。replacement slice 和它内部唯一 anchor 从创建
/// 那一刻就是同一 layout basis。
#[test]
fn fix1_split_replacement_anchor_same_basis_as_slice() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn build_split_replacement_units", 5000);

    // 修复后：先 clone anchor 再显式覆盖 from/to。
    assert!(
        window.contains("let mut new_anchor = anchor.clone();"),
        "修复后应先 clone anchor：let mut new_anchor = anchor.clone();"
    );
    // 修复后：new_anchor.from_document_rect 显式覆盖成 current_rect。
    assert!(
        window.contains("new_anchor.from_document_rect = current_rect.clone();"),
        "修复后应显式覆盖 new_anchor.from_document_rect = current_rect.clone();"
    );
    // 修复后：new_anchor.to_document_rect 显式覆盖成 target_rect。
    assert!(
        window.contains("new_anchor.to_document_rect = target_rect.clone();"),
        "修复后应显式覆盖 new_anchor.to_document_rect = target_rect.clone();"
    );
    // 修复后：reflow_anchors 用 new_anchor（与 slice 同 basis）。
    assert!(
        window.contains("reflow_anchors: vec![new_anchor]"),
        "修复后 reflow_anchors 应为 vec![new_anchor]（与 slice 同 basis）。"
    );
    // 修复后：不再用旧 basis 的 anchor.clone() 作为 reflow_anchors。
    assert!(
        !window.contains("reflow_anchors: vec![anchor.clone()]"),
        "修复后不应再用 reflow_anchors: vec![anchor.clone()] 旧 basis anchor。"
    );
}

// =========================================================================
// 修复 2 守卫: CrossFade 重绑接住当前透明度（opacity_from = frame.opacity）
// =========================================================================

/// 修复后守卫 2a: apply_old_side_rebind 采样当前 frame 后写
/// unit.slice.opacity_from = frame.opacity，不再从旧 opacity_from 重新开始。
#[test]
fn fix2a_apply_old_side_rebind_preserves_current_opacity() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn apply_old_side_rebind", 2500);

    // 修复后：采样当前 frame。
    assert!(
        window.contains("let frame = unit.slice.compute_frame(visible);"),
        "修复后应采样当前 frame：let frame = unit.slice.compute_frame(visible);"
    );
    // 修复后：写 opacity_from = frame.opacity。
    assert!(
        window.contains("unit.slice.opacity_from = frame.opacity;"),
        "修复后应写 unit.slice.opacity_from = frame.opacity（接住当前帧透明度）。"
    );
}

/// 修复后守卫 2b: RebindNewSide 应用分支采样当前 frame 后写
/// unit.slice.opacity_from = frame.opacity，不再从旧 opacity_from 重新开始。
#[test]
fn fix2b_rebind_new_side_preserves_current_opacity() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 32000);

    // 修复后：RebindNewSide 应用分支写 opacity_from = frame.opacity。
    // 定位 RebindNewSide match 分支，确认其后有 opacity_from = frame.opacity。
    let rebind_new_side_pos = window
        .find("RebindDecision::RebindNewSide { anchor_rebinds } =>")
        .expect("RebindNewSide { anchor_rebinds } => 分支必须存在");
    let after_branch = &window[rebind_new_side_pos..];
    assert!(
        after_branch.contains("unit.slice.opacity_from = frame.opacity;"),
        "修复后 RebindNewSide 应用分支应写 unit.slice.opacity_from = frame.opacity。"
    );
}

// =========================================================================
// 修复 3 守卫: RebindNewSide 逐 anchor 保留各自几何，不再抹平成总 frame/总 target
// =========================================================================

/// 修复后守卫 3a: RebindNewSide 变体携带 anchor_rebinds: Vec<(SourceRect, SourceRect)>，
/// 不再只带一个 target（union）。
#[test]
fn fix3a_rebind_new_side_carries_per_anchor_rebinds() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 32000);

    // 修复后：RebindNewSide 携带 anchor_rebinds 字段。
    assert!(
        window.contains("anchor_rebinds: Vec<(SourceRect, SourceRect)>"),
        "修复后 RebindNewSide 应携带 anchor_rebinds: Vec<(SourceRect, SourceRect)> 字段。"
    );
    // 修复后：match pattern 用 anchor_rebinds。
    assert!(
        window.contains("RebindDecision::RebindNewSide { anchor_rebinds } =>"),
        "修复后 match pattern 应为 RebindDecision::RebindNewSide {{ anchor_rebinds }} =>。"
    );
    // 修复后：不再有旧 match pattern RebindNewSide { target }。
    assert!(
        !window.contains("RebindDecision::RebindNewSide { target } =>"),
        "修复后不应再有 RebindDecision::RebindNewSide {{ target }} => 旧 match pattern。"
    );
}

/// 修复后守卫 3b: RebindNewSide 应用分支逐 anchor 通过 anchor_rebinds.get(k) 取自己的
/// (current_rect, target_rect)，不再把 merged unit 的总 frame/总 target 写给所有 anchors。
#[test]
fn fix3b_rebind_new_side_per_anchor_geometry_not_flattened() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 32000);

    // 修复后：RebindNewSide 应用分支逐 anchor 用 anchor_rebinds.get(k)。
    let rebind_new_side_pos = window
        .find("RebindDecision::RebindNewSide { anchor_rebinds } =>")
        .expect("RebindNewSide { anchor_rebinds } => 分支必须存在");
    let after_branch = &window[rebind_new_side_pos..];
    assert!(
        after_branch.contains("anchor_rebinds.get(k)"),
        "修复后 RebindNewSide 应用分支应逐 anchor 用 anchor_rebinds.get(k) 取自己的 rebind。"
    );
    // 修复后：逐 anchor 更新 from = current_rect.clone()。
    assert!(
        after_branch.contains("anchor.from_document_rect = current_rect.clone();"),
        "修复后 RebindNewSide 应用分支应逐 anchor 更新 anchor.from_document_rect = current_rect.clone()。"
    );
    // 修复后：逐 anchor 更新 to = target_rect.clone()。
    assert!(
        after_branch.contains("anchor.to_document_rect = target_rect.clone();"),
        "修复后 RebindNewSide 应用分支应逐 anchor 更新 anchor.to_document_rect = target_rect.clone()。"
    );
    // 修复后：不再把所有 anchor.to 写成 new_to.clone()（旧抹平代码）。
    assert!(
        !after_branch.contains("anchor.to_document_rect = new_to.clone();"),
        "修复后 RebindNewSide 不应再把所有 anchor.to_document_rect 写成 new_to.clone()。"
    );
}

/// 修复后守卫 3c: 存在 build_crossfade_split_replacement_units 辅助函数，
/// 用于 CrossFade new side merged unit 各 anchor movement vector 不一致时拆分。
#[test]
fn fix3c_crossfade_split_replacement_units_exists() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");

    // 修复后：存在 build_crossfade_split_replacement_units 函数。
    assert!(
        src.contains("fn build_crossfade_split_replacement_units"),
        "修复后应存在 build_crossfade_split_replacement_units 函数（CrossFade 拆分）。"
    );
    let window = function_window(&src, "fn build_crossfade_split_replacement_units", 5000);

    // 修复后：拆分后 kind 为 ReflowCrossFade（保留 CrossFade 语义）。
    assert!(
        window.contains("kind: AnimatedSliceKind::ReflowCrossFade"),
        "修复后拆分 replacement 的 kind 应为 ReflowCrossFade。"
    );
    // 修复后：保留原 unit 的 crossfade_group_id。
    assert!(
        window.contains("crossfade_group_id: unit.slice.crossfade_group_id"),
        "修复后拆分 replacement 应保留 unit.slice.crossfade_group_id。"
    );
    // 修复后：保留原 unit 的 crossfade_side。
    assert!(
        window.contains("crossfade_side: unit.slice.crossfade_side"),
        "修复后拆分 replacement 应保留 unit.slice.crossfade_side。"
    );
    // 修复后：replacement 的 reflow_anchors 与 slice 同 basis（问题1对称修复）。
    assert!(
        window.contains("let mut new_anchor = anchor.clone();"),
        "修复后拆分 replacement 应先 clone anchor 再覆盖 from/to。"
    );
    assert!(
        window.contains("reflow_anchors: vec![new_anchor]"),
        "修复后拆分 replacement 的 reflow_anchors 应为 vec![new_anchor]（与 slice 同 basis）。"
    );
}

/// 修复后守卫 3d: 决策阶段为 new side 逐 anchor 算 current_rect（旧 from→to 按 visible
/// 采样），不再只算 unit_target（union）。
#[test]
fn fix3d_new_side_decision_per_anchor_current_rect() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 32000);

    // 修复后：存在 new_side_rebinds 变量（逐 unit 收集 anchor_rebinds + target_union）。
    assert!(
        window.contains("new_side_rebinds"),
        "修复后应有 new_side_rebinds 变量收集每个 new side unit 的 anchor_rebinds。"
    );
    // 修复后：逐 anchor 算 current_rect（anchor 旧 from→to 采样）。
    assert!(
        window.contains("anchor.from_document_rect.x\n                                + (anchor.to_document_rect.x - anchor.from_document_rect.x)"),
        "修复后 new side 决策应逐 anchor 算 current_rect（旧 from→to 按 visible 采样）。"
    );
    // 修复后：逐 anchor 算 target_rect（target_hit.doc_rect）。
    assert!(
        window.contains("let target_rect = target_hit.doc_rect.clone();"),
        "修复后 new side 决策应逐 anchor 算 target_rect = target_hit.doc_rect.clone()。"
    );
    // 修复后：存在 movement vector 一致性判断（vectors_consistent）用于 new side。
    assert!(
        window.contains("vectors_consistent"),
        "修复后 new side 决策应有 movement vector 一致性判断 vectors_consistent。"
    );
}
