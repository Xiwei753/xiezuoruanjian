//! Issue #738 评论 5793319451 修复后守卫测试 — 验证 3 个并发视觉事务剩余问题
//! 已被正确修复。
//!
//! 本测试是源代码静态守卫测试（WHITE_BOX），通过读取源文件并检查代码模式来确认
//! 评论 5793319451 指出的 3 个缺陷的修复后正确结构在当前代码中确实存在。
//! 测试 PASS = 修复后结构正确。
//!
//! 3 个问题的修复后正确结构：
//! 1. pipeline.rs 中 `self.layout_revision = new_revision;` 无条件提交（移出
//!    `if let Some(key) = key` 块），animation_coordinator.rs 中 5 个 basis 守卫
//!    从 `<`/`>=` 改成 `!=`/`==`（future revision 也不属于当前 canonical）。
//! 2. CrossFade 从一对一 crossfade_pairs 改成多对多 crossfade_groups
//!    （CrossFadeGroup { old_indices, new_indices }），整组一起处理。
//! 3. build_split_replacement_units 接收外层 now 参数（不再内部 Instant::now()），
//!    帧计算方向与 compute_frame 一致（`* current_visible` 而非 `* (1.0 - current_visible)`），
//!    普通 Rebind 后同步更新 reflow_anchors 的 from/to_document_rect。

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
// 修复 1a 守卫: pipeline.rs layout_revision 无条件提交
// =========================================================================

/// 修复后守卫 1a: `self.layout_revision = new_revision;` 在 `if let Some(key) = key`
/// 之前无条件执行，if let 块内只有 `self.prepare_transaction_textures(key);`。
#[test]
fn issue738_comment5793319451_fix1a_layout_revision_unconditional_commit() {
    let src = read_src("src/sujian_editor_item/pipeline.rs");
    let window = function_window(&src, "fn prepare_edit_motion", 26000);

    // 修复后：layout_revision = new_revision 存在。
    assert!(
        window.contains("self.layout_revision = new_revision;"),
        "修复后 prepare_edit_motion 应有 self.layout_revision = new_revision; 无条件提交。"
    );

    // 修复后：layout_revision = new_revision 在 if let Some(key) = key 之前。
    let rev_pos = window
        .find("self.layout_revision = new_revision;")
        .expect("self.layout_revision = new_revision; 必须存在");
    let iflet_pos = window
        .find("if let Some(key) = key")
        .expect("if let Some(key) = key 必须存在");
    assert!(
        rev_pos < iflet_pos,
        "修复后 self.layout_revision = new_revision; 应在 if let Some(key) = key 之前，\
         实际 rev_pos={} > iflet_pos={}。"
    , rev_pos, iflet_pos);

    // 修复后：if let Some(key) = key 块内不再包含 layout_revision 赋值。
    // 取 if let 块之后 400 字符窗口检查不含 layout_revision = new_revision。
    let iflet_block = &window[iflet_pos..iflet_pos + 400];
    assert!(
        !iflet_block.contains("self.layout_revision = new_revision;"),
        "修复后 if let Some(key) = key 块内不应再包含 self.layout_revision = new_revision;，\
         应已移到块外无条件执行。"
    );
    // 修复后：if let 块内仍有 prepare_transaction_textures。
    assert!(
        iflet_block.contains("self.prepare_transaction_textures(key);"),
        "修复后 if let Some(key) = key 块内应保留 self.prepare_transaction_textures(key);。"
    );
}

// =========================================================================
// 修复 1b 守卫: animation_coordinator.rs basis 守卫用 != / ==
// =========================================================================

/// 修复后守卫 1b-1: active_text_transaction_key_with_epoch 用 `!=` 而非 `<`。
#[test]
fn issue738_comment5793319451_fix1b_caret_owner_guard_uses_neq() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let window = function_window(&src, "fn active_text_transaction_key_with_epoch", 3000);
    assert!(
        window.contains("tx.layout_basis_revision != current_layout_revision"),
        "修复后 caret owner 守卫应用 != 而非 <。"
    );
    assert!(
        !window.contains("tx.layout_basis_revision < current_layout_revision"),
        "修复后不应再有 tx.layout_basis_revision < current_layout_revision 旧守卫。"
    );
}

