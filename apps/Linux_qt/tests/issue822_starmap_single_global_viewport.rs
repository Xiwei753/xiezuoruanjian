//! Issue #822 — Linux_Qt 星图输入 / 命中 / 连线 WHITE_BOX 守卫。
//!
//! 诊断包 `sujian-diagnostics-20261003-222534.zip` 暴露两个根因：
//! 1. 371 次 `starmap.*` 交互里 `connect_begin=0` / `connect_end=0` —— 不是后端建边
//!    失败，而是前端根本没进 connect。旧实现让 `DragHandler` 一有位移就抢进 move，
//!    `TapHandler.onLongPressed` 再调 `beginConnect()` 时已经不是 idle。
//! 2. 子图解析先以 `depth=1 + pathKey=root` 出现，之后才变成正确的
//!    `root/embed_...`。子 Scene 的 `pathKey` 默认 `"root"`，初始化期间短暂冒充根层；
//!    而 Canvas 又用 `pathKey === "root"` 决定滚轮入口 —— 身份和相机耦合在一起。
//!
//! Issue #929 更新：独立画布协议取代 #822 的"单一全局视口 + 递归内容"。
//! 任意时刻只有一张活动画布 `StarMapActiveScene`（currentStarmapId）：
//!
//! - 相机不再是任何 Item 的 transform：Canvas 把镜头标量 {zoom, panX, panY} 注入
//!   活动画布，世界↔屏幕只在活动画布的单一投影入口现算（以视口中心为相对原点）。
//! - 进入 / 退出子画布走 Core 导航路由（enter_starmap_embed / exit_starmap_canvas，
//!   定位走 resolve_starmap_navigation），按 routeKey（根 ID + 完整 segments）保存
//!   每层进入前的父镜头；连续缩放穿越只按"入口投影直径 / 视口短边"70% 进 / 60% 出，
//!   zoom 数值本身绝不触发层级切换，平台不回写 Core 任何相机数据。
//! - 命中判断只有一个入口：`Canvas.hitTargetAtScreen` → 活动画布的
//!   `hitTargetAtWorld`，命中顺序 node → embed chrome → embed content → edge → blank；
//!   子星图内容区整体是"进入子画布"的入口命中，不再有 childContent 死区。
//! - 子星图入口在父图里只画正圆壳 + fit-to-bounds 只读微缩概览
//!   （StarMapEmbed → StarMapSceneContent），不再递归创建可交互子场景 / 子 Loader，
//!   也没有 maskedContent / 圆形 FBO 遮罩 / contentUsableSide 递归尺寸链。
//! - 手势仲裁显式化：`idle -> pressPending`，先超拖动阈值转 move，先到长按时间
//!   转 connect，状态只被提升一次；长按计时器挂在唯一 Router（Item）里。
//! - 拖动阈值吃原始 Qt scene 像素，累计公式是"离按下点的直线距离"
//!   （pressDragX/Y 向量和），不是逐段增量长度累加；Qt scene → 画布 world 只在
//!   归属画布 `qtSceneDeltaToWorld` 换算一次。
//! - 节点标题就地内联编辑（TextInput），不再经过外部 Inspector Popup；
//!   StarMapInspector.qml / StarMapScene.qml 直接删除。
//!
//! QML 组件依赖 qml_resources qrc 与 Rust 注册的上下文属性，无法在本仓库的
//! Rust 测试里实例化，因此按仓库既有惯例（issue801/issue814/issue817 系列
//! WHITE_BOX 守卫）读取 QML 源码确定性断言。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{
    count_occurrences, function_window, linux_qt_root, read_src, starmap_node_delegate_src,
};

const CANVAS: &str = "qml/StarMapCanvas.qml";
const ACTIVE_SCENE: &str = "qml/StarMapActiveScene.qml";
const CONTENT: &str = "qml/StarMapSceneContent.qml";
const EMBED: &str = "qml/StarMapEmbed.qml";
const INTERACTION: &str = "qml/StarMapInteractionController.qml";
const CONTROLLER: &str = "qml/StarMapGraphController.qml";
const WORKSPACE: &str = "qml/StarMapWorkspace.qml";
const MAIN_RS: &str = "src/main.rs";
const BUILD_RS: &str = "build.rs";

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
// 1. 删除旧入口：StarMapScene.qml / StarMapInspector.qml
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn star_map_scene_and_inspector_files_are_deleted() {
    let root = linux_qt_root();
    for rel in ["qml/StarMapScene.qml", "qml/StarMapInspector.qml"] {
        assert!(
            !root.join(rel).exists(),
            "Issue #822 要求删除 {rel}：\
             StarMapScene 的定义是“每个 Scene 实例自带 viewport”，与单一全局视口相反；\
             StarMapInspector 的节点标题编辑已移到节点自身的内联 TextInput"
        );
    }
    assert!(
        root.join(CONTENT).exists(),
        "必须存在新的单层内容容器 qml/StarMapSceneContent.qml"
    );
}

#[test]
fn resource_registration_tracks_deleted_and_new_files() {
    let main = strip_line_comments(&read_src(MAIN_RS));
    assert!(
        main.contains("qml/StarMapSceneContent.qml"),
        "src/main.rs 必须注册新的 qml/StarMapSceneContent.qml"
    );
    assert!(
        !main.contains("qml/StarMapScene.qml"),
        "src/main.rs 不得再注册已删除的 qml/StarMapScene.qml"
    );
    assert!(
        !main.contains("qml/StarMapInspector.qml"),
        "src/main.rs 不得再注册已删除的 qml/StarMapInspector.qml"
    );

    let build = strip_line_comments(&read_src(BUILD_RS));
    assert!(
        build.contains("qml/StarMapSceneContent.qml"),
        "build.rs 的 rerun-if-changed 必须包含 qml/StarMapSceneContent.qml"
    );
    assert!(
        !build.contains("qml/StarMapScene.qml") && !build.contains("qml/StarMapInspector.qml"),
        "build.rs 不得再跟踪已删除的 StarMapScene.qml / StarMapInspector.qml"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 2. 相机唯一：镜头标量只属于 Canvas，投影只在活动画布（#929）
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn scene_content_has_no_camera_or_camera_gestures() {
    let src = strip_line_comments(&read_src(CONTENT));
    for forbidden in [
        "panX",
        "panY",
        "zoomLevel",
        "WheelHandler",
        "PinchHandler",
        "MouseArea",
    ] {
        assert!(
            !src.contains(forbidden),
            "StarMapSceneContent 是纯内容容器，不得出现 {forbidden}：\
             Issue #822 明确要求本文件不含 panX/panY/zoomLevel/WheelHandler/\
             PinchHandler/背景 pan handler，递归的是内容不是视口"
        );
    }
}

#[test]
fn only_root_canvas_owns_the_camera_properties() {
    let canvas = read_src(CANVAS);
    assert!(
        canvas.contains("property real panX:") && canvas.contains("property real panY:"),
        "根 StarMapCanvas 必须持有全局相机 panX/panY"
    );
    assert!(
        canvas.contains("property real zoomLevel: 1.0"),
        "根 StarMapCanvas 必须持有全局 zoomLevel"
    );

    // 内容容器与 Embed 都不得自己定义相机。
    for rel in [CONTENT, EMBED] {
        let src = strip_line_comments(&read_src(rel));
        for forbidden in [
            "property real panX:",
            "property real panY:",
            "property real zoomLevel:",
        ] {
            assert!(
                !src.contains(forbidden),
                "{rel} 不得自己定义 {forbidden}：只有根 Canvas 拥有全局相机"
            );
        }
    }
}

#[test]
fn camera_zoom_has_one_entry_used_by_wheel_pinch_and_buttons() {
    let src = read_src(CANVAS);
    // Issue #949 复核：zoomAt 里补了锚点顺序与配对基准的注释，窗口相应放大；
    // 断言内容不变。
    let zoom = function_window(&src, "function zoomAt(", 3600);
    assert!(
        zoom.contains("zoomLevel = target") && zoom.contains("applyPan("),
        "zoomAt 必须是唯一缩放入口，内部改活动画布 zoomLevel 并 applyPan，实际窗口:\n{zoom}"
    );
    assert!(
        zoom.contains("var target = normalizedZoom(requested, zoomLevel)"),
        "zoomAt 只做 Number 边界规范化，不夹 1e-4×/1e5× 产品范围，实际窗口:\n{zoom}"
    );
    // #929 复核：连续缩放穿越也从同一入口评估，判定只按入口投影覆盖率
    //（maybeEnterEmbedAt / maybeExitCanvas），zoom 数值本身不是层级依据；
    // 且按缩放方向过滤：只有放大评估进场，只有缩小评估退场（任意层同规则），
    // 避免"缩小经过高覆盖入口却误进子画布"。
    assert!(
        zoom.contains("var zoomingIn = target > oldZoom")
            && zoom.contains("var zoomingOut = target < oldZoom"),
        "zoomAt 必须区分放大/缩小方向，实际窗口:\n{zoom}"
    );
    assert!(
        zoom.contains("if (zoomingIn)")
            && zoom.contains("switched = maybeEnterEmbedAt(screenX, screenY)")
            && zoom.contains("else if (zoomingOut)")
            && zoom.contains("switched = maybeExitCanvas()"),
        "zoomAt 必须方向过滤：仅放大评估进场、仅缩小评估退场（按入口覆盖率评估，不按 zoom 数值），实际窗口:\n{zoom}"
    );
    // #929 第三轮复核：捏合穿越成功后，补偿基准必须记“本帧传入的 nextZoom（旧基准
    // 累计值）”，不能记已被换算成新画布 zoom 的 zoomLevel，否则下一帧
    // requested = switched * (next / base) 换算失效、捏合刚穿越就跳变。
    assert!(
        zoom.contains("_pinchSwitchBaseZoom = nextZoom")
            && zoom.contains("_pinchSwitchCanvasZoom = switched")
            && !zoom.contains("_pinchSwitchBaseZoom = zoomLevel"),
        "捏合穿越后必须以本帧 nextZoom 重设基准配对，不能用新画布 zoomLevel，实际窗口:\n{zoom}"
    );

    // 滚轮/捏合（唯一 Router）与 +/- 按钮（Canvas）都必须走同一个入口。
    let router = read_src("qml/StarMapInputRouter.qml");
    assert!(
        router.contains("canvas.zoomAt(point.position.x, point.position.y, newZoom)"),
        "Router 的滚轮必须调 Canvas 的 zoomAt，不能自己换算 panX/panY"
    );
    assert!(
        router.contains("canvas.zoomAt(centroid.position.x, centroid.position.y,"),
        "Router 的捏合必须走 zoomAt 统一夹取/以中心缩放，不再自己写第二套 pan 公式"
    );
    assert!(
        count_occurrences(&src, "onClicked: zoomAt(") == 2,
        "触屏 +/- 两个按钮都必须调 zoomAt，不能直接写 zoomLevel"
    );
    assert!(
        !router.contains("panX =") && !router.contains("panY ="),
        "Router 不得自己改 panX/panY：相机只有一个入口"
    );
}

/// 相机范围跟 docs/starmap_viewport.md：连续缩放 + 乘法步进。
/// 不再有 0.35~2.5 / 1e-4~1e5 这类产品硬边界；只在 Number 边界做规范化。
#[test]
fn camera_range_and_steps_follow_the_shared_viewport_spec() {
    let src = read_src(CANVAS);
    assert!(
        src.contains("readonly property real _zoomFactor: 1.2"),
        "工具栏必须使用共享乘法步进常量"
    );
    assert!(
        !src.contains("_cameraScaleMin") && !src.contains("_cameraScaleMax"),
        "画布不得再声明 1e-4×/1e5× 产品缩放范围"
    );

    let stripped = strip_line_comments(&src);
    for forbidden in [
        "Math.max(0.35",
        "Math.min(2.5",
        "zoomLevel + 0.15",
        "zoomLevel - 0.15",
    ] {
        assert!(
            !stripped.contains(forbidden),
            "不得再保留旧的产品硬上限/加法步进：{forbidden}"
        );
    }
    // 滚轮乘法步进在唯一 Router 里；Canvas 只提供 zoomAt 纯方法。
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    let wheel = function_window(&router, "onWheel: function(event)", 900);
    assert!(
        wheel.contains("oldZoom * Math.pow(canvas._zoomFactor, delta)"),
        "滚轮必须乘法步进，实际窗口:\n{wheel}"
    );
    assert_eq!(
        count_occurrences(&stripped, "zoomLevel * _zoomFactor"),
        1,
        "+ 按钮必须乘法步进"
    );
    assert_eq!(
        count_occurrences(&stripped, "zoomLevel / _zoomFactor"),
        1,
        "− 按钮必须乘法步进"
    );
}

#[test]
fn wheel_and_pinch_have_no_scene_identity_gating() {
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    let canvas = strip_line_comments(&read_src(CANVAS));

    // 旧实现用 pathKey === "root" 决定谁能缩放。现在只有一个视口，
    // 这类“按场景身份决定谁能操作相机”的判断必须整体消失。
    for (name, src) in [("StarMapInputRouter", &router), ("StarMapCanvas", &canvas)] {
        assert!(
            !src.contains("pathKey === \"root\""),
            "{name} 不得再用 pathKey === \"root\" 这类场景身份判断决定谁能缩放"
        );
        assert!(
            !src.contains("_pinchBelongsToChild"),
            "{name} 不得再有“捏合归某个子星图”的判断：整棵星图只有一个视口"
        );
        assert!(
            !src.contains("_touchOwnerA") && !src.contains("_touchOwnerB"),
            "{name} 不得再有按手指分别记录所属子星图的 press-time owner：触屏手势不再按层让出"
        );
        assert!(
            !src.contains("childOwnerAtScreen"),
            "{name} 不得再保留 childOwnerAtScreen（每层各自有视口的产物）"
        );
    }

    let wheel = function_window(&router, "id: wheelHandler", 2400);
    assert!(
        wheel.contains("blocking: false")
            && wheel.contains("event.accepted = false")
            && wheel.contains("event.accepted = true"),
        "wheelHandler 不得阻塞编辑器滚动，并须显式声明 accepted，实际窗口:\n{wheel}"
    );
    assert!(
        !wheel.contains("enabled:"),
        "wheelHandler 不得再有任何 enabled 门控（鼠标停在哪一层都走同一个 zoomAt），\
         实际窗口:\n{wheel}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. 路由身份是 Canvas routeKey（根 ID + 完整 segments），无 "root" 默认
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn scene_path_key_is_required_without_root_default() {
    // #929：递归内容容器已删除，路径身份属于"当前活动画布"。
    let src = strip_line_comments(&read_src(ACTIVE_SCENE));
    assert!(
        src.contains("required property string scenePathKey"),
        "scenePathKey 必须是 required property"
    );
    // 旧 bug：子 Scene 的 pathKey 默认 "root"，初始化期间冒充根层。
    assert!(
        !src.contains("property string scenePathKey: \"root\"")
            && !src.contains("required property string scenePathKey: \"root\""),
        "scenePathKey 不得有默认 \"root\"：这正是诊断包里 depth=1 却记成 pathKey=root 的根因"
    );

    // 根层/子层统一由 Canvas 用 Core 导航路由的 routeKey（根 ID + 完整 segments）
    // 显式注入，不靠任何默认值。
    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        canvas.contains("readonly property string routeKey: routeKeyOf(currentRoute)"),
        "Canvas 必须用 routeKey（根 ID + 完整 segments）作为唯一路由身份"
    );
    let block = slice_between(&canvas, "StarMapActiveScene {", "onNodeSelected:");
    assert!(
        block.contains("scenePathKey: canvasArea.routeKey")
            && block.contains("rootStarmapId: canvasArea.currentRoute.rootStarmapId")
            && block.contains("pathSegments: canvasArea.currentRoute.segments"),
        "活动画布必须由 Canvas 显式传入完整路由身份，实际片段:\n{block}"
    );
}

/// #929 取代 #822：不再有"父内容里递归创建子内容"。父图里的子星图只剩
/// 只读微缩概览（没有导航身份）；真正的路由身份由 Canvas 一次性交给当前
/// 活动画布（root id + 完整 segments），不留给任何默认 "root"。
#[test]
fn active_canvas_receives_full_route_identity_once_at_creation() {
    let embed = strip_line_comments(&read_src(EMBED));
    for forbidden in [
        "childContentLoader",
        "childContentPathKey",
        "setSource(",
        "scenePathKey",
    ] {
        assert!(
            !embed.contains(forbidden),
            "Embed 不得再递归创建子内容 / 复制路径身份：{forbidden}"
        );
    }
    let preview = function_window(&embed, "StarMapSceneContent {", 200);
    assert!(
        preview.contains("starmapId: root.targetStarmapId")
            && preview.contains("starmapBackendRef: root.starmapBackendRef"),
        "只读微缩概览只接收目标图数据入口，不接收场景路径身份，实际窗口:\n{preview}"
    );

    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        canvas.contains("scenePathKey: canvasArea.routeKey")
            && canvas.contains("pathSegments: canvasArea.currentRoute.segments")
            && canvas.contains("rootStarmapId: canvasArea.currentRoute.rootStarmapId"),
        "Canvas 必须把完整路由身份（root id + 完整 segments）一次性注入活动画布"
    );
    assert!(
        canvas.contains("function routeKeyOf(route)")
            && canvas.contains("(route.rootStarmapId || \"\") + \"|\" + parts.join(\"/\")"),
        "routeKey 必须由根 ID + 完整 segments 拼成，同一目标图的不同入口各自独立"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 4. 不再递归子画布：入口只有只读微缩概览
// ─────────────────────────────────────────────────────────────────────────

/// #929 取代 #822：父图里不再嵌套可交互子场景，只加载只读微缩概览；
/// SceneContent 不再反向引用 Embed，因此可以直接用 Component 静态声明。
#[test]
fn embed_loads_readonly_overview_instead_of_nested_scene() {
    let src = strip_line_comments(&read_src(EMBED));
    assert!(
        !src.contains("StarMapScene.qml") && !src.contains("StarMapActiveScene"),
        "Embed 不得加载 StarMapScene.qml / StarMapActiveScene：那会把子画布递归塞进父图"
    );
    for forbidden in ["childContentLoader", "setSource(", "Qt.resolvedUrl"] {
        assert!(
            !src.contains(forbidden),
            "Embed 只能经 Component 声明只读概览，不得再运行时递归创建子内容：{forbidden}"
        );
    }
    assert!(
        src.contains("readonly property bool previewWanted:")
            && src.contains("width >= 32 && height >= 32"),
        "概览必须按入口投影懒加载（>=32px 才创建）"
    );
    assert!(
        src.contains("active: root.previewWanted")
            && src.contains("sourceComponent: previewComponent"),
        "概览 Loader 必须由 previewWanted 门控，实际源码缺少"
    );

    // 不再有编译期类型环：概览组件不得反向引用 Embed / 活动画布。
    let content = strip_line_comments(&read_src(CONTENT));
    assert!(
        !content.contains("StarMapEmbed") && !content.contains("StarMapActiveScene"),
        "StarMapSceneContent 不得引用 StarMapEmbed / StarMapActiveScene（避免编译期类型环）"
    );
}

/// #929 取代 #822：GraphController 属于唯一活动画布 StarMapActiveScene；
/// StarMapSceneContent 降级为只读概览（没有 GraphController，只读 Core 快照）。
#[test]
fn active_scene_owns_its_graph_controller_and_projects_world_coords() {
    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));
    assert_eq!(
        count_occurrences(&scene, "StarMapGraphController {"),
        1,
        "活动画布必须拥有且只拥有一个自己的 GraphController"
    );
    assert!(
        scene.contains("starmapId: scene.starmapId")
            && scene.contains("pathKey: scene.scenePathKey"),
        "活动画布 GraphController 的 starmapId/pathKey 必须来自 required 路由身份"
    );

    // 节点 delegate 用唯一投影把 authored world 坐标画到屏幕。
    let node_delegate = slice_between(&scene, "id: nodeRepeater", "id: embedRepeater");
    assert!(
        node_delegate.contains("x: scene.worldToScreenX(worldX)")
            && node_delegate.contains("y: scene.worldToScreenY(worldY)")
            && node_delegate.contains("width: nodeData.width * scene._safeZoom"),
        "Node delegate 必须经单一投影画 authored world 坐标，实际片段:\n{node_delegate}"
    );

    // 只读概览：没有 GraphController，只读 Core 数据 + 平台 edge renderer。
    let content = strip_line_comments(&read_src(CONTENT));
    assert!(
        !content.contains("StarMapGraphController {"),
        "StarMapSceneContent 只能读快照，不得自己新建第二个 GraphController"
    );
    assert!(
        content.contains("starmapBackendRef.get_starmap_graph(starmapId)")
            && content.contains("starmapBackendRef.compute_edge_renders("),
        "只读概览必须只读 Core 图数据与平台 edge renderer 快照"
    );
}

/// #929 取代 #822：相机不再是 cameraLayer 的 x/y/scale transform，
/// 而是 Canvas 注入活动画布的标量 + 活动画布的单一投影入口。
#[test]
fn camera_transform_lives_in_the_single_projection_entry_only() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        !canvas.contains("id: cameraLayer")
            && !canvas.contains("id: container")
            && !canvas.contains("scale: canvasArea.zoomLevel"),
        "Canvas 不得再保留任何相机 Item transform：相机只以标量注入活动画布"
    );
    assert!(
        canvas.contains("property real zoomLevel: 1.0")
            && canvas.contains("property real panX: 0")
            && canvas.contains("property real panY: 0"),
        "镜头标量必须由 Canvas 持有"
    );

    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));
    for entry in [
        "function worldToScreenX(",
        "function worldToScreenY(",
        "function screenToWorldX(",
        "function screenToWorldY(",
    ] {
        assert_eq!(
            count_occurrences(&scene, entry),
            1,
            "活动画布只能有一个投影入口定义：{entry}"
        );
    }
    let w2s = function_window(&scene, "function worldToScreenX(", 300);
    assert!(
        w2s.contains("width / 2") && w2s.contains("_originWorldX") && w2s.contains("_safeZoom"),
        "worldToScreenX 必须以视口中心为相对原点，实际窗口:\n{w2s}"
    );
    let origin = function_window(&scene, "readonly property real _originWorldX:", 200);
    assert!(
        origin.contains("_finiteQuotient(_finiteDifference(width / 2, panX), _safeZoom)"),
        "相对原点必须由当前镜头现算并做有限收口，实际窗口:\n{origin}"
    );

    // 活动画布不得自己写相机状态：只有投影读取，没有赋值。
    assert!(
        !scene.contains("zoomLevel =") && !scene.contains("panX =") && !scene.contains("panY ="),
        "活动画布不得回写相机：镜头只从 Canvas 注入"
    );
}

