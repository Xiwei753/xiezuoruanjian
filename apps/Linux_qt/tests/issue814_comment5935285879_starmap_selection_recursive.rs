//! Issue #814 评论 5935285879 — 星图共享选中 + 递归坐标模型守卫（主体功能 B）。
//!
//! WHITE_BOX 验证：读取 QML 源码，确定性断言共享选中与递归下传的不变量，防止回退。
//!
//! 锁住的结构：
//! 1. `StarMapSelectionController.qml` 存在，有 `select/clear/matches` 三个方法，
//!    选中身份由三元组 (scenePathKey, kind, itemId) 唯一确定。
//! 2. `StarMapWorkspace.qml` 实例化 `StarMapSelectionController`，传给根 Canvas。
//! 3. `StarMapCanvas.qml` 有 `selectionController` property，传给活动画布。
//! 4. `StarMapActiveScene.qml` 有 `selectionController` required property，传给
//!    自己的 GraphController；Node/Embed/Edge 的 isSelected 从
//!    `selectionController.matches` 派生，且必须带本 route 的 `scenePathKey`。
//! 5. `StarMapGraphController.qml` 不再用 `applySelection` 数组重建维护 isSelected，
//!    `selectNode/selectEdge/selectEmbed/clearSelection` 改用共享 selectionController。
//! 6. `StarMapEmbed.qml` 不再创建 child Content：父图里的只读微缩概览不参与选中，
//!    入口的选中环仍用主题强调色表达 isSelected。
//!
//! #929 更新：独立画布协议删除递归子画布与 childContentLoader，共享选中链路
//! 变成 Workspace → Canvas → 唯一 StarMapActiveScene → GraphController /
//! Node/Embed/Edge delegate；`StarMapSceneContent.qml` 只是只读微缩概览，
//! 既不持有 selectionController，也没有 scenePathKey。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

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

const CONTROLLER_QML: &str = "qml/StarMapSelectionController.qml";
const WORKSPACE: &str = "qml/StarMapWorkspace.qml";
const CONTENT: &str = "qml/StarMapSceneContent.qml";
const ACTIVE_SCENE: &str = "qml/StarMapActiveScene.qml";
const CANVAS: &str = "qml/StarMapCanvas.qml";
const GRAPH_CONTROLLER: &str = "qml/StarMapGraphController.qml";
const EMBED: &str = "qml/StarMapEmbed.qml";

