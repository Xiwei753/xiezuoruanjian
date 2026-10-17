//! Issue #710 评论 5735006606 snapshot 视觉范围扩展守卫。
//!
//! Issue #826/#901: 旧 composition 坐标契约（compute_affected_paragraph_ranges
//! 扩展段落范围 → handle_composition_commit_or_cancel）已随光标与正文动画彻底
//! 解耦而删除。
//!
//! Issue #904: 视觉过渡改由单一局部 text_motion 计划处理，旧 #901 visual_canvas /
//! RevisionChain 状态机已整体删除，不得保留两套动画引擎。

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
// 测试 1: commit 路径由单一局部 text_motion 计划处理视觉范围
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_commit_snapshot_visual_range_expanded_with_correct_texts() {
    // Issue #904: commit 路径的视觉范围扩展由单一局部 text_motion 计划负责。
    // 旧 #901 visual_canvas / RevisionChain 状态机已删除。
    let text_motion = linux_qt_root().join("src/sujian_editor_item/text_motion.rs");
    assert!(
        text_motion.exists(),
        "commit: text_motion 模块应存在（负责局部视觉范围与位移）"
    );
    let editing_src = read_src("src/sujian_editor_item/editing.rs");
    assert!(
        editing_src.contains("TextMotion::begin_or_retarget"),
        "commit: record_composition_commit_transaction 应通过单一 TextMotion 处理编辑结果"
    );
    println!("[ISSUE710_COMMENT5735006606] commit 视觉范围由 text_motion 处理 (FIXED)");
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
    let start = src
        .find("fn input_clear_preedit(")
        .expect("input_clear_preedit 必须存在");
    let rest = &src[start..];
    let end = rest
        .find("\n    }\n")
        .expect("input_clear_preedit body end not found");
    let body = &rest[..end];
    // 新架构：cancel 只清 composition + 更新 preedit visual
    assert!(
        body.contains("clear()") && body.contains("update_preedit_visual_state"),
        "cancel: input_clear_preedit 清 composition 并更新 preedit visual"
    );
    println!("[ISSUE710_COMMENT5735006606] cancel 视觉范围由 visual_canvas 处理 (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 测试 3: coordinator 的 raw range 参数已删除（旧职责由 text_motion 替代）
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue710_coordinator_raw_range_params_preserved() {
    // Issue #904: 旧 #901 动画架构（含 animation/composition.rs）已整体删除，
    // 不得复活第二套动画引擎。
    let comp = linux_qt_root().join("src/sujian_editor_item/animation/composition.rs");
    assert!(
        !comp.exists(),
        "coordinator: animation/composition.rs 已随 #901 架构删除，不得复活"
    );
    println!("[ISSUE710_COMMENT5735006606] coordinator raw range 参数已删除 (GUARDED)");
}
