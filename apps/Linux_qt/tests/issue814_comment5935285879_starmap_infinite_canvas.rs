//! Issue #814 评论 5935285879 — 星图无限画布坐标模型重构守卫（主体功能 A）。
//!
//! WHITE_BOX 验证：读取 QML 源码，确定性断言无限画布重构的不变量，防止回退。
//!
//! Issue #929 更新：独立画布协议取代 #822 的"单一全局视口 + 递归内容"。
//! 任意时刻只有一张活动画布 `StarMapActiveScene`（currentStarmapId）；Canvas 把
//! 当前镜头 {zoom, panX, panY} 作为标量注入它，世界→屏幕只在活动画布的单一
//! 投影入口里现算（以视口中心为相对原点），不再有 cameraLayer transform，
//! 也不再递归渲染子画布。`StarMapSceneContent` 降级为父图 Embed 内的只读
//! fit-to-bounds 微缩概览。
//!
//! 锁住的结构：
//! 1. `applyPan` 不再用 `Math.min(0, ...)` clamp — pan 两个方向都不设边界。
//! 2. 相机不再是任何 Item 的 transform：Canvas 只有一张活动画布，
//!    `cameraLayer` / `container` 均已删除，zoom/pan 只以标量注入。
//! 3. Canvas 的 `worldToScreenX/Y`、`screenToWorldX/Y` 统一转发活动画布的
//!    单一投影入口，无活动场景时才退回同一数学口径。
//! 4. 活动画布的 Node/Embed/Edge delegate 只能用 `scene.worldToScreenX/Y` 投影；
//!    只读微缩概览没有任何相机 / 命中 / 手势。
//! 5. transient move 起点用模型里的世界坐标（nodeData.x/y、embedData.x/y），
//!    且只经共享状态机的 `beginPress` / `pressPendingToMove` 提升。
//! 6. Node/Embed 只上抛原始 `activeTranslation` 增量（Qt scene 坐标），
//!    Qt scene → 画布 world 的换算只在归属画布 `qtSceneDeltaToWorld` 做一次。
//! 7. 背景交互统一走 `screenToWorld*`，不再各处分散手写 `(x - panX) / zoomLevel`。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{count_occurrences, function_window, read_src, starmap_node_delegate_src};

