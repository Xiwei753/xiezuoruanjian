//! Issue #814 评论 5935285879 — 星图无限画布坐标模型重构守卫（主体功能 A）。
//!
//! WHITE_BOX 验证：读取 QML 源码，确定性断言无限画布重构的不变量，防止回退。
//!
//! 锁住的结构：
//! 1. `applyPan` 不再用 `Math.min(0, ...)` clamp — pan 两个方向都不设边界。
//! 2. `container` 已改成 `sceneLayer`，`anchors.fill: parent`，始终覆盖视口。
//! 3. `worldToScreenX/Y` 和 `screenToWorldX/Y` 统一坐标换算入口存在。
//! 4. Node/Embed delegate 的 x/y 用 `worldToScreenX/Y`（世界→屏幕），scale=zoomLevel。
//! 5. transient move 的 `beginMove`/`beginEmbedMove` 用世界坐标（nodeData.x/y、
//!    embedData.x/y），不再拿 delegate 的屏幕 x/y。
//! 6. Node/Embed 不再用 `root.parent.scale` 猜缩放，改用 Canvas 传的 canvasZoomLevel。
//! 7. 背景交互统一走 `screenToWorld*`，不再各处分散手写 `(x - panX) / zoomLevel`。
//! 8. `projectedLeft/Top` 用 delegate 屏幕坐标（x/y），不再 `*zoomLevel+panX`。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

const CANVAS: &str = "qml/StarMapCanvas.qml";
const NODE: &str = "qml/StarMapNode.qml";
const EMBED: &str = "qml/StarMapEmbed.qml";

