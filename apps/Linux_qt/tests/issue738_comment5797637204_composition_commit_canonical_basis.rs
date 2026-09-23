//! Issue #738 评论 5797637204 修复后守卫测试 — 验证 composition commit 路径
//! `record_composition_commit_transaction` 已走 canonical basis 闭环，与普通正文路径
//! `pipeline.rs::prepare_edit_motion` 结构一致。
//!
//! 本测试是源代码静态守卫测试（WHITE_BOX），通过读取源文件并检查代码模式来确认
//! 评论 5797637204 指出的结构缺陷的修复后正确结构在当前代码中确实存在。
//! 测试 PASS = 修复后结构正确。
//!
//! 修复后正确结构：
//! 1. `record_composition_commit_transaction` 内有 `LayoutRevision::next()` 生成新 revision；
//! 2. 内有 `reconcile_active_transactions_with_canonical` 把旧活动事务重绑到新 canonical；
//! 3. 内有 `set_current_canonical_snapshot` 提交新 canonical identity；
//! 4. 内有 `set_layout_revision` 无条件提交新 revision；
//! 5. 内不再用 `let layout_basis_revision = self.pipeline.layout_revision()` 当新事务 basis；
//! 6. `build_editor_layout_snapshot_with_canonical` helper 存在，返回
//!    `(EditorLayoutSnapshot, CanonicalDocumentVisualSnapshot)`；
//! 7. `build_editor_layout_snapshot` 内部调 `build_editor_layout_snapshot_with_canonical`；
//! 8. 顺序守卫：`reconcile_active_transactions_with_canonical` 调用在
//!    `handle_composition_commit_or_cancel` 调用之前；
//! 9. 顺序守卫：`set_layout_revision` / `set_current_canonical_snapshot` 在
//!    `handle_composition_commit_or_cancel` 之后（先创建事务再提升 canonical）。

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
// 注释里写 `reconcile_active_transactions_with_canonical 的新 canonical` 不带 `(`，
// 实际调用 `.reconcile_active_transactions_with_canonical(` 带 `(`。
const RECONCILE_CALL: &str = ".reconcile_active_transactions_with_canonical(";
const HANDLE_CALL: &str = ".handle_composition_commit_or_cancel(";

// =========================================================================
// 守卫1: record_composition_commit_transaction 内有 canonical basis 闭环
// =========================================================================

/// 守卫1a: `record_composition_commit_transaction` 内有 `LayoutRevision::next()`
/// 生成新 canonical basis revision（不再用旧 self.pipeline.layout_revision() 当 basis）。
#[test]
fn fix1a_record_composition_commit_has_layout_revision_next() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn record_composition_commit_transaction", 14000);
    assert!(
        window.contains("LayoutRevision::next()"),
        "修复后 record_composition_commit_transaction 应含 LayoutRevision::next() 生成新 revision。"
    );
}

/// 守卫1b: `record_composition_commit_transaction` 内有
/// `reconcile_active_transactions_with_canonical` 把旧活动事务重绑到新 canonical。
#[test]
fn fix1b_record_composition_commit_has_reconcile() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn record_composition_commit_transaction", 14000);
    assert!(
        window.contains(RECONCILE_CALL),
        "修复后 record_composition_commit_transaction 应调 .reconcile_active_transactions_with_canonical( 。"
    );
}

/// 守卫1c: `record_composition_commit_transaction` 内有
/// `set_current_canonical_snapshot` 提交新 canonical identity。
#[test]
fn fix1c_record_composition_commit_has_set_current_canonical_snapshot() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn record_composition_commit_transaction", 14000);
    assert!(
        window.contains(".set_current_canonical_snapshot("),
        "修复后 record_composition_commit_transaction 应调 .set_current_canonical_snapshot( 提交新 canonical。"
    );
}

/// 守卫1d: `record_composition_commit_transaction` 内有
/// `set_layout_revision` 无条件提交新 revision。
#[test]
fn fix1d_record_composition_commit_has_set_layout_revision() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn record_composition_commit_transaction", 14000);
    assert!(
        window.contains(".set_layout_revision("),
        "修复后 record_composition_commit_transaction 应调 .set_layout_revision( 无条件提交新 revision。"
    );
}

