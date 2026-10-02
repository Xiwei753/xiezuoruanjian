//! Issue #814 评论 5935285879 — 星图共享选中 + 递归坐标模型守卫（主体功能 B）。
//!
//! WHITE_BOX 验证：读取 QML 源码，确定性断言共享选中与递归下传的不变量，防止回退。
//!
//! 锁住的结构：
//! 1. `StarMapSelectionController.qml` 存在，有 `select/clear/matches` 三个方法，
//!    选中身份由三元组 (scenePathKey, kind, itemId) 唯一确定。
//! 2. `StarMapWorkspace.qml` 实例化 `StarMapSelectionController`，传给根 Scene。
//! 3. `StarMapScene.qml` 有 `selectionController` property，传给内部 Canvas。
//! 4. `StarMapCanvas.qml` 有 `selectionController` property，传给 GraphController
//!    和 Embed delegate；Node/Embed/Edge 的 isSelected 从 `selectionController.matches`
//!    派生。
//! 5. `StarMapGraphController.qml` 不再用 `applySelection` 数组重建维护 isSelected，
//!    `selectNode/selectEdge/selectEmbed/clearSelection` 改用共享 selectionController。
//! 6. `StarMapEmbed.qml` 创建 child Scene 时传 `selectionController`，子 Scene 沿用
//!    同一个实例，不每层新建。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

const CONTROLLER_QML: &str = "qml/StarMapSelectionController.qml";
const WORKSPACE: &str = "qml/StarMapWorkspace.qml";
const SCENE: &str = "qml/StarMapScene.qml";
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
        "StarMapWorkspace 必须把共享 selectionController 传给根 Scene"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. Scene 有 selectionController property 并传给 Canvas
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn scene_has_selection_controller_property_and_forwards_to_canvas() {
    let src = read_src(SCENE);
    assert!(
        src.contains("property var selectionController: null"),
        "StarMapScene 必须有 selectionController property"
    );
    assert!(
        src.contains("selectionController: scene.selectionController"),
        "StarMapScene 必须把 selectionController 传给内部 Canvas"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 4. Canvas 有 selectionController property，isSelected 从 matches 派生
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_has_selection_controller_and_derives_is_selected() {
    let src = read_src(CANVAS);
    assert!(
        src.contains("property var selectionController: null"),
        "StarMapCanvas 必须有 selectionController property"
    );
    // 传给 GraphController
    assert!(
        src.contains("selectionController: canvasArea.selectionController"),
        "StarMapCanvas 必须把 selectionController 传给 graphController"
    );
    // Node isSelected 从 matches 派生
    assert!(
        src.contains("selectionController.matches(pathKey, \"node\", nodeData.id)"),
        "Node delegate 的 isSelected 必须从 selectionController.matches(pathKey, \"node\", id) 派生"
    );
    // Embed isSelected 从 matches 派生
    assert!(
        src.contains("selectionController.matches(pathKey, \"embed\", embedData.instanceId)"),
        "Embed delegate 的 isSelected 必须从 selectionController.matches(pathKey, \"embed\", instanceId) 派生"
    );
    // Edge isSelected 从 matches 派生
    assert!(
        src.contains("selectionController.matches(pathKey, \"edge\", edge.id)"),
        "Edge 的 isSelected 必须从 selectionController.matches(pathKey, \"edge\", id) 派生"
    );
    // Embed delegate 传 selectionController
    assert!(
        src.contains("selectionController: canvasArea.selectionController"),
        "Embed delegate 必须接收 selectionController（传给 child Scene）"
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
// 6. Embed 创建 child Scene 时传 selectionController
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn embed_passes_selection_controller_to_child_scene() {
    let src = read_src(EMBED);
    // Embed 必须有 selectionController property
    assert!(
        src.contains("property var selectionController: null"),
        "StarMapEmbed 必须有 selectionController property"
    );
    // setSource 时传 selectionController
    let sync = function_window(&src, "childSceneLoader.setSource(", 600);
    assert!(
        sync.contains("\"selectionController\": selectionController"),
        "Embed 创建 child Scene 时必须传 selectionController，子 Scene 沿用同一个，实际窗口:\n{sync}"
    );
}
