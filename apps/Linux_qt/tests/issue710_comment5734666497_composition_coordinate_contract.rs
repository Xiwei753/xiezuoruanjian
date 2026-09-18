//! Issue #710 评论 5734666497 调用层坐标契约守卫。
//!
//! WHITE_BOX 验证：composition commit/cancel 调用层把 old/new 坐标正确分开。
//! - commit: new_snapshot 用 candidate range（new committed text 坐标），不用 old preedit range；
//! - cancel: new_snapshot 和 committed_replace 参数用 session_replace_range，不用 preedit range。

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

/// 取出某个方法从签名到函数体结束之间的文本。
fn method_body(src: &str, signature: &str) -> String {
    let start = src
        .find(signature)
        .unwrap_or_else(|| panic!("method `{}` must exist", signature));
    let rest = &src[start..];
    let end = rest
        .find("\n    }\n")
        .unwrap_or_else(|| panic!("method `{}` body end not found", signature));
    rest[..end].to_string()
}

// ─────────────────────────────────────────────────────────────────────────
// 测试 1: commit 的 new_snapshot 用 candidate range，不用 old preedit range
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_commit_new_snapshot_uses_candidate_range_not_preedit_range() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let body = method_body(&src, "fn record_composition_commit_transaction(");

    // 必须拆成 old/new 两个 composition range 变量
    assert!(
        body.contains("old_composition_range") && body.contains("new_composition_range"),
        "commit: 必须拆分 old_composition_range 和 new_composition_range"
    );
    // old_composition_range 用 preedit 坐标
    assert!(
        body.contains("let old_composition_range = Some((preedit_byte_start, preedit_byte_end));"),
        "commit: old_composition_range 必须用 preedit_byte_start/end"
    );
    // new_composition_range 用 candidate 坐标
    assert!(
        body.contains("let new_composition_range = Some((candidate_byte_start, candidate_byte_end));"),
        "commit: new_composition_range 必须用 candidate_byte_start/end"
    );
    // new_snapshot 用 new_composition_range
    assert!(
        body.contains("build_editor_layout_snapshot(width, true, new_composition_range)"),
        "commit: new_snapshot 必须用 new_composition_range（candidate 坐标），不能用 old preedit range"
    );
    // old_snapshot fallback 用 old_composition_range；new_snapshot 不能用 old_composition_range
    assert!(
        body.contains("old_composition_range,"),
        "commit: old_snapshot fallback 必须用 old_composition_range"
    );
    assert!(
        !body.contains("build_editor_layout_snapshot(width, true, old_composition_range)"),
        "commit: new_snapshot 不能用 old_composition_range（那会是 old preedit 坐标系）"
    );
    println!("[ISSUE710_COMMENT5734666497] commit new_snapshot 用 candidate range (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 测试 2: cancel 的 new_snapshot 和 committed_replace 用 session_replace_range
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_cancel_new_snapshot_uses_session_replace_range_not_preedit_range() {
    let src = read_src("src/sujian_editor_item/input_host.rs");
    let body = method_body(&src, "fn input_clear_preedit(");

    // 必须在清 session 之前取 session_replace_range
    assert!(
        body.contains("session_replace_range(self.buffer.cursor)"),
        "cancel: 必须在清 session 之前调 session_replace_range 取 committed replace range"
    );
    assert!(
        body.contains("committed_replace_start") && body.contains("committed_replace_end"),
        "cancel: 必须有 committed_replace_start/end 变量"
    );
    // new_snapshot 用 committed_replace range，不用 composition_byte range
    assert!(
        body.contains("Some((committed_replace_start, committed_replace_end))"),
        "cancel: new_snapshot 必须用 committed_replace_start/end，不能用 composition_byte_start/end"
    );
    // composition_byte range 只能出现在 old_snapshot fallback 一处，不能出现在 new_snapshot
    // 用 count==1 守卫：若 new_snapshot 被改回用 composition_byte range，count 会变成 2
    assert_eq!(
        body.matches("Some((composition_byte_start, composition_byte_end))").count(),
        1,
        "cancel: composition_byte range 只能出现在 old_snapshot fallback 一处，不能出现在 new_snapshot"
    );
    // handle_composition_commit_or_cancel 的 committed_replace 参数用真正的值
    // 找到 handle_composition_commit_or_cancel 调用块
    let call_start = body
        .find("handle_composition_commit_or_cancel(")
        .expect("cancel: 必须调用 handle_composition_commit_or_cancel");
    let call_rest = &body[call_start..];
    let call_end = call_rest
        .find(");")
        .expect("cancel: handle_composition_commit_or_cancel 调用必须结束");
    let call_block = &call_rest[..call_end];
    assert!(
        call_block.contains("committed_replace_start,")
            && call_block.contains("committed_replace_end,"),
        "cancel: handle_composition_commit_or_cancel 必须传 committed_replace_start/end 作为 committed_replace 参数"
    );
    println!("[ISSUE710_COMMENT5734666497] cancel new_snapshot 和 committed_replace 用 session_replace_range (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 测试 3: 坐标契约文档化守卫（caller → snapshot → coordinator 一致性）
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_caller_snapshot_coordinator_coordinate_contract_documented() {
    // editing.rs: commit 路径 old=preedit, new=candidate
    let editing = read_src("src/sujian_editor_item/editing.rs");
    assert!(
        editing.contains("Issue #710 评论 5734666497: old/new snapshot 的 composition range 分属不同坐标系"),
        "editing.rs: 必须有坐标系分离的注释说明"
    );
    // input_host.rs: cancel 路径 old=preedit, new=committed_replace
    let input_host = read_src("src/sujian_editor_item/input_host.rs");
    assert!(
        input_host.contains("Issue #710 评论 5734666497: cancel 的 new-side 受影响范围是原 session replace range"),
        "input_host.rs: 必须有 cancel new-side range 的注释说明"
    );
    // 协调器内部已分清 old/new（上一轮已修，这里只做守卫确保不回退）
    let coord = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let commit_cancel_body = method_body(&coord, "pub fn handle_composition_commit_or_cancel(");
    assert!(
        commit_cancel_body.contains("is_commit")
            && commit_cancel_body.contains("candidate_byte_start")
            && commit_cancel_body.contains("committed_replace_start"),
        "coordinator: handle_composition_commit_or_cancel 必须保留 is_commit/candidate/committed_replace 参数分离"
    );
    assert!(
        commit_cancel_body.contains("let new_edit_range = if is_commit {"),
        "coordinator: commit 用 candidate range，cancel 用 committed_replace range 的分支必须保留"
    );
    println!("[ISSUE710_COMMENT5734666497] caller→snapshot→coordinator 坐标契约一致 (GUARDED)");
}