// ─────────────────────────────────────────────────────────────────────────
// 1. applyPan 不再 clamp
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_apply_pan_does_not_clamp_with_math_min() {
    let src = read_src(CANVAS);
    let window = function_window(&src, "function applyPan(", 200);
    assert!(
        !window.contains("Math.min(0,"),
        "applyPan 不得再用 Math.min(0, ...) clamp，pan 两个方向都不设边界，实际窗口:\n{window}"
    );
    // 必须直接赋值
    assert!(
        window.contains("panX = nextX") && window.contains("panY = nextY"),
        "applyPan 必须直接赋值 panX=nextX/panY=nextY，实际窗口:\n{window}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 2. sceneLayer 存在且 anchors.fill: parent
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_uses_scene_layer_not_container() {
    let src = read_src(CANVAS);
    // 旧的 container（x: panX, y: panY, scale: zoomLevel, width/height = canvas/zoomLevel）
    // 不应再存在
    assert!(
        !src.contains("id: container"),
        "不得再使用旧的 container（x: panX, y: panY, scale: zoomLevel, width/height=canvas/zoomLevel）"
    );
    // 新的 sceneLayer 必须存在且 anchors.fill: parent
    assert!(
        src.contains("id: sceneLayer") && src.contains("anchors.fill: parent"),
        "必须使用 sceneLayer 且 anchors.fill: parent，始终覆盖视口"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. worldToScreen / screenToWorld 统一换算入口
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_has_world_to_screen_and_screen_to_world_helpers() {
    let src = read_src(CANVAS);
    let w2sx = function_window(&src, "function worldToScreenX(", 200);
    assert!(
        w2sx.contains("panX + wx * zoomLevel"),
        "worldToScreenX 必须是 panX + wx * zoomLevel，实际窗口:\n{w2sx}"
    );
    let w2sy = function_window(&src, "function worldToScreenY(", 200);
    assert!(
        w2sy.contains("panY + wy * zoomLevel"),
        "worldToScreenY 必须是 panY + wy * zoomLevel，实际窗口:\n{w2sy}"
    );
    let s2wx = function_window(&src, "function screenToWorldX(", 200);
    assert!(
        s2wx.contains("(sx - panX) / zoomLevel"),
        "screenToWorldX 必须是 (sx - panX) / zoomLevel，实际窗口:\n{s2wx}"
    );
    let s2wy = function_window(&src, "function screenToWorldY(", 200);
    assert!(
        s2wy.contains("(sy - panY) / zoomLevel"),
        "screenToWorldY 必须是 (sy - panY) / zoomLevel，实际窗口:\n{s2wy}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 4. Node/Embed delegate x/y 用 worldToScreen，scale=zoomLevel
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_node_delegate_uses_world_to_screen() {
    let src = read_src(CANVAS);
    // Node delegate 的 x 绑定必须用 worldToScreenX
    assert!(
        src.contains("x: worldToScreenX(interaction.pointerMode === \"move\" && interaction.pressedNodeId === nodeData.id ? interaction.moveX : nodeData.x)"),
        "Node delegate 的 x 必须用 worldToScreenX 映射世界坐标到屏幕坐标"
    );
    assert!(
        src.contains("y: worldToScreenY(interaction.pointerMode === \"move\" && interaction.pressedNodeId === nodeData.id ? interaction.moveY : nodeData.y)"),
        "Node delegate 的 y 必须用 worldToScreenY 映射世界坐标到屏幕坐标"
    );
}

#[test]
fn canvas_embed_delegate_uses_world_to_screen() {
    let src = read_src(CANVAS);
    assert!(
        src.contains("x: worldToScreenX(interaction.pointerMode === \"move\" && interaction.pressedEmbedId === embedData.instanceId ? interaction.moveX : embedData.x)"),
        "Embed delegate 的 x 必须用 worldToScreenX 映射世界坐标到屏幕坐标"
    );
    assert!(
        src.contains("y: worldToScreenY(interaction.pointerMode === \"move\" && interaction.pressedEmbedId === embedData.instanceId ? interaction.moveY : embedData.y)"),
        "Embed delegate 的 y 必须用 worldToScreenY 映射世界坐标到屏幕坐标"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 5. transient move 用世界坐标（nodeData.x/y、embedData.x/y）
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_begin_move_uses_world_coordinates() {
    let src = read_src(CANVAS);
    // beginMove 必须用 nodeData.x/nodeData.y，不再用 delegate 的 x/y
    assert!(
        src.contains("interaction.beginMove(nodeData.id, nodeData.x, nodeData.y)"),
        "beginMove 必须用世界坐标 nodeData.x/nodeData.y，不再拿 delegate 的屏幕 x/y"
    );
    // beginEmbedMove 必须用 embedData.x/embedData.y
    assert!(
        src.contains("interaction.beginEmbedMove(embedData.instanceId, embedData.x, embedData.y)"),
        "beginEmbedMove 必须用世界坐标 embedData.x/embedData.y，不再拿 delegate 的屏幕 x/y"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 6. Node/Embed 不再用 root.parent.scale，改用 canvasZoomLevel
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn node_does_not_use_parent_scale() {
    let src = read_src(NODE);
    assert!(
        !src.contains("root.parent.scale"),
        "StarMapNode 不得再用 root.parent.scale 猜缩放（sceneLayer scale=1 会破坏旧换算）"
    );
    assert!(
        src.contains("property real canvasZoomLevel: 1.0"),
        "StarMapNode 必须有 canvasZoomLevel property 接收 Canvas 传的 zoomLevel"
    );
    assert!(
        src.contains("var zoom = canvasZoomLevel > 0 ? canvasZoomLevel : 1.0"),
        "StarMapNode 的 onMoveDelta 必须用 canvasZoomLevel 做世界增量换算"
    );
}

#[test]
fn embed_does_not_use_parent_scale() {
    let src = read_src(EMBED);
    assert!(
        !src.contains("root.parent.scale"),
        "StarMapEmbed 不得再用 root.parent.scale 猜缩放（sceneLayer scale=1 会破坏旧换算）"
    );
    assert!(
        src.contains("property real canvasZoomLevel: 1.0"),
        "StarMapEmbed 必须有 canvasZoomLevel property 接收 Canvas 传的 zoomLevel"
    );
    // 5 处 DragHandler 都应用 canvasZoomLevel
    let count = src
        .matches("var zoom = canvasZoomLevel > 0 ? canvasZoomLevel : 1.0")
        .count();
    assert!(
        count >= 5,
        "StarMapEmbed 的 5 个 DragHandler 都必须用 canvasZoomLevel，实际 {count} 处"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 7. 背景交互统一走 screenToWorld*
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_background_interaction_uses_screen_to_world() {
    let src = read_src(CANVAS);
    // logPointerPress 用 screenToWorldX/Y
    let lpp = function_window(&src, "function logPointerPress(", 400);
    assert!(
        lpp.contains("screenToWorldX(point.position.x)")
            && lpp.contains("screenToWorldY(point.position.y)"),
        "logPointerPress 必须用 screenToWorldX/Y，实际窗口:\n{lpp}"
    );
    // 不应再在背景交互里手写 (x - panX) / zoomLevel
    // （edgeCanvas 的 ctx.translate(panX,panY) 不算背景交互，允许保留）
    assert!(
        !src.contains("(eventPoint.position.x - panX) / zoomLevel"),
        "背景 TapHandler 不得再手写 (eventPoint.position.x - panX) / zoomLevel，统一用 screenToWorldX"
    );
    assert!(
        !src.contains("(mouse.x - panX) / zoomLevel"),
        "bgDragArea 不得再手写 (mouse.x - panX) / zoomLevel，统一用 screenToWorldX"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 8. projectedLeft/Top 用 delegate 屏幕坐标
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_projected_uses_delegate_screen_coords() {
    let src = read_src(CANVAS);
    // delegate x/y 已是屏幕坐标，projectedLeft/Top 直接用 x/y
    assert!(
        src.contains("readonly property real projectedLeft: x"),
        "projectedLeft 必须直接用 delegate 的屏幕 x，不再 *zoomLevel+panX"
    );
    assert!(
        src.contains("readonly property real projectedTop: y"),
        "projectedTop 必须直接用 delegate 的屏幕 y，不再 *zoomLevel+panY"
    );
    // 不应再保留旧的 * canvasArea.zoomLevel + canvasArea.panX 投影
    assert!(
        !src.contains("x * canvasArea.zoomLevel + canvasArea.panX"),
        "projectedLeft 不得再用 x * canvasArea.zoomLevel + canvasArea.panX（delegate x 已是屏幕坐标）"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 9. Embed 有独立尺寸常量（不再复用 node 150×60）
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn embed_has_independent_size_constants() {
    let src = read_src(EMBED);
    assert!(
        src.contains("_embedDefaultWidth") && src.contains("_embedDefaultHeight"),
        "StarMapEmbed 必须有独立尺寸常量 _embedDefaultWidth/_embedDefaultHeight，不再复用 node 150×60"
    );
}

#[test]
fn controller_uses_embed_independent_size_constants() {
    let src = read_src("qml/StarMapGraphController.qml");
    assert!(
        src.contains("_embedDefaultWidth") && src.contains("_embedDefaultHeight"),
        "GraphController 必须有 Embed 独立尺寸常量 _embedDefaultWidth/_embedDefaultHeight"
    );
    // buildModels 里 Embed 的 width/height 必须用独立常量。
    // 旧 portal Node 归一到 Embed 和正常 Embed 两处都用 _embedDefaultWidth。
    let embed_const_count = src.matches("width: _embedDefaultWidth").count();
    assert!(
        embed_const_count >= 2,
        "GraphController buildModels 里 Embed（含旧 portal 归一）必须用 width: _embedDefaultWidth，实际 {embed_const_count} 处"
    );
    let embed_height_const_count = src.matches("height: _embedDefaultHeight").count();
    assert!(
        embed_height_const_count >= 2,
        "GraphController buildModels 里 Embed（含旧 portal 归一）必须用 height: _embedDefaultHeight，实际 {embed_height_const_count} 处"
    );
}