/// #929 取代 #822：Deep Zoom 的进/退档位改成了"跨画布穿越"：
/// 进入阈值 70% / 退出阈值 60%，按入口投影直径占当前视口短边判定；
/// zoom 数值本身绝不触发层级切换，平台也不回写 Core 任何相机数据。
#[test]
fn canvas_enter_exit_transition_is_coverage_only_and_never_writes_back() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    for constant in [
        "readonly property real _enterRatio: 0.70",
        "readonly property real _exitRatio: 0.60",
    ] {
        assert!(
            canvas.contains(constant),
            "进入/退出阈值必须是 docs/starmap_viewport.md 的共享常量：缺 {constant}"
        );
    }
    let short = function_window(&canvas, "function viewportShortSide(", 200);
    assert!(
        short.contains("Math.min(width, height)"),
        "覆盖率口径必须是入口投影直径 / 视口短边，实际窗口:\n{short}"
    );
    let coverage = function_window(&canvas, "function coverageOfScreenPx(", 300);
    assert!(
        coverage.contains("var short = viewportShortSide()")
            && coverage.contains("short > 0 ? px / short : 0"),
        "coverage 必须现算 px / 视口短边，实际窗口:\n{coverage}"
    );

    let enter = function_window(&canvas, "function maybeEnterEmbedAt(", 800);
    assert!(
        enter.contains("var projectedPx = finiteProduct(Math.min(emb.width, emb.height), zoomLevel)")
            && enter.contains("if (coverageOfScreenPx(projectedPx) < _enterRatio)")
            && enter.contains("return enterEmbedViaCore(emb.instanceId, \"zoom\")"),
        "连续放大的进入判定只能比较入口投影覆盖率，实际窗口:\n{enter}"
    );
    assert!(
        !enter.contains("_cameraScaleMin") && !enter.contains("_cameraScaleMax"),
        "zoom 数值边界（数值安全）绝不能参与层级切换判定，实际窗口:\n{enter}"
    );

    let exit = function_window(&canvas, "function maybeExitCanvas(", 900);
    assert!(
        exit.contains("var virtualParentZoom = finiteQuotient(zoomLevel, ctx.ratio)")
            && exit.contains(
                "var coverageNow = coverageOfScreenPx(finiteProduct(ctx.embedDiameter, virtualParentZoom))"
            )
            && exit.contains("if (coverageNow > Math.min(_exitRatio, entryCoverage))")
            && exit.contains("return exitCanvasViaCore(true)"),
        "反向缩小必须按进入上下文反推父入口覆盖率，实际窗口:\n{exit}"
    );

    // 平台只在 routeKey 相机槽里保存/恢复父镜头，不向 Core 回写任何相机数据。
    // Issue #949 复核：enterEmbedViaCore 现在同时算预览 / 落点两个相机，窗口放大。
    let enter_core = function_window(&canvas, "function enterEmbedViaCore(", 5000);
    assert!(
        enter_core.contains("starmapBackendRef.enter_starmap_embed(")
            && enter_core.contains("storeCamera(routeKey, cameraSnapshot())")
            && enter_core.contains("storeCamera(childKey, childCamera)"),
        "进入必须走 Core 导航路由并按 routeKey 保存父镜头，实际窗口:\n{enter_core}"
    );
    let exit_core = function_window(&canvas, "function exitCanvasViaCore(", 2600);
    assert!(
        exit_core.contains("starmapBackendRef.exit_starmap_canvas(")
            && exit_core.contains("var cam = cameraStack[parentKey]")
            && exit_core.contains(
                "parentTargetCam = cam ? { zoom: cam.zoom, panX: cam.panX, panY: cam.panY }"
            ),
        "退出必须走 Core 导航路由并从平台相机槽取父画面镜头（按钮落点；滚轮落点 = 当前帧反推），实际窗口:\n{exit_core}"
    );

    // 旧 LOD 档位机制必须整体消失。
    for (name, src) in [
        ("StarMapCanvas", &canvas),
        ("StarMapEmbed", &strip_line_comments(&read_src(EMBED))),
        (
            "StarMapSceneContent",
            &strip_line_comments(&read_src(CONTENT)),
        ),
    ] {
        for forbidden in [
            "resolveChildContentDetail",
            "childContentDetail",
            "projectedCoverage",
            "visualScale",
            "ancestorScale",
            "_minContentScreenPx",
            "_minVisualScale",
            "_interactiveEnterCoverage",
            "_interactiveExitCoverage",
            "_previewEnterPx",
            "_previewExitPx",
        ] {
            assert!(
                !src.contains(forbidden),
                "{name} 不得再保留旧的 LOD 档位标识 {forbidden}"
            );
        }
    }
    // Embed 不得回写相机。
    let embed = strip_line_comments(&read_src(EMBED));
    for forbidden in ["panX", "panY", "zoomLevel"] {
        assert!(
            !embed.contains(forbidden),
            "Embed 不得出现 {forbidden}：概览只按父相机投影重画，绝不回写相机"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// 5. 命中测试：唯一入口，返回活动画布的命中结果
// ─────────────────────────────────────────────────────────────────────────

/// #929 取代 #822：不再递归进 interactive 子画布；活动画布的命中顺序是
/// node → embed chrome → embed content → edge → blank，子星图内容区也是入口。
#[test]
fn hit_testing_has_one_entry_returning_the_active_canvas_hit() {
    let src = strip_line_comments(&read_src(ACTIVE_SCENE));
    assert_eq!(
        count_occurrences(&src, "function hitTargetAtWorld("),
        1,
        "活动画布只能有一个命中入口定义"
    );
    let hit = function_window(&src, "function hitTargetAtWorld(", 2600);
    let i_node = hit
        .find("var node = graphController.findNodeAt(")
        .expect("命中入口必须先查节点");
    let i_chrome = hit
        .find("var chrome = findEmbedChromeAt(")
        .expect("命中入口必须再查 embed chrome");
    let i_inside = hit
        .find("var inside = findEmbedContentAt(")
        .expect("命中入口必须再查 embed 内容区");
    let i_edge = hit
        .find("var edge = graphController.hitTestEdge(")
        .expect("命中入口最后必须查连线");
    assert!(
        i_node < i_chrome && i_chrome < i_inside && i_inside < i_edge,
        "命中顺序必须 node → embed chrome → embed content → edge → blank，实际窗口:\n{hit}"
    );
    assert_eq!(
        count_occurrences(&hit, "kind: \"embed\""),
        2,
        "embed chrome 与内容区都必须落成 embed 入口命中，实际窗口:\n{hit}"
    );
    assert!(
        !hit.contains("childContent") && !hit.contains("renderDetail"),
        "不得再返回 childContent / 依赖子层 renderDetail：整个圆都是入口，实际窗口:\n{hit}"
    );
    for field in [
        "owner: scene",
        "scenePathKey: scenePathKey",
        "starmapId: starmapId",
    ] {
        assert!(
            hit.contains(field),
            "命中结果必须带 {field}，调用方要靠它把行为路由回归属层，实际窗口:\n{hit}"
        );
    }
    assert!(
        hit.contains("targetPath: embedPath("),
        "embed 命中必须带完整 targetPath（进子画布 / 连线都用它），实际窗口:\n{hit}"
    );
}

#[test]
fn canvas_routes_all_hit_consumers_through_the_single_active_scene_entry() {
    let src = strip_line_comments(&read_src(CANVAS));
    let entry = function_window(&src, "function hitTargetAtScreen(", 400);
    assert!(
        entry.contains("scene.hitTargetAtWorld(screenToWorldX(sx), screenToWorldY(sy))"),
        "Canvas 必须只有一个命中入口，从当前活动画布开始，实际窗口:\n{entry}"
    );
    assert_eq!(
        count_occurrences(&src, "function hitTargetAtScreen("),
        1,
        "Canvas 只能有一个命中入口定义"
    );

    // 根 Canvas 不再自己只认识本层几何。
    for forbidden in [
        "graphController.findNodeAt",
        "findEmbedChromeAt",
        "findEmbedContentAt",
        "hitTestEdge",
        "hitPointerAtScreen",
    ] {
        assert!(
            !src.contains(forbidden),
            "Canvas 不得再直接调用 {forbidden}：命中一律走 hitTargetAtScreen 单入口"
        );
    }

    // 唯一输入主人 Router 的每个入口都经 hitAt() 走同一个命中函数。
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    assert_eq!(
        count_occurrences(&router, "function hitAt("),
        1,
        "Router 必须只有一个命中转发入口 hitAt"
    );
    assert!(
        count_occurrences(&router, "hitAt(") >= 5,
        "按下/单击/双击/右键都必须走同一个命中入口，实际 {} 处",
        count_occurrences(&router, "hitAt(")
    );
    assert_eq!(
        count_occurrences(&router, "hitTargetAtScreen("),
        1,
        "Router 只通过 hitAt() 一个入口调 canvas.hitTargetAtScreen"
    );
    // 命中不再按旧场景身份分流。
    for forbidden in ["pathKey === \"root\"", "childOwnerAtScreen"] {
        assert!(
            !router.contains(forbidden),
            "Router 不得再按旧场景身份分流命中：{forbidden}"
        );
    }
}

/// #929 取代 #822：活动画布就是归属层（world 坐标 == 本层局部坐标），
/// 空白新建不再需要 scene→local 换算，也没有递归子内容容器；
/// 新建仍由"命中归属层"自己的 GraphController 完成。
#[test]
fn blank_right_click_creates_in_the_active_canvas_world_coords() {
    let src = strip_line_comments(&read_src(ACTIVE_SCENE));
    for expected in [
        "function createNodeWithName(name, sceneX, sceneY)",
        "function createSubStarmapWithName(name, sceneX, sceneY)",
    ] {
        assert!(
            src.contains(expected),
            "菜单归属层必须自己提供 {expected}：活动画布可以直接新建内容，不需要第二个 Canvas"
        );
    }
    let create_node = function_window(&src, "function createNodeWithName(", 400);
    assert!(
        create_node.contains("var spawn = findFreeSpawnPoint(sceneX, sceneY)")
            && create_node.contains("graphController.createNode(name, spawn.x, spawn.y)"),
        "新建节点必须写进归属层自己的 GraphController（本层坐标就是 authored world 坐标），\
         实际窗口:\n{create_node}"
    );
    let create_sub = function_window(&src, "function createSubStarmapWithName(", 500);
    assert!(
        create_sub.contains("var spawn = findFreeSpawnPoint(sceneX, sceneY)")
            && create_sub.contains("graphController.createSubStarmapAt(name, spawn.x, spawn.y)"),
        "新建子星图必须写进归属层自己的 GraphController，实际窗口:\n{create_sub}"
    );
    // 旧 scene→local 坐标链（sceneToLocal）与圆内安全区 clamp 在独立画布下整体消失。
    for forbidden in ["sceneToLocal", "clampToContentSafeArea"] {
        assert!(
            !src.contains(forbidden),
            "活动画布 local == world，不得再保留 {forbidden}"
        );
    }
    // Canvas 的右键新建菜单把 world 坐标交给命中归属层。
    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        canvas.contains(
            "menuOwnerContent.createNodeWithName(name, contextMenuSceneX, contextMenuSceneY)"
        ) && canvas.contains(
            "menuOwnerContent.createSubStarmapWithName(name, contextMenuSceneX, contextMenuSceneY)"
        ),
        "Canvas 空白新建菜单必须把 world 坐标交给命中归属层"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 6. 手势按下仲裁：pressPending 显式状态，状态只提升一次
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn interaction_controller_has_explicit_press_pending_arbitration() {
    let src = strip_line_comments(&read_src(INTERACTION));

    assert!(
        src.contains("property string pointerMode: \"idle\""),
        "InteractionController 必须保留 pointerMode 作为唯一状态入口"
    );
    let begin_press = function_window(&src, "function beginPress(", 700);
    assert!(
        begin_press.contains("pointerMode = \"pressPending\""),
        "按下节点/子星图 chrome 必须进入 pressPending，实际窗口:\n{begin_press}"
    );
    for field in [
        "pressScenePathKey",
        "pressKind",
        "pressId",
        "pressTargetPath",
    ] {
        assert!(
            src.contains(&format!("property string {field}: \"\""))
                || src.contains(&format!("property var {field}: null")),
            "pressPending 必须记录 {field}：归属层、完整 targetPath 都要记下来"
        );
    }

    // 两条互斥的提升路径，只能各有一个实现。
    assert!(
        src.contains("function pressPendingToMove(")
            && src.contains("function pressPendingToConnect("),
        "必须显式区分 pressPending -> move 与 pressPending -> connect 两条提升路径"
    );
    let to_move = function_window(&src, "function pressPendingToMove(", 700);
    assert!(
        to_move.contains("pointerMode !== \"pressPending\"") && to_move.contains("return false"),
        "pressPendingToMove 必须在状态不是 pressPending 时拒绝提升（状态只能被提升一次），\
         实际窗口:\n{to_move}"
    );
    let to_connect = function_window(&src, "function pressPendingToConnect(", 700);
    assert!(
        to_connect.contains("pointerMode !== \"pressPending\"")
            && to_connect.contains("return false"),
        "pressPendingToConnect 必须在状态不是 pressPending 时拒绝提升，实际窗口:\n{to_connect}"
    );

    assert!(
        src.contains("function isPressedTarget(") && src.contains("function isMovingTarget("),
        "视觉查询必须由共享状态机提供 isPressedTarget/isMovingTarget：Node/Embed 不再有 Handler.pressed"
    );
    assert!(
        !src.contains("signal pressTimeout()"),
        "长按超时信号必须删除：超时由唯一 Router 的 Timer 调用提升入口"
    );
    assert!(
        !src.contains("Timer {"),
        "长按 Timer 不能挂在 InteractionController 内：它继承 QObject，没有默认属性"
    );
}

#[test]
fn drag_threshold_and_long_press_are_the_only_two_ways_out_of_press_pending() {
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    let activated = function_window(&router, "function handleDragActivated(", 2000);
    assert!(
        activated.contains("ic.pressPendingToMove(pressHit.scenePathKey, pressHit.kind,")
            && activated.contains("pressHit.id, pressHit.targetPath,")
            && activated.contains("item.x, item.y)"),
        "拖动阈值先到必须经 pressPendingToMove 提升为 move（完整身份 + 模型起点），实际窗口:\n{activated}"
    );
    assert!(
        !activated.contains("beginMove("),
        "handleDragActivated 不得直接 beginMove，必须经 pressPendingToMove 仲裁，实际窗口:\n{activated}"
    );

    let timeout = function_window(&router, "function handleLongPress(", 1800);
    assert!(
        timeout.contains("if (ic.pointerMode === \"pressPending\")")
            && timeout.contains("ic.pressPendingToConnect(hit.kind, hit.id, hit.targetPath,"),
        "长按到点只在仍是 pressPending 时经 pressPendingToConnect 提升为 connect，实际窗口:\n{timeout}"
    );

    // 状态提升只允许 Router 调：递归层与 delegate 都不得出现这些入口。
    for (label, source) in [
        ("StarMapSceneContent.qml", read_src(CONTENT)),
        ("节点卡片 delegate", starmap_node_delegate_src()),
        ("StarMapEmbed.qml", read_src(EMBED)),
    ] {
        let src = strip_line_comments(&source);
        for forbidden in [
            "pressPendingToMove(",
            "pressPendingToConnect(",
            "contextPendingToConnect(",
            "pressPendingToPan(",
        ] {
            assert!(
                !src.contains(forbidden),
                "{label} 不得自行提升手势状态：{forbidden} 只允许唯一 Router 调用"
            );
        }
    }
}

/// 拖动阈值必须吃原始 Qt scene 像素，累计公式必须是"离按下点的直线距离"：
/// 连续两次同方向 5px 是 10px，不是 2×√(5²+5²)≈14.1 也不是平方累加。
#[test]
fn drag_accumulation_is_a_raw_scene_pixel_vector_sum() {
    let src = strip_line_comments(&read_src(INTERACTION));
    assert!(
        src.contains("property real pressDragX: 0") && src.contains("property real pressDragY: 0"),
        "状态机必须按分量累计 pressDragX/pressDragY"
    );
    let note = function_window(&src, "function noteDragDelta(", 500);
    assert!(
        note.contains("pressDragX += dx")
            && note.contains("pressDragY += dy")
            && note.contains("pressDragDistance = Math.hypot(pressDragX, pressDragY)"),
        "累计公式必须是向量和 Math.hypot(pressDragX, pressDragY)，实际窗口:\n{note}"
    );
    assert!(
        !note.contains("pressDragDistance * pressDragDistance"),
        "旧的逐段平方累加公式必须删除"
    );

    // beginPress / cancelPressPending / reset 都必须清分量。
    for marker in [
        "function beginPress(",
        "function cancelPressPending(",
        "function reset(",
    ] {
        let window = function_window(&src, marker, 800);
        assert!(
            window.contains("pressDragX = 0") && window.contains("pressDragY = 0"),
            "{marker} 必须一起清 pressDragX/pressDragY，实际窗口:\n{window}"
        );
    }
}

#[test]
fn release_has_a_single_exit_for_click_move_and_connect() {
    let src = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    let release = function_window(&src, "function finishLeftGesture(", 1800);
    for branch in [
        "if (mode === \"connect\")",
        "else if (mode === \"move\")",
        "else if (mode === \"contextPending\")",
        "else if (mode === \"pressPending\")",
    ] {
        assert!(
            release.contains(branch),
            "finishLeftGesture 必须用一个统一出口覆盖所有状态，缺 {branch}，实际窗口:\n{release}"
        );
    }
    assert!(
        release.contains("owner.finishConnect()")
            && release.contains("owner.finishMove()"),
        "结束 connect/move 必须走归属层的 finish 出口，实际窗口:\n{release}"
    );
    assert!(
        src.contains("router.handleRelease()") && src.contains("router.handleLeftCancel("),
        "左键正常松手与异常取消都必须走统一收尾出口，实际源码缺少"
    );
}

#[test]
fn only_root_canvas_creates_the_interaction_controller() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    assert_eq!(
        count_occurrences(&canvas, "StarMapInteractionController {"),
        1,
        "整棵递归树只允许根 Canvas 创建一次 StarMapInteractionController"
    );
    assert!(
        canvas.contains("interactionController: canvasArea.sharedInteraction"),
        "根层内容必须拿到同一个共享状态机实例"
    );

    let content = strip_line_comments(&read_src(CONTENT));
    assert!(
        !content.contains("StarMapInteractionController {"),
        "StarMapSceneContent 只能接收 interactionController，不得自己新建第二个状态机"
    );
    let embed = strip_line_comments(&read_src(EMBED));
    assert!(
        !embed.contains("StarMapInteractionController {"),
        "Embed 只能把 interactionController 原样下传，不得自己新建状态机"
    );
}

#[test]
fn long_press_timer_lives_in_the_router_item_not_in_the_qtobject() {
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    assert!(
        router.contains("Timer {")
            && router.contains("running: (router.ic && router.ic.pressTimerActive) || router.emptyLongPressArmed"),
        "长按计时 Timer 必须挂在 Router（Item）里，由共享状态机 pressTimerActive 驱动，实际源码缺少"
    );
    assert!(
        router.contains("interval: router.ic ? router.ic.longPressInterval : 800"),
        "长按阈值必须用共享状态机的 longPressInterval"
    );
    assert!(
        router.contains("onTriggered: router.handleLongPress()"),
        "长按到点必须由唯一 Router 调状态提升，实际源码缺少"
    );

    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        canvas.contains("Application.styleHints.mousePressAndHoldInterval"),
        "长按阈值必须对齐平台系统长按时间，由 Canvas 注入共享状态机"
    );
    assert!(
        !canvas.contains("pressTimeout"),
        "Canvas 不得再保留 pressTimeout 计时/信号：超时归 Router"
    );
}

#[test]
fn connect_preview_is_one_global_overlay_line() {
    let src = strip_line_comments(&read_src(CANVAS));
    assert_eq!(
        count_occurrences(&src, "Canvas {"),
        2,
        "Canvas 只允许两个 Canvas：背景网格 与 唯一连线预览 overlay，不每层各画一条"
    );
    assert_eq!(
        count_occurrences(&src, "id: connectPreview"),
        1,
        "整张工作区只允许一个 connectPreview overlay"
    );
    let preview = slice_between(&src, "id: connectPreview", "onClicked: zoomAt(");
    assert!(
        preview.contains("ctx.moveTo(canvasArea.worldToScreenX(interaction.connectFromSceneX)")
            && preview
                .contains("ctx.lineTo(canvasArea.worldToScreenX(interaction.connectPreviewEndX)")
            && preview.contains("canvasArea.worldToScreenY(interaction.connectPreviewEndY)"),
        "连线预览线必须经活动画布单一投影换算 world 坐标，实际片段:\n{preview}"
    );
    assert!(
        preview.contains("if (interaction.pointerMode !== \"connect\")"),
        "预览线只在 connect 状态下画，实际片段:\n{preview}"
    );
    assert_eq!(
        count_occurrences(&src, "ctx.lineTo("),
        1,
        "整张 Canvas 只允许一个 overlay 画连线预览，不得每层各画一条"
    );
}

/// #929 取代 #822：独立画布下能在 UI 里连线的两端都在当前活动画布内，
/// 因此 Core 规划的宿主必须是本画布；不再按 hostSegments 找递归宿主 Content。
#[test]
fn connect_end_creates_the_edge_on_the_active_canvas_host() {
    let src = strip_line_comments(&read_src(ACTIVE_SCENE));
    let finish = function_window(&src, "function finishConnect(", 2800);
    assert!(
        finish.contains("hitTargetAtWorld(ic.connectMouseX, ic.connectMouseY)"),
        "连线松手必须用活动画布的单一命中入口找落点，实际窗口:\n{finish}"
    );
    assert!(
        finish.contains("toPath = hit.targetPath"),
        "落点必须是命中给出的完整 targetPath，不得退化成 nodeId-only，实际窗口:\n{finish}"
    );
    // 宿主规划由 Core 完成；独立画布只接受"宿主就是本画布"的规划结果。
    assert!(
        finish.contains("planCrossLayerConnection(fromPath, toPath)"),
        "建边必须调用 Core 的跨层宿主规划，实际窗口:\n{finish}"
    );
    assert!(
        finish.contains("if (_planHostIsCurrent(plan))")
            && finish.contains("success = commitEdgeWithPaths(plan.from, plan.to)"),
        "边必须写进 Core 规划的宿主（活动画布）自己的 commitEdgeWithPaths，实际窗口:\n{finish}"
    );
    assert!(
        !finish.contains("findContentByPathSegments") && !src.contains("findContentByPathSegments"),
        "不得再按宿主路径段递归找 Content：子画布不是嵌进父图的坐标空间"
    );
    let host = function_window(&src, "function _planHostIsCurrent(", 500);
    assert!(
        host.contains("plan.hostStarmapId !== starmapId")
            && host.contains("return false")
            && host.contains("_sameSegments(plan.hostSegments || [], pathSegments || [])"),
        "宿主判定必须同时核对宿主 starmapId 与完整 segments（routeKey 语义），\
         实际窗口:\n{host}"
    );

    let commit = function_window(&src, "function commitEdgeWithPaths(", 300);
    assert!(
        commit.contains("return graphController.createEdgeWithPaths(fromPath, toPath)"),
        "活动画布的 commitEdgeWithPaths 必须把边交给自己的 GraphController，\
         实际窗口:\n{commit}"
    );
    assert!(
        finish.contains("\"success\": success"),
        "connect_end.success 必须是真实后端结果，实际窗口:\n{finish}"
    );
    assert!(
        finish.contains("var sameTarget =") && finish.contains("if (!sameTarget)"),
        "自连必须被拒绝，实际窗口:\n{finish}"
    );
}

/// 跨层连线的宿主规划已收回 Core：#818/#822 曾经在 QML 复制的
/// "最近公共祖先 + 端点相对宿主路径"必须整体消失，QML 只保留渲染需要的
/// UI instanceId 映射，宿主统一走 Core 的 plan_cross_layer_connection。
#[test]
fn path_planner_no_longer_ports_lca_rules() {
    let planner = read_src("qml/StarMapPathPlanner.js");
    for forbidden in [
        "commonScenePathPrefix",
        "resolveItemRef",
        "buildTargetPathForHost",
        "planCrossLayerRelation",
        "terminalSegment",
        "hostSegments.length",
    ] {
        assert!(
            !planner.contains(forbidden),
            "StarMapPathPlanner.js 仍残留 QML 侧宿主规划逻辑：{forbidden}"
        );
    }
    assert!(
        planner.contains("function uiInstanceIdOfSegment("),
        "StarMapPathPlanner.js 必须保留 UI instanceId 映射"
    );

    // #929：调用点从递归 Content 收进唯一活动画布；宿主规划仍全在 Core。
    let scene = read_src(ACTIVE_SCENE);
    assert!(
        scene.contains("planCrossLayerConnection(fromPath, toPath)")
            && scene.contains("plan_cross_layer_connection"),
        "活动画布必须把两条绝对端点路径交给 Core 规划宿主"
    );
    assert!(
        !scene.contains("bindPlanToHost"),
        "活动画布不得再自行绑定点 starmapId（Core 已返回宿主）"
    );

    // 新资源必须进 qrc 和 rerun-if-changed 清单。
    let main = strip_line_comments(&read_src(MAIN_RS));
    assert!(
        main.contains("qml/StarMapPathPlanner.js"),
        "src/main.rs 必须注册 qml/StarMapPathPlanner.js"
    );
    let build = strip_line_comments(&read_src(BUILD_RS));
    assert!(
        build.contains("qml/StarMapPathPlanner.js"),
        "build.rs 的 rerun-if-changed 必须包含 qml/StarMapPathPlanner.js"
    );
    // Issue #929 的活动画布也必须同时在两边注册。
    for (name, src) in [("src/main.rs", &main), ("build.rs", &build)] {
        assert!(
            src.contains("qml/StarMapActiveScene.qml"),
            "{name} 必须注册 qml/StarMapActiveScene.qml（#929 活动画布）"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// 7. 节点标题就地内联编辑，Inspector 路线彻底删除
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn node_title_is_edited_inline_with_a_text_input() {
    let src = strip_line_comments(&read_src(ACTIVE_SCENE));
    // Issue #984 第二轮复核：编辑会话收成场景级唯一状态，delegate 只显示。
    assert!(
        src.contains("property string editingNodeId: \"\"")
            && src.contains("property string editingTitleDraft: \"\"")
            && src.contains("property string editingBodyDraft: \"\""),
        "活动画布必须有场景级唯一的名称/正文编辑会话"
    );
    assert!(
        src.contains("id: editOverlay") && src.contains("id: titleInput"),
        "活动画布必须有稳定编辑容器与名称 TextInput"
    );
    assert!(
        src.contains("TextEdit {") && src.contains("id: bodyEdit"),
        "活动画布必须有正文多行 TextEdit"
    );
    assert!(
        src.contains("visible: scene.hasActiveNodeEdit"),
        "编辑层只在场景会话存在时显示并接管输入，实际源码缺少"
    );

    let begin = function_window(&src, "function beginInlineEdit(", 1400);
    assert!(
        begin.contains("if (editingNodeId !== \"\" && !commitActiveNodeEdit())")
            && begin.contains("return false")
            && begin.contains("editingNodeId = nodeId")
            && begin.contains("Qt.callLater(editOverlay.activate)"),
        "打开新节点前必须先提交旧会话，失败保留原编辑；再建立唯一会话，实际窗口:\n{begin}"
    );
    let commit = function_window(&src, "function commitEdit(", 3600);
    assert!(
        commit.contains("scene.commitNodeEdit(node.id,")
            && commit.contains("titleChanged ? nextTitle : null")
            && commit.contains("bodyChanged ? nextBody : null"),
        "提交必须只把真正变化的名称/正文交给归属层保存，实际窗口:\n{commit}"
    );
    assert!(
        commit.contains("if (!result || result.success !== true)")
            && commit.contains("scene.editingError = result && result.message"),
        "保存失败必须保留编辑态并显示错误，不能先销毁草稿，实际窗口:\n{commit}"
    );
    assert!(
        commit.contains("if (!titleChanged && !bodyChanged)")
            && commit.contains("scene.finishActiveNodeEdit()"),
        "无变化提交只退出编辑，不应产生 content 补丁，实际窗口:\n{commit}"
    );

    // 名称 Enter 移到正文；Ctrl+Enter 提交；Esc 取消；正文 Enter 保留换行。
    assert!(
        src.contains("Keys.onReturnPressed:")
            && src.contains("Keys.onEnterPressed:")
            && src.contains("Keys.onEscapePressed:")
            && src.contains("event.modifiers & Qt.ControlModifier")
            && src.contains("editOverlay.commitEdit()")
            && src.contains("scene.finishActiveNodeEdit()"),
        "提交/取消必须绑定名称输入与 Ctrl+Enter / Esc"
    );
    assert!(
        !src.contains("onAccepted:")
            && !src.contains("onEditingFinished:")
            && src.contains("if (titleInput.inputMethodComposing)"),
        "不得沿用 accepted/editingFinished 自动提交，IME 预编辑期间必须挡住提交"
    );
}

#[test]
fn node_geometry_uses_graph_level_text_measurement_and_stable_rows() {
    let controller = strip_line_comments(&read_src("qml/StarMapGraphController.qml"));
    assert!(
        controller.contains("property ListModel nodesModel")
            && controller.contains("function syncNodeRows(")
            && controller.contains("nodesModel.set(row, node)")
            && controller.contains("nodesModel.setProperty(row, \"x\", nx)"),
        "节点模型必须有稳定行身份：更新只 set/setProperty，不整体替换数组"
    );
    assert!(
        !controller.contains("nodesModel = newNodes")
            && !controller.contains("nodesModel = nextNodes")
            && !controller.contains("reportNodeMeasuredSize"),
        "节点模型不得再用数组整体赋值或 delegate 回传实测高度"
    );
    assert!(
        controller.contains("StarMapNodeMetrics.snapshotFor(")
            && controller.contains("textMeasurer.measureNode"),
        "图数据加载必须用平台排版器生成整图布局快照"
    );

    let measurer = strip_line_comments(&read_src("qml/StarMapNodeMeasurer.qml"));
    assert!(
        measurer.contains("probe.forceLayout()") && measurer.contains("probe.contentHeight"),
        "节点高度必须来自 Qt Text 的真实排版结果"
    );
    let content = strip_line_comments(&read_src(CONTENT));
    assert!(
        content.contains("StarMapNodeMetrics.snapshotFor(")
            && !content.contains("nodeHeight(")
            && !content.contains("_measuredHeights"),
        "子图概览必须读同一份图级布局快照，不得回退固定高度"
    );
    let main = strip_line_comments(&read_src(MAIN_RS));
    let build = strip_line_comments(&read_src(BUILD_RS));
    for (name, src) in [("src/main.rs", &main), ("build.rs", &build)] {
        assert!(
            src.contains("qml/StarMapNodeMeasurer.qml"),
            "{name} 必须注册 qml/StarMapNodeMeasurer.qml"
        );
    }
}

#[test]
fn node_editor_is_a_stable_viewport_anchored_overlay() {
    let active = strip_line_comments(&read_src(ACTIVE_SCENE));
    assert!(
        active.contains("property string editingNodeId: \"\"")
            && active.contains("readonly property string inlineEditingKey:")
            && active.contains("function finishActiveNodeEdit()"),
        "活动画布必须有唯一编辑会话，inlineEditingKey 由会话派生"
    );
    let begin = function_window(&active, "function beginInlineEdit(", 1400);
    assert!(
        begin.contains("if (editingNodeId !== \"\" && !commitActiveNodeEdit())")
            && begin.contains("Qt.callLater(editOverlay.activate)"),
        "打开新节点必须先提交旧会话，失败不切换，实际窗口:\n{begin}"
    );
    let anchor = function_window(&active, "readonly property var _anchor:", 1400);
    assert!(
        anchor.contains("x + w > vw - m") && anchor.contains("y + h > vh - m")
            && anchor.contains("(vw - w) / 2"),
        "编辑浮层必须按视口边界换向/居中，实际窗口:\n{anchor}"
    );
    assert!(
        active.contains("readonly property string inlineEditingKey:")
            && active.contains("StarMapNodeMetrics.canEditBody"),
        "编辑会话必须驱动输入让位与正文可编辑判断"
    );
}

#[test]
fn node_gestures_are_disabled_while_editing() {
    // Issue #984 第二轮复核：Node delegate 现在只显示，编辑输入层是场景级
    // 稳定容器，不再挂在 Repeater delegate 内。
    let node = strip_line_comments(&starmap_node_delegate_src());
    for forbidden in ["TapHandler", "DragHandler", "PointHandler", "MouseArea"] {
        assert!(
            !node.contains(forbidden),
            "节点卡片 delegate 不得再挂业务手势 {forbidden}：输入只有一个主人（Router）"
        );
    }
    assert!(
        !node.contains("property bool editing")
            && !node.contains("id: titleInput")
            && !node.contains("id: bodyEdit"),
        "节点 delegate 不得再持有编辑状态/输入层，否则模型更新会销毁草稿"
    );

    let active = strip_line_comments(&read_src(ACTIVE_SCENE));
    assert!(
        active.contains("id: editOverlay")
            && active.contains("id: titleInput") && active.contains("id: bodyEdit")
            && active.contains("visible: scene.hasActiveNodeEdit"),
        "场景级编辑容器必须同时提供名称单行输入与正文多行输入"
    );

    // Issue #987：编辑期间不再全局禁用 Router 业务手势，而是按实际编辑矩形
    // 对每一次左/右键手势单独仲裁；左右键的归属状态互相隔离。
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    for old in [
        "readonly property bool editingBlocks:",
        "enabled: !router.editingBlocks",
        "!router.pinchActive && !router.editingBlocks",
    ] {
        assert!(!router.contains(old), "不允许恢复全局禁用输入：{old}");
    }
    for old in [
        "enabled: !router.leftPointerInEditor",
        "enabled: !router.rightPointerInEditor",
    ] {
        assert!(!router.contains(old), "手势中途不得切换 Handler enabled：{old}");
    }
    for required in [
        "function arbitrateEditorPress(point, button, gestureId)",
        "canvas.commitNodeEditForOutsideGesture(",
        "property bool leftGestureBlocked: false",
        "property bool rightGestureBlocked: false",
        "property bool leftGestureFinished: true",
        "property bool rightGestureFinished: true",
        "onGrabChanged: function(transition, point)",
    ] {
        assert!(router.contains(required), "必须按实际编辑区域仲裁单次手势：{required}");
    }
    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        canvas.contains("property string inlineEditingKey: \"\"")
            && canvas.contains("function setInlineEditingKey(key)"),
        "Canvas 必须提供内联编辑汇总键，Router 据此让位"
    );
}

#[test]
fn no_external_node_edit_popup_route_remains() {
    let node = strip_line_comments(&starmap_node_delegate_src());
    assert!(
        !node.contains("editNodeRequested"),
        "节点卡片 delegate 不得再上抛 editNodeRequested：名称/正文编辑已在卡片内联完成"
    );

    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        !canvas.contains("editNodeRequested") && !canvas.contains("childEditNodeRequested"),
        "Canvas 不得再保留 editNodeRequested / childEditNodeRequested → Popup 路线"
    );

    let workspace = strip_line_comments(&read_src(WORKSPACE));
    for forbidden in [
        "inspectorPopup",
        "ownerScene",
        "ownerStarmapId",
        "ownerPathKey",
    ] {
        assert!(
            !workspace.contains(forbidden),
            "StarMapWorkspace 不得再保留 {forbidden}：节点标题编辑已移入节点，\
             Workspace 只创建根 Canvas、共享选中控制器和顶栏"
        );
    }
    assert!(
        workspace.contains("StarMapCanvas {") && workspace.contains("StarMapSelectionController {"),
        "Workspace 必须只创建根 StarMapCanvas 和共享的 StarMapSelectionController"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 8. 旧注释/抽象清理：不能再教人写“每层一个视口”
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn deleted_scene_canvas_hints_are_cleaned_up() {
    let canvas = read_src(CANVAS);
    assert!(
        !canvas.contains("右键空白处新建节点或子星图"),
        "空画布就是空画布，必须删掉教学占位文案（它只在两个模型都空时出现，\
         掩盖了真正的空图状态）"
    );
}

#[test]
fn no_source_documents_a_per_scene_viewport_abstraction() {
    // 旧注释会诱导后来者继续按“每个 Scene 自带 pan/zoom”写代码。
    for (label, source) in [
        (CANVAS, read_src(CANVAS)),
        (CONTENT, read_src(CONTENT)),
        (EMBED, read_src(EMBED)),
        ("节点卡片 delegate", starmap_node_delegate_src()),
        (INTERACTION, read_src(INTERACTION)),
        (WORKSPACE, read_src(WORKSPACE)),
    ] {
        let src = &source;
        for forbidden in [
            "每个 Scene 实例都有自己的",
            "子 Scene 自己的 pan",
            "childSceneLoader",
            "childScenePathKey",
        ] {
            assert!(
                !src.contains(forbidden),
                "{label} 不得再出现 {forbidden}：整棵星图只有一个全局视口，\
                 递归的是内容不是视口"
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// 9. 画布坐标只认 Qt 真实 Item 映射，不再手抄 sceneOrigin/sceneScale
// ─────────────────────────────────────────────────────────────────────────

/// #929：活动画布的局部坐标就是 authored world 坐标；输入增量只允许
/// 经 `qtSceneDeltaToWorld` 用真实 Item 映射（mapFromItem 两点差分）换算一次，
/// 不再手工维护 sceneOriginX/Y/sceneScale 矩阵，也没有每层 local fit 链。
#[test]
fn active_canvas_coordinates_use_real_item_mapping_not_a_manual_matrix() {
    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));
    let content = strip_line_comments(&read_src(CONTENT));
    let embed = strip_line_comments(&read_src(EMBED));

    for (rel, src) in [(ACTIVE_SCENE, &scene), (CONTENT, &content), (EMBED, &embed)] {
        for forbidden in ["sceneOriginX", "sceneOriginY", "sceneScale"] {
            assert!(
                !src.contains(forbidden),
                "{rel} 不得再手工维护 {forbidden}：命中/创建/连线只认真实 Item 映射"
            );
        }
    }

    let delta = function_window(&scene, "function qtSceneDeltaToWorld(", 500);
    assert!(
        delta.contains("var origin = scene.mapFromItem(null, 0, 0)")
            && delta.contains("var point = scene.mapFromItem(null, dx, dy)")
            && delta.contains("(point.x - origin.x) / _safeZoom")
            && delta.contains("(point.y - origin.y) / _safeZoom"),
        "Qt scene 增量必须先映射两个点再相减（mapFromItem 只能映射点），\
         再除以活动镜头比例换算 world，实际窗口:\n{delta}"
    );
    assert_eq!(
        count_occurrences(&scene, "function qtSceneDeltaToWorld("),
        1,
        "Qt scene → world 只允许一个换算入口定义"
    );
    assert_eq!(
        count_occurrences(&scene, "scene.mapFromItem(null,"),
        2,
        "增量换算只允许原点/增量两个点映射，不得再有第二套矩阵"
    );

    // 旧的 scene→local / root-scene 换算入口必须整体消失。
    for (rel, src) in [(ACTIVE_SCENE, &scene), (CONTENT, &content), (EMBED, &embed)] {
        for forbidden in [
            "qtSceneDeltaToLocal",
            "qtSceneDeltaToRootScene",
            "localToScene",
            "sceneToLocal",
            "mapToItem(",
        ] {
            assert!(
                !src.contains(forbidden),
                "{rel} 不得再保留旧的 scene→local 换算入口 {forbidden}"
            );
        }
    }

    // 命中对象的中心/矩形在活动画布 world 坐标里直接读取（local == world）。
    let center = function_window(&scene, "function itemCenterScene(", 300);
    assert!(
        center.contains("return { x: item.x + item.width / 2, y: item.y + item.height / 2 }"),
        "itemCenterScene 必须直接读 authored world 坐标（local == world），实际窗口:\n{center}"
    );
    let scene_rect = function_window(&scene, "function itemSceneRect(", 400);
    assert!(
        scene_rect
            .contains("return { x: item.x, y: item.y, width: item.width, height: item.height }"),
        "itemSceneRect 必须直接返回 authored world 矩形，实际窗口:\n{scene_rect}"
    );
    let move_delta = function_window(&scene, "function applyMoveDelta(", 400);
    assert!(
        move_delta.contains("qtSceneDeltaToWorld(dxQtScene, dyQtScene)"),
        "applyMoveDelta 必须用 qtSceneDeltaToWorld 换算一次，实际窗口:\n{move_delta}"
    );
    let connect_delta = function_window(&scene, "function applyConnectDelta(", 800);
    assert!(
        connect_delta.contains("qtSceneDeltaToWorld(dxQtScene, dyQtScene)"),
        "applyConnectDelta 必须用 qtSceneDeltaToWorld 换算，实际窗口:\n{connect_delta}"
    );

    // 只读概览不接收任何手工矩阵/视口参数：它自己 fit-to-bounds。
    for forbidden in [
        "childContentLoader",
        "sceneOriginX",
        "sceneOriginY",
        "sceneScale",
        "\"viewportRect\"",
        "ownerSceneContent",
    ] {
        assert!(
            !embed.contains(forbidden),
            "Embed 不得再向子内容复制 {forbidden}"
        );
    }

    // 换算函数里不得再手抄 contentViewport 的边框偏移常量。
    for marker in [
        "function qtSceneDeltaToWorld(",
        "function worldToScreenX(",
        "function screenToWorldX(",
    ] {
        let window = function_window(&scene, marker, 400);
        for forbidden in ["borderLeft", "borderTop", "titleBar", "- 24", "+ 6"] {
            assert!(
                !window.contains(forbidden),
                "{marker} 不得手抄边框偏移 {forbidden}：choreography 由 mapFromItem 承担，\
                 实际窗口:\n{window}"
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// 10. 只读概览 fit + 跨画布 70%/60% 穿越
// ─────────────────────────────────────────────────────────────────────────

/// #929 取代 #822：不再有"每层 local fit 显示变换"。活动画布直接按单一投影
/// 渲染 authored world 坐标；父图里的子入口只有只读 fit-to-bounds 微缩概览，
/// 缩放只走显示投影，不改 authored position。
#[test]
fn overview_fit_is_a_readonly_projection_not_a_world_transform() {
    let content = strip_line_comments(&read_src(CONTENT));
    for forbidden in [
        "worldLayer",
        "localFitScale",
        "localFitOffsetX",
        "localFitOffsetY",
        "effectiveScale",
        "contentUsableSide",
    ] {
        assert!(
            !content.contains(forbidden),
            "StarMapSceneContent 不得再出现每层 local fit / 递归比例链：{forbidden}"
        );
    }

    let fit = function_window(&content, "readonly property real fitScale:", 500);
    assert!(
        fit.contains("var side = Math.min(width, height) * _safeRatio")
            && fit.contains("return side / Math.max(b.width, b.height)"),
        "概览 fit 只按圆内接安全区对 authored bounds 做只读投影，实际窗口:\n{fit}"
    );
    let project = function_window(&content, "function projectX(wx)", 200);
    assert!(
        project.contains("width / 2 + (wx - _centerWorldX) * fitScale"),
        "概览投影必须用 fitScale 现算，不落到任何 Item transform，实际窗口:\n{project}"
    );

    // authored position 没有被改写：只读 position，不写任何 Core 接口。
    assert!(
        content.contains("n.position") && content.contains("e.position"),
        "只读概览必须直接读 authored position"
    );
    for forbidden in ["update_starmap", "create_starmap", "delete_starmap"] {
        assert!(
            !content.contains(forbidden),
            "只读概览不得调用任何 Core 写入接口：{forbidden}"
        );
    }

    // 活动画布的节点/入口也只用投影改显示尺寸，不把缩放乘进 authored 模型。
    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));
    assert!(
        scene.contains("x: scene.worldToScreenX(worldX)")
            && scene.contains("width: nodeData.width * scene._safeZoom")
            && scene.contains("width: embedData.width * scene._safeZoom"),
        "活动画布必须把 zoom 只乘进屏幕显示尺寸，实际源码缺少"
    );
    assert!(
        scene.contains("? scene.interactionController.moveX : nodeData.x"),
        "节点显示坐标的真相仍是 authored nodeData.x（投影只负责换算），实际源码缺少"
    );
}

/// #929 取代 #822：不再有 renderDetail 档位。父图里的子入口永远是只读
/// fit-to-bounds 矢量投影；命中永远不会穿透到概览内部，整块圆按入口命中。
#[test]
fn overview_renders_static_projection_and_passes_hits_through() {
    let content = strip_line_comments(&read_src(CONTENT));
    for forbidden in [
        "renderDetail",
        "hitTargetAtScene",
        "hitTargetAtWorld",
        "MouseArea",
        "TapHandler",
        "DragHandler",
        "WheelHandler",
        "PinchHandler",
    ] {
        assert!(
            !content.contains(forbidden),
            "只读概览不得参与命中 / 手势 / 档位：{forbidden}"
        );
    }
    assert!(
        content.contains("function projectX(wx)") && content.contains("function projectY(wy)"),
        "只读概览只能是静态 fit-to-bounds 投影"
    );
    assert_eq!(
        count_occurrences(&content, "PathSvg { path: overview."),
        3,
        "边 / 节点 / 子入口三条矢量路径都必须是绑定现算的静态投影"
    );

    // 懒加载且不参与命中：活动画布命中只看自己的模型，不查概览内容。
    let embed = strip_line_comments(&read_src(EMBED));
    assert!(
        embed.contains("active: root.previewWanted")
            && embed.contains("width >= 32 && height >= 32"),
        "概览必须由 previewWanted 懒加载，实际源码缺少"
    );
    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));
    assert!(
        !scene.contains("previewLoader") && !scene.contains("StarMapSceneContent"),
        "活动画布命中不得查概览内容：整块圆按 embed 入口命中（命中穿透到父层入口）"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 11. connect 端点：屏幕像素 vs 活动画布 world 增量两个口径分开
// ─────────────────────────────────────────────────────────────────────────

/// #929 取代 #822：端点存活动画布 world 坐标，只能累加
/// `qtSceneDeltaToWorld()` 的结果。把屏幕像素直接加到 world 上会随镜头
/// 缩放漂移（zoom=2 时端点多走一倍）。
#[test]
fn connect_endpoints_accumulate_active_canvas_world_delta_not_screen_pixels() {
    let src = strip_line_comments(&read_src(ACTIVE_SCENE));
    let convert = function_window(&src, "function qtSceneDeltaToWorld(", 400);
    assert!(
        convert.contains("scene.mapFromItem(null, 0, 0)")
            && convert.contains("scene.mapFromItem(null, dx, dy)")
            && convert.contains("point.x - origin.x")
            && convert.contains("point.y - origin.y"),
        "qtSceneDeltaToWorld 必须用活动画布的真实 Item 映射做两点差分，\
         实际窗口:\n{convert}"
    );

    let delta = function_window(&src, "function applyConnectDelta(", 800);
    assert!(
        delta.contains("var delta = qtSceneDeltaToWorld(dxQtScene, dyQtScene)")
            && delta.contains(
                "ic.updateConnect(ic.connectMouseX + delta.x, ic.connectMouseY + delta.y)"
            ),
        "connect 端点必须累加活动画布 world 增量，实际窗口:\n{delta}"
    );
    assert!(
        delta.contains("ic.connectMouseX += delta.x")
            && delta.contains("ic.connectMouseY += delta.y"),
        "contextPending 端点也必须累加换算后的 world 增量，实际窗口:\n{delta}"
    );
    assert!(
        !delta.contains("ic.connectMouseX += dxQtScene")
            && !delta.contains("ic.connectMouseY += dyQtScene"),
        "不得再把屏幕像素直接加到 world 端点上，实际窗口:\n{delta}"
    );

    // 拖动阈值在唯一 Router 里继续吃原始 Qt scene 像素。
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    let drag = function_window(&router, "function handleDragDelta(", 1400);
    assert!(
        drag.contains("ic.noteDragDelta(dx, dy)"),
        "拖动阈值必须继续吃原始 Qt scene 像素，实际窗口:\n{drag}"
    );
    assert!(
        drag.contains("if (ic.pressDragDistance > ic.moveThreshold)"),
        "contextPending 超过触屏阈值才转 connect，实际窗口:\n{drag}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 12. 概览几何靠绑定重算；按下归属用真正的画布坐标
// ─────────────────────────────────────────────────────────────────────────

/// #929 取代 #822：概览与活动画布都靠 QML 绑定重算几何，不再手工发
/// transformChanged 通知；任何输入（width/height/fitScale/zoom/pan）变化
/// 都会让这些 readonly 绑定自动重新求值。
#[test]
fn overview_geometry_rebinds_without_manual_transform_signal() {
    let content = strip_line_comments(&read_src(CONTENT));
    for forbidden in [
        "signal transformChanged",
        "transformChanged()",
        "onLocalFitScaleChanged",
    ] {
        assert!(
            !content.contains(forbidden),
            "只读概览不得再手工维护显示变换通知：{forbidden}"
        );
    }
    for content_prop in [
        "readonly property string edgesPath: {",
        "readonly property var projectedNodes: {",
        "readonly property var projectedEmbeds: {",
    ] {
        assert!(
            content.contains(content_prop),
            "概览几何必须是随投影重算的 readonly 绑定：缺 {content_prop}"
        );
    }
    assert!(
        content.contains("PathSvg { path: overview.edgesPath }")
            && content.contains("model: overview.projectedNodes")
            && content.contains("model: overview.projectedEmbeds"),
        "边用 ShapePath、节点/入口用真实投影 delegate，父图相机/尺寸变化自动重画"
    );

    // 活动画布同理：node/edge 投影绑定依赖 {zoomLevel, panX, panY, width, height}。
    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));
    let screen_x = function_window(&scene, "function worldToScreenX(", 200);
    assert!(
        screen_x.contains("_originWorldX") && screen_x.contains("_safeZoom"),
        "worldToScreenX 必须绑定当前镜头（zoom/pan）现算，实际窗口:\n{screen_x}"
    );
    let origin = function_window(&scene, "readonly property real _originWorldX:", 200);
    assert!(
        origin.contains("_finiteQuotient(_finiteDifference(width / 2, panX), _safeZoom)")
            && origin.contains("readonly property real _originWorldY:"),
        "投影原点必须同时依赖视口尺寸与 pan，并对极端值做有限收口，实际窗口:\n{origin}"
    );
}

/// Issue #832：Node/Embed 不再有按下 Handler；唯一 Router 铺满 Canvas，
/// `point.position` 就是 Canvas 局部坐标，命中与日志都从这一份坐标出发。
#[test]
fn press_handlers_pass_true_scene_coordinates() {
    for (label, source) in [
        ("节点卡片 delegate", starmap_node_delegate_src()),
        (EMBED, read_src(EMBED)),
        (CONTENT, read_src(CONTENT)),
    ] {
        let src = strip_line_comments(&source);
        assert!(
            !src.contains("itemPressed") && !src.contains("scenePressPosition"),
            "{label} 不得再保留 delegate press 通道 / scenePressPosition"
        );
    }

    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    let begin = function_window(&router, "function beginPress(", 2400);
    assert!(
        begin.contains("canvas.logPointerPress(\"left\", source, point, leftGestureId)")
            && begin.contains("hitAt(pressScreenX, pressScreenY)")
            && begin.contains("canvas.screenToWorldX(pressScreenX)"),
        "Router 必须用 Canvas 局部坐标命中并换算 scene 坐标，实际窗口:\n{begin}"
    );
    assert!(
        !router.contains("scenePressPosition"),
        "Router 铺满 Canvas，point.position 就是唯一需要的坐标口径"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 13. Embed 是正圆：圆外但落在外接矩形里的点不能命中 Embed
// ─────────────────────────────────────────────────────────────────────────

/// Embed 外壳 world 几何恒定 = 直径 200 的正圆；命中先做圆内判定，
/// 圆外即使还在外接矩形里也必须继续判为空白/下面的对象。
/// #929：命中几何搬到唯一活动画布 StarMapActiveScene（顶层 world 坐标）。
#[test]
fn embed_hit_testing_is_circular() {
    let src = strip_line_comments(&read_src(ACTIVE_SCENE));

    let circle = function_window(&src, "function _insideEmbedCircle(", 400);
    assert!(
        circle.contains("dx * dx + dy * dy <= c.radius * c.radius"),
        "圆内判定必须是 dx² + dy² <= r²，实际窗口:\n{circle}"
    );
    let ring = function_window(&src, "function _embedBorderRingWorld(", 300);
    assert!(
        ring.contains("Math.min(6 / _safeZoom, Math.min(e.width, e.height) * 0.25)"),
        "圆周环宽度必须按屏幕口径折算 world，实际窗口:\n{ring}"
    );

    let chrome = function_window(&src, "function findEmbedChromeAt(", 900);
    assert!(
        chrome.contains("if (!_insideEmbedCircle(e, wx, wy))")
            && chrome.contains("wy <= e.y + _embedTitleBandWorld(e)")
            && chrome.contains("var inner = c.radius - _embedBorderRingWorld(e)")
            && chrome.contains("if (dx * dx + dy * dy >= inner * inner)"),
        "findEmbedChromeAt 必须先做圆内判定，再分标题带/圆周环，实际窗口:\n{chrome}"
    );
    let content = function_window(&src, "function findEmbedContentAt(", 1200);
    assert!(
        content.contains("if (!_insideEmbedCircle(e, wx, wy))")
            && content.contains("wy <= e.y + _embedTitleBandWorld(e)")
            && content.contains("var inner = c.radius - _embedBorderRingWorld(e)")
            && content.contains("if (dx * dx + dy * dy >= inner * inner)"),
        "findEmbedContentAt 必须先做圆内判定，再排除标题带/圆周环，实际窗口:\n{content}"
    );

    // 旧的外接矩形整框命中必须整体消失：圆外不再有任何命中路径。
    for (name, file) in [
        (
            "StarMapActiveScene",
            &strip_line_comments(&read_src(ACTIVE_SCENE)),
        ),
        (
            "StarMapGraphController",
            &strip_line_comments(&read_src(CONTROLLER)),
        ),
        ("StarMapEmbed", &strip_line_comments(&read_src(EMBED))),
        (
            "StarMapSceneContent",
            &strip_line_comments(&read_src(CONTENT)),
        ),
    ] {
        assert!(
            !file.contains("_rectContains"),
            "{name} 不得再用外接矩形 _rectContains 当 Embed 命中真相"
        );
        assert!(
            !file.contains("_insideEmbedCircle(e, wx, wy) || ")
                && !file.contains("_insideEmbedCircle(em, wx, wy) || "),
            "{name} 的圆内判定不得被短路绕过"
        );
    }
    // 活动画布不得再使用固定 world chrome 常量：标题带/圆周环按屏幕口径换算。
    assert!(
        !src.contains("_chromeHeight") && !src.contains("_borderSlop"),
        "活动画布不得再使用固定 world chrome 常量，实际源码不符"
    );

    // Issue #832：Embed 自身不再维护输入 acceptance，命中只有一份真相；
    // Embed 只保留画出来的正圆和圆内安全区。
    let embed_src = strip_line_comments(&read_src(EMBED));
    for forbidden in [
        "TapHandler",
        "DragHandler",
        "PointHandler",
        "MouseArea",
        "containmentMask",
    ] {
        assert!(
            !embed_src.contains(forbidden),
            "Embed 不得再挂输入层 {forbidden}：命中统一走活动画布命中入口"
        );
    }
    assert!(
        embed_src.contains("radius: width / 2"),
        "Embed 外壳必须仍是正圆（radius = width / 2）"
    );
    assert!(
        embed_src.contains("readonly property real _inscribedSide: _diameter / Math.SQRT2"),
        "概览/标题必须约束在圆的内接正方形里，实际源码缺少"
    );
    // 四条矩形边框 + 矩形 titleBar 的 Handler 路线必须整体删除。
    for forbidden in [
        "id: titleBar",
        "id: borderTop",
        "id: borderBottom",
        "id: borderLeft",
        "id: borderRight",
    ] {
        assert!(
            !embed_src.contains(forbidden),
            "不得再保留矩形 chrome 命中结构 {forbidden}"
        );
    }

    // 概览 Loader 铺满整个圆盒：不再用更小的矩形 Item 制造接不到事件的死区。
    let preview_loader = function_window(&embed_src, "id: previewLoader", 200);
    assert!(
        preview_loader.contains("anchors.fill: parent"),
        "previewLoader 必须铺满圆盒，实际窗口:\n{preview_loader}"
    );

    // #1009 复审：概览已是真实节点卡片 / 文字 / 入口圆形几何的同一投影，
    // 不再构造节点小圆点或入口小圆点。
    let content_src = strip_line_comments(&read_src(CONTENT));
    assert!(
        content_src.contains("width: size.width * fitScale")
            && content_src.contains("text: modelData.title")
            && content_src.contains("text: modelData.body")
            && content_src.contains("radius: modelData.diameter / 2"),
        "概览必须投影真实节点卡片、正文和入口圆壳，实际源码缺少"
    );
    assert!(
        !content_src.contains("_overviewDotRadius")
            && !content_src.contains("readonly property string nodesPath")
            && !content_src.contains("readonly property string embedsPath"),
        "概览不得再按倍率把节点 / 入口换成小圆点"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 14. 网格 LOD / 按钮锚点 / 内容安全区 / 宿主连接（评论 5972557963）
// ─────────────────────────────────────────────────────────────────────────

/// 相机放开到 1e-4 后，背景网格必须自己抬 world 间距，
/// 屏幕上实际绘制间距不掉到亚像素，循环次数只跟屏幕尺寸有关。
#[test]
fn grid_lod_keeps_screen_spacing_bounded() {
    let src = strip_line_comments(&read_src(CANVAS));
    let grid = function_window(&src, "id: gridCanvas", 900);
    assert!(
        grid.contains("var worldSpacing = 50")
            && grid.contains("while (gridSpacing < 16)")
            && grid.contains("worldSpacing *= 5"),
        "网格必须按 5 倍档抬 world 间距直到屏幕间距 >= 16px，实际窗口:\n{grid}"
    );
    assert!(
        !grid.contains("var gridSpacing = 50 * zoomLevel\n"),
        "不得再直接用 50 * zoomLevel 当网格间距（1e-4 时会爆）"
    );
}

/// 触屏 +/- 的缩放锚点必须是画布中心，而不是按钮自己的宽高。
#[test]
fn zoom_buttons_anchor_at_canvas_center() {
    let src = strip_line_comments(&read_src(CANVAS));
    assert_eq!(
        count_occurrences(&src, "zoomAt(canvasArea.width / 2, canvasArea.height / 2,"),
        2,
        "+/- 两个按钮都必须以画布中心为锚点"
    );
    assert!(
        !src.contains("zoomAt(width / 2, height / 2,"),
        "不得再用按钮自己的宽高当缩放锚点"
    );
}

/// #929 取代 #822：不再有"父 Embed 传给子内容的安全区 + clamp"递归链。
/// 圆内安全区只约束**只读概览**（内接正方形 / 0.66 比例）；活动画布是
/// 无限画布，新建只做对象间空位选择，不做圆内 clamp。
#[test]
fn overview_fit_uses_the_circle_inscribed_safe_area_without_world_clamp() {
    let content = strip_line_comments(&read_src(CONTENT));
    let embed = strip_line_comments(&read_src(EMBED));
    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));

    for (name, src) in [
        ("StarMapSceneContent", &content),
        ("StarMapEmbed", &embed),
        ("StarMapActiveScene", &scene),
    ] {
        for forbidden in [
            "contentUsableSide",
            "clampToContentSafeArea",
            "_contentSafeSide",
        ] {
            assert!(
                !src.contains(forbidden),
                "{name} 不得再保留递归内容安全区 / world clamp：{forbidden}"
            );
        }
    }

    // 概览 fit 用圆内接安全区：内接正方形 1/√2 ≈ 0.707，取 0.66 留边距。
    assert!(
        content.contains("readonly property real _safeRatio: 0.66")
            && content.contains("var side = Math.min(width, height) * _safeRatio"),
        "概览必须以圆内接安全区为上限 fit-to-bounds"
    );
    assert!(
        embed.contains("readonly property real _inscribedSide: _diameter / Math.SQRT2"),
        "Embed 的标题/概览区域必须来自圆的内接正方形"
    );

    // 活动画布是无限画布：新建只做碰撞空位选择，不做安全区 clamp。
    assert!(
        !scene.contains("clamp"),
        "活动画布不得有任何安全区 clamp（无限画布），实际源码不符"
    );
    let spawn = function_window(&scene, "function findFreeSpawnPoint(", 1200);
    assert!(
        spawn.contains("overlapsExistingNode(candidates[i].x, candidates[i].y,")
            && spawn.contains("StarMapNodeMetrics.defaultNodeWidth()")
            && spawn.contains("StarMapNodeMetrics.defaultNodeHeight(), 12)")
            && !spawn.contains(", 150, 60,")
            && spawn.contains("return { x: wx, y: wy }"),
        "空位选择只避开已有对象，并用统一卡片度量，不把新建拽回圆内安全区，实际窗口:\n{spawn}"
    );
    let create_node = function_window(&scene, "function createNodeWithName(", 400);
    assert!(
        create_node.contains("var spawn = findFreeSpawnPoint(sceneX, sceneY)")
            && create_node.contains("graphController.createNode(name, spawn.x, spawn.y)"),
        "新建节点必须写进活动画布自己的 GraphController，实际窗口:\n{create_node}"
    );

    // 拖动提交写的是拖动候选坐标本身：显示 == 写 Core，没有二次 clamp。
    let finish = function_window(&scene, "function finishMove(", 900);
    assert!(
        finish.contains("committed = graphController.commitNodeMove(id, nx, ny)")
            && finish.contains("committed = graphController.commitEmbedMove(id, nx, ny)"),
        "拖动提交必须直接写候选坐标（无限画布无安全区 clamp），实际窗口:\n{finish}"
    );
    assert!(
        finish.contains("var nx = ic.moveX") && finish.contains("var ny = ic.moveY"),
        "提交坐标必须就是拖动显示坐标（共享状态机的 moveX/moveY），实际窗口:\n{finish}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 15. 边几何 / 屏幕命中 / 入口命中 / Core 宿主规划（评论 5976758184）
// ─────────────────────────────────────────────────────────────────────────

/// 正式边端点必须由形状真实边界求交得到：Node 取矩形边、Embed 取圆周，
/// 不再靠固定 42px 内缩猜边界（200 正圆上会扎进圆内 ~58px）。
#[test]
fn edge_render_uses_real_shape_boundaries() {
    let src = read_src("src/starmap_view/edge_render.rs");
    assert!(
        !src.contains("DEFAULT_ARROW_PADDING") && !src.contains("arrow_padding"),
        "固定 42px 内缩常量必须删除，不能再作为端点定位依据"
    );
    assert!(
        src.contains("pub fn endpoint_boundary_point(")
            && src.contains("fn line_circle_entry(")
            && src.contains("fn line_rect_entry("),
        "端点必须按形状（矩形 / 圆周）与连线求交"
    );
    let render = slice_between(&src, "fn compute_renders_from_resolved(", "fn coords_eq(");
    assert!(
        render.contains("endpoint_boundary_point(&edge.from,")
            && render.contains("endpoint_boundary_point(&edge.to,")
            && render.contains("start_x: sx")
            && render.contains("end_x: ex"),
        "start/end 必须直接取真实边界交点，实际片段:\n{render}"
    );
    // 旧 Portal 在 UI 归一到正圆 Embed：可见边界也必须走圆周。
    assert!(
        src.contains("legacy-portal:{}")
            && src.contains("is_embed: true")
            && src.contains(".map(|n| n.portal.is_some())"),
        "旧 Portal 端点的可见身份是正圆 Embed，必须按圆求交"
    );
    // UI 命中路径不得再吃固定 world 阈值。
    let bridge = read_src("src/starmap_view/bridge.rs");
    assert!(
        bridge.contains("hit_test_edge_renders_with_threshold(x, y, &renders, threshold)"),
        "bridge 的边命中必须走带 threshold 的入口"
    );
}

/// 边命中阈值必须按屏幕像素折算到活动画布 world 单位（镜头 1e-4~1e5 后不能再固定）。
#[test]
fn edge_hit_threshold_is_screen_relative() {
    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));
    assert!(
        scene.contains("readonly property real _edgeHitScreenPx: 10"),
        "命中阈值必须是屏幕像素常量"
    );
    // 评论 5977083591 问题 2：命中范围不得小于真实可见线宽的一半，
    // 高倍放大时不能"点在肉眼很粗的线里却点不中"。
    assert!(
        scene.contains("readonly property real _edgeLineWorldWidth: 2")
            && scene.contains("Math.max(_edgeHitScreenPx / _safeZoom, _edgeLineWorldWidth / 2)"),
        "阈值必须同时覆盖屏幕像素下限与可见线宽下限，实际源码缺少"
    );
    let threshold = function_window(&scene, "function _edgeHitThresholdWorld(", 400);
    assert!(
        threshold.contains("_edgeHitScreenPx / _safeZoom"),
        "阈值必须除以活动镜头比例折算到画布 world 单位，实际窗口:\n{threshold}"
    );
    assert!(
        scene.contains("graphController.hitTestEdge(wx, wy, _edgeHitThresholdWorld())"),
        "活动画布命中必须把折算后的阈值传给 hitTestEdge"
    );

    // 绘制与命中同源：活动画布 / 概览 / 预览线都用同一份线宽口径。
    assert_eq!(
        count_occurrences(&scene, "strokeWidth: scene._edgeScreenStrokeWidth"),
        1,
        "活动画布连线必须用屏幕口径 _edgeScreenStrokeWidth 绘制"
    );
    assert!(
        scene.contains("Math.max(1.0, Math.min(_edgeLineWorldWidth * _safeZoom, 6.0))"),
        "线宽必须由 _edgeLineWorldWidth × 镜头比例夹定，实际源码缺少"
    );
    let content = strip_line_comments(&read_src(CONTENT));
    assert_eq!(
        count_occurrences(&content, "strokeWidth: overview._previewStrokeWidth"),
        3,
        "概览的边 / 节点 / 子入口都必须用同一份概览线宽常量"
    );
    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        canvas.contains("ctx.lineWidth = canvasArea._edgeScreenStrokeWidth"),
        "Canvas 的连线预览线必须跟同一份线宽常量"
    );
    for (name, src) in [
        ("StarMapActiveScene", &scene),
        ("StarMapSceneContent", &content),
        ("StarMapCanvas", &canvas),
    ] {
        assert!(
            !src.contains("ctx.lineWidth = 2") && !src.contains("lineWidth = 2"),
            "{name} 不得再保留字面量线宽（绘制与命中必须同源）"
        );
    }

    let controller = strip_line_comments(&read_src(CONTROLLER));
    assert!(
        controller.contains("function hitTestEdge(wx, wy, threshold)")
            && controller
                .contains("hit_test_edge_renders(JSON.stringify(edgeRenders), wx, wy, threshold)"),
        "GraphController 必须把 threshold 透传给后端"
    );
}

/// 拉线预览与正式边共用同一套几何真相：悬停合法 target 时先让 Core 规划宿主，
/// 再把路径交给平台 edge renderer（候选边临时追加进宿主边表）；
/// 深路径投影、双向偏移都只有 Rust 一份实现，QML 不再抄第二份。
/// #929：宿主必须是当前活动画布，坐标就是画布 world 坐标。
#[test]
fn connect_preview_reuses_platform_edge_renderer() {
    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));
    let boundary = function_window(&scene, "function boundaryPointWorld(", 1500);
    assert!(
        boundary.contains("var radius = Math.min(width, height) / 2")
            && boundary.contains("var tMin = Math.max(txMin, tyMin)")
            && boundary.contains("var t = tMin >= 0 ? tMin : tMax"),
        "自由预览的边界必须按矩形/圆周求交（与 Rust line_rect_entry / line_circle_entry 同语义），\
         实际窗口:\n{boundary}"
    );

    // 宿主把规划结果交给平台 edge renderer：候选边 = 现有边表 + __preview__。
    let prospective = function_window(&scene, "function prospectiveEdgeRenderForPlan(", 500);
    assert!(
        prospective.contains("graphController.computeProspectiveEdgeRender(plan.from, plan.to)"),
        "活动画布必须把 Core 规划的 from/to 交给平台候选边入口，实际窗口:\n{prospective}"
    );
    assert!(
        scene.contains("function planCrossLayerConnection(")
            && scene.contains("starmapBackendRef.plan_cross_layer_connection("),
        "活动画布必须把两条绝对端点路径交给 Core 的 plan_cross_layer_connection"
    );

    // 悬停合法 target：Core plan → 活动画布 → 平台 renderer → world 端点。
    let refresh = function_window(&scene, "function refreshConnectPreview(", 2400);
    assert!(
        refresh.contains("planCrossLayerConnection(ic.connectFromPath, hit.targetPath)")
            && refresh.contains("_planHostIsCurrent(plan)")
            && refresh.contains("var preview = prospectiveEdgeRenderForPlan(plan)"),
        "预览必须先让 Core 规划宿主（宿主必须是本画布）再交给平台 renderer，\
         实际窗口:\n{refresh}"
    );
    assert!(
        refresh.contains("ic.connectFromSceneX = preview.startX")
            && refresh.contains("ic.connectFromSceneY = preview.startY")
            && refresh.contains("ic.connectPreviewEndX = preview.endX")
            && refresh.contains("ic.connectPreviewEndY = preview.endY"),
        "预览起止点必须直接取候选边 render（就是画布 world 坐标），实际窗口:\n{refresh}"
    );
    assert!(
        refresh.contains("ic.connectPreviewEndX = ic.connectMouseX")
            && refresh.contains(
                "var boundary = boundaryPointWorld(item.x, item.y, item.width, item.height,"
            ),
        "没有合法 target 时必须退回“源对象边界 → 当前鼠标”，实际窗口:\n{refresh}"
    );

    // 第二份几何真相（QML 版双向偏移 / 段身份映射）必须整体消失。
    for (name, src) in [
        (
            "StarMapSceneContent",
            &strip_line_comments(&read_src(CONTENT)),
        ),
        ("StarMapActiveScene", &scene),
    ] {
        assert!(
            !src.contains("visibleEndpointShapeForHostPath")
                && !src.contains("uiInstanceIdOfPathSegment"),
            "{name} 不得再保留第二份宿主可见形状 / 段身份映射：几何真相只在 Rust"
        );
    }
    for (name, src) in [
        (
            "StarMapSceneContent",
            &strip_line_comments(&read_src(CONTENT)),
        ),
        (
            "StarMapGraphController",
            &strip_line_comments(&read_src(CONTROLLER)),
        ),
    ] {
        assert!(
            !src.contains("bidirectional") && !src.contains("BIDIRECTIONAL"),
            "{name} 不得自己实现双向边偏移——必须复用平台 edge renderer"
        );
    }

    // 平台侧候选边入口：临时追加 __preview__ 边后只取它自己的 render。
    let rust = read_src("src/starmap_view/edge_render.rs");
    assert!(
        rust.contains("pub const PROSPECTIVE_EDGE_ID: &str = \"__preview__\"")
            && rust.contains("pub fn compute_prospective_edge_render(")
            && rust.contains("edges.push(candidate)"),
        "Linux edge_render 必须有候选边预览入口（临时追加候选边）"
    );
    let batch = function_window(&rust, "pub fn compute_prospective_edge_render(", 2200);
    assert!(
        batch.contains(
            "compute_edge_renders_from_paths(&edges, graph, layout, embed_rects, params)"
        ),
        "候选边必须复用正式边 renderer，不能另写一套算法，实际窗口:\n{batch}"
    );
    let bridge = read_src("src/starmap_view/bridge.rs");
    assert!(
        bridge.contains("pub fn compute_prospective_edge_render_json(")
            && bridge.contains("edge_render::compute_prospective_edge_render("),
        "bridge 必须透出候选边预览入口"
    );
    let controller = strip_line_comments(&read_src(CONTROLLER));
    assert!(
        controller.contains("function computeProspectiveEdgeRender(fromPath, toPath)")
            && controller.contains("starmapBackendRef.compute_prospective_edge_render("),
        "GraphController 必须把候选边请求透传给后端"
    );

    // Router 在进入 connect 与 connect 拖动（左键长按、contextPending 转换、
    // 右键拖线）都必须刷新预览端点；#930 复核后端点刷新收敛到唯一 helper
    // updateConnectEndpointFromScreen()：终点取指针绝对位置（screen → 活动画布
    // world），不累计位移，所有进入/每帧路径都经它刷新。
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    assert_eq!(
        count_occurrences(&router, "target.refreshConnectPreview()"),
        1,
        "connect 端点刷新必须收敛到唯一 helper（owner 由按键分别传入，绝对指针端点）"
    );
    assert!(
        count_occurrences(&router, "updateConnectEndpointFromScreen(") >= 8,
        "长按进入 connect / contextPending 转 connect / 左键与右键每帧拖动都必须走同一端点刷新 helper"
    );
    let scene_delta = function_window(&scene, "function applyConnectDelta(", 800);
    assert!(
        scene_delta.contains("refreshConnectPreview()"),
        "connect 拖动必须刷新预览端点，实际窗口:\n{scene_delta}"
    );

    // 控制器持有预览端点字段；Canvas overlay 经单一投影画到它。
    let interaction = read_src(INTERACTION);
    assert!(
        interaction.contains("property real connectPreviewEndX: 0")
            && interaction.contains("property real connectPreviewEndY: 0"),
        "预览端点必须由共享状态机持有，实际源码缺少"
    );
    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        canvas.contains("ctx.lineTo(canvasArea.worldToScreenX(interaction.connectPreviewEndX),"),
        "预览线必须经活动画布投影画到 connectPreviewEnd，实际源码缺少"
    );
}

/// 边命中范围与可见线宽同源：缩小时至少 10 屏幕像素，放大时至少覆盖线本体；
/// #929：活动画布用 ShapePath 画边、概览用 ShapePath、预览 overlay 用 Canvas，
/// 三处都必须从同一份 _edgeLineWorldWidth 派生屏幕线宽。
#[test]
fn edge_hit_threshold_covers_visible_line_width() {
    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));
    assert!(
        scene.contains("readonly property real _edgeLineWorldWidth: 2"),
        "边线宽必须是单一 world 常量（绘制与命中共用）"
    );
    let threshold = function_window(&scene, "function _edgeHitThresholdWorld(", 400);
    assert!(
        threshold
            .contains("return Math.max(_edgeHitScreenPx / _safeZoom, _edgeLineWorldWidth / 2)"),
        "命中阈值必须同时覆盖屏幕像素下限与真实线宽，实际窗口:\n{threshold}"
    );

    // 绘制不许再手写 lineWidth = 2：活动画布/概览/预览 overlay 都从同一常量派生。
    assert!(
        count_occurrences(&scene, "strokeWidth: scene._edgeScreenStrokeWidth") == 1
            && scene.contains("readonly property real _edgeScreenStrokeWidth:")
            && scene.contains("Math.min(_edgeLineWorldWidth * _safeZoom, 6.0)"),
        "活动画布连线必须由 _edgeLineWorldWidth 派生屏幕线宽，实际源码不符"
    );
    let content = strip_line_comments(&read_src(CONTENT));
    assert_eq!(
        count_occurrences(&content, "strokeWidth: overview._previewStrokeWidth"),
        3,
        "概览的边 / 节点 / 子入口必须统一用 _previewStrokeWidth"
    );
    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        canvas.contains("ctx.lineWidth = canvasArea._edgeScreenStrokeWidth")
            && canvas.contains("readonly property real _edgeScreenStrokeWidth:")
            && canvas.contains("Math.min(_edgeLineWorldWidth * zoomLevel, 6.0)"),
        "连线预览线宽必须跟同一份 world 线宽常量，实际源码不符"
    );
    for (name, src) in [
        ("StarMapActiveScene", &scene),
        ("StarMapSceneContent", &content),
        ("StarMapCanvas", &canvas),
    ] {
        assert!(
            !src.contains("ctx.lineWidth = 2") && !src.contains("lineWidth = 2"),
            "{name} 不得再保留字面量线宽（绘制与命中必须同源）"
        );
    }
}

/// #929 取代 #822：不再有 LOD / childContent 档位。子入口整个圆在任何
/// 投影大小下都是 embed 命中（不是死区）；右键开的是这个子星图自己的菜单，
/// 不会落到空白新建。
#[test]
fn embed_entry_hit_is_not_a_dead_zone() {
    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));
    let hit = function_window(&scene, "function hitTargetAtWorld(", 2600);
    assert!(
        hit.contains("var inside = findEmbedContentAt(wx, wy)")
            && hit.contains("kind: \"embed\", id: inside.instanceId")
            && !hit.contains("kind: \"childContent\""),
        "子星图内容区必须整体按 embed 入口命中，不再有 childContent 死区，实际窗口:\n{hit}"
    );
    assert!(
        !hit.contains("zoomLevel") && !hit.contains("_safeZoom"),
        "入口命中不得按投影大小 / LOD 降级：整个圆始终可命中，实际窗口:\n{hit}"
    );

    // 右键统一分发：embed 命中开 embed 菜单；只有真正 empty 才走空白新建。
    let canvas = strip_line_comments(&read_src(CANVAS));
    let dispatch = function_window(&canvas, "function openHitContextMenu(", 2400);
    assert!(
        dispatch.contains("hit.kind === \"embed\"")
            && dispatch.contains("embedContextMenu.popup(screenX, screenY)"),
        "embed 命中必须开 embed 菜单，不弹空白新建，实际窗口:\n{dispatch}"
    );
    assert!(
        dispatch.contains("openBlankMenu(sx, sy, hit, screenX, screenY)"),
        "只有真正 empty 才走该层空白菜单，实际窗口:\n{dispatch}"
    );
}

/// #929 取代 #822：不再有父内容里递归的子内容路径段。旧 Portal 的身份分流
/// 仍在 GraphController.embedPathSegment（enterPortal / enterEmbed），由活动
/// 画布拼绝对路径；跨层宿主规划仍整体归 Core。
#[test]
fn legacy_portal_path_segments_and_edge_paths_share_one_truth() {
    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));
    let embed_path = function_window(&scene, "function embedPath(instanceId)", 400);
    assert!(
        embed_path.contains("graphController.embedPathSegment(instanceId)"),
        "embedPath 的路径段必须由 Controller 分流后传入，实际窗口:\n{embed_path}"
    );
    let embed_source = function_window(&scene, "function embedSourcePath(instanceId)", 400);
    assert!(
        embed_source.contains("graphController.embedPathSegment(instanceId)"),
        "embedSourcePath 也必须经 Controller 分流，实际窗口:\n{embed_source}"
    );

    // Controller 分流唯一真相：旧 portal 走 enterPortal，正式 Embed 走 enterEmbed。
    let controller = strip_line_comments(&read_src(CONTROLLER));
    assert!(
        controller.contains("function embedPathSegment(instanceId)")
            && controller.contains("type: \"enterPortal\"")
            && controller.contains("nodeId: embed.legacyPortalNodeId")
            && controller.contains("type: \"enterEmbed\""),
        "GraphController.embedPathSegment 必须保留 enterPortal / enterEmbed 分流"
    );

    // Embed 自己不再持有路径段，也不自己猜 enterEmbed 段。
    let embed = strip_line_comments(&read_src(EMBED));
    assert!(
        !embed.contains("pathSegment")
            && !embed.contains("childContentPathSegments")
            && !embed.contains("{ type: \"enterEmbed\", instanceId: instanceId, nodeId: null }"),
        "Embed 不得再持有 / 猜路径段（节点身份只在活动画布与 Controller）"
    );

    // Embed-like 端点的原始 terminal segment（含旧 portal 的 enterPortal）保留
    // 逻辑已收回 Core：见 core/writer_core/src/starmap/graph/path_planner.rs 的
    // endpoint_scene / build_target_path_for_host。QML planner 不再持有它。
    let planner = read_src("qml/StarMapPathPlanner.js");
    assert!(
        !planner.contains("terminalSegment"),
        "planner 不得再持有 Embed-like terminal segment 逻辑（已收回 Core）"
    );
    assert!(
        scene.contains("planCrossLayerConnection(")
            && scene.contains("plan_cross_layer_connection"),
        "活动画布必须把两端绝对路径交给 Core 的 plan_cross_layer_connection"
    );
}
// ─────────────────────────────────────────────────────────────────────────
// 14. 双指 Pinch 纳入唯一手势状态机（pinch 优先，单指业务不得跨缩放执行）
// ─────────────────────────────────────────────────────────────────────────