const CANVAS: &str = "qml/StarMapCanvas.qml";
const ACTIVE_SCENE: &str = "qml/StarMapActiveScene.qml";
const CONTENT: &str = "qml/StarMapSceneContent.qml";
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
fn canvas_mounts_camera_as_scalars_on_the_single_active_scene() {
    // #929 取代 #822：不再有 cameraLayer transform，也不再递归渲染子画布。
    // 任意时刻只有一张活动画布，Canvas 把当前镜头 {zoom, panX, panY} 作为
    // 标量注入 StarMapActiveScene，由它的单一投影入口现算屏幕几何。
    let src = strip_line_comments(&read_src(CANVAS));

    assert!(
        !src.contains("id: cameraLayer") && !src.contains("id: container"),
        "不得再保留 cameraLayer / container 相机 Item：相机不再是 Item transform"
    );
    assert!(
        !src.contains("scale: canvasArea.zoomLevel"),
        "整棵星图不得再有任何相机 scale transform：相机只以标量注入活动画布"
    );

    // 任意时刻只有一张**活动**画布：Canvas 只按 currentStarmapId 加载一张
    // StarMapActiveScene，没有第二张带相机的子画布。
    // Issue #949 更新：穿越过渡期间会临时多一张"旧画面层"（冻结 route / 镜头、
    // interactive: false，过渡结束立即销毁），它同样是整屏单画布、不是递归
    // 子视口；活动画布仍然只有一张。
    assert_eq!(
        count_occurrences(&src, "sourceComponent: activeSceneComponent"),
        1,
        "Canvas 只能有一个活动画布加载器（currentStarmapId 的那张 StarMapActiveScene）"
    );
    assert!(
        count_occurrences(&src, "StarMapActiveScene {") <= 2
            && src.contains("id: outgoingSceneComponent")
            && src.contains("interactive: false"),
        "除唯一活动画布外，最多只允许 Issue #949 的旧画面层（禁交互、过渡后销毁）"
    );
    let loader = slice_between(&src, "id: activeSceneLoader", "Component {");
    assert!(
        loader.contains("active: canvasArea.currentStarmapId !== \"\"")
            && loader.contains("sourceComponent: activeSceneComponent"),
        "活动画布必须由唯一 Loader 按 currentStarmapId 加载，实际片段:\n{loader}"
    );
    assert!(
        !src.contains("childContentLoader") && !src.contains("delegate: StarMapNode {"),
        "Node/Embed delegate 只属于活动画布，Canvas 不再递归铺子画布"
    );

    // 镜头注入活动画布：zoom/pan 只是数字，不落到任何 transform 上。
    let active = slice_between(&src, "id: activeSceneComponent", "onNodeSelected");
    for inject in [
        "zoomLevel: canvasArea.zoomLevel",
        "panX: canvasArea.panX",
        "panY: canvasArea.panY",
        "scenePathKey: canvasArea.routeKey",
    ] {
        assert!(
            active.contains(inject),
            "活动画布必须接收 {inject}，实际片段:\n{active}"
        );
    }
    assert!(
        !active.contains("scale:") && !active.contains("transform:"),
        "活动画布不得用 transform/scale 承担相机，实际片段:\n{active}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. worldToScreen / screenToWorld 统一换算入口
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_has_world_to_screen_and_screen_to_world_helpers() {
    // #929：世界↔屏幕的单一投影入口在活动画布 StarMapActiveScene（视口中心
    // 相对原点）。Canvas 的换算入口转发给它，无活动场景时才退回同一数学口径。
    let src = read_src(CANVAS);
    for (name, delegate) in [
        ("function worldToScreenX(", "scene.worldToScreenX(wx)"),
        ("function worldToScreenY(", "scene.worldToScreenY(wy)"),
        ("function screenToWorldX(", "scene.screenToWorldX(sx)"),
        ("function screenToWorldY(", "scene.screenToWorldY(sy)"),
    ] {
        let window = function_window(&src, name, 320);
        assert!(
            window.contains("var scene = activeScene()") && window.contains(delegate),
            "{name} 必须转发给活动画布的单一投影入口，实际窗口:\n{window}"
        );
    }
    let w2sx = function_window(&src, "function worldToScreenX(", 320);
    assert!(
        w2sx.contains("panX + wx * zoomLevel"),
        "无活动场景兜底：worldToScreenX = panX + wx * zoomLevel，实际窗口:\n{w2sx}"
    );
    let w2sy = function_window(&src, "function worldToScreenY(", 320);
    assert!(
        w2sy.contains("panY + wy * zoomLevel"),
        "无活动场景兜底：worldToScreenY = panY + wy * zoomLevel，实际窗口:\n{w2sy}"
    );
    let s2wx = function_window(&src, "function screenToWorldX(", 320);
    assert!(
        s2wx.contains("(sx - panX) / (zoomLevel > 0 ? zoomLevel : 1)"),
        "无活动场景兜底：screenToWorldX 必须防 0 除，实际窗口:\n{s2wx}"
    );
    let s2wy = function_window(&src, "function screenToWorldY(", 320);
    assert!(
        s2wy.contains("(sy - panY) / (zoomLevel > 0 ? zoomLevel : 1)"),
        "无活动场景兜底：screenToWorldY 必须防 0 除，实际窗口:\n{s2wy}"
    );

    // 单一投影入口本体：以视口中心为相对原点现算，避免巨大世界坐标 × 极大 zoom
    // 的浮点抖动；数学上仍等价于 屏幕 = pan + world × zoom。
    let scene = read_src(ACTIVE_SCENE);
    let scene_w2s = function_window(&scene, "function worldToScreenX(", 200);
    assert!(
        scene_w2s.contains("width / 2 + (wx - _originWorldX) * _safeZoom"),
        "活动画布 worldToScreenX 必须用视口中心相对原点，实际窗口:\n{scene_w2s}"
    );
    let scene_s2w = function_window(&scene, "function screenToWorldX(", 200);
    assert!(
        scene_s2w.contains("_originWorldX + (sx - width / 2) / _safeZoom"),
        "活动画布 screenToWorldX 必须是同一投影的逆，实际窗口:\n{scene_s2w}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 4. 活动画布 delegate 经单一投影入口绘制；只读概览无相机/命中/手势
// ─────────────────────────────────────────────────────────────────────────

/// #929 取代 #822：活动画布的 Node/Embed/Edge delegate 只能通过
/// `scene.worldToScreenX/Y` 的单一投影入口把 authored world 坐标画到屏幕上，
/// 不得再自己维护第二份换算。
#[test]
fn active_scene_delegates_project_through_single_projection_entry() {
    let src = strip_line_comments(&read_src(ACTIVE_SCENE));

    let node_repeater = function_window(&src, "id: nodeRepeater", 1600);
    assert!(
        node_repeater.contains("x: scene.worldToScreenX(worldX)")
            && node_repeater.contains("y: scene.worldToScreenY(worldY)"),
        "Node delegate 必须经单一投影入口定位，实际片段:\n{node_repeater}"
    );
    let embed_repeater = function_window(&src, "id: embedRepeater", 1600);
    assert!(
        embed_repeater.contains("x: scene.worldToScreenX(worldX)")
            && embed_repeater.contains("y: scene.worldToScreenY(worldY)"),
        "Embed delegate 必须经单一投影入口定位，实际片段:\n{embed_repeater}"
    );
    let edge_repeater = function_window(&src, "id: edgeRepeater", 1600);
    assert!(
        edge_repeater.contains("scene.worldToScreenX(edgeItem.render.startX)")
            && edge_repeater.contains("scene.worldToScreenY(edgeItem.render.startY)"),
        "Edge delegate 必须经单一投影入口画路径，实际片段:\n{edge_repeater}"
    );

    // 投影定义只有一处：活动画布自己的 worldToScreenX/Y。
    assert_eq!(
        count_occurrences(&src, "function worldToScreenX("),
        1,
        "活动画布只能有一个 worldToScreenX 定义"
    );
    assert_eq!(
        count_occurrences(&src, "function screenToWorldX("),
        1,
        "活动画布只能有一个 screenToWorldX 定义"
    );
}

/// #929：StarMapSceneContent 降级为父图 Embed 内的只读 fit-to-bounds 微缩概览：
/// 没有相机（投影）、没有命中、没有任何手势，也不递归加载子画布。
#[test]
fn content_overview_is_readonly_without_camera_hit_or_gestures() {
    let src = strip_line_comments(&read_src(CONTENT));
    for forbidden in [
        "worldToScreenX",
        "worldToScreenY",
        "screenToWorldX",
        "screenToWorldY",
        "hitTargetAtWorld",
        "hitTargetAtScene",
        "DragHandler",
        "HoverHandler",
        "TapHandler",
        "PinchHandler",
        "WheelHandler",
        "childContentLoader",
        "selectionController",
        "scenePathKey",
        "StarMapActiveScene",
        "StarMapEmbed",
    ] {
        assert!(!src.contains(forbidden), "只读微缩概览不得出现 {forbidden}");
    }
    assert!(
        src.contains("function projectX(wx)") && src.contains("function projectY(wy)"),
        "微缩概览只能用 fit-to-bounds 的只读投影"
    );
    assert!(
        src.contains("readonly property real fitScale:"),
        "微缩概览必须显式给出 fitScale（进入子画布时的比例来源）"
    );
    assert!(
        src.contains("readonly property real _safeRatio: 0.66"),
        "概览必须留在圆的内接安全区（66% 内接，天然不溢出圆外，不需裁剪纹理）"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 5. transient move 用模型里的世界坐标，且只经共享状态机提升
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn content_begin_move_uses_model_world_coordinates() {
    // Issue #832：拖动阈值到点由唯一 Router 提升；起点坐标来自归属层模型，
    // 不是 delegate 的屏幕 x/y。
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    let promote = function_window(&router, "function handleDragActivated(", 2000);
    assert!(
        promote.contains("gestureOwner.itemOf(pressHit.kind, pressHit.id)"),
        "提升前必须从归属层模型取条目，起点用模型世界坐标，实际窗口:\n{promote}"
    );
    assert!(
        promote.contains("item.x, item.y"),
        "提升必须传模型里的 item.x/item.y 作为起点，实际窗口:\n{promote}"
    );
    assert!(
        promote.contains("ic.pressPendingToMove(pressHit.scenePathKey, pressHit.kind,")
            && promote.contains("pressHit.id, pressHit.targetPath,"),
        "提升必须经 pressPendingToMove 仲裁并保留完整身份，实际窗口:\n{promote}"
    );

    // 归属画布只负责把 Qt scene 增量换算成本画布 world 并更新 move 位置。
    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));
    let apply = function_window(&scene, "function applyMoveDelta(", 700);
    assert!(
        apply.contains("qtSceneDeltaToWorld(dxQtScene, dyQtScene)")
            && apply.contains("ic.updateMove(ic.moveX + delta.x, ic.moveY + delta.y)"),
        "归属画布必须把增量换算成 world 后更新 move 位置，实际窗口:\n{apply}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 6. Node/Embed 只上抛原始 Qt scene 增量，换算只在归属画布做一次
// ─────────────────────────────────────────────────────────────────────────

/// Issue #832：delegate 不再参与位移传递；唯一 Router 的 DragHandler
/// `activeTranslation` 是 Qt scene 坐标增量，只交给状态机 + 归属层，
/// 由归属画布 `StarMapActiveScene.qtSceneDeltaToWorld` 统一换算一次。
/// 旧实现 delegate 先换算一次、归属层又按"原始 Qt scene delta"换算第二次，
/// 全局 zoom=2 时拖动会只剩 1/4。
/// #929：归属画布从递归 Content 换成唯一活动画布。
#[test]
fn node_does_not_self_convert_drag_delta() {
    let stripped = strip_line_comments(&starmap_node_delegate_src());
    assert!(
        !stripped.contains("root.parent.scale"),
        "StarMapNode 不得再用 root.parent.scale 猜缩放（会与相机双重缩放）"
    );
    assert!(
        !stripped.contains("canvasZoomLevel"),
        "StarMapNode 不得再保留 canvasZoomLevel：全局缩放已由祖先 transform 承担"
    );
    assert!(
        !stripped.contains("DragHandler") && !stripped.contains("moveDelta"),
        "Node 不得再自己换算/上抛位移：唯一 Router 的 DragHandler 只交原始增量"
    );

    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    let drag = function_window(&router, "onActiveTranslationChanged:", 500);
    assert!(
        drag.contains("var dx = activeTranslation.x - lastTx")
            && drag.contains("var dy = activeTranslation.y - lastTy")
            && drag.contains("router.handleDragDelta(dx, dy)"),
        "Router 的 DragHandler 必须只交原始 activeTranslation 增量，实际窗口:\n{drag}"
    );
    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));
    let apply = function_window(&scene, "function applyMoveDelta(", 300);
    assert!(
        apply.contains("qtSceneDeltaToWorld(dxQtScene, dyQtScene)"),
        "Qt scene → 画布 world 只允许在归属画布做一次，实际窗口:\n{apply}"
    );
}

#[test]
fn embed_does_not_self_convert_drag_delta() {
    let stripped = strip_line_comments(&read_src(EMBED));
    assert!(
        !stripped.contains("root.parent.scale"),
        "StarMapEmbed 不得再用 root.parent.scale 猜缩放（会与相机双重缩放）"
    );
    assert!(
        !stripped.contains("canvasZoomLevel") && !stripped.contains("DragHandler"),
        "StarMapEmbed 不得再自己维护位移换算：输入只有一个主人（Router）"
    );
    assert!(
        !stripped.contains("moveDelta"),
        "Embed 不得再上抛位移：Router 直接驱动归属层"
    );

    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    assert_eq!(
        count_occurrences(&router, "router.handleDragDelta(dx, dy)"),
        1,
        "整棵星图只允许 Router 的一个 DragHandler 交位移"
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
        lpp.contains("screenToWorldX(point.position.x)")
            && lpp.contains("screenToWorldY(point.position.y)"),
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
    // #929：外壳仍是正圆、world 几何恒定；旧递归方案的圆形 FBO 遮罩与
    // contentViewport / contentUsableSideNow 已删除，概览留在圆内接安全区。
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
    assert!(
        src.contains("readonly property real _inscribedSide: _diameter / Math.SQRT2"),
        "标题与概览必须被圆的内接正方形约束（不再需要圆形裁剪纹理）"
    );
    // 只画微缩概览：没有 layer FBO / MultiEffect 遮罩 / ShaderEffectSource
    // 快照纹理 / 可交互子内容 Loader。
    let stripped = strip_line_comments(&src);
    for removed in [
        "contentViewport",
        "contentUsableSideNow",
        "maskedContent",
        "MultiEffect",
        "ShaderEffectSource",
        "circleMaskTexture",
        "layer.enabled",
        "childContentLoader",
        "_syncChildContent",
    ] {
        assert!(
            !stripped.contains(removed),
            "StarMapEmbed 不得再保留圆形裁剪 / 递归子内容机制 {removed}"
        );
    }
    // 概览懒加载：入口投影足够大（>=32px）才创建。
    assert!(
        src.contains("readonly property bool previewWanted:")
            && src.contains("width >= 32 && height >= 32"),
        "概览必须只在入口投影足够大时创建，实际源码不符"
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
