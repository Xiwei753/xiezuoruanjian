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
//! - 相机平移/缩放集中在 `cameraLayer` 一张 Item 上，根内容只用 `anchors.fill`
//!   占满它：同一个 Item 不允许 anchors 和相机两套几何来源。
//! - 子层坐标只认 Qt 真实 Item 映射（`mapFromItem`/`mapToItem(rootContent)`），
//!   不再手工维护 sceneOrigin/sceneScale 矩阵；delegate 的 DragHandler 只上抛
//!   原始 `activeTranslation` 增量，Qt scene → 本层 local 只换算一次。
//! - 拖动阈值吃原始 Qt scene 像素，累计公式是"离按下点的直线距离"
//!   （pressDragX/Y 向量和），不是逐段增量长度累加。
//! - Deep Zoom 档位只由 coverage（投影尺寸 / 根视口短边）决定：
//!   interactive 完整交互 / preview 静态投影 / shell 只留外壳，带滞回；
//!   每层内容做 local fit，但档位绝不改 Embed world 几何、绝不反写全局相机。
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
    let zoom = function_window(&src, "function zoomAround(", 600);
    assert!(
        zoom.contains("zoomLevel = target") && zoom.contains("applyPan("),
        "zoomAround 必须是唯一缩放入口，内部改根 zoomLevel 并 applyPan，实际窗口:\n{zoom}"
    );
    assert!(
        zoom.contains("Math.max(_cameraScaleMin, Math.min(_cameraScaleMax, nextZoom))"),
        "zoomAround 只夹数值安全范围（CAMERA_SCALE_MIN/MAX），实际窗口:\n{zoom}"
    );

    // 滚轮与捏合与 +/- 按钮都必须走同一个入口。
    assert!(
        src.contains("zoomAround(mx, my, newZoom)"),
        "sceneWheel 必须调 zoomAround，不能自己换算 panX/panY"
    );
    assert!(
        src.contains("zoomAround(cx, cy, _pinchStartZoom * activeScale)"),
        "canvasPinch 必须走 zoomAround 统一夹取/以中心缩放，不再自己写第二套 pan 公式"
    );
    assert!(
        count_occurrences(&src, "onClicked: zoomAround(") == 2,
        "触屏 +/- 两个按钮都必须调 zoomAround，不能直接写 zoomLevel"
    );
}

/// 相机范围跟 docs/starmap_viewport.md：只保留数值安全边界 + 乘法步进。
/// 旧的 0.35~2.5 硬上限会让 1080 高窗口里的子星图永远停在 preview（进不了 interactive）。
#[test]
fn camera_range_and_steps_follow_the_shared_viewport_spec() {
    let src = read_src(CANVAS);
    for constant in [
        "readonly property real _cameraScaleMin: 1e-4",
        "readonly property real _cameraScaleMax: 1e5",
        "readonly property real _zoomFactor: 1.2",
    ] {
        assert!(
            src.contains(constant),
            "相机常量必须与 docs/starmap_viewport.md 一致：缺 {constant}"
        );
    }

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
    let wheel = function_window(&stripped, "onWheel: function(event)", 700);
    assert!(
        wheel.contains("oldZoom * Math.pow(_zoomFactor, delta)"),
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
fn camera_transform_lives_on_the_camera_layer_only() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        canvas.contains("id: cameraLayer")
            && canvas.contains("scale: canvasArea.zoomLevel")
            && canvas.contains("x: canvasArea.panX")
            && canvas.contains("y: canvasArea.panY"),
        "相机变换必须集中在 cameraLayer 这一张 Item 上"
    );
    assert!(
        count_occurrences(&canvas, "scale: canvasArea.zoomLevel") == 1,
        "整棵递归树只能有一处相机 scale，子层不得再有第二处"
    );

    // 根内容只用 anchors.fill 占满相机层，不能再自己拿 x/y 当平移：
    // 同一个 Item 上不允许 anchors 和相机两套几何来源同时控制位置。
    let root_block = slice_between(
        &canvas,
        "id: rootContent",
        "property var selectionController: null",
    );
    assert!(
        root_block.contains("anchors.fill: parent"),
        "根层内容必须 anchors.fill 占满相机层，实际片段:\n{root_block}"
    );
    for forbidden in ["x: canvasArea.panX", "y: canvasArea.panY", "scale: canvasArea.zoomLevel"] {
        assert!(
            !root_block.contains(forbidden),
            "根层内容不得再自己持有 {forbidden}（anchors 与相机不能抢同一个 Item），\
             实际片段:\n{root_block}"
        );
    }
}

