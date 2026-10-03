//! Issue #822 — Linux_Qt 星图重构为单一全局视口守卫。
//!
//! 诊断包 `sujian-diagnostics-20261003-222534.zip` 暴露两个根因：
//! 1. 371 次 `starmap.*` 交互里 `connect_begin=0` / `connect_end=0` —— 不是后端建边
//!    失败，而是前端根本没进 connect。旧实现让 `DragHandler` 一有位移就抢进 move，
//!    `TapHandler.onLongPressed` 再调 `beginConnect()` 时已经不是 idle。
//! 2. 子图解析先以 `depth=1 + pathKey=root` 出现，之后才变成正确的
//!    `root/embed_...`。子 Scene 的 `pathKey` 默认 `"root"`，初始化期间短暂冒充根层；
//!    而 Canvas 又用 `pathKey === "root"` 决定滚轮入口 —— 身份和相机耦合在一起。
//!
//! 本文件锁住重构后的架构不变量，防止回退到"每个 Scene 一个视口"：
//!
//! - 整棵星图只有一个全局相机：panX/panY/zoomLevel 与 WheelHandler/PinchHandler
//!   只存在于根 StarMapCanvas；子层内容容器不含任何相机属性或相机手势。
//! - 递归的是内容不是视口：StarMapSceneContent 递归创建自己，路径身份
//!   `scenePathKey` 是 required 且没有默认 `"root"`，子层创建时一次性传全路径。
//! - 命中判断只有一个入口：根层内容的递归 `hitTargetAtScene()`，返回真正命中的
//!   那一层；连线松手 / 右键空白新建 / 选中 / pointer_press 都走它。
//! - 手势仲裁显式化：`idle -> pressPending`，先超拖动阈值转 move，先到长按时间
//!   转 connect，状态只被提升一次；长按计时器挂在 Item 下的 Canvas 里。
//! - 节点标题就地内联编辑（TextInput），不再经过外部 Inspector Popup；
//!   StarMapInspector.qml / StarMapScene.qml 直接删除。
//!
//! QML 组件依赖 qml_resources qrc 与 Rust 注册的上下文属性，无法在本仓库的
//! Rust 测试里实例化，因此按仓库既有惯例（issue801/issue814/issue817 系列
//! WHITE_BOX 守卫）读取 QML 源码确定性断言。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{count_occurrences, function_window, linux_qt_root, read_src};

const CANVAS: &str = "qml/StarMapCanvas.qml";
const CONTENT: &str = "qml/StarMapSceneContent.qml";
const EMBED: &str = "qml/StarMapEmbed.qml";
const NODE: &str = "qml/StarMapNode.qml";
const INTERACTION: &str = "qml/StarMapInteractionController.qml";
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
// 2. 全局相机唯一：panX/panY/zoomLevel 与相机手势只在根 Canvas
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
        for forbidden in ["property real panX:", "property real panY:", "property real zoomLevel:"] {
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
    let zoom = function_window(&src, "function zoomAround(", 500);
    assert!(
        zoom.contains("zoomLevel = target") && zoom.contains("applyPan("),
        "zoomAround 必须是唯一缩放入口，内部改根 zoomLevel 并 applyPan，实际窗口:\n{zoom}"
    );

    // 滚轮与捏合与 +/- 按钮都必须走同一个入口。
    assert!(
        src.contains("zoomAround(mx, my, newZoom)"),
        "sceneWheel 必须调 zoomAround，不能自己换算 panX/panY"
    );
    assert!(
        count_occurrences(&src, "applyPan(\n                    screenX - (screenX - panX)")
            + count_occurrences(&src, "applyPan(cx - (cx - _pinchStartPanX)") >= 1,
        "zoomAround 内部与 canvasPinch 都必须以手势中心缩放"
    );
    assert!(
        count_occurrences(&src, "onClicked: zoomAround(") == 2,
        "触屏 +/- 两个按钮都必须调 zoomAround，不能直接写 zoomLevel"
    );
}

