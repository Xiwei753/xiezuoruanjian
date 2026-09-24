//! Issue #738 评论 5792244119 修复后守卫测试 — 验证 3 个并发视觉事务问题
//! 已被正确修复。
//!
//! 本测试是源代码静态守卫测试（WHITE_BOX），通过读取源文件并检查代码模式来确认
//! 评论 5792244119 指出的 3 个缺陷的修复后正确结构在当前代码中确实存在。
//! 测试 PASS = 修复后结构正确。
//!
//! 3 个问题的修复后正确结构：
//! 1. build_canonical_snapshot_for_current_layout 复用 EditorLayout 当前 generation
//!    提取动画视觉（prepare_animation_visuals_from_layout + inject_animation_visuals_into_snapshot），
//!    fallback 路径才 begin_layout_generation 且用完 clear_layout_generation 释放。
//! 2. rebind_timed_units_to_canonical 对 merged ReflowMove 逐 anchor 计算 movement vector，
//!    不一致时用 RebindDecision::Split 拆回多个 PreparedVisualUnit
//!    （build_split_replacement_units），不再用 union_of_hits 合成跨行大矩形。
//! 3. CrossFade 多对多阶段 unmatched_old / unmatched_new 共享同一个 group_id，
//!    rebind 的 CrossFade pair new side 逐 anchor 校验 shaping identity
//!    （is_same_shaping + anchor_shaping_match），缺 side 的 group 整组 Remove。

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
// 问题 1 修复后守卫: build_canonical_snapshot_for_current_layout 复用 EditorLayout
// generation 提取动画视觉，fallback 才 begin/clear layout generation
// =========================================================================

/// 修复后守卫 1: build_canonical_snapshot_for_current_layout 接收 editor_layout 参数，
/// 复用其 current_prepared_layout() 的 generation 做基础 snapshot。
#[test]
fn issue1_fix_build_canonical_reuses_editor_layout_generation() {
    let src = read_src("src/sujian_editor_item/pipeline.rs");
    let window = function_window(&src, "fn build_canonical_snapshot_for_current_layout", 4800);
    assert!(
        window.contains("editor_layout: &crate::editor::layout::EditorLayout"),
        "修复后 build_canonical_snapshot_for_current_layout 应接收 editor_layout 参数，\
         复用其当前 generation 而非分配临时 generation。"
    );
    assert!(
        window.contains("current_prepared_layout"),
        "修复后应通过 editor_layout.current_prepared_layout() 复用当前 generation。"
    );
    assert!(
        window.contains("prepare_animation_visuals_from_layout"),
        "修复后应调 prepare_animation_visuals_from_layout 提取动画视觉（QImage/clusters），\
         使 rebind 路径 find_clusters_in_canonical 能找到 cluster。"
    );
    assert!(
        window.contains("inject_animation_visuals_into_snapshot"),
        "修复后应调 inject_animation_visuals_into_snapshot 把提取的动画视觉注入新 canonical。"
    );
}

/// 修复后守卫 2: build_canonical_snapshot_for_current_layout 的 fallback 路径
///（EditorLayout 无 prepared layout 时）才 begin_layout_generation，
/// 且用完 clear_layout_generation 释放，不泄漏。
#[test]
fn issue1_fix_fallback_path_clears_layout_generation() {
    let src = read_src("src/sujian_editor_item/pipeline.rs");
    let window = function_window(&src, "fn build_canonical_snapshot_for_current_layout", 4800);
    assert!(
        window.contains("begin_layout_generation"),
        "fallback 路径（无 prepared layout）应 begin_layout_generation 分配临时 generation。"
    );
    assert!(
        window.contains("clear_layout_generation"),
        "修复后 fallback 路径用完临时 generation 应 clear_layout_generation 释放，不泄漏。"
    );
    assert!(
        window.contains("fallback_gen"),
        "修复后应有 fallback_gen 变量追踪临时 generation 的释放。"
    );
}

/// 修复后守卫 3: reconcile_after_layout_change 传入 &self.editor_layout。
#[test]
fn issue1_fix_reconcile_after_layout_change_passes_editor_layout() {
    let src = read_src("src/sujian_editor_item/layout_ops.rs");
    let window = function_window(&src, "fn reconcile_after_layout_change", 800);
    assert!(
        window.contains("&self.editor_layout"),
        "修复后 reconcile_after_layout_change 应传入 &self.editor_layout 给\
         build_canonical_snapshot_for_current_layout。"
    );
}

// =========================================================================
// 问题 2 修复后守卫: rebind 按 anchor 拆分多个 PreparedVisualUnit
// =========================================================================

/// 修复后守卫 4: RebindDecision 枚举有 Split 变体，携带 Vec<PreparedVisualUnit>。
#[test]
fn issue2_fix_rebind_decision_has_split_variant() {
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    let window = function_window(&src, "enum RebindDecision", 600);
    assert!(
        window.contains("Keep") && window.contains("Rebind") && window.contains("Remove"),
        "RebindDecision 应保留 Keep/Rebind/Remove 三种决策。"
    );
    assert!(
        window.contains("Split"),
        "修复后 RebindDecision 应有 Split 变体，携带 Vec<PreparedVisualUnit>，\
         用于把 merged unit 拆回多个 Timed unit。"
    );
    assert!(
        window.contains("Vec<PreparedVisualUnit>"),
        "Split 变体应携带 Vec<PreparedVisualUnit>。"
    );
}