// ─────────────────────────────────────────────────────────────────────────
// 1. StarMapSelectionController.qml 存在且有 select/clear/matches
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn selection_controller_file_exists_with_three_methods() {
    let src = read_src(CONTROLLER_QML);
    // 三个核心方法
    let select = function_window(&src, "function select(", 300);
    assert!(
        select.contains("scenePathKey = pathKey")
            && select.contains("kind = nextKind")
            && select.contains("itemId = nextItemId"),
        "select 必须设置 scenePathKey/kind/itemId 三元组，实际窗口:\n{select}"
    );
    let clear = function_window(&src, "function clear()", 200);
    assert!(
        clear.contains("scenePathKey = \"\"")
            && clear.contains("kind = \"\"")
            && clear.contains("itemId = \"\""),
        "clear 必须清空 scenePathKey/kind/itemId，实际窗口:\n{clear}"
    );
    let matches = function_window(&src, "function matches(", 300);
    assert!(
        matches.contains("scenePathKey === pathKey")
            && matches.contains("kind === nextKind")
            && matches.contains("itemId === nextItemId"),
        "matches 必须比较 scenePathKey/kind/itemId 三元组，实际窗口:\n{matches}"
    );
    // 三个 property
    assert!(
        src.contains("property string scenePathKey:")
            && src.contains("property string kind:")
            && src.contains("property string itemId:"),
        "StarMapSelectionController 必须有 scenePathKey/kind/itemId 三个 property"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 2. Workspace 实例化 selectionController 并传给根 Scene
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn workspace_instantiates_shared_selection_controller() {
    let src = read_src(WORKSPACE);
    assert!(
        src.contains("StarMapSelectionController"),
        "StarMapWorkspace 必须实例化 StarMapSelectionController"
    );
    assert!(
        src.contains("selectionController: sharedSelectionController"),
        "StarMapWorkspace 必须把共享 selectionController 传给根 Canvas"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. Canvas 有 selectionController property 并传给根 Content
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_has_selection_controller_and_forwards_to_root_content() {
    let src = read_src(CANVAS);
    assert!(
        src.contains("property var selectionController: null"),
        "StarMapCanvas 必须有 selectionController property"
    );
    assert!(
        src.contains("selectionController: canvasArea.selectionController"),
        "StarMapCanvas 必须把 selectionController 传给根 StarMapSceneContent"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 4. 活动画布有 selectionController required property，isSelected 从 matches 派生
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn active_scene_has_selection_controller_and_derives_is_selected() {
    // #929 取代 #822：递归 Content 不再持有共享选中；唯一实时场景
    // StarMapActiveScene 接收共享 selectionController，传给本画布
    // GraphController，并由 Node/Embed/Edge delegate 派生 isSelected。
    let src = read_src(ACTIVE_SCENE);
    assert!(
        src.contains("required property var selectionController"),
        "StarMapActiveScene 必须有 selectionController required property"
    );
    // 传给本画布 GraphController
    assert!(
        src.contains("selectionController: scene.selectionController"),
        "StarMapActiveScene 必须把 selectionController 传给本画布 graphController"
    );
    // Node isSelected 从 matches 派生，且带本 route 的 scenePathKey
    assert!(
        src.contains(
            "scene.selectionController.matches(scene.scenePathKey, \"node\", nodeData.id)"
        ),
        "Node delegate 的 isSelected 必须从 selectionController.matches(scene.scenePathKey, \"node\", id) 派生"
    );
    // Embed isSelected 从 matches 派生
    assert!(
        src.contains(
            "scene.selectionController.matches(scene.scenePathKey, \"embed\", embedData.instanceId)"
        ),
        "Embed delegate 的 isSelected 必须从 selectionController.matches(scene.scenePathKey, \"embed\", instanceId) 派生"
    );
    // Edge isSelected 从 matches 派生
    assert!(
        src.contains("scene.selectionController.matches(scene.scenePathKey, \"edge\", render.edgeId)"),
        "Edge 的 isSelected 必须从 selectionController.matches(scene.scenePathKey, \"edge\", id) 派生"
    );
    // 只读微缩概览不参与选中，不得再持有 selectionController。
    let content = read_src(CONTENT);
    assert!(
        !content.contains("selectionController"),
        "只读微缩概览不得持有 selectionController"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 5. GraphController 不再用 applySelection 数组重建
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn graph_controller_does_not_rebuild_arrays_for_is_selected() {
    let src = read_src(GRAPH_CONTROLLER);
    // applySelection 不应再包含数组重建逻辑（nextNodes/nextEdges/nextEmbeds 浅拷贝）
    let window = function_window(&src, "function applySelection(", 1200);
    assert!(
        !window.contains("n.isSelected = nodeId !== \"\" && n.id === nodeId"),
        "applySelection 不得再原地改 n.isSelected（数组重建已删除），实际窗口:\n{window}"
    );
    assert!(
        !window.contains("nodesModel = nextNodes"),
        "applySelection 不得再 nodesModel = nextNodes 数组重建，实际窗口:\n{window}"
    );
}

#[test]
fn graph_controller_select_methods_use_shared_selection_controller() {
    let src = read_src(GRAPH_CONTROLLER);
    // 必须有 selectionController 和 pathKey property
    assert!(
        src.contains("property var selectionController: null")
            && src.contains("property string pathKey:"),
        "GraphController 必须有 selectionController 和 pathKey property"
    );
    // selectNode 调 selectionController.select
    let select_node = function_window(&src, "function selectNode(", 300);
    assert!(
        select_node.contains("selectionController.select(pathKey, \"node\", nodeId)"),
        "selectNode 必须调 selectionController.select(pathKey, \"node\", nodeId)，实际窗口:\n{select_node}"
    );
    // selectEdge 调 selectionController.select
    let select_edge = function_window(&src, "function selectEdge(", 300);
    assert!(
        select_edge.contains("selectionController.select(pathKey, \"edge\", edgeId)"),
        "selectEdge 必须调 selectionController.select(pathKey, \"edge\", edgeId)，实际窗口:\n{select_edge}"
    );
    // selectEmbed 调 selectionController.select
    let select_embed = function_window(&src, "function selectEmbed(", 300);
    assert!(
        select_embed.contains("selectionController.select(pathKey, \"embed\", instanceId)"),
        "selectEmbed 必须调 selectionController.select(pathKey, \"embed\", instanceId)，实际窗口:\n{select_embed}"
    );
    // clearSelection 调 selectionController.clear
    let clear = function_window(&src, "function clearSelection()", 200);
    assert!(
        clear.contains("selectionController.clear()"),
        "clearSelection 必须调 selectionController.clear()，实际窗口:\n{clear}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 6. Embed 只读概览不建子场景；选中环用主题强调色
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn embed_is_readonly_overview_with_selection_ring() {
    // #929 取代 #822：Embed 不再创建可交互 child Content，也不再有
    // childContentLoader / _syncChildContent / childScenePathKey /
    // selectionController 传递链；父图里只有圆壳 + 只读微缩概览。
    // 入口选中由活动画布 delegate 按共享 selectionController 算好后传入，
    // 选中环用主题强调色（accent），不依赖 selectionRing token。
    let src = strip_line_comments(&read_src(EMBED));
    for removed in [
        "childContentLoader",
        "_syncChildContent",
        "childScenePathKey",
        "selectionController",
        "maskedContent",
        "contentViewport",
    ] {
        assert!(
            !src.contains(removed),
            "Embed 不得再保留递归子内容 / 子场景选中链 {removed}"
        );
    }
    // 概览 Loader 只在本入口投影足够大时加载只读数据。
    let loader = function_window(&src, "id: previewLoader", 400);
    assert!(
        loader.contains("active: root.previewWanted")
            && loader.contains("sourceComponent: previewComponent"),
        "概览必须懒加载，实际窗口:\n{loader}"
    );
    let preview = function_window(&src, "StarMapSceneContent {", 300);
    for prop in [
        "dt: root.dt",
        "starmapId: root.targetStarmapId",
        "starmapBackendRef: root.starmapBackendRef",
    ] {
        assert!(
            preview.contains(prop),
            "概览只允许接收只读数据 {prop}，实际窗口:\n{preview}"
        );
    }
    assert!(
        !preview.contains("selectionController"),
        "概览不得接收选中状态，实际窗口:\n{preview}"
    );
    assert!(
        src.contains("border.color: root.dt.accent") && !src.contains("selectionRing"),
        "选中环必须用主题强调色表达，不依赖 selectionRing token"
    );
}
