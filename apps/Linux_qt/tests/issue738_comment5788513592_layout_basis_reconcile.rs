//! Issue #738 评论 5788513592 复现测试 — 前一轮（commit 61b25083c）已完成基础工作后
//! 仍残留的 3 个实质问题：多链路一起工作时仍会分叉。本测试为 WHITE_BOX 结构守卫
//! 复现：验证当前实现仍违反评论 5788513592 指出的 3 个核心语义。
//!
//! 每个子测试断言评论期望的正确结构，当前（未修复）代码违反这些断言
//! → 测试 FAIL → 缺陷复现成功。
//!
//! ## 评论 5788513592 指出的 3 个问题
//!
//! 1. resize/字号/字体/行距等纯布局变化没有真正 reconcile：
//!    geometry_changed() 和 layout_property_changed() 仅 bump_layout_revision()，
//!    旧事务仍留在 active queue；build_render_plan_full 先 sample_coordinated_motion_frame
//!    后才在 build_text_animation_plan_with_sample 检查 layout_basis_revision；
//!    caret owner 选择（active_text_transaction_key_with_epoch）不看 layout basis。
//!    结果：旧事务仍可能继续拥有 coordinated caret，用旧 caret track 驱动光标。
//!
//! 2. merged Reflow unit 现在无法重绑，会被误删：
//!    merge_adjacent_slices 合并后 unit byte range 跨多 cluster，
//!    find_cluster_in_canonical 要求单 canonical cluster 完整包含 unit 的 mapped byte range，
//!    合并后跨多 cluster 永远找不到 → RebindOutcome::Remove。
//!
//! 3. ReflowCrossFade 当前必然在 reconcile 时被删除：
//!    reflow_crossfade_old/new 创建的 slice 都是 shaping_identity: None，
//!    rebind_timed_units_to_canonical 对 ReflowMove|ReflowCrossFade 执行
//!    Some(sid)=>is_same_shaping(...), None=>false，
//!    只要发生一次新 canonical reconcile，仍在播放的 ReflowCrossFade 一定进入 Remove。

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

/// 在 `src` 中定位 `fn_marker`（如 "fn geometry_changed"），返回该函数体窗口。
/// 窗口从 marker 起始位置开始，向后取 `window_size` 个字符。
/// Issue #738 评论 5788513592: 按 window_size 切字节可能落在多字节 UTF-8 字符中间，
/// 导致 `&src[pos..end]` panic（not a char boundary）。回退到最近的 char boundary，
/// 窗口可能少几个字节，但不改变守卫语义。
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
// 问题1: resize/字号/字体/行距等纯布局变化没有真正 reconcile
// =========================================================================

/// 问题1 守卫1: qquickitem_impl.rs::geometry_changed 应在纯布局变化时真正 reconcile
/// 旧事务。Issue #738 评论 5789470425 后 reconcile 入口收口为
/// reconcile_after_layout_change（新排版完成后用新 canonical reconcile）。
#[test]
fn issue1_geometry_changed_must_reconcile_not_just_bump() {
    let src = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    let window = function_window(&src, "fn geometry_changed", 1600);
    let has_reconcile = window.contains("reconcile_after_layout_change");
    assert!(
        has_reconcile,
        "qquickitem_impl.rs::geometry_changed 应调用 reconcile_after_layout_change，\
         在新排版完成后用新 canonical reconcile 旧事务。"
    );
}

/// 问题1 守卫2: properties.rs::layout_property_changed 应真正 reconcile。
#[test]
fn issue1_layout_property_changed_must_reconcile_not_just_bump() {
    let src = read_src("src/sujian_editor_item/properties.rs");
    let window = function_window(&src, "fn layout_property_changed", 1600);
    let has_reconcile = window.contains("reconcile_after_layout_change");
    assert!(
        has_reconcile,
        "properties.rs::layout_property_changed 应调用 reconcile_after_layout_change，\
         在新排版完成后用新 canonical reconcile 旧事务。"
    );
}