/// 修复后守卫 5: rebind_timed_units_to_canonical 逐 anchor 计算 movement vector，
/// 不一致时调 build_split_replacement_units 拆分。
#[test]
fn issue2_fix_rebind_uses_split_for_inconsistent_vectors() {
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 12000);
    assert!(
        window.contains("anchor_vectors"),
        "修复后 rebind 应计算各 anchor 的 movement vector (anchor_vectors)。"
    );
    assert!(
        window.contains("vectors_consistent"),
        "修复后 rebind 应判断所有 anchor movement vector 是否一致 (vectors_consistent)。"
    );
    assert!(
        window.contains("build_split_replacement_units"),
        "修复后 movement vector 不一致时应调 build_split_replacement_units 拆分。"
    );
    assert!(
        window.contains("RebindDecision::Split(replacement_units)"),
        "修复后应产生 RebindDecision::Split(replacement_units) 决策。"
    );
}

/// 修复后守卫 6: build_split_replacement_units 函数存在，逐 anchor 构造独立
/// PreparedVisualUnit，保留自己的 snapshot_id/source_rect/from rect。
#[test]
fn issue2_fix_build_split_replacement_units_exists() {
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    assert!(
        src.contains("fn build_split_replacement_units"),
        "修复后应有 build_split_replacement_units 函数。"
    );
    let window = function_window(&src, "fn build_split_replacement_units", 5000);
    assert!(
        window.contains("anchor.snapshot_id") && window.contains("anchor.source_rect"),
        "修复后 build_split_replacement_units 应逐 anchor 保留自己的 snapshot_id/source_rect。"
    );
    assert!(
        window.contains("reflow_anchors: vec![new_anchor]"),
        "修复后每个 replacement unit 应只包含自己的单个 ReflowAnchor（与 slice 同 basis）。"
    );
}

/// 修复后守卫 7: 应用决策时处理 Split，把 replacement units 替换原 merged unit。
#[test]
fn issue2_fix_apply_decision_handles_split() {
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 32000);
    assert!(
        window.contains("Some(RebindDecision::Split(replacement_units))"),
        "修复后应用决策时应处理 Split 变体。"
    );
    assert!(
        window.contains("self.units.insert(i, new_unit)"),
        "修复后应把 replacement units insert 到 self.units 替换原 merged unit。"
    );
}

// =========================================================================
// 问题 3 修复后守卫: CrossFade 多对多成组配对 + new side shaping 校验 + 缺 side 整组失效
// =========================================================================

/// 修复后守卫 8: 多对多阶段 unmatched_old 和 unmatched_new 共享同一个 group_id。
#[test]
fn issue3_fix_many_to_many_old_new_share_group_id() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder.rs");
    // 定位多对多阶段：unmatched_old 循环
    let unmatched_old_pos = src
        .find("for &oi in &unmatched_old")
        .expect("多对多阶段 unmatched_old 循环必须存在");
    // 定位 unmatched_new 循环
    let unmatched_new_pos = src
        .find("for &ni in &unmatched_new")
        .expect("多对多阶段 unmatched_new 循环必须存在");

    assert!(
        unmatched_old_pos < unmatched_new_pos,
        "多对多阶段应先遍历 unmatched_old 再遍历 unmatched_new。"
    );

    // 修复后：group_id 在 old/new 两个循环之前分配一次，old 和 new 共享同一个 group_id。
    // 定位多对多 if 块开始（unmatched_old.is_empty() 判断），group_id 应在该判断之后、
    // unmatched_old 循环之前分配。
    let if_block_pos = src
        .find("if !unmatched_old.is_empty() && !unmatched_new.is_empty()")
        .expect("多对多 if 块必须存在");
    let group_id_alloc_pos = src[if_block_pos..unmatched_old_pos]
        .find("let group_id = next_crossfade_group_id;")
        .map(|p| if_block_pos + p)
        .expect("修复后 group_id 应在 unmatched_old 循环之前分配一次");
    assert!(
        if_block_pos < group_id_alloc_pos && group_id_alloc_pos < unmatched_old_pos,
        "修复后 group_id 应在多对多 if 块内、unmatched_old 循环之前分配，\
         old 和 new 共享同一个 group_id。"
    );

    // 取从 unmatched_old 循环到 unmatched_new 循环结束的窗口
    let window_end = unmatched_new_pos + 1600;
    let window_end = src
        .char_indices()
        .take_while(|(i, _)| *i < window_end)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(src.len())
        .min(src.len());
    let window = &src[unmatched_old_pos..window_end];

    // 修复后：old/new 循环内不再各自独立分配 group_id。
    let independent_alloc_in_loops = window
        .matches("let group_id = next_crossfade_group_id;")
        .count();
    assert!(
        independent_alloc_in_loops == 0,
        "修复后 old/new 循环内不应再各自独立分配 group_id（共 {independent_alloc_in_loops} 处），\
         group_id 在循环外分配一次由 old/new 共享。"
    );

    // 确认 old 创建 reflow_crossfade_old，new 创建 reflow_crossfade_new，都用同一个 group_id。
    assert!(
        window.contains("reflow_crossfade_old") && window.contains("reflow_crossfade_new"),
        "多对多阶段应分别创建 reflow_crossfade_old 和 reflow_crossfade_new。"
    );
    assert!(
        window.contains("Some(group_id)"),
        "old/new 都应使用同一个 group_id。"
    );
}

