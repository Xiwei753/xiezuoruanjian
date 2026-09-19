//! Issue #710 评论 5735006606 零长度 range 段落扩展修复的 WHITE_BOX 守卫测试。
//!
//! 这个测试文件最初在 reproduction 阶段确认 bug 存在（零长度 range 不生成动画视觉）。
//! 修复后（评论 5735006606 方案：用 compute_affected_paragraph_ranges 扩展段落边界），
//! 测试 3/4 的断言已更新为确认 bug 已修复。
//!
//! 它通过读取源代码字符串，验证：
//! 1. build_editor_layout_snapshot 用 `if affected_start < affected_end` 作为动画视觉提取开关
//!    （开关逻辑本身不变，修复在调用方扩展 range 使其不再零长度）；
//! 2. record_composition_commit_transaction 用 compute_affected_paragraph_ranges 扩展后传给 new_snapshot；
//! 3. input_clear_preedit 用 compute_affected_paragraph_ranges 扩展后传给 new_snapshot；
//! 4. ime_replace_and_insert 在 inserted.is_empty() 时产生零长度 candidate range（零长度来源仍在）；
//! 5. session_replace_range 无 session 时返回零长度 (cursor, cursor)（零长度来源仍在）。
//!
//! 修复后零长度 raw range 会被扩展到所在段落边界，不再是零长度，
//! 从而让 `if affected_start < affected_end` 为 true，生成动画视觉资源。

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
// 测试 1: build_editor_layout_snapshot 用 `if affected_start < affected_end`
//        作为动画视觉提取开关（开关逻辑不变，修复在调用方扩展 range）
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_comment5735006606_build_snapshot_skips_visual_when_zero_length_range() {
    let src = read_src("src/sujian_editor_item/layout_ops.rs");
    let body = method_body(&src, "fn build_editor_layout_snapshot(");

    // composition_range 解包为 (affected_start, affected_end)
    assert!(
        body.contains("let (affected_start, affected_end) = composition_range.unwrap_or((0, 0));"),
        "build_editor_layout_snapshot 必须把 composition_range 解包为 affected_start/affected_end"
    );
    // 动画视觉提取用 `if affected_start < affected_end` 作为开关
    // 修复后调用方不再传零长度 range（先扩展到段落边界），此开关为 true
    assert!(
        body.contains("if affected_start < affected_end {"),
        "build_editor_layout_snapshot 必须用 `if affected_start < affected_end` 作为动画视觉提取开关"
    );
    println!(
        "[ISSUE710_COMMENT5735006606] build_editor_layout_snapshot 动画视觉提取开关保留 (GUARDED)"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 测试 2: build_virtual_layout_snapshot 同样用 `if affected_start < affected_end`
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_comment5735006606_build_virtual_snapshot_skips_visual_when_zero_length_range() {
    let src = read_src("src/sujian_editor_item/layout_ops.rs");
    let body = method_body(&src, "fn build_virtual_layout_snapshot(");

    assert!(
        body.contains("let (affected_start, affected_end) = composition_range.unwrap_or((0, 0));"),
        "build_virtual_layout_snapshot 必须把 composition_range 解包为 affected_start/affected_end"
    );
    assert!(
        body.contains("if affected_start < affected_end {"),
        "build_virtual_layout_snapshot 必须用 `if affected_start < affected_end` 作为动画视觉提取开关"
    );
    println!(
        "[ISSUE710_COMMENT5735006606] build_virtual_layout_snapshot 动画视觉提取开关保留 (GUARDED)"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 测试 3: record_composition_commit_transaction 用 compute_affected_paragraph_ranges
//        扩展后传给 new_snapshot → 零长度 candidate range 会被扩成段落范围 (FIXED)
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_comment5735006606_commit_expands_candidate_range_before_new_snapshot() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let body = method_body(&src, "fn record_composition_commit_transaction(");

    // 必须引入 compute_affected_paragraph_ranges 段落扩展
    assert!(
        body.contains("compute_affected_paragraph_ranges("),
        "record_composition_commit_transaction 必须调用 compute_affected_paragraph_ranges 扩展段落边界 (FIXED)"
    );
    // new_composition_range 用扩展后的 new_affected range，不再直接用 candidate_byte_start/end
    assert!(
        body.contains("let new_composition_range = Some((new_affected_start, new_affected_end));"),
        "record_composition_commit_transaction 必须把扩展后的 new_affected range 作为 new_composition_range (FIXED)"
    );
    // new_snapshot 用 new_composition_range
    assert!(
        body.contains("build_editor_layout_snapshot(width, true, new_composition_range)"),
        "record_composition_commit_transaction 必须把 new_composition_range 传给 new_snapshot"
    );
    // raw candidate range 不能直接作为 composition_range
    assert!(
        !body.contains("Some((candidate_byte_start, candidate_byte_end))"),
        "record_composition_commit_transaction 不能把 raw candidate range 直接传给 snapshot（必须先扩展）(FIXED)"
    );
    println!("[ISSUE710_COMMENT5735006606] commit 用段落扩展后的 range 传给 new_snapshot (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 测试 4: input_clear_preedit 用 compute_affected_paragraph_ranges
//        扩展后传给 new_snapshot → 零长度 session_replace_range 会被扩成段落范围 (FIXED)
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_comment5735006606_cancel_expands_session_replace_range_before_new_snapshot() {
    let src = read_src("src/sujian_editor_item/input_host.rs");
    let body = method_body(&src, "fn input_clear_preedit(");

    // 取 session_replace_range
    assert!(
        body.contains("session_replace_range(self.buffer.cursor)"),
        "input_clear_preedit 必须取 session_replace_range 作为 committed_replace_start/end"
    );
    // 必须引入 compute_affected_paragraph_ranges 段落扩展
    assert!(
        body.contains("compute_affected_paragraph_ranges("),
        "input_clear_preedit 必须调用 compute_affected_paragraph_ranges 扩展段落边界 (FIXED)"
    );
    // new_snapshot 用扩展后的 new_affected range，不再直接用 committed_replace_start/end
    assert!(
        body.contains("Some((new_affected_start, new_affected_end))"),
        "input_clear_preedit 必须把扩展后的 new_affected range 传给 new_snapshot (FIXED)"
    );
    // raw committed_replace range 不能直接作为 composition_range
    assert!(
        !body.contains("Some((committed_replace_start, committed_replace_end))"),
        "input_clear_preedit 不能把 raw committed_replace range 直接传给 snapshot（必须先扩展）(FIXED)"
    );
    println!("[ISSUE710_COMMENT5735006606] cancel 用段落扩展后的 range 传给 new_snapshot (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 测试 5: ime_replace_and_insert 在 inserted.is_empty() 时产生零长度 candidate range
//        → 场景2 的零长度来源（来源不变，修复在下游扩展）
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_comment5735006606_ime_replace_empty_inserted_produces_zero_length_candidate() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let body = method_body(&src, "fn ime_replace_and_insert(");

    // candidate_byte_end = rep_start + inserted.len()  →  inserted.is_empty() 时 == rep_start
    assert!(
        body.contains("let candidate_byte_start = rep_start;"),
        "ime_replace_and_insert 必须有 candidate_byte_start = rep_start"
    );
    assert!(
        body.contains("let candidate_byte_end = rep_start + inserted.len();"),
        "ime_replace_and_insert 必须有 candidate_byte_end = rep_start + inserted.len() \
         （inserted.is_empty() 时 candidate_byte_end == candidate_byte_start 零长度，\
         修复前直接传给 snapshot 触发 bug，修复后被 compute_affected_paragraph_ranges 扩展）"
    );
    // 允许空 commit + replacement 进入（不提前 return）
    assert!(
        body.contains("!event.has_any_deletion() && inserted.is_empty()"),
        "ime_replace_and_insert 必须只在既无删除又无插入时 return，允许空 commit + replacement 进入"
    );
    println!("[ISSUE710_COMMENT5735006606] ime_replace_and_insert 空 inserted 产生零长度 candidate range（来源保留，下游扩展修复）(GUARDED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 测试 6: session_replace_range 无 session 时返回零长度 (cursor, cursor)
//        → 场景1 的零长度来源（来源不变，修复在下游扩展）
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_comment5735006606_session_replace_range_fallback_is_zero_length() {
    let src = read_src("src/sujian_editor_item/pipeline.rs");
    let body = method_body(&src, "fn session_replace_range(");

    // 无 session 时 fallback 为 (fallback_cursor, fallback_cursor) 零长度
    assert!(
        body.contains(".unwrap_or((fallback_cursor, fallback_cursor))"),
        "session_replace_range 无 session 时必须返回 (fallback_cursor, fallback_cursor) 零长度 \
         （场景1 普通 ESC/cancel 无 selection 时产生零长度，\
         修复前直接传给 snapshot 触发 bug，修复后被 compute_affected_paragraph_ranges 扩展）"
    );
    println!("[ISSUE710_COMMENT5735006606] session_replace_range 无 session 时返回零长度 (cursor, cursor)（来源保留，下游扩展修复）(GUARDED)");
}
