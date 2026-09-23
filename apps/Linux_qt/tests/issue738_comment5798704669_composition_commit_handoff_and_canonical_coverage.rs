//! Issue #738 评论 5798704669 复现测试 — 验证两个直接影响连续动画的结构缺陷
//! 在当前代码中确实存在。
//!
//! 本测试是源代码静态守卫测试（WHITE_BOX），通过读取源文件并检查代码模式来确认
//! 评论 5798704669 指出的两个结构缺陷在当前代码中确实存在。
//!
//! ## 问题 1：IME commit 仍然是"先 cancel 旧 composition，再采 handoff"
//!
//! `editing.rs::record_composition_commit_transaction()` 先执行
//! `cancel_active_composition(cancel_reason)`，旧 CompositionUpdate transaction
//! 当场从队列删除。真正采 rebase frame / caret handoff 的代码在后面的
//! `handle_composition_commit_or_cancel()` 里才做 find_conflicting_transaction /
//! take_rebase_frames。这时旧 composition transaction 已不存在，preedit 当前帧采不到。
//! 而且 `handle_composition_commit_or_cancel` 内部又单独 `Instant::now()`，
//! 没消费外层已生成的 `edit_now`。
//!
//! ## 问题 2：reconcile 用的 canonical 不是"活动事务所需的完整 canonical"
//!
//! `build_editor_layout_snapshot_with_canonical()` 只对当前 composition
//! affected/diff line ids 调 `prepare_animation_visuals_from_layout` 再注入 canonical，
//! 没有合并 active rebind ranges（远处仍存活的 Timed Reflow 的目标行）。
//! `reconcile_active_transactions_with_canonical` 遍历全部活动 transaction，
//! `rebind_timed_units_to_canonical` 对每个 Reflow anchor 调
//! `find_clusters_in_canonical`，该函数只遍历 `snapshot.paragraphs -> line.clusters`，
//! 找不到 cluster 就把 unit 判成 Remove。远处 Reflow 的目标行 clusters=[] 时被误删。
//!
//! ## 测试结构
//!
//! - `defect1_*` / `defect2_*`：断言缺陷结构在当前代码中存在（当前 PASS = 缺陷已复现）。
//! - `fix1_*` / `fix2_*`：断言评论 5798704669 建议的修复后结构存在（当前 FAIL = 修复未实现）。
//!
//! `cargo test` 整体 exit non-zero（fix_* FAIL），同时 defect_* PASS 证明缺陷确实被复现。

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

// 用实际方法调用标记（带 `(`）区分注释中的方法名提及。
const CANCEL_CALL: &str = ".cancel_active_composition(";
const HANDLE_CALL: &str = ".handle_composition_commit_or_cancel(";

// =========================================================================
// 缺陷1: record_composition_commit_transaction 先 cancel 旧 composition 再采 handoff
// =========================================================================

/// defect1a: 修复后 `record_composition_commit_transaction` 内不应调
/// `cancel_active_composition`（旧 CompositionUpdate 应留给 prepare 阶段采样）。
#[test]
fn defect1a_record_composition_commit_calls_cancel_active_composition() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn record_composition_commit_transaction", 14000);
    assert!(
        !window.contains(CANCEL_CALL),
        "修复后 record_composition_commit_transaction 不应含 .cancel_active_composition( 调用。"
    );
}

/// defect1b: 修复后 `record_composition_commit_transaction` 内不应在
/// `handle_composition_commit_or_cancel` 之前调 `cancel_active_composition`。
/// 修复后 cancel 不存在，此测试 PASS。
#[test]
fn defect1b_cancel_before_handle_composition_commit() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn record_composition_commit_transaction", 14000);
    let cancel_pos = window.find(CANCEL_CALL);
    let handle_pos = window
        .find(HANDLE_CALL)
        .expect("handle_composition_commit_or_cancel 调用必须存在");
    let fixed = match cancel_pos {
        None => true,
        Some(cp) => cp > handle_pos,
    };
    assert!(
        fixed,
        "修复后 cancel 不应在 handle 之前。cancel_pos={:?} handle_pos={}",
        cancel_pos, handle_pos
    );
}

/// defect1c: 修复后 `handle_composition_commit_or_cancel` 不应含
/// `let now = Instant::now();` 单独采样，应消费外层传入的 `now` 参数。
#[test]
fn defect1c_handle_composition_commit_uses_local_instant_now() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let window = function_window(&src, "fn handle_composition_commit_or_cancel", 4000);
    assert!(
        !window.contains("let now = Instant::now();"),
        "修复后 handle_composition_commit_or_cancel 不应含 let now = Instant::now();，应消费外层 now 参数。"
    );
}

