//! Issue #814 评论 5935285879 — 星图无限画布坐标模型重构守卫（主体功能 A）。
//!
//! WHITE_BOX 验证：读取 QML 源码，确定性断言无限画布重构的不变量，防止回退。
//!
//! Issue #822 更新：整棵星图只剩一个全局视口，delegate 不再画在根 Canvas 里，
//! 而是画在根层 `StarMapSceneContent` 的**本层局部坐标**里（相机作用在根层
//! Content 这一张 Item 上）。因此原守卫里"delegate 用 `worldToScreenX/Y` 把世界
//! 坐标换算成屏幕坐标"的断言整体失效——那正是"每个 Scene 一个视口"的产物。
//! 换算方向现在反过来：屏幕→世界仍由根 Canvas 的 `screenToWorldX/Y` 负责，
//! 世界→屏幕不再逐个 delegate 做，而是由相机 transform 一次性承担。
//!
//! 锁住的结构：
//! 1. `applyPan` 不再用 `Math.min(0, ...)` clamp — pan 两个方向都不设边界。
//! 2. 旧的 `container`（width/height = canvas/zoomLevel）不再存在；相机只作用在
//!    根层 `StarMapSceneContent` 一张 Item 上，`anchors.fill: parent` 恒定覆盖视口。
//! 3. `worldToScreenX/Y` 和 `screenToWorldX/Y` 统一坐标换算入口存在。
//! 4. Node/Embed delegate 画在本层局部坐标里（相机在祖先 Content 上），
//!    不再逐个 delegate 做 `worldToScreen` 换算。
//! 5. transient move 起点用模型里的世界坐标（nodeData.x/y、embedData.x/y），
//!    且只经共享状态机的 `beginPress` / `pressPendingToMove` 提升。
//! 6. Node/Embed 不再用 `root.parent.scale` 猜缩放，改用 `mapFromItem` 差分。
//! 7. 背景交互统一走 `screenToWorld*`，不再各处分散手写 `(x - panX) / zoomLevel`。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{count_occurrences, function_window, read_src};

const CANVAS: &str = "qml/StarMapCanvas.qml";
const CONTENT: &str = "qml/StarMapSceneContent.qml";
const NODE: &str = "qml/StarMapNode.qml";
const EMBED: &str = "qml/StarMapEmbed.qml";

