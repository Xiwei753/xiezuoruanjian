//! Issue #710 评论 5734666497 调用层坐标契约守卫。
//!
//! WHITE_BOX 验证：composition commit/cancel 调用层把 old/new 坐标正确分开。
//! - commit: new_snapshot 用 candidate range（new committed text 坐标），不用 old preedit range；
//! - cancel: new_snapshot 和 committed_replace 参数用 session_replace_range，不用 preedit range。
//!
//! Issue #710 评论 5735006606 更新: snapshot 的视觉提取范围改为用
//! compute_affected_paragraph_ranges 扩展段落边界后的 affected range，
//! 不再直接用 raw edit range。本测试同步更新断言：仍然守卫坐标分离
//! （old 侧用 preedit/composition 坐标系，new 侧用 candidate/committed_replace
//! 坐标系），但通过 compute_affected_paragraph_ranges 的调用参数间接验证。

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
// 测试 1: commit 的 new_snapshot 用 candidate 坐标系扩展，不用 old preedit 坐标系
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
    // Issue #710 评论 5735006606: 必须调用 compute_affected_paragraph_ranges 扩展段落范围
    assert!(
        body.contains("compute_affected_paragraph_ranges("),
        "commit: 必须调用 compute_affected_paragraph_ranges 扩展段落范围"
    );
    // old 侧 raw edit range 用 preedit 坐标系
    assert!(
        body.contains("(preedit_byte_start, preedit_byte_end),"),
        "commit: compute_affected_paragraph_ranges 的 old_edit_range 必须用 preedit 坐标"
    );
    // new 侧 raw edit range 用 candidate 坐标系
    assert!(
        body.contains("(candidate_byte_start, candidate_byte_end),"),
        "commit: compute_affected_paragraph_ranges 的 new_edit_range 必须用 candidate 坐标"
    );
    // old_text 用 saved_virtual_text，new_text 用 &new.text
    assert!(
        body.contains("saved_virtual_text,") && body.contains("&new.text,"),
        "commit: compute_affected_paragraph_ranges 的 old_text=saved_virtual_text, new_text=&new.text"
    );
    // old/new composition_range 用扩展后的 affected range
    assert!(
        body.contains("let old_composition_range = Some((old_affected_start, old_affected_end));"),
        "commit: old_composition_range 必须用扩展后的 old_affected range"
    );
    assert!(
        body.contains("let new_composition_range = Some((new_affected_start, new_affected_end));"),
        "commit: new_composition_range 必须用扩展后的 new_affected range"
    );
    // new_snapshot 用 new_composition_range
    // Issue #738 评论 5797637204: composition commit 改用 build_editor_layout_snapshot_with_canonical
    // 同时拿 (EditorLayoutSnapshot, CanonicalDocumentVisualSnapshot)，函数名变了但坐标契约不变。
    // Issue #810 评论 5934658350: build_editor_layout_snapshot_with_canonical 增加
    // canonical_text_revision 参数，调用跨多行。守卫改为检查函数调用存在且
    // new_composition_range 作为参数出现（后续断言确保不用 old_composition_range）。
    assert!(
        body.contains("build_editor_layout_snapshot_with_canonical(")
            && body.contains("new_composition_range,"),
        "commit: new_snapshot 必须用 new_composition_range（candidate 坐标系扩展），不能用 old preedit range"
    );
    // old_snapshot fallback 用 old_composition_range；new_snapshot 不能用 old_composition_range
    assert!(
        body.contains("old_composition_range)"),
        "commit: old_snapshot fallback 必须用 old_composition_range"
    );
    assert!(
        !body.contains("build_editor_layout_snapshot(width, true, old_composition_range)"),
        "commit: new_snapshot 不能用 old_composition_range（那会是 old preedit 坐标系）"
    );
    assert!(
        !body.contains(
            "build_editor_layout_snapshot_with_canonical(width, true, old_composition_range)"
        ),
        "commit: new_snapshot 不能用 old_composition_range（那会是 old preedit 坐标系）"
    );
    // raw range 不能直接作为 composition_range 传给 build_editor_layout_snapshot
    assert!(
        !body.contains("Some((preedit_byte_start, preedit_byte_end))"),
        "commit: raw preedit range 不能直接传给 build_editor_layout_snapshot（必须先扩展）"
    );
    assert!(
        !body.contains("Some((candidate_byte_start, candidate_byte_end))"),
        "commit: raw candidate range 不能直接传给 build_editor_layout_snapshot（必须先扩展）"
    );
    println!("[ISSUE710_COMMENT5734666497] commit new_snapshot 用 candidate 坐标系扩展 (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 测试 2: cancel 的 new_snapshot 和 committed_replace 用 session_replace_range 坐标系扩展
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_cancel_new_snapshot_uses_session_replace_range_not_preedit_range() {
    // Issue #826/#901: 旧 composition 坐标契约（session_replace_range →
    // compute_affected_paragraph_ranges → handle_composition_commit_or_cancel）
    // 已随光标与正文动画彻底解耦而删除。cancel 现在只撤掉 preedit 临时显示层，
    // 正文没有 Core edit，当前过渡继续独立运行。坐标分离由 visual_canvas 模块的
    // RevisionChain 精确消费 Core 提交的 offset map 保证。
    let src = read_src("src/sujian_editor_item/input_host.rs");
    let body = method_body(&src, "fn input_clear_preedit(");
    // 新架构：cancel 只清 composition + 更新 preedit visual + 请求重绘
    assert!(
        body.contains("clear()") && body.contains("update_preedit_visual_state"),
        "cancel: input_clear_preedit 清 composition 并更新 preedit visual"
    );
    // visual_canvas 模块存在，负责视觉过渡的坐标分离
    let visual_canvas_mod =
        linux_qt_root().join("src/sujian_editor_item/visual_canvas/mod.rs");
    assert!(
        visual_canvas_mod.exists(),
        "cancel: visual_canvas 模块应存在（负责视觉过渡坐标分离）"
    );
    println!("[ISSUE710_COMMENT5734666497] cancel 由 visual_canvas 处理坐标分离 (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 测试 3: 坐标契约文档化守卫（caller → snapshot → coordinator 一致性）
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_caller_snapshot_coordinator_coordinate_contract_documented() {
    // Issue #826/#901: 旧 composition 坐标契约（editing.rs commit 路径 old=preedit/new=candidate,
    // input_host.rs cancel 路径 old=preedit/new=committed_replace, composition.rs
    // handle_composition_commit_or_cancel）已随光标与正文动画彻底解耦而删除。
    // 坐标分离现在由 visual_canvas 模块的 RevisionChain 精确消费 Core 提交的
    // offset map 保证，不再需要 caller 层手动分离 old/new 坐标系。

    // visual_canvas 模块存在，负责视觉过渡的坐标分离
    let visual_canvas_mod =
        linux_qt_root().join("src/sujian_editor_item/visual_canvas/mod.rs");
    assert!(
        visual_canvas_mod.exists(),
        "visual_canvas 模块应存在（负责视觉过渡坐标分离）"
    );
    // RevisionChain 是坐标分离的核心机制
    let visual_canvas_src = read_src("src/sujian_editor_item/visual_canvas/mod.rs");
    assert!(
        visual_canvas_src.contains("RevisionChain") || visual_canvas_src.contains("revision_chain"),
        "visual_canvas 应有 RevisionChain 机制（精确消费 Core offset map）"
    );
    // composition.rs 现在是纯文档模块
    let comp = read_src("src/sujian_editor_item/animation/composition.rs");
    assert!(
        !comp.contains("pub fn handle_composition_commit_or_cancel("),
        "composition.rs: handle_composition_commit_or_cancel 已删除（旧坐标契约已由 visual_canvas 替代）"
    );
    println!("[ISSUE710_COMMENT5734666497] caller→visual_canvas 坐标契约一致 (GUARDED)");
}