/// 修复后守卫 1b-2: clip rects 收集用 `==` 而非 `>=`。
#[test]
fn issue738_comment5793319451_fix1b_clip_rects_guard_uses_eq() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    assert!(
        src.contains("tx.layout_basis_revision == frame_context.layout_basis_revision"),
        "修复后 clip rects 守卫应用 == 而非 >=。"
    );
    assert!(
        !src.contains("tx.layout_basis_revision >= frame_context.layout_basis_revision"),
        "修复后不应再有 tx.layout_basis_revision >= frame_context.layout_basis_revision 旧守卫。"
    );
}

/// 修复后守卫 1b-3: build_text_animation_plan_with_sample 用 `!=` 而非 `<`（两处）。
#[test]
fn issue738_comment5793319451_fix1b_plan_guards_use_neq() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let window = function_window(&src, "fn build_text_animation_plan_with_sample", 12000);
    // 修复后：两处 != layout_basis_revision（retire caret motion + glyph 计划）。
    let neq_count = window
        .matches("tx.layout_basis_revision != layout_basis_revision")
        .count();
    assert!(
        neq_count >= 2,
        "修复后 build_text_animation_plan_with_sample 应有至少 2 处 \
         tx.layout_basis_revision != layout_basis_revision（retire + glyph），实际 {neq_count}。"
    );
    // 修复后：不再有 < layout_basis_revision 旧守卫。
    assert!(
        !window.contains("tx.layout_basis_revision < layout_basis_revision"),
        "修复后不应再有 tx.layout_basis_revision < layout_basis_revision 旧守卫。"
    );
}

/// 修复后守卫 1b-4: CursorOnly 查找也用 `!=` 而非 `<`。
#[test]
fn issue738_comment5793319451_fix1b_cursor_only_guard_uses_neq() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    // CursorOnly 查找在 find_transaction_for_cursor_target 或类似函数中。
    // 检查整个文件中不再有 tx.layout_basis_revision < current_layout_revision。
    assert!(
        !src.contains("tx.layout_basis_revision < current_layout_revision"),
        "修复后不应再有 tx.layout_basis_revision < current_layout_revision 旧守卫。"
    );
}

// =========================================================================
// 修复 2a 守卫: CrossFade 多对多 group 结构
// =========================================================================

/// 修复后守卫 2a: rebind_timed_units_to_canonical 用多对多 CrossFadeGroup 结构。
#[test]
fn issue738_comment5793319451_fix2a_crossfade_many_to_many_group_struct() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 12000);

    // 修复后：多对多 group 结构存在。
    assert!(
        window.contains("struct CrossFadeGroup"),
        "修复后应有 CrossFadeGroup 多对多 group 结构。"
    );
    assert!(
        window.contains("old_indices: Vec<usize>"),
        "修复后 CrossFadeGroup 应有 old_indices: Vec<usize>。"
    );
    assert!(
        window.contains("new_indices: Vec<usize>"),
        "修复后 CrossFadeGroup 应有 new_indices: Vec<usize>。"
    );
    assert!(
        window.contains("crossfade_groups: HashMap<u64, CrossFadeGroup>"),
        "修复后应有 crossfade_groups: HashMap<u64, CrossFadeGroup>。"
    );

    // 修复后：旧的一对一结构已删除。
    assert!(
        !window.contains("crossfade_pairs: Vec<(usize, usize)>"),
        "修复后不应再有 crossfade_pairs: Vec<(usize, usize)> 旧一对一结构。"
    );
    assert!(
        !window.contains("let mut groups: HashMap<u64, (Option<usize>, Option<usize>)>"),
        "修复后不应再有 let mut groups: HashMap<u64, (Option<usize>, Option<usize>)> 旧分组。"
    );
    assert!(
        !window.contains("groups.entry(gid).or_insert((None, None))"),
        "修复后不应再有 groups.entry(gid).or_insert((None, None)) 旧分组插入。"
    );
}

// =========================================================================
// 修复 2b 守卫: 多对多 group 处理逻辑
// =========================================================================

