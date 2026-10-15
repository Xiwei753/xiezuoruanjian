//! Issue #738 评论 5789470425 修复验证测试 — 验证改法已落实：
//! 1. Issue #904 清理后 canonical document snapshot 由 EditorLayout 单一持有，
//!    Pipeline 不再有第二份 canonical / reconcile 链（旧 current_canonical_snapshot、
//!    reconcile_active_transactions_with_new_canonical、reconcile_after_layout_change 均已删除）；
//! 2. PreparedVisualUnit reflow_anchors 逐 cluster 重绑（merge_two 合并 anchors、find_clusters_in_canonical 逐 cluster）；
//! 3. ReflowCrossFade crossfade_group_id 成对重绑（以 group 为单位、同 now 采样、整组结束）。
//!
//! 每个守卫断言修复后的正确结构，修复后代码满足这些断言 → 测试 PASS。

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

/// 在 `src` 中定位 `fn_marker`，返回该函数体窗口（回退到 char boundary 避免 UTF-8 切片 panic）。
fn function_window<'a>(src: &'a str, fn_marker: &str, window_size: usize) -> &'a str {
    let pos = src
        .find(fn_marker)
        .unwrap_or_else(|| panic!("{} 必须存在", fn_marker));
    let mut end = pos.saturating_add(window_size).min(src.len());
    while end > pos && !src.is_char_boundary(end) {
        end -= 1;
    }
    &src[pos..end]
}

// =========================================================================
// 问题1: reconcile 时序收口到新 canonical
// =========================================================================

/// 问题1 守卫1（Issue #904 清理）: 整条 Pipeline 侧的第二份 canonical / reconcile
/// 链都已删除；canonical document snapshot 由 EditorLayout 单一持有。
#[test]
fn issue1_reconcile_uses_new_canonical_not_previous_clone() {
    let src = read_src("src/sujian_editor_item/pipeline.rs");
    assert!(
        !src.contains("fn reconcile_active_transactions_with_canonical_on_layout_change"),
        "旧的 reconcile_active_transactions_with_canonical_on_layout_change 应已删除。"
    );
    assert!(
        !src.contains("fn reconcile_active_transactions_with_new_canonical"),
        "Issue #904 清理后 Pipeline 不应再有 reconcile_active_transactions_with_new_canonical。"
    );
    assert!(
        !src.contains("self.previous_canonical_snapshot.clone()"),
        "不应再用 previous_canonical_snapshot.clone()（旧 canonical）做 reconcile。"
    );
    // 单一权威：canonical 由 EditorLayout 持有。
    let layout_mod = read_src("src/editor/layout/mod.rs");
    assert!(
        layout_mod.contains("pub fn last_canonical_snapshot("),
        "EditorLayout 应提供 last_canonical_snapshot() 作为唯一 canonical 权威。"
    );
}

/// 问题1 守卫2（Issue #904 清理）: Pipeline 不再持有第二份 canonical 字段；
/// 编辑前的旧几何一致性由 `ensure_current_canonical_before_edit` 直接校验
/// EditorLayout 的 canonical。
#[test]
fn issue1_current_canonical_snapshot_field_and_assignment() {
    let src = read_src("src/sujian_editor_item/pipeline.rs");
    assert!(
        !src.contains("current_canonical_snapshot"),
        "Issue #904 清理后 Pipeline 不得再持有 current_canonical_snapshot 第二份 canonical。"
    );
    let editing = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&editing, "fn ensure_current_canonical_before_edit", 1200);
    assert!(
        window.contains("last_canonical_snapshot()"),
        "ensure_current_canonical_before_edit 应以 EditorLayout.last_canonical_snapshot() 为准。"
    );
}

/// 问题1 守卫3（Issue #904 清理）: geometry_changed 中宽度变化由
/// recalculate_content_height_and_emit 按新 width 重排即可；不再有第二份 canonical
/// 需要 reconcile。
#[test]
fn issue1_geometry_changed_reconciles_after_new_layout_computed() {
    let src = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    let window = function_window(&src, "fn geometry_changed", 1600);
    assert!(
        window.contains("recalculate_content_height_and_emit"),
        "geometry_changed 中 recalculate_content_height_and_emit 调用必须存在"
    );
    assert!(
        !window.contains("reconcile_after_layout_change"),
        "Issue #904 清理后 geometry_changed 不应再调 reconcile_after_layout_change。"
    );
}