/// 去掉整行 `//` 注释，只留可执行语句。
fn strip_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

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
// 2. 相机只作用在根层 Content 一张 Item 上
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_mounts_camera_on_root_content_item_only() {
    let src = strip_line_comments(&read_src(CANVAS));

    // 旧的 container（x: panX, y: panY, scale: zoomLevel, width/height = canvas/zoomLevel）
    // 不应再存在。
    assert!(
        !src.contains("id: container"),
        "不得再使用旧的 container（x: panX, y: panY, scale: zoomLevel, width/height=canvas/zoomLevel）"
    );

    // 相机 transform 挂在根层 StarMapSceneContent 上，且只有这一处。
    assert!(
        src.contains("x: canvasArea.panX")
            && src.contains("y: canvasArea.panY")
            && src.contains("scale: canvasArea.zoomLevel")
            && src.contains("transformOrigin: Item.TopLeft"),
        "相机变换必须作用在根层 StarMapSceneContent 这张 Item 上"
    );
    assert_eq!(
        count_occurrences(&src, "scale: canvasArea.zoomLevel"),
        1,
        "整棵递归树只能有一处相机 scale：子层不得再建第二个带相机的 Canvas"
    );

    // delegate 已经搬去内容层，根 Canvas 不再自己铺一层 sceneLayer。
    assert!(
        !src.contains("id: sceneLayer") && !src.contains("delegate: StarMapNode {"),
        "Node/Embed delegate 必须渲染在 StarMapSceneContent 里，根 Canvas 不再重复铺一层"
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
// 4. delegate 画在本层局部坐标（#822 更新：不再逐个 delegate 做 worldToScreen）
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn content_delegates_use_local_coordinates() {
    let src = strip_line_comments(&read_src(CONTENT));

    let node_delegate = function_window(&src, "delegate: StarMapNode {", 1600);
    assert!(
        node_delegate.contains(
            "x: content.isMovingNode(nodeData.id) ? content.interactionController.moveX : nodeData.x"
        ),
        "Node delegate 的 x 必须直接用本层局部坐标，不做 worldToScreen 换算，实际片段:\n{node_delegate}"
    );
    assert!(
        node_delegate.contains(
            "y: content.isMovingNode(nodeData.id) ? content.interactionController.moveY : nodeData.y"
        ),
        "Node delegate 的 y 必须直接用本层局部坐标，实际片段:\n{node_delegate}"
    );

    let embed_delegate = function_window(&src, "delegate: StarMapEmbed {", 1800);
    assert!(
        embed_delegate.contains(
            "x: content.isMovingEmbed(embedData.instanceId) ? content.interactionController.moveX : embedData.x"
        ),
        "Embed delegate 的 x 必须直接用本层局部坐标，实际片段:\n{embed_delegate}"
    );
    assert!(
        embed_delegate.contains(
            "y: content.isMovingEmbed(embedData.instanceId) ? content.interactionController.moveY : embedData.y"
        ),
        "Embed delegate 的 y 必须直接用本层局部坐标，实际片段:\n{embed_delegate}"
    );

    // delegate 不得自己再做一次相机换算。
    assert!(
        !src.contains("worldToScreenX(") && !src.contains("worldToScreenY("),
        "内容层不得再逐个 delegate 做 worldToScreen：相机由根层 Content 的 transform 一次性承担"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 5. transient move 用模型里的世界坐标，且只经共享状态机提升
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn content_begin_move_uses_model_world_coordinates() {
    let src = strip_line_comments(&read_src(CONTENT));

    // 按下只登记 pressPending，起点坐标来自模型而不是 delegate 的屏幕 x/y。
    let promote = function_window(&src, "function promoteToMove(", 700);
    assert!(
        promote.contains("graphController.getNode(id)") || promote.contains("graphController.getEmbed(id)"),
        "promoteToMove 必须从模型取条目，起点用模型世界坐标，实际窗口:\n{promote}"
    );
    assert!(
        promote.contains("item.x") && promote.contains("item.y"),
        "promoteToMove 必须传模型里的 item.x/item.y 作为起点，实际窗口:\n{promote}"
    );
    assert!(
        promote.contains("interactionController.pressPendingToMove(kind, id, scenePathKey, item.x, item.y)"),
        "提升必须经 pressPendingToMove 仲裁，不能直接从 DragHandler 进 move，实际窗口:\n{promote}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 6. Node/Embed 不再用 root.parent.scale 猜缩放
// ─────────────────────────────────────────────────────────────────────────

/// Node/Embed 都不得再用 `root.parent.scale` 反推缩放：根层 Content 自身的
/// `scale` 就是全局 zoom，再乘一次会双重缩放。位移换算改用 `mapFromItem`
/// 差分，让 Qt 自己算完祖先链上的累积变换。
#[test]
fn node_does_not_use_parent_scale() {
    let src = read_src(NODE);
    assert!(
        !src.contains("root.parent.scale"),
        "StarMapNode 不得再用 root.parent.scale 猜缩放（会与根层 Content 的 scale 双重缩放）"
    );
    assert!(
        !src.contains("canvasZoomLevel"),
        "StarMapNode 不得再保留 canvasZoomLevel：全局缩放已由祖先 transform 承担"
    );
    let scene_delta = function_window(&src, "function sceneDelta(", 400);
    assert!(
        scene_delta.contains("root.mapFromItem(null, 0, 0)")
            && scene_delta.contains("root.mapFromItem(null, dx, dy)")
            && scene_delta.contains("point.x - origin.x"),
        "Node 的位移换算必须用 mapFromItem 差分（mapFromItem 只能映射点，\
         用自身原点做差顺带抵消 wobble 视觉偏移），实际窗口:\n{scene_delta}"
    );
}

#[test]
fn embed_does_not_use_parent_scale() {
    let src = read_src(EMBED);
    assert!(
        !src.contains("root.parent.scale"),
        "StarMapEmbed 不得再用 root.parent.scale 猜缩放（会与根层 Content 的 scale 双重缩放）"
    );
    assert!(
        !src.contains("canvasZoomLevel"),
        "StarMapEmbed 不得再保留 canvasZoomLevel：全局缩放已由祖先 transform 承担"
    );
    let scene_delta = function_window(&src, "function sceneDelta(", 400);
    assert!(
        scene_delta.contains("root.mapFromItem(null, 0, 0)")
            && scene_delta.contains("root.mapFromItem(null, dx, dy)")
            && scene_delta.contains("point.x - origin.x"),
        "Embed 的位移换算必须用 mapFromItem 差分，实际窗口:\n{scene_delta}"
    );
    // 5 处 DragHandler 都要走同一个 sceneDelta。
    assert_eq!(
        count_occurrences(&strip_line_comments(&src), "root.sceneDelta("),
        5,
        "标题 + 四条边框共 5 个 DragHandler，必须统一用 root.sceneDelta 换算后再上抛"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 7. 背景交互统一走 screenToWorld*
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_background_interaction_uses_screen_to_world() {
    let src = read_src(CANVAS);
    // logPointerPress 用 screenToWorldX/Y
    let lpp = function_window(&src, "function logPointerPress(", 900);
    assert!(
        lpp.contains("screenToWorldX(point.position.x)") && lpp.contains("screenToWorldY(point.position.y)"),
        "logPointerPress 必须用 screenToWorldX/Y，实际窗口:\n{lpp}"
    );
    // 不应再在背景交互里手写 (x - panX) / zoomLevel
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
// 8. Embed 有独立尺寸常量（不再复用 node 150×60）
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