/// 修复后守卫 9: CrossFade 多对多 group 的 new side 校验包含 shaping identity 校验
///（is_same_shaping + anchor_shaping_match），与 ReflowMove 对称。
/// Issue #738 评论 5793319451 问题2: CrossFade 从一对一 crossfade_pairs 改成多对多
/// crossfade_groups（CrossFadeGroup { old_indices, new_indices }），整组一起处理。
#[test]
fn issue3_fix_crossfade_new_side_has_shaping_identity_check() {
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    // 修复后：多对多 group 结构存在。
    assert!(
        src.contains("struct CrossFadeGroup"),
        "修复后应有 CrossFadeGroup 多对多 group 结构。"
    );
    assert!(
        src.contains("old_indices") && src.contains("new_indices"),
        "修复后 CrossFadeGroup 应有 old_indices 和 new_indices 收集整组所有成员。"
    );
    // 定位多对多 group 处理循环
    let group_marker = "for group in crossfade_groups.values()";
    let group_pos = src
        .find(group_marker)
        .expect("rebind 中 crossfade_groups 多对多处理循环必须存在");
    // 取该循环到结束的窗口（足够覆盖 new side 校验逻辑）
    let window_end = group_pos + 8000;
    let window_end = src
        .char_indices()
        .take_while(|(i, _)| *i < window_end)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(src.len())
        .min(src.len());
    let group_window = &src[group_pos..window_end];

    // 修复后：new side 校验包含 shaping identity 校验。
    assert!(
        group_window.contains("is_same_shaping"),
        "修复后 CrossFade group 的 new side 校验应包含 is_same_shaping，\
         检查 anchor.shaping_identity == 当前 canonical hit.shaping。"
    );
    assert!(
        group_window.contains("anchor_shaping_match"),
        "修复后 CrossFade group 的 new side 应有 anchor_shaping_match 逻辑，\
         与 ReflowMove 的 shaping identity 校验对称。"
    );
    // 修复后：逐个检查所有 new side（多对多），任一失效整组 Remove。
    assert!(
        group_window.contains("all_new_side_ok"),
        "修复后应逐个检查所有 new side（all_new_side_ok），任一失效整组 Remove。"
    );
}

/// 修复后守卫 10: CrossFade 多对多 group 缺 side（old 或 new 任一侧为空）时
/// 整组所有成员一起 Remove，不再走独立 Rebind。
/// Issue #738 评论 5793319451 问题2: 旧实现用一对一 crossfade_pairs，缺 side 的 group
/// 成员走单独循环 Remove；新实现用多对多 crossfade_groups，在 group 处理循环内
/// 检查 old_indices/new_indices 是否为空，整组一起 Remove。
#[test]
fn issue3_fix_unpaired_crossfade_group_remove() {
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 16000);
    assert!(
        window.contains("缺 side 的 group"),
        "修复后未配对 CrossFade 应有 '缺 side 的 group' 注释说明整组失效处理。"
    );
    // 修复后：多对多 group 在循环内检查 old/new 任一侧为空 → 整组 Remove。
    assert!(
        window.contains("group.old_indices.is_empty() || group.new_indices.is_empty()"),
        "修复后应在 group 循环内检查 old_indices/new_indices 任一侧为空。"
    );
    // 修复后：缺 side 时整组所有成员一起 Remove（遍历 old_indices 和 new_indices 设 Remove）。
    let group_marker = "for group in crossfade_groups.values()";
    let group_pos = src
        .find(group_marker)
        .expect("rebind 中 crossfade_groups 多对多处理循环必须存在");
    let window_end = group_pos + 1200;
    let window_end = src
        .char_indices()
        .take_while(|(i, _)| *i < window_end)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(src.len())
        .min(src.len());
    let group_head = &src[group_pos..window_end];
    assert!(
        group_head.contains("group.old_indices.is_empty() || group.new_indices.is_empty()"),
        "修复后缺 side 检查应在 group 处理循环开头。"
    );
    assert!(
        group_head.contains("for &oi in &group.old_indices"),
        "修复后缺 side 时应遍历 group.old_indices 整组 Remove。"
    );
    assert!(
        group_head.contains("for &ni in &group.new_indices"),
        "修复后缺 side 时应遍历 group.new_indices 整组 Remove。"
    );
}