/// #373 的"双指缩放优先"：pinch 一旦激活就接管整个状态机。
/// Qt 的 passive grab 在别的 handler 拿到 exclusive grab 后仍会收到移动和
/// release，只靠 grab 层级清不掉 pointerMode，必须显式 beginPinch/endPinch。
#[test]
fn pinch_takes_over_the_shared_state_machine() {
    let controller_src = read_src(INTERACTION);
    assert!(
        controller_src
            .contains("idle / pressPending / pan / connect / move / contextPending / pinch"),
        "状态机文档必须包含 pinch 状态"
    );
    let controller = strip_line_comments(&controller_src);
    let begin = function_window(&controller, "function beginPinch()", 300);
    assert!(
        begin.contains("reset()")
            && begin.contains("pointerMode = \"pinch\"")
            && begin.contains("pointerSource = \"touch\""),
        "beginPinch 必须先清单指现场再进 pinch，实际窗口:\n{begin}"
    );
    let end = function_window(&controller, "function endPinch()", 220);
    assert!(
        end.contains("if (pointerMode === \"pinch\")") && end.contains("reset()"),
        "endPinch 必须在 pinch 状态下整体复位，实际窗口:\n{end}"
    );

    // Issue #832：PinchHandler 与单指 Handler 都在唯一 Router 里。
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    let pinch = function_window(&router, "id: pinchHandler", 1300);
    assert!(
        pinch.contains("canvas.resetInteraction()")
            && pinch.contains("canvas.hideTouchPreview()")
            && pinch.contains("router.ic.beginPinch()"),
        "Pinch 激活必须先清瞬时现场再进 pinch，实际窗口:\n{pinch}"
    );
    assert!(
        pinch.contains("router.ic.endPinch()"),
        "Pinch 结束必须复位 pinch 状态，实际窗口:\n{pinch}"
    );

    // 单指业务在 pinch 期间必须整体让位：grab 与 pointerMode 两层都要看。
    let helper = function_window(&router, "readonly property bool pinchActive:", 200);
    assert!(
        helper.contains("pinchHandler.active") && helper.contains("ic.pointerMode === \"pinch\""),
        "必须有一个同时看 grab 与共享状态机的触屏让位判定，实际窗口:\n{helper}"
    );
    let drag = function_window(&router, "function handleDragDelta(", 500);
    assert!(
        drag.contains("if (pinchActive)") && drag.contains("return"),
        "pinch 期间单指拖动必须直接忽略，实际窗口:\n{drag}"
    );
    let single = function_window(&router, "function handleSingleTap(", 400);
    assert!(
        single.contains("if (ic.pointerMode === \"pinch\")") && single.contains("return"),
        "pinch 期间必须拒绝迟到的触屏点选，实际窗口:\n{single}"
    );
    let long_press = function_window(&router, "function handleLongPress(", 300);
    assert!(
        long_press.contains("if (pinchActive)") && long_press.contains("return"),
        "晚到的长按必须拒绝 pinch 状态，实际窗口:\n{long_press}"
    );

    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));
    let begin_edit = function_window(&scene, "function beginInlineEdit(", 300);
    assert!(
        begin_edit.contains("interactionController.pointerMode === \"pinch\"")
            && begin_edit.contains("return"),
        "晚到的双击进入编辑必须拒绝 pinch 状态，实际窗口:\n{begin_edit}"
    );
    let release = function_window(&router, "function finishLeftGesture(", 1800);
    for mode in [
        "\"connect\"",
        "\"move\"",
        "\"contextPending\"",
        "\"pressPending\"",
    ] {
        assert!(
            release.contains(mode),
            "finishLeftGesture 必须继续处理 {mode}，实际窗口:\n{release}"
        );
    }
    assert!(
        !release.contains("\"pinch\" === mode") && !release.contains("if (mode === \"pinch\")"),
        "手势收尾不得把 pinch 当业务状态（缩放结束由 endPinch 复位），实际窗口:\n{release}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 15. 单击选中：delegate 只上抛点击信号，归属层负责选中与边界日志
// ─────────────────────────────────────────────────────────────────────────

/// #822 重构后 Node 的 singleClicked / Embed 的 clicked 一度无人接：单击节点
/// 或子星图不产生任何选中。点击语义必须回到归属层（delegate 不直接碰 controller）。
#[test]
fn node_and_embed_single_click_wire_to_selection() {
    // Issue #832：单击语义收进唯一 Router；delegate 不再上抛点击信号。
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    let select = function_window(&router, "function selectHit(", 1000);
    assert!(
        select.contains("hit.owner.selectNode(hit.id)")
            && select
                .contains("hit.owner.logInteraction(\"selection_changed\", \"node\", hit.id, {})"),
        "node 单击必须选中并记 selection_changed，实际窗口:\n{select}"
    );
    assert!(
        select.contains("hit.owner.selectEmbed(hit.id)")
            && select
                .contains("hit.owner.logInteraction(\"selection_changed\", \"embed\", hit.id, {})"),
        "embed 单击必须选中并记 selection_changed，实际窗口:\n{select}"
    );
    for (label, source) in [
        ("节点卡片 delegate", starmap_node_delegate_src()),
        (EMBED, read_src(EMBED)),
    ] {
        let src = strip_line_comments(&source);
        assert!(
            !src.contains("singleClicked") && !src.contains("signal clicked"),
            "{label} 不得再上抛点击信号：输入只有一个主人"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// 16. Pinch 期间在 enabled 层禁用触屏 TapHandler（不是回调时判断）
// ─────────────────────────────────────────────────────────────────────────

/// 评论 5977714294：TapHandler 默认 DragThreshold + passive grab，singleTapped
/// 在 release 时才发；"Pinch 先变 inactive → 状态机复位 → 同一个 release 再判
/// tap" 的时序下，回调里的 guard 两个条件都已回到 false。必须在 pinch 激活期间
/// 直接把 enabled 置 false，让这一轮 tap 识别当场取消（回调 guard 留作双保险）。
#[test]
fn pinch_disables_touch_taps_at_the_handler_level() {
    // Issue #832：单指 TapHandler 只有 Router 一份，pinch 期间在 enabled 层直接禁用，
    // 让 passive grab 的 tap 识别当场取消；回调 guard 作双保险。
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    let touch_tap = function_window(&router, "id: touchTap", 500);
    assert!(
        touch_tap.contains("enabled: !router.pinchActive"),
        "Router 触屏 TapHandler 必须在 pinch 期间直接禁用，实际窗口:\n{touch_tap}"
    );
    let helper = function_window(&router, "readonly property bool pinchActive:", 200);
    assert!(
        helper.contains("pinchHandler.active") && helper.contains("ic.pointerMode === \"pinch\""),
        "pinchActive 必须同时看 grab 与共享状态机，实际窗口:\n{helper}"
    );
    let single = function_window(&router, "function handleSingleTap(", 400);
    assert!(
        single.contains("if (ic.pointerMode === \"pinch\")") && single.contains("return"),
        "迟到的点选必须拒绝 pinch 状态，实际窗口:\n{single}"
    );

    let pinch = function_window(&router, "id: pinchHandler", 1200);
    assert!(
        pinch.contains("router.ic.beginPinch()") && pinch.contains("router.ic.endPinch()"),
        "Pinch 激活/结束必须整体接管与复位单指状态，实际窗口:\n{pinch}"
    );

    // 递归 delegate 不再持有触屏手势，也就不需要 touchGestureBlocked。
    for (label, source) in [
        ("节点卡片 delegate", starmap_node_delegate_src()),
        (EMBED, read_src(EMBED)),
        (CONTENT, read_src(CONTENT)),
    ] {
        let src = strip_line_comments(&source);
        assert!(
            !src.contains("touchGestureBlocked"),
            "{label} 不得再保留 touchGestureBlocked：delegate 已没有触屏 Handler"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// 17. 箭头命中 + 鼠标入口收敛（评论 5977879544）
// ─────────────────────────────────────────────────────────────────────────

/// 画出来的箭头三角形也是边的一部分：命中必须覆盖箭头内部与三条箭头边，
/// 否则高倍缩放（阈值为 1 world）时会出现"点在箭头上却点不中"。
#[test]
fn edge_hit_testing_covers_rendered_arrow_triangle() {
    let hittest = read_src("src/starmap_view/hittest.rs");
    assert!(
        hittest.contains("pub fn point_in_triangle("),
        "命中几何必须提供三角形内部判定"
    );
    let edge = read_src("src/starmap_view/edge_render.rs");
    assert!(
        edge.contains("fn arrow_triangle_distance(")
            && edge.contains("shaft.min(arrow_triangle_distance(x, y, r))")
            && edge.contains("point_in_triangle(x, y, tx, ty, lx, ly, rx, ry)"),
        "边命中必须取 箭杆 ∪ 箭头三角形 的最短距离，实际源码缺少"
    );
    let tests = read_src("src/starmap_view/edge_render/tests.rs");
    assert!(
        tests.contains("fn hit_test_edge_render_covers_arrow_triangle(")
            && tests.contains("fn formal_edge_arrow_is_clickable_at_small_threshold("),
        "必须有「箭头内部小 threshold 命中」的回归测试"
    );
}

/// 鼠标入口只有既定那套：中键历史 pan 分支（观察器 / acceptedButtons / 分支 /
/// 日志）必须彻底删除，不再维护第二个历史快捷入口。
#[test]
fn middle_button_pan_entry_is_removed() {
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    for (name, src) in [
        ("StarMapCanvas", strip_line_comments(&read_src(CANVAS))),
        ("StarMapInputRouter", router.clone()),
    ] {
        assert!(
            !src.contains("Qt.MiddleButton") && !src.contains("\"middle\""),
            "{name} 不得再有中键 press 观察器 / pan 分支 / middle 日志"
        );
    }
    let drag = function_window(&router, "id: dragHandler", 800);
    assert!(
        drag.contains("acceptedButtons: Qt.LeftButton"),
        "Router 的拖动只接受左键，实际窗口:\n{drag}"
    );
}