/// 守卫1e: `record_composition_commit_transaction` 内不再用
/// `let layout_basis_revision = self.pipeline.layout_revision()` 当新事务 basis（旧模式已删除）。
/// 用精确的旧模式字符串匹配，避免误匹配注释中的 `self.pipeline.layout_revision()` 提及。
#[test]
fn fix1e_record_composition_commit_no_old_layout_revision_as_basis() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn record_composition_commit_transaction", 14000);
    assert!(
        !window.contains("let layout_basis_revision = self.pipeline.layout_revision()"),
        "修复后 record_composition_commit_transaction 不应再有 let layout_basis_revision = self.pipeline.layout_revision() 旧模式。"
    );
}

/// 守卫1f: `record_composition_commit_transaction` 用新 helper
/// `build_editor_layout_snapshot_with_canonical` 同时拿 new_snapshot 和 new_canonical。
#[test]
fn fix1f_record_composition_commit_uses_helper_with_canonical() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn record_composition_commit_transaction", 14000);
    assert!(
        window.contains("build_editor_layout_snapshot_with_canonical"),
        "修复后 record_composition_commit_transaction 应调 build_editor_layout_snapshot_with_canonical 同时拿 new_snapshot 和 new_canonical。"
    );
}

// =========================================================================
// 守卫2: build_editor_layout_snapshot_with_canonical helper 存在且被原函数调用
// =========================================================================

/// 守卫2a: `build_editor_layout_snapshot_with_canonical` helper 存在，返回
/// `(EditorLayoutSnapshot, CanonicalDocumentVisualSnapshot)`。
#[test]
fn fix2a_helper_build_editor_layout_snapshot_with_canonical_exists() {
    let src = read_src("src/sujian_editor_item/layout_ops.rs");
    let window = function_window(
        &src,
        "fn build_editor_layout_snapshot_with_canonical",
        8000,
    );
    assert!(
        window.contains("-> ("),
        "修复后 build_editor_layout_snapshot_with_canonical 应返回 tuple。"
    );
    assert!(
        window.contains("EditorLayoutSnapshot"),
        "修复后 build_editor_layout_snapshot_with_canonical 返回类型应含 EditorLayoutSnapshot。"
    );
    assert!(
        window.contains("CanonicalDocumentVisualSnapshot"),
        "修复后 build_editor_layout_snapshot_with_canonical 返回类型应含 CanonicalDocumentVisualSnapshot。"
    );
}

/// 守卫2b: 原 `build_editor_layout_snapshot` 内部调
/// `build_editor_layout_snapshot_with_canonical` 取 `.0`，保留原签名。
#[test]
fn fix2b_build_editor_layout_snapshot_calls_helper() {
    let src = read_src("src/sujian_editor_item/layout_ops.rs");
    // 定位原 build_editor_layout_snapshot（不是 _with_canonical）。
    let marker = "fn build_editor_layout_snapshot(";
    let pos = src
        .find(marker)
        .unwrap_or_else(|| panic!("{} 必须存在", marker));
    // 取原函数体前 600 字符（wrapper 应该很短）。
    let window_end = (pos + 600).min(src.len());
    let window = &src[pos..window_end];
    assert!(
        window.contains("build_editor_layout_snapshot_with_canonical"),
        "修复后 build_editor_layout_snapshot 应调 build_editor_layout_snapshot_with_canonical。"
    );
    assert!(
        window.contains(".0"),
        "修复后 build_editor_layout_snapshot 应取 tuple 的 .0（EditorLayoutSnapshot）。"
    );
}

// =========================================================================
// 守卫3: pipeline.rs 有 set_layout_revision setter
// =========================================================================

