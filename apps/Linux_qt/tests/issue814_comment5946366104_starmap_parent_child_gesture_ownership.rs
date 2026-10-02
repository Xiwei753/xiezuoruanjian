//! Issue #814 评论 5946366104 — 父子 Scene 事件所有权守卫。
//!
//! 评论 5946366104 复核发现父层背景 Handler 仍把 child content 当背景：
//! 1. 四个背景 tap Handler（bgMouseLeftTap/bgTouchLeftTap.onSingleTapped、
//!    bgTouchLeftTap.onLongPressed、backgroundRightTap.onSingleTapped）在
//!    Node/Embed chrome 判断后没有 findEmbedContentAt 判断，点击子星图内部
//!    child node 时父 Scene 仍 clearSelection() 吞掉子场景选中。
//! 2. 父层 bgTouchDrag / canvasPinch 没有手势所有权状态，子星图内部拖动/
//!    双指缩放会让父 Scene 跟着动。
//!
//! 本测试锁住评论 5946366104 的修复不再回退。

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

// ─────────────────────────────────────────────────────────────────────────
// 2. 父层手势所有权状态属性存在
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_has_gesture_owned_by_child_content_property() {
    let src = read_src(CANVAS);
    assert!(
        src.contains("property bool _gestureOwnedByChildContent: false"),
        "StarMapCanvas 必须有 _gestureOwnedByChildContent 状态属性"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. bgTouchDrag 遵守手势所有权
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn bg_touch_drag_respects_gesture_ownership() {
    let src = read_src(CANVAS);
    let window = function_window(&src, "id: bgTouchDrag", 5500);
    // onActiveChanged 时必须用 findEmbedContentAt(screenToWorldX(...), screenToWorldY(...)) 判定起点
    assert!(
        window.contains("findEmbedContentAt(screenToWorldX(_gsx), screenToWorldY(_gsy))"),
        "bgTouchDrag.onActiveChanged 必须用 findEmbedContentAt(screenToWorldX, screenToWorldY) 判定手势起点所有权，实际窗口:\n{window}"
    );
    // onActiveTranslationChanged 必须在开头检查 _gestureOwnedByChildContent return
    assert!(
        window.contains("if (_gestureOwnedByChildContent) return"),
        "bgTouchDrag.onActiveTranslationChanged 必须在开头加 if (_gestureOwnedByChildContent) return，实际窗口:\n{window}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 4. canvasPinch 遵守手势所有权
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_pinch_respects_gesture_ownership() {
    let src = read_src(CANVAS);
    let window = function_window(&src, "id: canvasPinch", 1600);
    // onActiveChanged 时必须用 findEmbedContentAt 判定中心点
    assert!(
        window.contains("findEmbedContentAt(screenToWorldX(_psx), screenToWorldY(_psy))"),
        "canvasPinch.onActiveChanged 必须用 findEmbedContentAt(screenToWorldX, screenToWorldY) 判定缩放中心所有权，实际窗口:\n{window}"
    );
    // onActiveScaleChanged 必须在开头检查 _gestureOwnedByChildContent return
    assert!(
        window.contains("if (_gestureOwnedByChildContent) return"),
        "canvasPinch.onActiveScaleChanged 必须在开头加 if (_gestureOwnedByChildContent) return，实际窗口:\n{window}"
    );
}