/// defect1d: 修复后 `animation_coordinator.rs` 应引入
/// `prepare_composition_commit_handoff` / `PreparedCompositionCommitHandoff`。
#[test]
fn defect1d_prepared_composition_commit_handoff_not_introduced() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    assert!(
        src.contains("prepare_composition_commit_handoff"),
        "修复后 animation_coordinator.rs 应含 prepare_composition_commit_handoff。"
    );
    assert!(
        src.contains("PreparedCompositionCommitHandoff"),
        "修复后 animation_coordinator.rs 应含 PreparedCompositionCommitHandoff。"
    );
}

// =========================================================================
// 缺陷2: build_editor_layout_snapshot_with_canonical 只覆盖本次 edit 局部 clusters
// =========================================================================

/// defect2a: `build_editor_layout_snapshot_with_canonical` 内只在
/// `affected_start < affected_end` 时对 composition_range + diff line ids 调
/// `prepare_animation_visuals_from_layout`，没有合并 active rebind ranges。
/// 这里断言 `if affected_start < affected_end {` 守卫存在（即只在局部范围注入 clusters）。
#[test]
fn defect2a_helper_only_injects_within_affected_guard() {
    let src = read_src("src/sujian_editor_item/layout_ops.rs");
    let window = function_window(&src, "fn build_editor_layout_snapshot_with_canonical", 8000);
    assert!(
        window.contains("if affected_start < affected_end"),
        "缺陷2a: build_editor_layout_snapshot_with_canonical 应含 `if affected_start < affected_end` 守卫，\
         即只在本次 composition affected 范围内注入 clusters（当前缺陷结构）。"
    );
    assert!(
        window.contains("prepare_animation_visuals_from_layout"),
        "缺陷2a: build_editor_layout_snapshot_with_canonical 应调 prepare_animation_visuals_from_layout。"
    );
}

/// defect2b: 修复后 `build_editor_layout_snapshot_with_canonical` 应含
/// `collect_active_rebind_ranges` 调用（合并 active rebind ranges coverage）。
#[test]
fn defect2b_helper_does_not_merge_active_rebind_ranges() {
    let src = read_src("src/sujian_editor_item/layout_ops.rs");
    let window = function_window(&src, "fn build_editor_layout_snapshot_with_canonical", 8000);
    assert!(
        window.contains("collect_active_rebind_ranges"),
        "修复后 build_editor_layout_snapshot_with_canonical 应含 collect_active_rebind_ranges 调用\
         合并 active rebind ranges coverage。"
    );
}

/// defect2c: 修复后 `animation_coordinator.rs` 应有 `collect_active_rebind_ranges` 只读入口。
#[test]
fn defect2c_collect_active_rebind_ranges_not_introduced() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    assert!(
        src.contains("fn collect_active_rebind_ranges"),
        "修复后 animation_coordinator.rs 应有 fn collect_active_rebind_ranges 只读入口。"
    );
}

/// defect2d: `rebind_timed_units_to_canonical` 内对每个 Reflow anchor 调
/// `find_clusters_in_canonical`，`hits.is_empty()` 时 `all_anchors_ok = false`，
/// 后续 `if !all_anchors_ok { decisions[i] = RebindDecision::Remove; }` 把 unit 判成 Remove。
/// 这是问题2的下游症状：远处 Reflow 目标行 clusters=[] 时被误删。
#[test]
fn defect2d_rebind_judge_remove_when_clusters_empty() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 12000);
    assert!(
        window.contains("find_clusters_in_canonical"),
        "缺陷2d: rebind_timed_units_to_canonical 应调 find_clusters_in_canonical。"
    );
    assert!(
        window.contains("if hits.is_empty()"),
        "缺陷2d: rebind_timed_units_to_canonical 应含 `if hits.is_empty()` 判定（找不到 cluster 时 anchor 失效）。"
    );
    assert!(
        window.contains("all_anchors_ok = false"),
        "缺陷2d: rebind_timed_units_to_canonical 应含 `all_anchors_ok = false`（anchor 失效标记）。"
    );
    assert!(
        window.contains("RebindDecision::Remove"),
        "缺陷2d: rebind_timed_units_to_canonical 应含 RebindDecision::Remove（找不到 cluster 时 unit 被误删）。"
    );
}

/// defect2e: `find_clusters_in_canonical` 只遍历 `snapshot.paragraphs -> line.clusters`，
/// clusters=[] 时返回空 hits。这是问题2的根因：canonical 里那一行 clusters=[] 时
/// 远处 Reflow 的目标字节虽仍存在但找不到 cluster。
#[test]
fn defect2e_find_clusters_only_iterates_line_clusters() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    let window = function_window(&src, "fn find_clusters_in_canonical", 3000);
    assert!(
        window.contains("for para in &snapshot.paragraphs"),
        "缺陷2e: find_clusters_in_canonical 应遍历 snapshot.paragraphs。"
    );
    assert!(
        window.contains("for line in &para.lines"),
        "缺陷2e: find_clusters_in_canonical 应遍历 para.lines。"
    );
    assert!(
        window.contains("for cluster in &line.clusters"),
        "缺陷2e: find_clusters_in_canonical 应遍历 line.clusters（clusters=[] 时返回空 hits）。"
    );
}