/// 守卫3a: `pipeline.rs` 有 `set_layout_revision` setter，供 editing.rs 无条件提交新 revision。
#[test]
fn fix3a_pipeline_has_set_layout_revision_setter() {
    let src = read_src("src/sujian_editor_item/pipeline.rs");
    // 用 pub fn 作为 marker，确保窗口从 pub 开始（包含 pub 修饰符）。
    let window = function_window(&src, "pub fn set_layout_revision", 2000);
    assert!(
        window.contains("pub fn set_layout_revision"),
        "修复后 pipeline.rs 应有 pub fn set_layout_revision setter。"
    );
    assert!(
        window.contains("LayoutRevision"),
        "修复后 set_layout_revision 应接收 LayoutRevision 参数。"
    );
}

// =========================================================================
// 守卫4: 顺序守卫 — reconcile 在 handle_composition_commit_or_cancel 之前
// =========================================================================

/// 守卫4a: `record_composition_commit_transaction` 中
/// `reconcile_active_transactions_with_canonical` 调用在
/// `handle_composition_commit_or_cancel` 调用之前（先 reconcile 旧事务再创建新事务）。
#[test]
fn fix4a_reconcile_before_handle_composition_commit() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn record_composition_commit_transaction", 14000);

    assert!(
        window.contains(RECONCILE_CALL),
        "修复后 record_composition_commit_transaction 应调 .reconcile_active_transactions_with_canonical( 。"
    );
    assert!(
        window.contains(HANDLE_CALL),
        "修复后 record_composition_commit_transaction 应调 .handle_composition_commit_or_cancel( 。"
    );

    let reconcile_pos = window
        .find(RECONCILE_CALL)
        .expect("reconcile 调用已确认存在");
    let handle_pos = window.find(HANDLE_CALL).expect("handle 调用已确认存在");

    assert!(
        reconcile_pos < handle_pos,
        "修复后 .reconcile_active_transactions_with_canonical( 调用 ({}) 必须在 .handle_composition_commit_or_cancel( 调用 ({}) 之前，\
         保证旧活动事务先被重绑到新 canonical，再创建新事务。",
        reconcile_pos, handle_pos
    );
}

/// 守卫4b: `record_composition_commit_transaction` 中
/// `set_layout_revision` / `set_current_canonical_snapshot` 调用在
/// `handle_composition_commit_or_cancel` 之后或同一次提交（先创建事务再提升 canonical）。
#[test]
fn fix4b_set_layout_revision_after_handle_composition_commit() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn record_composition_commit_transaction", 14000);

    let set_rev_marker = ".set_layout_revision(";
    let set_canonical_marker = ".set_current_canonical_snapshot(";

    let handle_pos = window
        .find(HANDLE_CALL)
        .expect("handle 调用已确认存在");
    let set_rev_pos = window
        .find(set_rev_marker)
        .expect("set_layout_revision 调用已确认存在");
    let set_canonical_pos = window
        .find(set_canonical_marker)
        .expect("set_current_canonical_snapshot 调用已确认存在");

    assert!(
        handle_pos < set_rev_pos,
        "修复后 .set_layout_revision( 调用 ({}) 必须在 .handle_composition_commit_or_cancel( 调用 ({}) 之后，\
         先创建新事务再提升 canonical revision。",
        set_rev_pos, handle_pos
    );
    assert!(
        handle_pos < set_canonical_pos,
        "修复后 .set_current_canonical_snapshot( 调用 ({}) 必须在 .handle_composition_commit_or_cancel( 调用 ({}) 之后，\
         先创建新事务再提交 canonical identity。",
        set_canonical_pos, handle_pos
    );
}

/// 守卫4c: `record_composition_commit_transaction` 用统一 `edit_now` 采样
///（与普通正文路径 prepare_edit_motion 行 1398 一致）。
#[test]
fn fix4c_unified_edit_now_in_record_composition_commit() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let window = function_window(&src, "fn record_composition_commit_transaction", 14000);
    assert!(
        window.contains("let edit_now = std::time::Instant::now();"),
        "修复后 record_composition_commit_transaction 应有 let edit_now = std::time::Instant::now(); 统一时间采样。"
    );
    // edit_now 传给 reconcile_active_transactions_with_canonical。
    assert!(
        window.contains("new_revision,\n                edit_now,\n            );"),
        "修复后 edit_now 应传给 reconcile_active_transactions_with_canonical。"
    );
}