/// 修复后守卫 2b: 多对多 group 处理循环 + 缺 side 整组 Remove + 逐 new side 校验。
#[test]
fn issue738_comment5793319451_fix2b_crossfade_group_processing_logic() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 20000);

    // 修复后：多对多 group 处理循环存在。
    assert!(
        window.contains("for (_gid, group) in &crossfade_groups"),
        "修复后应有 for (_gid, group) in &crossfade_groups 多对多处理循环。"
    );

    // 修复后：缺 side 整组 Remove。
    assert!(
        window.contains("group.old_indices.is_empty() || group.new_indices.is_empty()"),
        "修复后应检查 group.old_indices.is_empty() || group.new_indices.is_empty()（缺 side 整组 Remove）。"
    );

    // 修复后：逐个检查所有 new side。
    assert!(
        window.contains("all_new_side_ok"),
        "修复后应有 all_new_side_ok 逐个检查所有 new side 逻辑。"
    );

    // 修复后：整组处理 old/new side。
    assert!(
        window.contains("for &oi in &group.old_indices"),
        "修复后应有 for &oi in &group.old_indices 整组处理 old side。"
    );
    assert!(
        window.contains("for &ni in &group.new_indices"),
        "修复后应有 for &ni in &group.new_indices 整组处理 new side。"
    );

    // 修复后：旧的一对一循环已删除。
    assert!(
        !window.contains("for &(old_idx, new_idx) in &crossfade_pairs"),
        "修复后不应再有 for &(old_idx, new_idx) in &crossfade_pairs 旧一对一循环。"
    );
    assert!(
        !window.contains("crossfade_pairs.iter().any(|&(o, n)| o == i || n == i)"),
        "修复后不应再有 crossfade_pairs.iter().any(|&(o, n)| o == i || n == i) 旧缺 side 循环。"
    );
}

// =========================================================================
// 修复 3a 守卫: build_split_replacement_units 接收外层 now 参数
// =========================================================================

/// 修复后守卫 3a: build_split_replacement_units 签名有 now: Instant，不再内部 Instant::now()。
#[test]
fn issue738_comment5793319451_fix3a_split_replacement_uses_outer_now() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn build_split_replacement_units", 3000);

    // 修复后：函数签名包含 now: Instant 参数。
    assert!(
        window.contains("now: Instant"),
        "修复后 build_split_replacement_units 签名应包含 now: Instant 参数。"
    );

    // 修复后：不再内部重新 Instant::now()。
    assert!(
        !window.contains("let now_for_timing = Instant::now();"),
        "修复后不应再有 let now_for_timing = Instant::now(); 内部重新取时间。"
    );
}

// =========================================================================
// 修复 3b 守卫: 帧计算方向与 compute_frame 一致
// =========================================================================

/// 修复后守卫 3b: 用 `* current_visible` 而非 `* (1.0 - current_visible)`。
#[test]
fn issue738_comment5793319451_fix3b_split_frame_direction_matches_compute_frame() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn build_split_replacement_units", 4000);

    // 修复后：用 * current_visible（正确方向）。
    assert!(
        window.contains("* current_visible"),
        "修复后帧计算应用 * current_visible（与 compute_frame 的 from + (to - from) * visible 一致）。"
    );

    // 修复后：不再用 * (1.0 - current_visible)（错误方向）。
    assert!(
        !window.contains("* (1.0 - current_visible)"),
        "修复后不应再用 * (1.0 - current_visible) 旧错误方向。"
    );
}

// =========================================================================
// 修复 3c 守卫: 普通 Rebind 后同步更新 reflow_anchors
// =========================================================================

/// 修复后守卫 3c: Rebind 应用循环中同步更新 reflow_anchors 的 from/to_document_rect。
#[test]
fn issue738_comment5793319451_fix3c_rebind_updates_reflow_anchors() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 20000);

    // 修复后：Rebind 应用循环中有 reflow_anchors 同步更新。
    assert!(
        window.contains("for anchor in &mut unit.slice.reflow_anchors"),
        "修复后 Rebind 应用循环应遍历 for anchor in &mut unit.slice.reflow_anchors 同步更新。"
    );

    // 修复后：anchor.from_document_rect 更新为当前帧位置（frame.x/y/w/h）。
    assert!(
        window.contains("anchor.from_document_rect = SourceRect"),
        "修复后应更新 anchor.from_document_rect = SourceRect 结构体。"
    );

    // 修复后：anchor.to_document_rect 更新为 new_to。
    assert!(
        window.contains("anchor.to_document_rect = new_to.clone();"),
        "修复后应更新 anchor.to_document_rect = new_to.clone();。"
    );

    // 修复后：注释中标记了 5793319451 问题3C。
    assert!(
        window.contains("5793319451"),
        "修复后 rebind 中应有 5793319451 注释标记此修复。"
    );
}
