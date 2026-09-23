//! Issue #738 评论 5789470425 修复验证测试 — 验证 3 项改法已落实：
//! 1. reconcile 时序收口到新 canonical（current_canonical_snapshot + 新入口 + 推迟 reconcile）；
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

/// 问题1 守卫1: 旧的 reconcile_active_transactions_with_canonical_on_layout_change
/// （先 bump 再拿 previous_canonical_snapshot.clone() reconcile）应已删除，
/// 替换为 reconcile_active_transactions_with_new_canonical(new_snapshot) 接收已完成的新 canonical。
#[test]
fn issue1_reconcile_uses_new_canonical_not_previous_clone() {
    let src = read_src("src/sujian_editor_item/pipeline.rs");
    // 旧入口应已删除。
    assert!(
        !src.contains("fn reconcile_active_transactions_with_canonical_on_layout_change"),
        "旧的 reconcile_active_transactions_with_canonical_on_layout_change 应已删除，\
         替换为接收新 canonical 的入口。"
    );
    // 新入口应存在且接收 new_snapshot 参数。
    assert!(
        src.contains("fn reconcile_active_transactions_with_new_canonical"),
        "应存在 reconcile_active_transactions_with_new_canonical 入口，接收已完成的新 canonical。"
    );
    // 不应再用 previous_canonical_snapshot.clone() 做 reconcile。
    assert!(
        !src.contains("self.previous_canonical_snapshot.clone()"),
        "不应再用 previous_canonical_snapshot.clone()（旧 canonical）做 reconcile。"
    );
}

/// 问题1 守卫2: 字段应重命名为 current_canonical_snapshot（语义清晰），
/// 且新入口把新 canonical 存为 current_canonical_snapshot。
#[test]
fn issue1_current_canonical_snapshot_field_and_assignment() {
    let src = read_src("src/sujian_editor_item/pipeline.rs");
    assert!(
        src.contains("current_canonical_snapshot:"),
        "字段应重命名为 current_canonical_snapshot（语义清晰，不再用 previous_canonical_snapshot）。"
    );
    let window = function_window(
        &src,
        "fn reconcile_active_transactions_with_new_canonical",
        1200,
    );
    assert!(
        window.contains("self.current_canonical_snapshot = Some(new_snapshot)"),
        "reconcile_active_transactions_with_new_canonical 应把新 canonical 存为\
         current_canonical_snapshot。"
    );
}

/// 问题1 守卫3: geometry_changed 中 reconcile 应在 recalculate_content_height_and_emit
/// 之后（新排版完成后再 reconcile），而非之前。
#[test]
fn issue1_geometry_changed_reconciles_after_new_layout_computed() {
    let src = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    let window = function_window(&src, "fn geometry_changed", 1600);
    let reconcile_marker = "reconcile_after_layout_change";
    let recalc_marker = "recalculate_content_height_and_emit";
    let recalc_pos = window
        .find(recalc_marker)
        .expect("geometry_changed 中 recalculate_content_height_and_emit 调用必须存在");
    let reconcile_pos = window
        .find(reconcile_marker)
        .expect("geometry_changed 中 reconcile_after_layout_change 调用必须存在");
    assert!(
        reconcile_pos > recalc_pos,
        "geometry_changed 中 reconcile_after_layout_change（位置 {reconcile_pos}）应在\
         recalculate_content_height_and_emit（位置 {recalc_pos}）之后，即新排版完成后再 reconcile。"
    );
}

/// 问题1 守卫4: layout_property_changed 中 reconcile 应在 recalculate 之后。
#[test]
fn issue1_layout_property_changed_reconciles_after_new_layout_computed() {
    let src = read_src("src/sujian_editor_item/properties.rs");
    let window = function_window(&src, "fn layout_property_changed", 1600);
    let reconcile_marker = "reconcile_after_layout_change";
    let recalc_marker = "recalculate_content_height_and_emit";
    let recalc_pos = window
        .find(recalc_marker)
        .expect("layout_property_changed 中 recalculate_content_height_and_emit 调用必须存在");
    let reconcile_pos = window
        .find(reconcile_marker)
        .expect("layout_property_changed 中 reconcile_after_layout_change 调用必须存在");
    assert!(
        reconcile_pos > recalc_pos,
        "layout_property_changed 中 reconcile_after_layout_change（位置 {reconcile_pos}）应在\
         recalculate_content_height_and_emit（位置 {recalc_pos}）之后。"
    );
}