#[test]
fn wheel_and_pinch_have_no_scene_identity_gating() {
    let src = strip_line_comments(&read_src(CANVAS));

    // 旧实现用 pathKey === "root" 决定谁能缩放。现在只有一个视口，
    // 这类“按场景身份决定谁能操作相机”的判断必须整体消失。
    assert!(
        !src.contains("pathKey === \"root\""),
        "根 Canvas 不得再用 pathKey === \"root\" 这类场景身份判断决定谁能缩放"
    );
    assert!(
        !src.contains("_pinchBelongsToChild"),
        "不得再有“捏合归某个子星图”的判断：整棵星图只有一个视口"
    );
    assert!(
        !src.contains("_touchOwnerA") && !src.contains("_touchOwnerB"),
        "不得再有按手指分别记录所属子星图的 press-time owner：触屏手势不再按层让出"
    );
    assert!(
        !src.contains("childOwnerAtScreen"),
        "childOwnerAtScreen 是“每层各自有视口”的产物，单一全局视口下必须删除"
    );

    let wheel = function_window(&src, "id: sceneWheel", 1200);
    assert!(
        wheel.contains("blocking: true"),
        "sceneWheel 仍需 blocking 决定阻塞语义，实际窗口:\n{wheel}"
    );
    assert!(
        !wheel.contains("enabled:"),
        "sceneWheel 不得再有任何 enabled 门控（鼠标停在哪一层都走同一个 zoomAround），\
         实际窗口:\n{wheel}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. scenePathKey 是 required 且没有默认 "root"
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn scene_path_key_is_required_without_root_default() {
    let src = strip_line_comments(&read_src(CONTENT));
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

    // 根层必须由外部显式传 "root"。
    let canvas = strip_line_comments(&read_src(CANVAS));
    let content_block = slice_between(
        &canvas,
        "StarMapSceneContent {",
        "property var selectionController: null",
    );
    assert!(
        content_block.contains("scenePathKey: \"root\""),
        "根层内容必须由 StarMapCanvas 显式传 scenePathKey: \"root\"，不靠默认值"
    );
}

#[test]
fn child_content_receives_full_path_key_once_at_creation() {
    let src = strip_line_comments(&read_src(EMBED));
    assert!(
        src.contains("readonly property string childContentPathKey: parentPathKey + \"/embed_\" + instanceId"),
        "子层 pathKey 必须由父 pathKey + \"/embed_\" + instanceId 拼成，永远不等于 \"root\""
    );
    let set_source = function_window(&src, "childContentLoader.setSource(", 700);
    assert!(
        set_source.contains("\"scenePathKey\": childContentPathKey"),
        "Embed 创建子内容时必须一次性传完整 scenePathKey，实际窗口:\n{set_source}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 4. 递归的是内容不是视口
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn embed_loads_scene_content_instead_of_nested_scene() {
    let src = strip_line_comments(&read_src(EMBED));
    assert!(
        !src.contains("StarMapScene.qml"),
        "Embed 不得再加载 StarMapScene.qml（那会为每个子星图新建一个带相机的 Canvas）"
    );
    assert!(
        src.contains("Qt.resolvedUrl(\"StarMapSceneContent.qml\")"),
        "Embed 必须用运行时 Qt.resolvedUrl + Loader.setSource 递归创建 StarMapSceneContent"
    );
    // 静态引用自身类型会形成编译期环（SceneContent → Embed → SceneContent）。
    assert!(
        !src.contains("StarMapSceneContent {"),
        "Embed 不得静态声明 StarMapSceneContent，只能运行时 setSource，否则 Qt 类型加载死锁"
    );
}

#[test]
fn scene_content_owns_its_own_graph_controller_and_renders_local_coords() {
    let src = strip_line_comments(&read_src(CONTENT));
    assert!(
        count_occurrences(&src, "StarMapGraphController {") == 1,
        "每个 StarMapSceneContent 必须拥有且只拥有一个自己的 GraphController"
    );
    assert!(
        src.contains("pathKey: content.scenePathKey"),
        "本层 GraphController 的 pathKey 必须是本层 scenePathKey"
    );
    // 节点与连线都画在本层局部坐标里，不再经 worldToScreen（相机在祖先 Content 上）。
    let node_delegate = slice_between(&src, "delegate: StarMapNode {", "Repeater {");
    assert!(
        node_delegate.contains("x: content.isMovingNode(nodeData.id) ? content.interactionController.moveX : nodeData.x"),
        "Node delegate 的 x 必须是本层局部坐标，不做 worldToScreen 换算，实际片段:\n{node_delegate}"
    );
    assert!(
        node_delegate.contains("y: content.isMovingNode(nodeData.id) ? content.interactionController.moveY : nodeData.y"),
        "Node delegate 的 y 必须是本层局部坐标，实际片段:\n{node_delegate}"
    );
}

#[test]
fn camera_transform_lives_only_on_the_root_content_item() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        canvas.contains("scale: canvasArea.zoomLevel")
            && canvas.contains("x: canvasArea.panX")
            && canvas.contains("y: canvasArea.panY"),
        "相机变换只能作用在根层 StarMapSceneContent 这一张 Item 上"
    );
    assert!(
        count_occurrences(&canvas, "scale: canvasArea.zoomLevel") == 1,
        "整棵递归树只能有一处相机 scale，子层不得再有第二处"
    );
}

#[test]
fn embed_visual_scale_is_lod_only_and_never_writes_back() {
    let embed = read_src(EMBED);
    let lod = function_window(&embed, "readonly property real visualScale:", 500);
    assert!(
        lod.contains("var projected = width * base")
            && lod.contains("if (projected >= _minContentScreenPx)")
            && lod.contains("_minContentScreenPx / projected"),
        "visualScale 必须只由父层传来的 ancestorScale + 屏幕投影尺寸算出，实际窗口:\n{lod}"
    );

    let stripped = strip_line_comments(&embed);
    assert!(
        stripped.contains("property real ancestorScale: 1.0"),
        "Embed 必须单独接收 ancestorScale（不含自身 visualScale 的累积比例），否则形成循环绑定"
    );
    // 子内容缩放绝不允许反写全局相机。
    for forbidden in ["panX", "panY", "zoomLevel"] {
        assert!(
            !stripped.contains(forbidden),
            "Embed 不得出现 {forbidden}：子星图“看起来多大”只是视觉 LOD，不能反写全局相机"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// 5. 递归命中测试：唯一入口，返回真正命中的那一层
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn hit_testing_has_one_recursive_entry_returning_the_real_hit_layer() {
    let src = strip_line_comments(&read_src(CONTENT));
    let hit = function_window(&src, "function hitTargetAtScene(", 3200);
    for expected in [
        "graphController.findNodeAt(",
        "graphController.findEmbedChromeAt(",
        "graphController.findEmbedContentAt(",
        "graphController.hitTestEdge(",
    ] {
        assert!(
            hit.contains(expected),
            "hitTargetAtScene 必须先查本层节点与 Embed chrome，再落到子内容区与连线，缺 {expected}，\
             实际窗口:\n{hit}"
        );
    }
    assert!(
        hit.contains("var deeper = child.hitTargetAtScene(sceneX, sceneY)"),
        "落在子星图内容区时必须递归进子层内容返回真正命中的那一层，实际窗口:\n{hit}"
    );
    assert!(
        hit.contains("child: childContentOf(inside.instanceId)") || hit.contains("childContentOf(inside.instanceId)"),
        "递归命中必须通过 childContentOf(instanceId) 拿到子层内容，实际窗口:\n{hit}"
    );
    assert!(
        hit.contains("kind: \"childContent\""),
        "子内容还没加载出来时必须落成 childContent，不得冒充 empty（empty 只表示任何一层都没命中）"
    );
    for field in ["owner: content", "scenePathKey: scenePathKey", "starmapId: finalStarmapId"] {
        assert!(
            hit.contains(field),
            "命中结果必须带 {field}，调用方要靠它把行为路由回归属层，实际窗口:\n{hit}"
        );
    }
}

#[test]
fn canvas_routes_all_hit_consumers_through_the_recursive_entry() {
    let src = strip_line_comments(&read_src(CANVAS));
    let entry = function_window(&src, "function hitTargetAtScreen(", 400);
    assert!(
        entry.contains("rootContent.hitTargetAtScene(screenToWorldX(sx), screenToWorldY(sy))"),
        "根 Canvas 必须只有一个命中入口，从根内容开始递归，实际窗口:\n{entry}"
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
            "根 Canvas 不得再直接调用 {forbidden}：命中一律走 hitTargetAtScreen 递归入口"
        );
    }

    // 空白右键 / 右键菜单 / 拖动归属 / pointer_press 日志都走同一个入口。
    assert!(
        count_occurrences(&src, "hitTargetAtScreen(") >= 5,
        "命中入口必须被右键菜单、背景 tap、bgDragArea、pointer_press 共用，\
         实际 {} 处调用",
        count_occurrences(&src, "hitTargetAtScreen(")
    );
}

#[test]
fn blank_right_click_creates_in_the_hit_layer_with_scene_to_local_conversion() {
    let src = strip_line_comments(&read_src(CONTENT));
    for expected in [
        "function createNodeWithName(name, sceneX, sceneY)",
        "function createSubStarmapWithName(name, sceneX, sceneY)",
    ] {
        assert!(
            src.contains(expected),
            "菜单归属层必须自己提供 {expected}：空的子星图可以直接新建内容，\
             不用“进入”另一个页面，也不需要第二个 Canvas"
        );
    }
    let create_node = function_window(&src, "function createNodeWithName(", 400);
    assert!(
        create_node.contains("var p = sceneToLocal(sceneX, sceneY)"),
        "归属层必须把 scene 坐标换算成本层局部坐标再写 Core，实际窗口:\n{create_node}"
    );
    let create_sub = function_window(&src, "function createSubStarmapWithName(", 400);
    assert!(
        create_sub.contains("var p = sceneToLocal(sceneX, sceneY)")
            && create_sub.contains("graphController.createSubStarmapAt(name, spawn.x, spawn.y)"),
        "新建子星图必须写归属层自己的 GraphController，实际窗口:\n{create_sub}"
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
    for field in ["pressScenePathKey", "pressKind", "pressId", "pressTargetPath"] {
        assert!(
            src.contains(&format!("property string {field}: \"\""))
                || src.contains(&format!("property var {field}: null")),
            "pressPending 必须记录 {field}：归属层、完整 targetPath 都要记下来"
        );
    }

    // 两条互斥的提升路径，只能各有一个实现。
    assert!(
        src.contains("function pressPendingToMove(") && src.contains("function pressPendingToConnect("),
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
        to_connect.contains("pointerMode !== \"pressPending\"") && to_connect.contains("return false"),
        "pressPendingToConnect 必须在状态不是 pressPending 时拒绝提升，实际窗口:\n{to_connect}"
    );

    assert!(
        src.contains("signal pressTimeout()"),
        "长按到点只能由共享状态机发 pressTimeout 信号，由归属层决定提升成 connect"
    );
    assert!(
        !src.contains("Timer {"),
        "长按 Timer 不能挂在 InteractionController 内：它继承 QObject，没有默认属性"
    );
}

#[test]
fn drag_threshold_and_long_press_are_the_only_two_ways_out_of_press_pending() {
    let src = strip_line_comments(&read_src(CONTENT));
    let delta = function_window(&src, "function onSceneDragDelta(", 2200);
    assert!(
        delta.contains("ic.noteDragDelta(sceneDelta.x, sceneDelta.y)")
            && delta.contains("if (ic.pressDragDistance >= ic.dragThreshold)")
            && delta.contains("promoteToMove(ic.pressKind, ic.pressId)"),
        "pressPending 下位移先累计，超阈值才提升为 move，实际窗口:\n{delta}"
    );
    assert!(
        !delta.contains("beginMove("),
        "onSceneDragDelta 不得直接 beginMove，必须经 pressPendingToMove 仲裁"
    );

    let timeout = function_window(&src, "function onPressTimeout(", 1200);
    assert!(
        timeout.contains("if (ic.pointerMode !== \"pressPending\")") && timeout.contains("return"),
        "长按到点只在仍是 pressPending 时提升为 connect，实际窗口:\n{timeout}"
    );
    assert!(
        timeout.contains("ic.pressPendingToConnect("),
        "长按提升必须走 pressPendingToConnect，实际窗口:\n{timeout}"
    );
}

#[test]
fn release_has_a_single_exit_for_click_move_and_connect() {
    let src = strip_line_comments(&read_src(CONTENT));
    let release = function_window(&src, "function releaseOwnerGesture(", 700);
    for branch in [
        "if (mode === \"connect\")",
        "else if (mode === \"move\")",
        "else if (mode === \"contextPending\")",
        "else if (mode === \"pressPending\")",
    ] {
        assert!(
            release.contains(branch),
            "releaseOwnerGesture 必须用一个统一出口覆盖所有状态，缺 {branch}，实际窗口:\n{release}"
        );
    }
    assert!(
        src.contains("onLeftReleased: content.releaseOwnerGesture()"),
        "delegate 松手必须全部走这一个出口"
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
fn long_press_timer_lives_in_the_canvas_item_not_in_the_qtobject() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        canvas.contains("Timer {") && canvas.contains("running: interaction.pressTimerActive"),
        "长按计时 Timer 必须挂在 Item 下（Canvas），由共享状态机的 pressTimerActive 驱动"
    );
    assert!(
        canvas.contains("interval: interaction.longPressInterval"),
        "长按阈值必须用系统的 mousePressAndHoldInterval，由 Canvas 注入共享状态机"
    );
    assert!(
        canvas.contains("Application.styleHints.mousePressAndHoldInterval"),
        "长按阈值必须对齐平台系统长按时间，而不是写死常量"
    );
}

#[test]
fn connect_preview_is_one_global_overlay_line() {
    let src = strip_line_comments(&read_src(CANVAS));
    assert_eq!(
        count_occurrences(&src, "Canvas {"),
        2,
        "根 Canvas 只允许两个 Canvas：背景网格 与 全局连线预览线，不每层各画一条"
    );
    let preview = slice_between(&src, "id: connectPreview", "onClicked: zoomAround(");
    assert!(
        preview.contains("ctx.translate(panX, panY)") && preview.contains("ctx.scale(zoomLevel, zoomLevel)"),
        "连线预览线必须用全局相机换算 scene 坐标，实际片段:\n{preview}"
    );
    assert!(
        preview.contains("if (interaction.pointerMode !== \"connect\")"),
        "预览线只在 connect 状态下画，实际片段:\n{preview}"
    );
}

#[test]
fn connect_end_creates_the_edge_with_full_target_paths() {
    let src = strip_line_comments(&read_src(CONTENT));
    let finish = function_window(&src, "function finishConnect(", 1800);
    assert!(
        finish.contains("rootContent.hitTargetAtScene(ic.connectMouseX, ic.connectMouseY)"),
        "连线松手必须用递归命中找到落点所在那一层，实际窗口:\n{finish}"
    );
    assert!(
        finish.contains("toPath = hit.targetPath")
            && finish.contains("success = createEdgeWithPaths(fromPath, toPath)"),
        "落点必须是命中层给出的完整 targetPath，source/target 都不得退化成 nodeId-only，\
         实际窗口:\n{finish}"
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

// ─────────────────────────────────────────────────────────────────────────
// 7. 节点标题就地内联编辑，Inspector 路线彻底删除
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn node_title_is_edited_inline_with_a_text_input() {
    let src = strip_line_comments(&read_src(NODE));
    assert!(
        src.contains("property bool editing: false"),
        "StarMapNode 必须有 editing 状态"
    );
    assert!(
        src.contains("TextInput {") && src.contains("id: titleInput"),
        "节点框内必须是 TextInput（需要多行时再换 TextEdit），不是只读 AppText"
    );
    assert!(
        src.contains("readOnly: !root.editing") && src.contains("enabled: root.editing"),
        "TextInput 平时只读显示，只有 editing 时才接管输入，实际源码缺少"
    );

    let begin = function_window(&src, "function beginEdit(", 400);
    assert!(
        begin.contains("editing = true")
            && begin.contains("titleInput.forceActiveFocus()"),
        "进入编辑必须把光标放进节点框内（forceActiveFocus），实际窗口:\n{begin}"
    );
    let commit = function_window(&src, "function commitEdit(", 700);
    assert!(
        commit.contains("root.titleCommitted(next)"),
        "Enter / editingFinished 必须把 title 提交给归属层，实际窗口:\n{commit}"
    );
    let cancel = function_window(&src, "function cancelEdit(", 400);
    assert!(
        cancel.contains("titleInput.text = root.title"),
        "Esc 必须还原原值，实际窗口:\n{cancel}"
    );

    // accepted / editingFinished 是 Qt TextInput 自带的信号，够用，不需要 Popup。
    assert!(
        src.contains("onAccepted: root.commitEdit()")
            && src.contains("onEditingFinished: root.commitEdit()")
            && src.contains("Keys.onEscapePressed: root.cancelEdit()"),
        "提交/取消必须绑定 TextInput 的 accepted / editingFinished 与 Esc"
    );
}

#[test]
fn node_gestures_are_disabled_while_editing() {
    let src = strip_line_comments(&read_src(NODE));
    // nodeMouseTap / nodeTouchTap / nodeDragHandler / leftPointTracker 都要让位。
    let gates = count_occurrences(&src, "enabled: !root.editing");
    assert_eq!(
        gates, 4,
        "编辑中必须关闭节点的 4 个手势 Handler（鼠标 tap / 触屏 tap / drag / point），\
         否则文本选区、光标和输入法会被节点手势抢走；实际 {gates} 处"
    );
}

#[test]
fn no_external_node_edit_popup_route_remains() {
    let node = strip_line_comments(&read_src(NODE));
    assert!(
        !node.contains("editNodeRequested"),
        "StarMapNode 不得再上抛 editNodeRequested：标题编辑已在节点自身内联完成"
    );

    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        !canvas.contains("editNodeRequested") && !canvas.contains("childEditNodeRequested"),
        "Canvas 不得再保留 editNodeRequested / childEditNodeRequested → Popup 路线"
    );

    let workspace = strip_line_comments(&read_src(WORKSPACE));
    for forbidden in ["inspectorPopup", "ownerScene", "ownerStarmapId", "ownerPathKey"] {
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
    for rel in [CANVAS, CONTENT, EMBED, NODE, INTERACTION, WORKSPACE] {
        let src = read_src(rel);
        for forbidden in [
            "每个 Scene 实例都有自己的",
            "子 Scene 自己的 pan",
            "childSceneLoader",
            "childScenePathKey",
        ] {
            assert!(
                !src.contains(forbidden),
                "{rel} 不得再出现 {forbidden}：整棵星图只有一个全局视口，\
                 递归的是内容不是视口"
            );
        }
    }
}