#[test]
fn embed_deep_zoom_lod_is_coverage_only_and_never_writes_back() {
    let embed = read_src(EMBED);
    let stripped = strip_line_comments(&embed);

    // 旧的反向公式必须整体消失：它在第一层之后恒为 1，也只有投影 < 140 时才
    // 反过来放大 Embed。
    for forbidden in ["visualScale", "ancestorScale", "_minContentScreenPx", "_minVisualScale"] {
        assert!(
            !stripped.contains(forbidden),
            "Embed 不得再保留旧的视觉 LOD 标识 {forbidden}"
        );
    }
    assert!(
        !stripped.contains("scale: visualScale"),
        "不得通过 scale 改整颗 Embed 的几何：档位只决定子内容渲染多少细节，\
         Embed 外壳 world 尺寸恒定"
    );

    // ownerEffectiveScale 从所属 Scene 现读（全局相机 × 祖先 local fit），不复制标量。
    assert!(
        stripped.contains("property real ownerEffectiveScale: 1.0")
            && stripped.contains("property var ownerSceneContent: null"),
        "Embed 必须从所属 Scene 现读 ownerEffectiveScale（globalZoom × 祖先 local fit）"
    );

    // coverage = projectedSize / 根视口短边；两个量都在判定函数里从原始输入现算，
    // 不能读派生绑定（QML 属性变更信号先于依赖绑定标脏，处理器里读会拿到旧值）。
    let detail = function_window(&stripped, "function resolveChildContentDetail(", 1200);
    for expected in [
        "var projectedSize = Math.min(width, height) * scale",
        "var coverage = viewportShortSide > 0 ? projectedSize / viewportShortSide : 0",
        "coverage >= _interactiveEnterCoverage",
        "previous === \"interactive\" && coverage >= _interactiveExitCoverage",
        "projectedSize >= _previewEnterPx",
        "previous === \"preview\" && projectedSize >= _previewExitPx",
        "next = \"interactive\"",
        "next = \"preview\"",
        "childContentDetail = next",
    ] {
        assert!(
            detail.contains(expected),
            "Deep Zoom 档位判定缺 {expected}，实际窗口:\n{detail}"
        );
    }
    assert!(
        !stripped.contains("projectedCoverage"),
        "档位判定不得经过派生绑定读数（会拿到上一帧的旧值），只在函数内现算"
    );
    for constant in [
        "readonly property real _interactiveEnterCoverage: 0.70",
        "readonly property real _interactiveExitCoverage: 0.60",
        "readonly property real _previewEnterPx: 48",
        "readonly property real _previewExitPx: 40",
    ] {
        assert!(
            stripped.contains(constant),
            "阈值/滞回带必须是 docs/starmap_viewport.md 的共享常量：缺 {constant}"
        );
    }

    // shell 不建子内容；preview/interactive 用运行时 URL + 档位参数创建。
    assert!(
        stripped.contains("childContentActivated && childContentDetail !== \"shell\""),
        "shell 档不得创建子内容"
    );
    let set_source = function_window(&stripped, "childContentLoader.setSource(", 900);
    assert!(
        set_source.contains("\"renderDetail\": childContentDetail"),
        "子内容创建时必须带当前档位，实际窗口:\n{set_source}"
    );

    // 子内容缩放绝不允许反写全局相机。
    for forbidden in ["panX", "panY", "zoomLevel"] {
        assert!(
            !stripped.contains(forbidden),
            "Embed 不得出现 {forbidden}：档位只决定渲染细节，不能反写全局相机"
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
    let create_sub = function_window(&src, "function createSubStarmapWithName(", 500);
    assert!(
        create_sub.contains("var p = sceneToLocal(sceneX, sceneY)")
            && create_sub.contains("graphController.createSubStarmapAt(name, safe.x, safe.y)"),
        "新建子星图必须把安全区 clamp 后的坐标写进归属层自己的 GraphController，\
         实际窗口:\n{create_sub}"
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
    let delta = function_window(&src, "function onSceneDragDelta(", 2600);
    assert!(
        delta.contains("ic.noteDragDelta(dxQtScene, dyQtScene)")
            && delta.contains("if (ic.pressDragDistance >= ic.dragThreshold)")
            && delta.contains("promoteToMove(ic.pressKind, ic.pressId)"),
        "pressPending 下位移先累计原始 Qt scene 像素，超阈值才提升为 move，实际窗口:\n{delta}"
    );
    assert!(
        !delta.contains("noteDragDelta(sceneDelta") && !delta.contains("localDeltaToScene("),
        "noteDragDelta 不得再吃 world 单位，也不得再维护 scene 增量换算，实际窗口:\n{delta}"
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
fn connect_end_creates_the_edge_on_the_lca_host() {
    let src = strip_line_comments(&read_src(CONTENT));
    let finish = function_window(&src, "function finishConnect(", 2800);
    assert!(
        finish.contains("rootContent.hitTargetAtScene(ic.connectMouseX, ic.connectMouseY)"),
        "连线松手必须用递归命中找到落点所在那一层，实际窗口:\n{finish}"
    );
    assert!(
        finish.contains("toPath = hit.targetPath"),
        "落点必须是命中层给出的完整 targetPath，不得退化成 nodeId-only，实际窗口:\n{finish}"
    );
    // 宿主 = 两端 Scene 的最近公共祖先；边写进宿主图，starmapId 必须是宿主的。
    assert!(
        finish.contains("StarMapPathPlanner.planCrossLayerEdge(fromPath, toPath)"),
        "建边必须先做跨层宿主规划（宿主 = 最近公共祖先），实际窗口:\n{finish}"
    );
    assert!(
        finish.contains("rootContent.findContentByPathSegments(plan.hostSegments)"),
        "必须按宿主路径段找到宿主 Content，实际窗口:\n{finish}"
    );
    assert!(
        finish.contains("plan.from.starmapId = hostStarmapId")
            && finish.contains("plan.to.starmapId = hostStarmapId"),
        "端点 starmapId 必须等于宿主的 finalStarmapId，实际窗口:\n{finish}"
    );
    assert!(
        finish.contains("success = host.commitEdgeWithPaths(plan.from, plan.to)"),
        "边必须写进宿主图的 GraphController（经宿主的 commitEdgeWithPaths 入口），\
         不能永远写进起点那一层，实际窗口:\n{finish}"
    );
    let commit = function_window(&src, "function commitEdgeWithPaths(", 300);
    assert!(
        commit.contains("return graphController.createEdgeWithPaths(fromPath, toPath)"),
        "宿主 Content 的 commitEdgeWithPaths 必须把边交给自己的 GraphController，\
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

/// 跨层连线的宿主规划必须照搬 Harmony 已验证的规则：
/// 宿主 = 两端所在 Scene 的最近公共祖先，segments 只保留从宿主往下的部分。
#[test]
fn path_planner_ports_harmony_lca_rules() {
    let planner = read_src("qml/StarMapPathPlanner.js");
    for expected in [
        "function commonScenePathPrefix(",
        "function resolveItemRef(",
        "function buildTargetPathForHost(",
        "function planCrossLayerEdge(",
        "function uiInstanceIdOfSegment(",
        "itemRef.scenePath.slice(hostSegments.length)",
        "segments.slice(0, segments.length - 1)",
        "var hostSegments = commonScenePathPrefix(from.scenePath, to.scenePath)",
    ] {
        assert!(
            planner.contains(expected),
            "StarMapPathPlanner.js 缺 {expected}：宿主必须是两端 Scene 的最近公共祖先"
        );
    }
    // Embed-like 端点必须保留原来的 terminal segment（enterEmbed 或旧 portal 的
    // enterPortal），重建相对宿主的路径时不再无条件改写成 enterEmbed。
    assert!(
        planner.contains("last.type !== \"enterEmbed\" && last.type !== \"enterPortal\"")
            && planner.contains("terminalSegment: cloneSegment(last)")
            && planner.contains("segments.push(cloneSegment(itemRef.terminalSegment))"),
        "Embed-like 端点必须沿用原始 terminal segment，实际源码缺少"
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

// ─────────────────────────────────────────────────────────────────────────
// 9. 子层坐标只认 Qt 真实 Item 映射，不再手抄 sceneOrigin/sceneScale
// ─────────────────────────────────────────────────────────────────────────

/// 子 Content 的逻辑原点和实际画出来的原点必须是一套：contentViewport 的
/// 6/24 边框偏移、每层 local fit、祖先 Embed 的变换全部交给
/// `mapFromItem` / `mapToItem` 走真实 Item 变换，不再手工维护
/// sceneOriginX/Y/sceneScale 矩阵（那套矩阵在第二层开始就和实际渲染错位）。
#[test]
fn child_coordinates_use_real_item_mapping_not_a_manual_matrix() {
    let content = strip_line_comments(&read_src(CONTENT));
    let embed = strip_line_comments(&read_src(EMBED));

    for (rel, src) in [(CONTENT, &content), (EMBED, &embed)] {
        for forbidden in ["sceneOriginX", "sceneOriginY", "sceneScale"] {
            assert!(
                !src.contains(forbidden),
                "{rel} 不得再手工维护 {forbidden}：命中/创建/连线只认 Qt 真实 Item 映射"
            );
        }
    }

    let to_local = function_window(&content, "function sceneToLocal(", 200);
    assert!(
        to_local.contains("worldLayer.mapFromItem(rootContent, sceneX, sceneY)"),
        "sceneToLocal 必须用 mapFromItem(rootContent) 落到本层 authored 坐标，实际窗口:\n{to_local}"
    );
    let to_scene = function_window(&content, "function localToScene(", 200);
    assert!(
        to_scene.contains("worldLayer.mapToItem(rootContent, localX, localY)"),
        "localToScene 必须用 mapToItem(rootContent)，实际窗口:\n{to_scene}"
    );
    let delta = function_window(&content, "function qtSceneDeltaToLocal(", 500);
    assert!(
        delta.contains("worldLayer.mapFromItem(null, 0, 0)")
            && delta.contains("worldLayer.mapFromItem(null, dx, dy)")
            && delta.contains("point.x - origin.x"),
        "Qt scene 增量必须先映射两个点再相减（mapFromItem 只能映射点），实际窗口:\n{delta}"
    );

    // 按下登记也直接映射到 scene 坐标，不再 local→scene 两跳。
    let pressed = function_window(&content, "function onItemPressed(", 300);
    assert!(
        pressed.contains("rootContent.mapFromItem(null, qtSceneX, qtSceneY)"),
        "onItemPressed 必须一次 mapFromItem 到 scene 坐标，实际窗口:\n{pressed}"
    );

    // Embed 创建子内容不再传手工矩阵参数，只传 owner 引用让子层现读。
    let set_source = function_window(&embed, "childContentLoader.setSource(", 1100);
    for forbidden in ["sceneOriginX", "sceneOriginY", "sceneScale", "\"viewportRect\""] {
        assert!(
            !set_source.contains(forbidden),
            "Embed 不得再向子内容复制 {forbidden}，实际窗口:\n{set_source}"
        );
    }
    assert!(
        set_source.contains("\"ownerSceneContent\": ownerSceneContent"),
        "子内容必须沿 ownerSceneContent 链现读有效比例/视口，实际窗口:\n{set_source}"
    );

    // 换算函数里不得再手抄 contentViewport 的边框偏移常量。
    for marker in ["function sceneToLocal(", "function localToScene(", "function qtSceneDeltaToLocal("] {
        let window = function_window(&content, marker, 400);
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
// 10. 每层 local fit + preview 档
// ─────────────────────────────────────────────────────────────────────────

/// 每层内容做一次 local fit：只改显示（worldLayer 的 x/y/scale），
/// 不改 authored position，也不进命中/创建/连线的坐标真相。
#[test]
fn local_fit_is_a_display_transform_on_world_layer() {
    let content = strip_line_comments(&read_src(CONTENT));
    let world = function_window(&content, "id: worldLayer", 400);
    assert!(
        world.contains("x: content.localFitOffsetX")
            && world.contains("y: content.localFitOffsetY")
            && world.contains("scale: content.localFitScale")
            && world.contains("transformOrigin: Item.TopLeft"),
        "每层 local fit 必须落在 worldLayer 的显示变换上，实际窗口:\n{world}"
    );

    let fit = function_window(&content, "readonly property real localFitScale:", 900);
    assert!(
        fit.contains("if (depth === 0)")
            && fit.contains("return 1.0")
            && fit.contains("var available = _contentSafeSide")
            && fit.contains("Math.min(available / b.width, available / b.height)")
            && fit.contains("Math.max(_minFitScale, Math.min(_maxFitScale, raw))"),
        "根层是相机本体不做 fit；子层按内容安全区适配并夹在安全范围，实际窗口:\n{fit}"
    );

    // 子层有效比例 = local fit × ownerSceneContent 链（globalZoom × 祖先 local fit）。
    let scale = function_window(&content, "readonly property real effectiveScale:", 600);
    assert!(
        scale.contains("if (depth === 0)")
            && scale.contains("return globalZoom")
            && scale.contains("return localFitScale * inherited")
            && scale.contains("ownerSceneContent.effectiveScale"),
        "effectiveScale 必须沿 ownerSceneContent 链现算，实际窗口:\n{scale}"
    );

    // authored position 没有被改写：delegate 仍直接读 model 的 x/y。
    // （新建/拖动时的安全区 clamp 是写 Core 前的约束，见 content_safe_area 守卫。）
    assert!(
        content.contains("nodeData.x") && content.contains("embedData.x"),
        "local fit 只改显示，不得改写 authored position"
    );
}

/// preview 档只画静态投影，不参与命中，也不实例化交互 delegate。
#[test]
fn preview_detail_renders_static_projection_and_passes_hits_through() {
    let content = strip_line_comments(&read_src(CONTENT));
    assert!(
        content.contains("property string renderDetail: \"interactive\""),
        "内容层必须有 renderDetail，根层默认 interactive"
    );
    let hit = function_window(&content, "function hitTargetAtScene(", 500);
    assert!(
        hit.contains("if (renderDetail !== \"interactive\")") && hit.contains("return null"),
        "preview 档不参与命中：必须返回 null 让父层落成 childContent，实际窗口:\n{hit}"
    );
    assert!(
        content.contains("id: previewCanvas")
            && content.contains("visible: content.renderDetail === \"preview\""),
        "preview 档必须由静态投影 Canvas 渲染"
    );
    // preview 档不实例化节点/Embed delegate，也不再往里递归。
    assert_eq!(
        count_occurrences(&content, "model: content.renderDetail === \"interactive\" ?"),
        2,
        "preview 档不得实例化节点/Embed delegate"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 11. connect 端点：屏幕像素 vs root-world 增量两个口径分开
// ─────────────────────────────────────────────────────────────────────────

/// 拖动阈值吃原始 Qt scene 像素；connect/contextPending 的端点存 scene 坐标，
/// 只能累加 qtSceneDeltaToRootScene() 的结果。把屏幕像素直接加到 root-world
/// 上会随全局缩放漂移（zoom=2 时端点多走一倍）。
#[test]
fn connect_endpoints_accumulate_root_world_delta_not_screen_pixels() {
    let src = strip_line_comments(&read_src(CONTENT));
    let convert = function_window(&src, "function qtSceneDeltaToRootScene(", 400);
    assert!(
        convert.contains("rootContent.mapFromItem(null, 0, 0)")
            && convert.contains("rootContent.mapFromItem(null, dx, dy)")
            && convert.contains("point.x - origin.x"),
        "qtSceneDeltaToRootScene 必须用 rootContent 的真实 Item 映射做两点差分，\
         实际窗口:\n{convert}"
    );

    let delta = function_window(&src, "function onSceneDragDelta(", 3200);
    assert!(
        delta.contains("ic.noteDragDelta(dxQtScene, dyQtScene)"),
        "拖动阈值必须继续吃原始 Qt scene 像素，实际窗口:\n{delta}"
    );
    assert!(
        delta.contains("var contextDelta = qtSceneDeltaToRootScene(dxQtScene, dyQtScene)")
            && delta.contains("ic.connectMouseX += contextDelta.x")
            && delta.contains("ic.connectMouseY += contextDelta.y"),
        "contextPending 端点必须累加 root-world 增量，实际窗口:\n{delta}"
    );
    assert!(
        delta.contains("var connectDelta = qtSceneDeltaToRootScene(dxQtScene, dyQtScene)")
            && delta.contains("ic.updateConnect(ic.connectMouseX + connectDelta.x,")
            && delta.contains("ic.connectMouseY + connectDelta.y)"),
        "connect 端点必须累加 root-world 增量，实际窗口:\n{delta}"
    );
    assert!(
        !delta.contains("ic.connectMouseX += dxQtScene")
            && !delta.contains("ic.connectMouseY += dyQtScene"),
        "不得再把屏幕像素直接加到 root-world 端点上，实际窗口:\n{delta}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 12. local fit offset 变化也要通知子层；按下归属用真正的 scene 坐标
// ─────────────────────────────────────────────────────────────────────────

/// offset 也是真实显示变换：内容整体平移时 scale/宽高可能不变，
/// 只有 offset 变；漏掉它子层投影位置变化就收不到通知。
#[test]
fn local_fit_offset_changes_notify_transform_changed() {
    let src = strip_line_comments(&read_src(CONTENT));
    let transforms = function_window(&src, "signal transformChanged()", 400);
    for handler in [
        "onLocalFitScaleChanged: transformChanged()",
        "onLocalFitOffsetXChanged: transformChanged()",
        "onLocalFitOffsetYChanged: transformChanged()",
        "onWidthChanged: transformChanged()",
        "onHeightChanged: transformChanged()",
    ] {
        assert!(
            transforms.contains(handler),
            "显示变换变化必须通知子层重算懒加载裁剪：缺 {handler}，实际窗口:\n{transforms}"
        );
    }
}

/// Qt 的 pressPosition 是相对 Handler parent 的局部坐标，
/// 真正相对 QQuickWindow 的是 scenePressPosition；归属层只接 scene 坐标。
/// 局部 pressPosition 只允许喂给同一份圆壳几何（chromeRegionAt）。
#[test]
fn press_handlers_pass_true_scene_coordinates() {
    let node = strip_line_comments(&read_src(NODE));
    assert!(
        node.contains("nodeMouseTap.point.scenePressPosition.x")
            && node.contains("nodeMouseTap.point.scenePressPosition.y"),
        "Node 的按下归属必须用 scenePressPosition，实际源码缺少"
    );
    assert!(
        !node.contains("root.itemPressed(nodeMouseTap.point.pressPosition"),
        "Node 不得把 Handler 局部 pressPosition 当 scene 坐标传出去"
    );

    let embed = strip_line_comments(&read_src(EMBED));
    assert_eq!(
        count_occurrences(&embed, "root.itemPressed(point.scenePressPosition.x,"),
        1,
        "chrome 输入层的按下归属必须用 scenePressPosition 传给 itemPressed"
    );
    assert!(
        !embed.contains("root.itemPressed(point.pressPosition"),
        "Embed 不得把 Handler 局部 pressPosition 当 scene 坐标传给 itemPressed"
    );
    assert!(
        embed.contains("chromeRegionAt(point.pressPosition.x, point.pressPosition.y)"),
        "局部 pressPosition 只允许用于 chromeRegionAt 的圆壳几何判定"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 13. Embed 是正圆：圆外但落在外接矩形里的点不能命中 Embed
// ─────────────────────────────────────────────────────────────────────────

/// Embed 外壳 world 几何恒定 = 直径 200 的正圆；命中先做圆内判定，
/// 圆外即使还在外接矩形里也必须继续判为空白/下面的对象。
#[test]
fn embed_hit_testing_is_circular() {
    let src = strip_line_comments(&read_src(CONTROLLER));

    let circle = function_window(&src, "function _insideEmbedCircle(", 400);
    assert!(
        circle.contains("dx * dx + dy * dy <= c.radius * c.radius"),
        "圆内判定必须是 dx² + dy² <= r²，实际窗口:\n{circle}"
    );
    let ring = function_window(&src, "function _insideEmbedBorderRing(", 400);
    assert!(
        ring.contains("var inner = c.radius - _borderSlop")
            && ring.contains("dx * dx + dy * dy >= inner * inner"),
        "圆周边框必须是内半径到圆边的一圈，实际窗口:\n{ring}"
    );

    let chrome = function_window(&src, "function findEmbedChromeAt(", 900);
    assert!(
        chrome.contains("if (!_insideEmbedCircle(em, wx, wy)) continue")
            && chrome.contains("wy <= em.y + _chromeHeight")
            && chrome.contains("if (_insideEmbedBorderRing(em, wx, wy)) return em"),
        "findEmbedChromeAt 必须先做圆内判定，再分标题带/圆周环，实际窗口:\n{chrome}"
    );
    let content = function_window(&src, "function findEmbedContentAt(", 1200);
    assert!(
        content.contains("if (!_insideEmbedCircle(em, wx, wy)) continue")
            && content.contains("wy <= em.y + _chromeHeight")
            && content.contains("if (_insideEmbedBorderRing(em, wx, wy)) continue"),
        "findEmbedContentAt 必须先做圆内判定，再排除标题带/圆周环，实际窗口:\n{content}"
    );

    // 旧的外接矩形整框命中必须整体消失：圆外不再有任何命中路径。
    assert!(
        !src.contains("_rectContains"),
        "不得再用外接矩形 _rectContains 当 Embed 命中真相"
    );
    assert!(
        !src.contains("_insideEmbedCircle(em, wx, wy) || "),
        "圆内判定不得被短路绕过"
    );

    // 输入层：整颗 Embed 只有一层 chrome，acceptance 用同一份圆壳几何。
    let embed_src = strip_line_comments(&read_src(EMBED));
    assert!(
        embed_src.contains("id: chromeLayer")
            && embed_src.contains("containmentMask: chromeMask")
            && embed_src.contains("containsMode: Shape.FillContains"),
        "chrome 输入必须只有一层，并用 Shape.contains（FillContains）决定 acceptance"
    );
    assert!(
        embed_src.contains("function isChromeLocalPoint(")
            && embed_src.contains("_insideCircleAt(")
            && embed_src.contains("_insideRingAt("),
        "JS 侧 chrome 判定必须和圆壳几何共用同一组常量"
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

    // contentViewport 铺满整个圆盒：不再用更小的矩形 Item 制造接不到事件的死区。
    let viewport_window = function_window(&embed_src, "id: contentViewport", 200);
    assert!(
        viewport_window.contains("anchors.fill: parent"),
        "contentViewport 必须铺满圆盒，实际窗口:\n{viewport_window}"
    );

    // 预览档也要画圆，不能把子星图画回长方形卡片。
    let content_src = strip_line_comments(&read_src(CONTENT));
    let preview = function_window(&content_src, "id: previewCanvas", 3000);
    assert!(
        preview.contains("ctx.arc(e.x + e.width / 2, e.y + e.height / 2,")
            && preview.contains("Math.min(e.width, e.height) / 2"),
        "preview 档的 Embed 必须用 ctx.arc 画正圆，实际窗口:\n{preview}"
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
        count_occurrences(&src, "zoomAround(canvasArea.width / 2, canvasArea.height / 2,"),
        2,
        "+/- 两个按钮都必须以画布中心为锚点"
    );
    assert!(
        !src.contains("zoomAround(width / 2, height / 2,"),
        "不得再用按钮自己的宽高当缩放锚点"
    );
}

/// local fit 与移动/新建 clamp 必须共用同一份内容安全区：
/// 安全区来自父圆的内接正方形扣交互壳，fit 完的内容天然合法，
/// 拖动/新建显示的就是写进 Core 的那一份坐标。
#[test]
fn content_safe_area_is_shared_by_fit_move_and_create() {
    let content = strip_line_comments(&read_src(CONTENT));
    assert!(
        content.contains("property real contentUsableSide: 0")
            && content.contains("Math.max(0, contentUsableSide - _fitPadding * 2)"),
        "内容安全区必须来自父 Embed 传入的可用边长再扣留白"
    );
    let fit = function_window(&content, "readonly property real localFitScale:", 900);
    assert!(
        fit.contains("var available = _contentSafeSide"),
        "local fit 必须用同一份内容安全区，实际窗口:\n{fit}"
    );
    let clamp = function_window(&content, "function clampToContentSafeArea(", 1200);
    assert!(
        clamp.contains("var halfSafe = _contentSafeSide / 2")
            && clamp.contains("Math.max(minLeft, Math.min(maxLeft, x * fit + localFitOffsetX))"),
        "clamp 必须和 local fit 用同一个安全区与比例，实际窗口:\n{clamp}"
    );

    // 拖动与新建都要过 clamp。
    let delta = function_window(&content, "function onSceneDragDelta(", 3200);
    assert!(
        delta.contains("var clamped = clampToContentSafeArea(candidateX, candidateY,")
            && delta.contains("ic.updateMove(clamped.x, clamped.y)"),
        "拖动候选位置必须先夹回安全区再显示/提交，实际窗口:\n{delta}"
    );
    let create_node = function_window(&content, "function createNodeWithName(", 500);
    assert!(
        create_node.contains("clampToContentSafeArea(spawn.x, spawn.y, _newNodeWidth, _newNodeHeight)")
            && create_node.contains("graphController.createNode(name, safe.x, safe.y)"),
        "新建节点必须走同一个 clamp 再写 Core，实际窗口:\n{create_node}"
    );
    let create_sub = function_window(&content, "function createSubStarmapWithName(", 500);
    assert!(
        create_sub.contains("var diameter = graphController._embedDiameter")
            && create_sub.contains("clampToContentSafeArea(spawn.x, spawn.y, diameter, diameter)"),
        "新建子星图必须按直径走同一个 clamp，实际窗口:\n{create_sub}"
    );

    // Embed 必须把可用边长沿递归链传给子内容，并在创建完成/尺寸变化时同步，
    // 避免在创建事务里读到旧值。
    let embed = strip_line_comments(&read_src(EMBED));
    assert!(
        embed.contains("\"contentUsableSide\": contentUsableSideNow()"),
        "Embed 创建子内容时必须传内容安全区来源（函数现算）"
    );
    assert!(
        embed.contains("function syncChildUsableSide()")
            && embed.contains("childContentLoader.item.contentUsableSide = contentUsableSideNow()"),
        "Embed 必须在子内容挂上后同步安全区，实际源码缺少"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 15. 边几何 / 屏幕命中 / 低 LOD 输入 / 旧 Portal 递归（评论 5976758184）
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
    let render = slice_between(
        &src,
        "fn compute_renders_from_resolved(",
        "fn coords_eq(",
    );
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

/// 边命中阈值必须按屏幕像素折算到本层 world 单位（相机 1e-4~1e5 后不能再固定）。
#[test]
fn edge_hit_threshold_is_screen_relative() {
    let content = strip_line_comments(&read_src(CONTENT));
    assert!(
        content.contains("readonly property real _edgeHitScreenPx: 10"),
        "命中阈值必须是屏幕像素常量"
    );
    assert!(
        content.contains("return _edgeHitScreenPx / Math.max(effectiveScale, 1e-6)"),
        "阈值必须除以 effectiveScale 折算到本层 world 单位"
    );
    assert!(
        content.contains("graphController.hitTestEdge(p.x, p.y, _edgeHitLocalThreshold())"),
        "递归命中必须把折算后的阈值传给 hitTestEdge"
    );

    let controller = strip_line_comments(&read_src(CONTROLLER));
    assert!(
        controller.contains("function hitTestEdge(wx, wy, threshold)")
            && controller.contains("hit_test_edge_renders(JSON.stringify(edgeRenders), wx, wy, threshold)"),
        "GraphController 必须把 threshold 透传给后端"
    );
}

/// 拉线预览起点 = 源形状朝当前鼠标方向的边界交点，与正式边同一套边界语义，
/// 否则松手瞬间端点会从圆心跳到圆周。
#[test]
fn connect_preview_origin_uses_shape_boundary() {
    let content = strip_line_comments(&read_src(CONTENT));
    let boundary = function_window(&content, "function boundaryPointLocal(", 1600);
    assert!(
        boundary.contains("var radius = Math.min(width, height) / 2")
            && boundary.contains("var tMin = Math.max(txMin, tyMin)")
            && boundary.contains("var t = tMin >= 0 ? tMin : tMax"),
        "预览起点必须按矩形/圆周求交（与 Rust line_rect_entry / line_circle_entry 同语义），\
         实际窗口:\n{boundary}"
    );
    let refresh = function_window(&content, "function refreshConnectPreviewOrigin(", 1000);
    assert!(
        refresh.contains("boundaryPointLocal(item.x, item.y, item.width, item.height,")
            && refresh.contains("ic.connectFromSceneX = scenePoint.x")
            && refresh.contains("ic.connectFromSceneY = scenePoint.y"),
        "预览起点必须写回 connectFromSceneX/Y（scene 坐标），实际窗口:\n{refresh}"
    );
    let delta = slice_between(
        &content,
        "function onSceneDragDelta(",
        "function releaseOwnerGesture(",
    );
    assert_eq!(
        count_occurrences(&delta, "refreshConnectPreviewOrigin()"),
        2,
        "contextPending 转 connect 与 connect 拖动都必须刷新预览起点，实际片段:\n{delta}"
    );
}

/// preview / shell 子图内部不能是死区：左拖继续全局 pan，右键不弹父层新建菜单。
#[test]
fn low_lod_child_content_is_not_a_dead_zone() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    let bg = function_window(&canvas, "id: bgDragArea", 6000);
    assert!(
        bg.contains("hit.kind === \"node\" || hit.kind === \"embed\"")
            && !bg.contains("hit.kind === \"childContent\""),
        "bgDragArea 只对 node/embed 放弃事件，childContent 继续 pan，实际窗口:\n{bg}"
    );
    let right = function_window(&canvas, "id: backgroundRightTap", 2600);
    let child_idx = right
        .find("hit.kind === \"childContent\"")
        .expect("backgroundRightTap 必须有 childContent 分支");
    let blank_idx = right
        .find("openBlankMenu(sx, sy, hit, px, py)")
        .expect("backgroundRightTap 必须保留空白菜单入口");
    assert!(
        child_idx < blank_idx,
        "childContent 分支必须排在 openBlankMenu 之前，低 LOD 内部不得弹父层菜单，\
         实际窗口:\n{right}"
    );
}

/// 旧 Portal 的递归加载路径与 LCA 建边路径必须同源：
/// 都由 Controller 的 embedPathSegment() 分流（enterEmbed / enterPortal）。
#[test]
fn legacy_portal_recurse_and_edge_paths_share_one_truth() {
    let content = strip_line_comments(&read_src(CONTENT));
    assert!(
        content.contains("pathSegment: graphController.embedPathSegment(embedData.instanceId)"),
        "Embed delegate 的递归路径段必须由 Controller 分流后传入"
    );
    let embed = strip_line_comments(&read_src(EMBED));
    assert!(
        embed.contains("property var pathSegment")
            && embed.contains("readonly property var childContentPathSegments: parentPathSegments.concat([pathSegment])"),
        "Embed 必须用归属层传入的 pathSegment 拼子内容路径"
    );
    assert!(
        !embed.contains("{ type: \"enterEmbed\", instanceId: instanceId, nodeId: null }"),
        "Embed 不得再自己猜 enterEmbed 段（旧 portal 会拿不存在的 instanceId 解析）"
    );

    let planner = read_src("qml/StarMapPathPlanner.js");
    assert!(
        planner.contains("last.type !== \"enterEmbed\" && last.type !== \"enterPortal\"")
            && planner.contains("terminalSegment: cloneSegment(last)")
            && planner.contains("segments.push(cloneSegment(itemRef.terminalSegment))"),
        "planner 必须保留 Embed-like 端点的原始 terminal segment（含旧 portal）"
    );
}