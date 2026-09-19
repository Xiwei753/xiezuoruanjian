//! Issue #710 评论 5735006606 snapshot 视觉范围扩展守卫。
//!
//! WHITE_BOX 验证：composition commit/cancel 在构建 layout snapshot 前，
//! 用 compute_affected_paragraph_ranges 把 raw edit range 扩展到所在段落边界，
//! 使零长度 range（普通 ESC/cancel 的 (cursor, cursor) 或空 commit + replacement
//! 纯删除的 candidate_byte_start == candidate_byte_end）也能扩成所在段落的
//! 非空视觉范围，从而让 build_editor_layout_snapshot 的
//! `if affected_start < affected_end` 分支生成动画视觉资源。
//!
//! 本测试聚焦评论 5735006606 特有的守卫：
//! 1. commit/cancel 都调用 compute_affected_paragraph_ranges；
//! 2. 扩展用的 old_text/new_text 参数正确：
//!    - commit: old_text = saved_virtual_text, new_text = &new.text
//!    - cancel: old_text = session virtual_text（fallback buffer.text.clone()）,
//!              new_text = &self.buffer.text
//! 3. cancel 路径的 old_virtual_text 取法正确（从 composition_session 取）。

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
// 测试 1: commit 路径调用 compute_affected_paragraph_ranges 且文本参数正确
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_commit_snapshot_visual_range_expanded_with_correct_texts() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let body = method_body(&src, "fn record_composition_commit_transaction(");

    // 必须调用 compute_affected_paragraph_ranges
    assert!(
        body.contains("crate::editor::layout::compute_affected_paragraph_ranges("),
        "commit: 必须调用 compute_affected_paragraph_ranges 扩展段落范围"
    );

    // 取出 compute_affected_paragraph_ranges 调用块，验证文本参数
    let call_start = body
        .find("compute_affected_paragraph_ranges(")
        .expect("commit: compute_affected_paragraph_ranges 调用必须存在");
    let call_rest = &body[call_start..];
    // 找到匹配的右括号：compute_affected_paragraph_ranges( ... )
    // 调用参数跨多行，以 `);` 结束
    let call_end = call_rest
        .find(");")
        .expect("commit: compute_affected_paragraph_ranges 调用必须结束");
    let call_block = &call_rest[..call_end];

    // old_text 参数 = saved_virtual_text（old virtualText，preedit 所在文本）
    assert!(
        call_block.contains("saved_virtual_text,"),
        "commit: compute_affected_paragraph_ranges 的 old_text 必须是 saved_virtual_text（old virtualText）"
    );
    // new_text 参数 = &new.text（new committed text）
    assert!(
        call_block.contains("&new.text,"),
        "commit: compute_affected_paragraph_ranges 的 new_text 必须是 &new.text（new committed text）"
    );

    // old/new composition_range 用扩展后的 affected range
    assert!(
        body.contains(
            "let (old_affected_start, old_affected_end, new_affected_start, new_affected_end) ="
        ),
        "commit: 必须解构 compute_affected_paragraph_ranges 的返回值为 old/new affected range"
    );
    assert!(
        body.contains("let old_composition_range = Some((old_affected_start, old_affected_end));"),
        "commit: old_composition_range 必须用扩展后的 old_affected range"
    );
    assert!(
        body.contains("let new_composition_range = Some((new_affected_start, new_affected_end));"),
        "commit: new_composition_range 必须用扩展后的 new_affected range"
    );

    // 必须有评论 5735006606 的注释说明（文档化守卫）
    assert!(
        body.contains("Issue #710 评论 5735006606"),
        "commit: 必须有 Issue #710 评论 5735006606 的注释说明"
    );

    println!("[ISSUE710_COMMENT5735006606] commit snapshot 视觉范围扩展 + 文本参数正确 (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 测试 2: cancel 路径调用 compute_affected_paragraph_ranges 且文本参数正确
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_cancel_snapshot_visual_range_expanded_with_correct_texts() {
    let src = read_src("src/sujian_editor_item/input_host.rs");
    let body = method_body(&src, "fn input_clear_preedit(");

    // 必须调用 compute_affected_paragraph_ranges
    assert!(
        body.contains("crate::editor::layout::compute_affected_paragraph_ranges("),
        "cancel: 必须调用 compute_affected_paragraph_ranges 扩展段落范围"
    );

    // cancel 路径的 old_virtual_text 必须从 composition_session 取 virtual_text，
    // fallback 到 buffer.text.clone()
    assert!(
        body.contains("let old_virtual_text = self"),
        "cancel: 必须定义 old_virtual_text 变量"
    );
    assert!(
        body.contains(".composition_session"),
        "cancel: old_virtual_text 必须从 composition_session 取"
    );
    assert!(
        body.contains(".as_ref()"),
        "cancel: composition_session 必须用 as_ref() 取引用"
    );
    assert!(
        body.contains(".map(|s| s.virtual_text())"),
        "cancel: 必须用 session.virtual_text() 取 old virtualText"
    );
    assert!(
        body.contains(".unwrap_or_else(|| self.buffer.text.clone())"),
        "cancel: old_virtual_text 的 fallback 必须是 self.buffer.text.clone()"
    );

    // 取出 compute_affected_paragraph_ranges 调用块，验证文本参数
    let call_start = body
        .find("compute_affected_paragraph_ranges(")
        .expect("cancel: compute_affected_paragraph_ranges 调用必须存在");
    let call_rest = &body[call_start..];
    let call_end = call_rest
        .find(");")
        .expect("cancel: compute_affected_paragraph_ranges 调用必须结束");
    let call_block = &call_rest[..call_end];

    // old_text 参数 = &old_virtual_text（session virtual_text）
    assert!(
        call_block.contains("&old_virtual_text,"),
        "cancel: compute_affected_paragraph_ranges 的 old_text 必须是 &old_virtual_text（session virtual_text）"
    );
    // new_text 参数 = &self.buffer.text（cancel 恢复原文）
    assert!(
        call_block.contains("&self.buffer.text,"),
        "cancel: compute_affected_paragraph_ranges 的 new_text 必须是 &self.buffer.text（cancel 恢复原文）"
    );

    // old/new snapshot 用扩展后的 affected range
    assert!(
        body.contains(
            "let (old_affected_start, old_affected_end, new_affected_start, new_affected_end) ="
        ),
        "cancel: 必须解构 compute_affected_paragraph_ranges 的返回值为 old/new affected range"
    );
    assert!(
        body.contains("Some((old_affected_start, old_affected_end))"),
        "cancel: old_snapshot fallback 必须用扩展后的 old_affected range"
    );
    assert!(
        body.contains("Some((new_affected_start, new_affected_end))"),
        "cancel: new_snapshot 必须用扩展后的 new_affected range"
    );

    // 必须有评论 5735006606 的注释说明（文档化守卫）
    assert!(
        body.contains("Issue #710 评论 5735006606"),
        "cancel: 必须有 Issue #710 评论 5735006606 的注释说明"
    );

    println!("[ISSUE710_COMMENT5735006606] cancel snapshot 视觉范围扩展 + 文本参数正确 (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 测试 3: coordinator 的 raw range 参数保留不动（职责不冲突守卫）
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_coordinator_raw_range_params_preserved() {
    // coordinator 里自己计算的 visual_affected_byte_range_old/new 保留不动，
    // 这里解决的是构建 snapshot 时生成哪些动画视觉资源，两者职责不冲突。
    let coord = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let body = method_body(&coord, "pub fn handle_composition_commit_or_cancel(");

    // coordinator 仍然接收 raw preedit/candidate/committed_replace 参数
    assert!(
        body.contains("preedit_byte_start") && body.contains("preedit_byte_end"),
        "coordinator: 必须保留 raw preedit_byte_start/end 参数"
    );
    assert!(
        body.contains("candidate_byte_start") && body.contains("candidate_byte_end"),
        "coordinator: 必须保留 raw candidate_byte_start/end 参数"
    );
    assert!(
        body.contains("committed_replace_start") && body.contains("committed_replace_end"),
        "coordinator: 必须保留 raw committed_replace_start/end 参数"
    );

    println!("[ISSUE710_COMMENT5735006606] coordinator raw range 参数保留不动 (GUARDED)");
}