/// 问题1 守卫3: build_render_plan_full 先 sample_coordinated_motion_frame，
/// 之后才在 build_text_animation_plan_with_sample 检查 layout_basis_revision。
/// 修复后应先按 layout basis 过滤/收口旧事务，再采样 caret motion，
/// 或让 caret owner 选择看 layout basis。
#[test]
fn issue1_build_render_plan_full_samples_caret_before_basis_check() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let plan_marker = "fn build_render_plan_full";
    let plan_pos = src
        .find(plan_marker)
        .expect("build_render_plan_full 必须存在");
    // 取 build_render_plan_full 函数体前 1500 字符窗口
    let plan_window = &src[plan_pos..plan_pos.saturating_add(1500).min(src.len())];

    let sample_pos = plan_window
        .find("sample_coordinated_motion_frame")
        .expect("sample_coordinated_motion_frame 调用必须存在");
    let build_anim_pos = plan_window
        .find("build_text_animation_plan_with_sample")
        .expect("build_text_animation_plan_with_sample 调用必须存在");

    // 当前缺陷：sample_coordinated_motion_frame 在 build_text_animation_plan_with_sample 之前
    // （即先采样 caret motion，后才检查 layout_basis_revision）
    let samples_caret_before_basis_check = sample_pos < build_anim_pos;
    assert!(
        !samples_caret_before_basis_check,
        "build_render_plan_full 先调用 sample_coordinated_motion_frame（位置 {}），\
         之后才调用 build_text_animation_plan_with_sample（位置 {}）检查 \
         layout_basis_revision。caret motion 在 basis 检查之前采样，旧事务的 caret \
         track 已被采样喂给 cursor layer，basis 守卫只挡 glyph/clip 不挡 caret。",
        sample_pos, build_anim_pos
    );
}

/// 问题1 守卫4: active_text_transaction_key_with_epoch（caret owner 选择）不看
/// layout_basis_revision。修复后应跳过 layout basis 过期的事务，不让它拥有
/// coordinated caret。
#[test]
fn issue1_caret_owner_selection_ignores_layout_basis() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let window = function_window(&src, "fn active_text_transaction_key_with_epoch", 800);
    // 当前缺陷：函数体里只检查 cursor_owner_epoch 和 caret_motion_retired，
    // 不检查 tx.layout_basis_revision
    let checks_layout_basis = window.contains("layout_basis_revision");
    assert!(
        checks_layout_basis,
        "active_text_transaction_key_with_epoch 选择 caret owner 时只看 \
         cursor_owner_epoch 和 caret_motion_retired，不看 tx.layout_basis_revision。\
         旧事务即使 layout basis 过期仍可能继续拥有 coordinated caret，用旧 caret track \
         驱动光标，与 canonical 新布局分叉。"
    );
}

// =========================================================================
// 问题2: merged Reflow unit 现在无法重绑，会被误删
// =========================================================================

/// 问题2 守卫1: merge_two 合并相邻 slice 时 byte_start = a.byte_start.min(b.byte_start)，
/// byte_end = a.byte_end.max(b.byte_end)，合并后 unit byte range 跨多个原始 cluster。
/// 修复后应保留 per-cluster 重绑所需的细分信息（如记录被合并的子 cluster byte ranges），
/// 或让 find_cluster_in_canonical 支持跨多 cluster 的 range 匹配。
#[test]
fn issue2_merge_two_spans_multiple_clusters_byte_range() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let window = function_window(&src, "fn merge_two", 800);
    // 当前缺陷：合并后 byte range 取 min/max，覆盖多个原始 cluster
    let merges_byte_range_across_clusters = window
        .contains("byte_start: a.byte_start.min(b.byte_start)")
        && window.contains("byte_end: a.byte_end.max(b.byte_end)");
    assert!(
        !merges_byte_range_across_clusters,
        "merge_two 把相邻 slice 合并为 byte_start = a.byte_start.min(b.byte_start)、\
         byte_end = a.byte_end.max(b.byte_end)，合并后 unit byte range 跨多个原始 \
         cluster。find_cluster_in_canonical 要求单 canonical cluster 完整包含 unit 的 \
         mapped byte range，合并后跨多 cluster 永远找不到 → RebindOutcome::Remove，\
         动画 Snap 回 canonical。"
    );
}

