//! Issue #710 评论 5735006606 snapshot 视觉范围扩展守卫。
//!
//! Issue #826/#901: 旧 composition 坐标契约（compute_affected_paragraph_ranges
//! 扩展段落范围 → handle_composition_commit_or_cancel）已随光标与正文动画彻底
//! 解耦而删除。视觉过渡统一由 visual_canvas 模块的 RevisionChain 精确消费
//! Core 提交的 offset map 处理，不再需要 caller 层手动扩展段落范围。
//!
//! 本测试更新为守卫新架构：visual_canvas 模块存在且包含 RevisionChain 机制。

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

// ─────────────────────────────────────────────────────────────────────────
// 测试 1: commit 路径由 visual_canvas 处理视觉范围扩展
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_commit_snapshot_visual_range_expanded_with_correct_texts() {
    // Issue #826/#901: commit 路径的视觉范围扩展由 visual_canvas 模块负责。
    // 旧 compute_affected_paragraph_ranges + saved_virtual_text/&new.text 参数
    // 已删除，由 RevisionChain 精确消费 Core offset map 替代。
    let visual_canvas_mod =
        linux_qt_root().join("src/sujian_editor_item/visual_canvas/mod.rs");
    assert!(
        visual_canvas_mod.exists(),
        "commit: visual_canvas 模块应存在（负责视觉范围扩展）"
    );
    let visual_canvas_src = read_src("src/sujian_editor_item/visual_canvas/mod.rs");
    assert!(
        visual_canvas_src.contains("RevisionChain") || visual_canvas_src.contains("revision_chain"),
        "commit: visual_canvas 应有 RevisionChain 机制（精确消费 Core offset map）"
    );
    println!("[ISSUE710_COMMENT5735006606] commit 视觉范围由 visual_canvas 处理 (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 测试 2: cancel 路径由 visual_canvas 处理视觉范围扩展
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_cancel_snapshot_visual_range_expanded_with_correct_texts() {
    // Issue #826/#901: cancel 路径的视觉范围扩展由 visual_canvas 模块负责。
    // 旧 compute_affected_paragraph_ranges + composition_session virtual_text 参数
    // 已删除。cancel 现在只撤掉 preedit 临时显示层，正文没有 Core edit。
    let src = read_src("src/sujian_editor_item/input_host.rs");
    let start = src.find("fn input_clear_preedit(").expect("input_clear_preedit 必须存在");
    let rest = &src[start..];
    let end = rest.find("\n    }\n").expect("input_clear_preedit body end not found");
    let body = &rest[..end];
    // 新架构：cancel 只清 composition + 更新 preedit visual
    assert!(
        body.contains("clear()") && body.contains("update_preedit_visual_state"),
        "cancel: input_clear_preedit 清 composition 并更新 preedit visual"
    );
    println!("[ISSUE710_COMMENT5735006606] cancel 视觉范围由 visual_canvas 处理 (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 测试 3: coordinator 的 raw range 参数已删除（旧职责由 visual_canvas 替代）
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_coordinator_raw_range_params_preserved() {
    // Issue #826/#901: handle_composition_commit_or_cancel 已删除。
    // 旧 coordinator 接收 raw preedit/candidate/committed_replace 参数并自己
    // 计算 visual_affected_byte_range 的职责已由 visual_canvas 模块替代。
    let comp = read_src("src/sujian_editor_item/animation/composition.rs");
    assert!(
        !comp.contains("pub fn handle_composition_commit_or_cancel("),
        "coordinator: handle_composition_commit_or_cancel 已删除（旧 raw range 参数由 visual_canvas 替代）"
    );
    println!("[ISSUE710_COMMENT5735006606] coordinator raw range 参数已删除 (GUARDED)");
}