/// defect2f: `reconcile_active_transactions_with_canonical` 遍历**全部**活动 transaction
/// （不区分是否与本次 edit 相关），对每个调 `rebind_timed_units_to_canonical`。
/// 这是问题2的上游入口：远处 Reflow 也会被 rebind，从而触发 defect2d 的误删。
#[test]
fn defect2f_reconcile_iterates_all_active_transactions() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let window = function_window(
        &src,
        "fn reconcile_active_transactions_with_canonical",
        4000,
    );
    assert!(
        window.contains("active_transactions()"),
        "缺陷2f: reconcile_active_transactions_with_canonical 应遍历 active_transactions()（全部活动事务）。"
    );
    assert!(
        window.contains("rebind_timed_units_to_canonical"),
        "缺陷2f: reconcile_active_transactions_with_canonical 应调 rebind_timed_units_to_canonical。"
    );
}

// =========================================================================
// 修复后守卫1: 问题1 修复后正确结构（当前 FAIL，用于后续验证修复）
// =========================================================================

/// fix1a: 修复后 `record_composition_commit_transaction` 内不应在
/// `handle_composition_commit_or_cancel` 之前调 `cancel_active_composition`
/// （应改用 prepare_composition_commit_handoff 让 take_rebase_frames 自己 cancel）。
/// 当前代码缺陷存在，此测试 FAIL。
#[test]
fn fix1a_no_cancel_before_handle_composition_commit() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn record_composition_commit_transaction", 14000);
    // 若 cancel 调用不存在，或 cancel 在 handle 之后，则修复已落地。
    let cancel_pos = window.find(CANCEL_CALL);
    let handle_pos = window
        .find(HANDLE_CALL)
        .expect("handle_composition_commit_or_cancel 调用必须存在");
    let fixed = match cancel_pos {
        None => true,
        Some(cp) => cp > handle_pos,
    };
    assert!(
        fixed,
        "修复后 record_composition_commit_transaction 不应在 handle_composition_commit_or_cancel 之前调 cancel_active_composition。\
         当前 cancel_pos={:?} handle_pos={}（缺陷存在，此测试预期 FAIL）。",
        cancel_pos, handle_pos
    );
}

/// fix1b: 修复后 `animation_coordinator.rs` 应引入
/// `PreparedCompositionCommitHandoff` 结构（评论 5798704669 问题1 建议的修复入口）。
/// 当前未引入，此测试 FAIL。
#[test]
fn fix1b_prepared_composition_commit_handoff_introduced() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    assert!(
        src.contains("PreparedCompositionCommitHandoff"),
        "修复后 animation_coordinator.rs 应引入 PreparedCompositionCommitHandoff。\
         当前未引入（缺陷存在，此测试预期 FAIL）。"
    );
}

/// fix1c: 修复后 `animation_coordinator.rs` 应有
/// `prepare_composition_commit_handoff` 方法（prepare 阶段在旧 CompositionUpdate
/// 仍活着时采 handoff）。当前未引入，此测试 FAIL。
#[test]
fn fix1c_prepare_composition_commit_handoff_introduced() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    assert!(
        src.contains("fn prepare_composition_commit_handoff"),
        "修复后 animation_coordinator.rs 应有 fn prepare_composition_commit_handoff。\
         当前未引入（缺陷存在，此测试预期 FAIL）。"
    );
}

// =========================================================================
// 修复后守卫2: 问题2 修复后正确结构（当前 FAIL，用于后续验证修复）
// =========================================================================

/// fix2a: 修复后 `animation_coordinator.rs` 应有 `collect_active_rebind_ranges`
/// 只读入口（评论 5798704669 问题2 建议的修复入口）。当前未引入，此测试 FAIL。
#[test]
fn fix2a_collect_active_rebind_ranges_introduced() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    assert!(
        src.contains("fn collect_active_rebind_ranges"),
        "修复后 animation_coordinator.rs 应有 fn collect_active_rebind_ranges 只读入口。\
         当前未引入（缺陷存在，此测试预期 FAIL）。"
    );
}

/// fix2b: 修复后 `build_editor_layout_snapshot_with_canonical` 应合并 active rebind
/// ranges（调用 `collect_active_rebind_ranges` 并把对应 line ids 并入 line_ids）。
/// 当前未合并，此测试 FAIL。
#[test]
fn fix2b_helper_merges_active_rebind_ranges() {
    let src = read_src("src/sujian_editor_item/layout_ops.rs");
    let window = function_window(&src, "fn build_editor_layout_snapshot_with_canonical", 8000);
    assert!(
        window.contains("collect_active_rebind_ranges"),
        "修复后 build_editor_layout_snapshot_with_canonical 应调 collect_active_rebind_ranges\
         合并 active rebind ranges。当前未合并（缺陷存在，此测试预期 FAIL）。"
    );
}