/// 问题2 守卫2: Issue #738 评论 5789470425 后 find_cluster_in_canonical 已替换为
/// find_clusters_in_canonical（逐 cluster 返回列表，不再要求单 cluster 完整包含）。
/// 此守卫确认新函数存在且不要求单 cluster 包含判定。
#[test]
fn issue2_find_cluster_in_canonical_requires_single_cluster_containment() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    assert!(
        src.contains("fn find_clusters_in_canonical"),
        "应存在 find_clusters_in_canonical（逐 cluster 返回列表），替代旧的\
         find_cluster_in_canonical（要求单 cluster 完整包含）。"
    );
    let window = function_window(&src, "fn find_clusters_in_canonical", 2000);
    // 修复后：不再要求单 cluster 完整包含，改为相交判定 + 逐 cluster push。
    let requires_single_cluster_containment = window
        .contains("cluster.document_byte_start <= byte_start")
        && window.contains("cluster.document_byte_end >= byte_end");
    assert!(
        !requires_single_cluster_containment,
        "find_clusters_in_canonical 不应要求单个 cluster 完整包含 byte range，\
         应改为相交判定并逐 cluster 收集。"
    );
}

/// 问题2 守卫3: rebind_timed_units_to_canonical 应通过 find_clusters_in_canonical
/// 逐 cluster 重绑。此守卫确认新函数调用存在。
#[test]
fn issue2_rebind_removes_unit_when_cluster_not_found() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 8000);
    assert!(
        window.contains("find_clusters_in_canonical"),
        "rebind_timed_units_to_canonical 应调用 find_clusters_in_canonical 逐 cluster 重绑。"
    );
}

// =========================================================================
// 问题3: ReflowCrossFade 当前必然在 reconcile 时被删除
// =========================================================================

/// 问题3 守卫1: reflow_crossfade_old 创建的 slice shaping_identity: None。
/// 修复后应传入有效的 ShapingIdentity（如从 cluster 构造），使 rebind 时
/// is_same_shaping 能返回 true。
#[test]
fn issue3_reflow_crossfade_old_has_none_shaping_identity() {
    let src = read_src("src/sujian_editor_item/animated_slice.rs");
    let window = function_window(&src, "fn reflow_crossfade_old", 900);
    // 当前缺陷：shaping_identity: None
    let has_none_shaping = window.contains("shaping_identity: None");
    assert!(
        !has_none_shaping,
        "reflow_crossfade_old 创建的 slice shaping_identity: None。\
         rebind_timed_units_to_canonical 对 ReflowCrossFade 执行 \
         None => false，只要发生一次新 canonical reconcile，仍在播放的 \
         ReflowCrossFade 一定进入 Remove，没有按新布局继续的可能。"
    );
}

/// 问题3 守卫2: reflow_crossfade_new 创建的 slice shaping_identity: None。
#[test]
fn issue3_reflow_crossfade_new_has_none_shaping_identity() {
    let src = read_src("src/sujian_editor_item/animated_slice.rs");
    let window = function_window(&src, "fn reflow_crossfade_new", 900);
    let has_none_shaping = window.contains("shaping_identity: None");
    assert!(
        !has_none_shaping,
        "reflow_crossfade_new 创建的 slice shaping_identity: None。\
         与 reflow_crossfade_old 同一类缺陷，rebind 时 None => false → Remove。"
    );
}

/// 问题3 守卫3: rebind_timed_units_to_canonical 中 shaping_identity 为 None 时
/// 返回 false（None => false 分支）。与守卫1/守卫2 组合证明 ReflowCrossFade
/// 必然被 Remove。
#[test]
fn issue3_rebind_none_shaping_identity_returns_false() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 3000);
    // 当前缺陷：match unit.slice.shaping_identity.as_ref() { Some(sid) => ..., None => false }
    let none_returns_false = window.contains("None => false");
    assert!(
        !none_returns_false,
        "rebind_timed_units_to_canonical 中 shaping_identity 为 None 时返回 false \
         （None => false）。ReflowCrossFade 的 shaping_identity 恒为 None，因此 \
         is_same_shaping 恒为 false → !shaping_ok → RebindOutcome::Remove。\
         只要发生一次新 canonical reconcile，仍在播放的 ReflowCrossFade 一定被删除，\
         没有\"按新布局继续\"的可能。"
    );
}
