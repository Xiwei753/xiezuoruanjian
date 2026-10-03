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
//! 2. 旧的 `container`（width/height = canvas/zoomLevel）不再存在；相机
//!    x/y/scale 全部集中在 `cameraLayer` 一张 Item 上，根层内容只用
//!    `anchors.fill` 占满它，不再让 anchors 和相机两套几何来源抢同一个 Item。
//! 3. `worldToScreenX/Y` 和 `screenToWorldX/Y` 统一坐标换算入口存在。
//! 4. Node/Embed delegate 画在本层局部坐标里（相机在祖先 Content 上），
//!    不再逐个 delegate 做 `worldToScreen` 换算。
//! 5. transient move 起点用模型里的世界坐标（nodeData.x/y、embedData.x/y），
//!    且只经共享状态机的 `beginPress` / `pressPendingToMove` 提升。
//! 6. Node/Embed 的 DragHandler 只上抛原始 `activeTranslation` 增量
//!    （Qt scene 坐标），Qt scene → 本层 local 的换算只在
//!    `StarMapSceneContent.qtSceneDeltaToLocal` 做一次，不再双重换算。
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

/// 取 `start`（含）到 `end`（不含）之间的源码片段；缺任一 marker 直接失败。
fn slice_between(src: &str, start: &str, end: &str) -> String {
    let s = src
        .find(start)
        .unwrap_or_else(|| panic!("missing marker `{start}`"));
    let e = src[s..]
        .find(end)
        .map(|i| s + i)
        .unwrap_or_else(|| panic!("missing marker `{end}`"));
    src[s..e].to_string()
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
fn canvas_mounts_camera_on_camera_layer_only() {
    let src = strip_line_comments(&read_src(CANVAS));

    // 旧的 container（x: panX, y: panY, scale: zoomLevel, width/height = canvas/zoomLevel）
    // 不应再存在。
    assert!(
        !src.contains("id: container"),
        "不得再使用旧的 container（x: panX, y: panY, scale: zoomLevel, width/height=canvas/zoomLevel）"
    );

    // 相机只有一套几何真相：x/y/scale 全在 cameraLayer 上，且只有这一处。
    assert!(
        src.contains("id: cameraLayer")
            && src.contains("x: canvasArea.panX")
            && src.contains("y: canvasArea.panY")
            && src.contains("scale: canvasArea.zoomLevel")
            && src.contains("transformOrigin: Item.TopLeft"),
        "相机变换必须集中在 cameraLayer 这一张 Item 上"
    );
    assert_eq!(
        count_occurrences(&src, "scale: canvasArea.zoomLevel"),
        1,
        "整棵递归树只能有一处相机 scale：子层不得再建第二个带相机的 Canvas"
    );

    // 根内容只用 anchors.fill 占满相机层，不能自己再拿 x/y 当相机平移：
    // 一个 Item 上不允许 anchors 和相机两套几何来源同时控制位置。
    let root_block = slice_between(&src, "id: rootContent", "property var selectionController: null");
    assert!(
        root_block.contains("anchors.fill: parent"),
        "根层内容必须 anchors.fill 占满相机层，实际片段:\n{root_block}"
    );
    for forbidden in ["x: canvasArea.panX", "y: canvasArea.panY", "scale: canvasArea.zoomLevel"] {
        assert!(
            !root_block.contains(forbidden),
            "根层内容不得再自己持有 {forbidden}，实际片段:\n{root_block}"
        );
    }

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
// 6. Node/Embed 只上抛原始 Qt scene 增量，换算只在 Content 做一次
// ─────────────────────────────────────────────────────────────────────────

/// Node/Embed 都不再自己维护 `sceneDelta()` 矩阵：DragHandler 的
/// `activeTranslation` 是 Qt scene 坐标增量，delegate 原样上抛，
/// 由 `StarMapSceneContent.qtSceneDeltaToLocal` 统一换算一次。
/// 旧实现 delegate 先换算一次、归属层又按"原始 Qt scene delta"换算第二次，
/// 全局 zoom=2 时拖动会只剩 1/4。
#[test]
fn node_does_not_self_convert_drag_delta() {
    let src = read_src(NODE);
    let stripped = strip_line_comments(&src);
    assert!(
        !stripped.contains("root.parent.scale"),
        "StarMapNode 不得再用 root.parent.scale 猜缩放（会与相机双重缩放）"
    );
    assert!(
        !stripped.contains("canvasZoomLevel"),
        "StarMapNode 不得再保留 canvasZoomLevel：全局缩放已由祖先 transform 承担"
    );
    assert!(
        !stripped.contains("function sceneDelta(") && !stripped.contains("mapFromItem("),
        "Node 不得再自己换算位移：Qt scene → 本层 local 只允许在归属层做一次"
    );
    let drag = function_window(&stripped, "onActiveTranslationChanged:", 500);
    assert!(
        drag.contains("var dx = activeTranslation.x - lastTx")
            && drag.contains("var dy = activeTranslation.y - lastTy")
            && drag.contains("root.moveDelta(dx, dy)"),
        "Node 的 DragHandler 必须只上抛原始 activeTranslation 增量，实际窗口:\n{drag}"
    );
}

#[test]
fn embed_does_not_self_convert_drag_delta() {
    let src = read_src(EMBED);
    let stripped = strip_line_comments(&src);
    assert!(
        !stripped.contains("root.parent.scale"),
        "StarMapEmbed 不得再用 root.parent.scale 猜缩放（会与相机双重缩放）"
    );
    assert!(
        !stripped.contains("canvasZoomLevel") && !stripped.contains("function sceneDelta("),
        "StarMapEmbed 不得再自己维护位移换算：Qt scene → 本层 local 只允许在归属层做一次"
    );
    // 标题 + 四条边框共 5 个 DragHandler，全部原样上抛。
    assert_eq!(
        count_occurrences(&stripped, "root.moveDelta(dx, dy)"),
        5,
        "5 个 DragHandler 必须统一只上抛原始 activeTranslation 增量"
    );
    assert!(
        !stripped.contains("root.moveDelta(d.x, d.y)"),
        "不再有经 sceneDelta 换算后再上抛的旧路径"
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
// 8. Embed 是正圆（world 几何恒定，直径 200，不再复用 node 150×60）
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn embed_shell_is_a_circle() {
    let src = read_src(EMBED);
    assert!(
        !src.contains("_embedDefaultWidth") && !src.contains("_embedDefaultHeight"),
        "StarMapEmbed 不得再有 240×220 的矩形尺寸常量"
    );
    let shell = function_window(&src, "id: visualEmbed", 400);
    assert!(
        shell.contains("radius: width / 2"),
        "Embed 外壳必须是正圆（radius = width / 2），实际窗口:\n{shell}"
    );
    // 内容区取圆的内接正方形，子内容不溢出圆外。
    assert!(
        src.contains("readonly property real _contentSide:")
            && src.contains("width: root._contentSide")
            && src.contains("height: root._contentSide"),
        "contentViewport 必须取圆的内接正方形"
    );
}

#[test]
fn controller_uses_embed_diameter_constant() {
    let src = read_src("qml/StarMapGraphController.qml");
    assert!(
        src.contains("readonly property int _embedDiameter: 200"),
        "GraphController 必须有 Embed 直径常量 _embedDiameter: 200（DEFAULT_EMBED_DIAMETER）"
    );
    assert!(
        !src.contains("_embedDefaultWidth") && !src.contains("_embedDefaultHeight"),
        "GraphController 不得再保留 240×220 的矩形尺寸常量"
    );
    // buildModels 里 Embed 的 width/height 必须用直径常量。
    // 旧 portal Node 归一到 Embed 和正常 Embed 两处都用 _embedDiameter。
    let embed_const_count = src.matches("width: _embedDiameter").count();
    assert!(
        embed_const_count >= 2,
        "GraphController buildModels 里 Embed（含旧 portal 归一）必须用 width: _embedDiameter，实际 {embed_const_count} 处"
    );
    let embed_height_const_count = src.matches("height: _embedDiameter").count();
    assert!(
        embed_height_const_count >= 2,
        "GraphController buildModels 里 Embed（含旧 portal 归一）必须用 height: _embedDiameter，实际 {embed_height_const_count} 处"
    );
}