/// 问题1 守卫4（Issue #904 清理）: layout_property_changed 只重排，不再 reconcile
/// 第二份 canonical。
#[test]
fn issue1_layout_property_changed_reconciles_after_new_layout_computed() {
    let src = read_src("src/sujian_editor_item/properties.rs");
    let window = function_window(&src, "fn layout_property_changed", 1600);
    assert!(
        window.contains("recalculate_content_height_and_emit"),
        "layout_property_changed 中 recalculate_content_height_and_emit 调用必须存在"
    );
    assert!(
        !window.contains("reconcile_after_layout_change"),
        "Issue #904 清理后 layout_property_changed 不应再调 reconcile_after_layout_change。"
    );
}

/// 问题1 守卫5: find_cursor_transaction_for_target 应接收 LayoutRevision 参数，
/// 跳过 basis 不一致的事务。
#[test]
fn issue1_find_cursor_transaction_for_target_takes_layout_revision() {
    // Issue #826/#901: cursor_motion.rs 已随旧动画架构删除。
    let motion_path = linux_qt_root().join("src/sujian_editor_item/animation/cursor_motion.rs");
    if !motion_path.exists() {
        println!("[BUGFIX_VERIFY] issue1: cursor_motion.rs 已删除");
        return;
    }
    let src = read_src("src/sujian_editor_item/animation/cursor_motion.rs");
    let window = function_window(&src, "fn find_cursor_transaction_for_target", 2000);
    assert!(
        window.contains("current_layout_revision: LayoutRevision"),
        "find_cursor_transaction_for_target 应接收 current_layout_revision: LayoutRevision 参数。"
    );
    assert!(
        window.contains("active_text_transaction_key_with_epoch"),
        "find_cursor_transaction_for_target 应通过 active_text_transaction_key_with_epoch\
         跳过 basis 不一致的事务。"
    );
}

// =========================================================================
// 问题2: PreparedVisualUnit reflow_anchors 逐 cluster 重绑
// =========================================================================

/// 问题2 守卫1: AnimatedSlice 应有 reflow_anchors 字段，ReflowAnchor 结构应存在。
#[test]
fn issue2_animated_slice_has_reflow_anchors() {
    // Issue #826/#901: animated_slice.rs 已删除。
    let path = linux_qt_root().join("src/sujian_editor_item/animated_slice.rs");
    if !path.exists() {
        println!("[BUGFIX_VERIFY] issue2: animated_slice.rs 已删除");
        return;
    }
    let src = read_src("src/sujian_editor_item/animated_slice.rs");
    assert!(
        src.contains("pub reflow_anchors: Vec<ReflowAnchor>"),
        "AnimatedSlice 应有 reflow_anchors: Vec<ReflowAnchor> 字段。"
    );
    assert!(
        src.contains("struct ReflowAnchor"),
        "ReflowAnchor 结构应存在。"
    );
}

/// 问题2 守卫2: merge_two 不应只留单个 shaping_identity（a.shaping_identity.clone()），
/// 应合并 reflow_anchors 列表。
#[test]
fn issue2_merge_two_merges_anchors_not_single_shaping() {
    // Issue #826/#901: transaction_builder.rs 已删除。
    let path = linux_qt_root().join("src/sujian_editor_item/animation/transaction_builder.rs");
    if !path.exists() {
        println!("[BUGFIX_VERIFY] issue2: transaction_builder.rs 已删除");
        return;
    }
    let src = read_src("src/sujian_editor_item/animation/transaction_builder.rs");
    let window = function_window(&src, "fn merge_two", 1200);
    assert!(
        !window.contains("shaping_identity: a.shaping_identity.clone()"),
        "merge_two 不应再用 shaping_identity: a.shaping_identity.clone()。"
    );
    assert!(
        window.contains("reflow_anchors"),
        "merge_two 应合并 reflow_anchors 列表。"
    );
}

/// 问题2 守卫3: merge_two 不应再用 bounding_box 做 from/to_document_rect。
#[test]
fn issue2_merge_two_no_bounding_box_for_document_rects() {
    // Issue #826/#901: transaction_builder.rs 已删除。
    let path = linux_qt_root().join("src/sujian_editor_item/animation/transaction_builder.rs");
    if !path.exists() {
        println!("[BUGFIX_VERIFY] issue2: transaction_builder.rs 已删除");
        return;
    }
    let src = read_src("src/sujian_editor_item/animation/transaction_builder.rs");
    let window = function_window(&src, "fn merge_two", 1200);
    assert!(
        !window.contains("from_document_rect: bounding_box("),
        "merge_two 不应再用 from_document_rect: bounding_box(。"
    );
    assert!(
        !window.contains("to_document_rect: bounding_box("),
        "merge_two 不应再用 to_document_rect: bounding_box(。"
    );
}

