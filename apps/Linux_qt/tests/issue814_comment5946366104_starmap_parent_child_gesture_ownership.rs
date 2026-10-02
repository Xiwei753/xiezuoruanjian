//! Issue #814 评论 5946366104 — 父子 Scene 事件所有权守卫（tap 部分）。
//!
//! 评论 5946366104 复核发现父层背景 Handler 仍把 child content 当背景：
//! 四个背景 tap Handler（bgMouseLeftTap/bgTouchLeftTap.onSingleTapped、
//! bgTouchLeftTap.onLongPressed、backgroundRightTap.onSingleTapped）在
//! Node/Embed chrome 判断后没有 findEmbedContentAt 判断，点击子星图内部
//! child node 时父 Scene 仍 clearSelection() 吞掉子场景选中。
//!
//! 评论 5946795049 复核后，父子 Scene 手势所有权改为 press-time passive
//! PointHandler owner + 父层 Handler 用 enabled 让出（详见
//! issue814_comment5946795049_press_time_gesture_ownership.rs）。本文件只保留
//! tap 的 findEmbedContentAt 守卫，不再锁已废弃的 onActiveChanged 所有权判定。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

const CANVAS: &str = "qml/StarMapCanvas.qml";

// ─────────────────────────────────────────────────────────────────────────
// 1. 四个背景 tap Handler 在 Node/Embed chrome 判断后有 findEmbedContentAt 判断
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn bg_mouse_left_tap_skips_embed_content() {
    let src = read_src(CANVAS);
    let window = function_window(&src, "id: bgMouseLeftTap", 1500);
    assert!(
        window.contains("findEmbedContentAt(mx, my)"),
        "bgMouseLeftTap.onSingleTapped 必须在 Node/Embed chrome 判断后加 findEmbedContentAt(mx, my) return，实际窗口:\n{window}"
    );
}

#[test]
fn bg_touch_left_tap_single_skips_embed_content() {
    let src = read_src(CANVAS);
    let window = function_window(&src, "id: bgTouchLeftTap", 3000);
    assert!(
        window.contains("findEmbedContentAt(mx, my)"),
        "bgTouchLeftTap.onSingleTapped 必须在 Node/Embed chrome 判断后加 findEmbedContentAt(mx, my) return，实际窗口:\n{window}"
    );
    // 触屏长按用 wx/wy
    assert!(
        window.contains("findEmbedContentAt(wx, wy)"),
        "bgTouchLeftTap.onLongPressed 必须在 Node/Embed chrome 判断后加 findEmbedContentAt(wx, wy) return，实际窗口:\n{window}"
    );
}

#[test]
fn background_right_tap_skips_embed_content() {
    let src = read_src(CANVAS);
    let window = function_window(&src, "id: backgroundRightTap", 1800);
    assert!(
        window.contains("findEmbedContentAt(mx, my)"),
        "backgroundRightTap.onSingleTapped 必须在 Node/Embed chrome 判断后加 findEmbedContentAt(mx, my) return，实际窗口:\n{window}"
    );
}