/// 问题1 守卫5: find_cursor_transaction_for_target 应接收 LayoutRevision 参数，
/// 跳过 basis 不一致的事务。
#[test]
fn issue1_find_cursor_transaction_for_target_takes_layout_revision() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
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
    let src = read_src("src/sujian_editor_item/animated_slice.rs");
    assert!(
        src.contains("pub reflow_anchors: Vec<ReflowAnchor>"),
        "AnimatedSlice 应有 reflow_anchors: Vec<ReflowAnchor> 字段。"
    );
    assert!(
        src.contains("struct ReflowAnchor"),
        "ReflowAnchor 结构应存在，保存原始 cluster 的 byte range、shaping identity、\
         from/to document rect 等身份。"
    );
}

/// 问题2 守卫2: merge_two 不应只留单个 shaping_identity（a.shaping_identity.clone()），
/// 应合并 reflow_anchors 列表。
#[test]
fn issue2_merge_two_merges_anchors_not_single_shaping() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let window = function_window(&src, "fn merge_two", 1200);
    assert!(
        !window.contains("shaping_identity: a.shaping_identity.clone()"),
        "merge_two 不应再用 shaping_identity: a.shaping_identity.clone()（只保留单个 shaping）。"
    );
    assert!(
        window.contains("reflow_anchors"),
        "merge_two 应合并 reflow_anchors 列表，保留被合并各 cluster 的细分身份。"
    );
}

/// 问题2 守卫3: merge_two 不应再用 bounding_box 做 from/to_document_rect。
#[test]
fn issue2_merge_two_no_bounding_box_for_document_rects() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let window = function_window(&src, "fn merge_two", 1200);
    assert!(
        !window.contains("from_document_rect: bounding_box("),
        "merge_two 不应再用 from_document_rect: bounding_box( 框成大矩形。"
    );
    assert!(
        !window.contains("to_document_rect: bounding_box("),
        "merge_two 不应再用 to_document_rect: bounding_box( 框成大矩形。"
    );
}

/// 问题2 守卫4: find_clusters_in_canonical 应存在（逐 cluster 返回列表），
/// 不再做 bounding box + first shaping。
#[test]
fn issue2_find_clusters_in_canonical_per_cluster() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    assert!(
        src.contains("fn find_clusters_in_canonical"),
        "应存在 find_clusters_in_canonical（逐 cluster 返回列表），替代旧的\
         find_cluster_in_canonical（bounding box + first shaping）。"
    );
    let window = function_window(&src, "fn find_clusters_in_canonical", 2000);
    assert!(
        !window.contains("first_shaping.is_none()"),
        "find_clusters_in_canonical 不应只用 first_shaping.is_none() 取第一个 cluster 的 shaping。"
    );
    assert!(
        window.contains("hits.push"),
        "find_clusters_in_canonical 应逐 cluster push 到 hits 列表，而非做 bounding box。"
    );
}

// =========================================================================
// 问题3: ReflowCrossFade crossfade_group_id 成对重绑
// =========================================================================

/// 问题3 守卫1: AnimatedSlice 应有 crossfade_group_id 和 crossfade_side 字段。
#[test]
fn issue3_animated_slice_has_crossfade_group_id_and_side() {
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
        "CrossFadeSide 枚举应存在（Old/New 侧标记）。"
    );
}

/// 问题3 守卫2: rebind_timed_units_to_canonical 不应把每个 unit 独立和 canonical 比 shaping
/// （is_same_shaping(&cluster_shaping)），应以 group 为单位成对重绑。
#[test]
fn issue3_rebind_pairwise_not_independent_shaping_compare() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 4000);
    assert!(
        !window.contains("is_same_shaping(&cluster_shaping)"),
        "rebind_timed_units_to_canonical 不应再用 is_same_shaping(&cluster_shaping)\
         把每个 unit 独立和 canonical 比 shaping。"
    );
    assert!(
        !window.contains("RebindOutcome::Remove"),
        "rebind_timed_units_to_canonical 不应再用 RebindOutcome::Remove 独立判死 unit。"
    );
}

/// 问题3 守卫3: rebind_timed_units_to_canonical 应有成对重绑逻辑（crossfade_pair）。
#[test]
fn issue3_rebind_has_pairwise_crossfade_rebind() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 4000);
    assert!(
        window.contains("crossfade_pair"),
        "rebind_timed_units_to_canonical 应有 crossfade_pair 成对重绑逻辑：\
         查找配对 old/new side、按同一剩余时间同步、new-side anchor 失效时整组结束。"
    );
    assert!(
        window.contains("CrossFadeSide::Old") && window.contains("CrossFadeSide::New"),
        "rebind_timed_units_to_canonical 应按 CrossFadeSide::Old / New 区分配对两侧。"
    );
}
