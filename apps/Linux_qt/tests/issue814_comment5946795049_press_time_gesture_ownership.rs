//! Issue #814 评论 5946795049 — press-time 手势所有权守卫。
//!
//! 评论 5946795049 复核发现上一轮（5946366104）的父子 Scene 手势所有权实现有三个问题：
//! 1. bgTouchDrag 用了 DragHandler 不存在的 point 属性（应读 centroid）。
//! 2. onActiveChanged{if(active)...} 才判所有权太晚——active===true 表示已取得 exclusive grab，
//!    此时 return 不能把抓取还给 child，会出现"按在子星图里手势没反应"。
//! 3. 单指 drag 与双指 pinch 共用一个 _gestureOwnedByChildContent bool，单指转双指时互相清掉。
//!
//! 新方案：两个 passive PointHandler（touchOwnerA/touchOwnerB）在 press 时记录前两根手指
//! 各自所属 child Embed，父层 bgTouchDrag/canvasPinch 通过 enabled 在 grab 之前让出。
//! 本测试锁住新结构不回退到旧的 onActiveChanged 判定。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

const CANVAS: &str = "qml/StarMapCanvas.qml";

// ─────────────────────────────────────────────────────────────────────────
// 1. press-time ownership 属性与辅助函数存在
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_has_press_time_owner_properties() {
    let src = read_src(CANVAS);
    assert!(
        src.contains("property string _touchOwnerA: \"\""),
        "StarMapCanvas 必须有 _touchOwnerA press-time owner 属性"
    );
    assert!(
        src.contains("property string _touchOwnerB: \"\""),
        "StarMapCanvas 必须有 _touchOwnerB press-time owner 属性"
    );
    assert!(
        src.contains("readonly property bool _pinchBelongsToChild:"),
        "StarMapCanvas 必须有 _pinchBelongsToChild 派生属性"
    );
    assert!(
        src.contains("function childOwnerAtScreen(sx, sy)"),
        "StarMapCanvas 必须有 childOwnerAtScreen 辅助函数"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 2. _pinchBelongsToChild 语义：两根都在同一 child 才归 child
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn pinch_belongs_to_child_requires_both_in_same_child() {
    let src = read_src(CANVAS);
    let window = function_window(&src, "readonly property bool _pinchBelongsToChild:", 400);
    assert!(
        window.contains("touchOwnerA.active") && window.contains("touchOwnerB.active"),
        "_pinchBelongsToChild 必须同时要求 touchOwnerA.active && touchOwnerB.active"
    );
    assert!(
        window.contains("_touchOwnerA !== \"\""),
        "_pinchBelongsToChild 必须要求 _touchOwnerA 非空（落在 child content）"
    );
    assert!(
        window.contains("_touchOwnerA === _touchOwnerB"),
        "_pinchBelongsToChild 必须要求两根手指在同一 child Embed（_touchOwnerA === _touchOwnerB）"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. 两个 passive PointHandler 在 press 时用 pressPosition 记录 owner
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn touch_owner_point_handlers_record_press_position() {
    let src = read_src(CANVAS);
    let wa = function_window(&src, "id: touchOwnerA", 600);
    assert!(
        wa.contains("PointerDevice.TouchScreen") && wa.contains("Qt.LeftButton"),
        "touchOwnerA 必须限定 TouchScreen + LeftButton"
    );
    assert!(
        wa.contains("childOwnerAtScreen(point.pressPosition.x, point.pressPosition.y)"),
        "touchOwnerA.onActiveChanged 必须用 childOwnerAtScreen(point.pressPosition.x, point.pressPosition.y) 记录 press-time owner"
    );
    let wb = function_window(&src, "id: touchOwnerB", 600);
    assert!(
        wb.contains("PointerDevice.TouchScreen") && wb.contains("Qt.LeftButton"),
        "touchOwnerB 必须限定 TouchScreen + LeftButton"
    );
    assert!(
        wb.contains("childOwnerAtScreen(point.pressPosition.x, point.pressPosition.y)"),
        "touchOwnerB.onActiveChanged 必须用 childOwnerAtScreen(point.pressPosition.x, point.pressPosition.y) 记录 press-time owner"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 4. bgTouchDrag 通过 enabled 在 grab 之前让出，不在 onActiveChanged 里判所有权
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn bg_touch_drag_uses_enabled_press_time_ownership() {
    let src = read_src(CANVAS);
    let window = function_window(&src, "id: bgTouchDrag", 1200);
    assert!(
        window.contains("enabled: _touchOwnerA === \"\" && _touchOwnerB === \"\""),
        "bgTouchDrag 必须用 enabled: _touchOwnerA === \"\" && _touchOwnerB === \"\" 在 grab 之前让出，实际窗口:\n{window}"
    );
    // 不得再出现旧的 onActiveChanged 所有权判定
    assert!(
        !window.contains("_gestureOwnedByChildContent"),
        "bgTouchDrag 不得再引用已废弃的 _gestureOwnedByChildContent"
    );
    assert!(
        !window.contains("bgTouchDrag.point"),
        "bgTouchDrag 不得再使用不存在的 point 属性（DragHandler 用 centroid）"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 5. canvasPinch 通过 enabled 让出，不在 onActiveChanged 里判所有权
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_pinch_uses_enabled_press_time_ownership() {
    let src = read_src(CANVAS);
    let window = function_window(&src, "id: canvasPinch", 1000);
    assert!(
        window.contains("enabled: !_pinchBelongsToChild"),
        "canvasPinch 必须用 enabled: !_pinchBelongsToChild 在 grab 之前让出，实际窗口:\n{window}"
    );
    assert!(
        !window.contains("_gestureOwnedByChildContent"),
        "canvasPinch 不得再引用已废弃的 _gestureOwnedByChildContent"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 6. 旧共用 bool 已彻底移除
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn old_shared_gesture_owned_bool_removed() {
    let src = read_src(CANVAS);
    assert!(
        !src.contains("_gestureOwnedByChildContent"),
        "StarMapCanvas 不得再保留旧的 _gestureOwnedByChildContent 共用 bool"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 7. Issue #814 评论 5947130795 — TouchScreen PointHandler 只能是 owner A/B
//    上一轮在 touchOwnerA/B 之外还留了一个独立 TouchScreen PointHandler 日志观察器。
//    Qt 对同 parent 下多个 PointHandler 组成分配组，独立观察器会把第一根手指分走，
//    导致 touchOwnerA/B 凑不齐两根、press-time ownership 在一指/两指场景里失效。
//    本测试锁住：TouchScreen PointHandler 只能有 touchOwnerA/touchOwnerB 两个，
//    且两者的 active 分支都记录 pointer_press touch 日志。
// ─────────────────────────────────────────────────────────────────────────

/// 收集源码中所有 `PointHandler {` 块（按花括号深度匹配到对应 `}`）。
fn point_handler_blocks(src: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = src;
    while let Some(idx) = rest.find("PointHandler {") {
        let body_start = idx + "PointHandler {".len();
        let mut depth = 1usize;
        let mut end = body_start;
        for (i, c) in rest[body_start..].char_indices() {
            if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    end = body_start + i + 1;
                    break;
                }
            }
        }
        blocks.push(rest[idx..end].to_string());
        rest = &rest[end..];
    }
    blocks
}

#[test]
fn touch_screen_point_handlers_are_only_owner_a_and_b() {
    let src = read_src(CANVAS);
    let blocks = point_handler_blocks(&src);
    let touch_blocks: Vec<_> = blocks
        .iter()
        .filter(|b| b.contains("PointerDevice.TouchScreen"))
        .collect();
    assert_eq!(
        touch_blocks.len(),
        2,
        "TouchScreen PointHandler 只能有 touchOwnerA 和 touchOwnerB 两个，实际 {:?}",
        touch_blocks
    );
    for b in &touch_blocks {
        assert!(
            b.contains("id: touchOwnerA") || b.contains("id: touchOwnerB"),
            "TouchScreen PointHandler 必须是 touchOwnerA 或 touchOwnerB，实际:\n{b}"
        );
        assert!(
            b.contains("canvasArea.logPointerPress(\"left\", \"touch\", point)"),
            "touchOwnerA/B 的 active 分支必须包含 canvasArea.logPointerPress(\"left\", \"touch\", point)，实际:\n{b}"
        );
    }
}