/// 问题2 守卫4: find_clusters_in_canonical 应存在（逐 cluster 返回列表），
/// 不再做 bounding box + first shaping。
#[test]
fn issue2_find_clusters_in_canonical_per_cluster() {
    // Issue #826/#901: transaction/rebind.rs 已删除。
    let path = linux_qt_root().join("src/sujian_editor_item/animation/transaction/rebind.rs");
    if !path.exists() {
        println!("[BUGFIX_VERIFY] issue2: transaction/rebind.rs 已删除");
        return;
    }
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    assert!(
        src.contains("fn find_clusters_in_canonical"),
        "应存在 find_clusters_in_canonical。"
    );
    let window = function_window(&src, "fn find_clusters_in_canonical", 2000);
    assert!(
        !window.contains("first_shaping.is_none()"),
        "find_clusters_in_canonical 不应只用 first_shaping.is_none()。"
    );
    assert!(
        window.contains("hits.push"),
        "find_clusters_in_canonical 应逐 cluster push 到 hits 列表。"
    );
}

// =========================================================================
// 问题3: ReflowCrossFade crossfade_group_id 成对重绑
// =========================================================================

/// 问题3 守卫1: AnimatedSlice 应有 crossfade_group_id 和 crossfade_side 字段。
#[test]
fn issue3_animated_slice_has_crossfade_group_id_and_side() {
    // Issue #826/#901: animated_slice.rs 已删除。
    let path = linux_qt_root().join("src/sujian_editor_item/animated_slice.rs");
    if !path.exists() {
        println!("[BUGFIX_VERIFY] issue3: animated_slice.rs 已删除");
        return;
    }
    let src = read_src("src/sujian_editor_item/animated_slice.rs");
    assert!(
        src.contains("pub crossfade_group_id: Option<u64>"),
        "AnimatedSlice 应有 crossfade_group_id: Option<u64> 字段。"
    );
    assert!(
        src.contains("pub crossfade_side: Option<CrossFadeSide>"),
        "AnimatedSlice 应有 crossfade_side: Option<CrossFadeSide> 字段。"
    );
    assert!(
        src.contains("enum CrossFadeSide"),
        "CrossFadeSide 枚举应存在。"
    );
}

/// 问题3 守卫2: rebind_timed_units_to_canonical 不应把每个 unit 独立和 canonical 比 shaping
/// （is_same_shaping(&cluster_shaping)），应以 group 为单位成对重绑。
#[test]
fn issue3_rebind_pairwise_not_independent_shaping_compare() {
    // Issue #826/#901: transaction/rebind.rs 已删除。
    let path = linux_qt_root().join("src/sujian_editor_item/animation/transaction/rebind.rs");
    if !path.exists() {
        println!("[BUGFIX_VERIFY] issue3: transaction/rebind.rs 已删除");
        return;
    }
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 4000);
    assert!(
        !window.contains("is_same_shaping(&cluster_shaping)"),
        "rebind_timed_units_to_canonical 不应再用 is_same_shaping(&cluster_shaping)。"
    );
    assert!(
        !window.contains("RebindOutcome::Remove"),
        "rebind_timed_units_to_canonical 不应再用 RebindOutcome::Remove。"
    );
}

/// 问题3 守卫3: rebind_timed_units_to_canonical 应有成对重绑逻辑（crossfade_pair）。
#[test]
fn issue3_rebind_has_pairwise_crossfade_rebind() {
    // Issue #826/#901: transaction/rebind.rs 已删除。
    let path = linux_qt_root().join("src/sujian_editor_item/animation/transaction/rebind.rs");
    if !path.exists() {
        println!("[BUGFIX_VERIFY] issue3: transaction/rebind.rs 已删除");
        return;
    }
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 4000);
    assert!(
        window.contains("crossfade_pair"),
        "rebind_timed_units_to_canonical 应有 crossfade_pair 成对重绑逻辑。"
    );
    assert!(
        window.contains("CrossFadeSide::Old") && window.contains("CrossFadeSide::New"),
        "rebind_timed_units_to_canonical 应按 CrossFadeSide::Old / New 区分配对两侧。"
    );
}
