//! Issue #834 — Linux_Qt 星图：超链接改内部跳转 Link、右键补连线、子星图改反色圆形视觉。
//!
//! 本文件锁住本轮结构不变量，防止回退：
//! 1. Rust bridge 有 StarMapLink 的 add/update/delete/list envelope（生成 lk_{uuid}）。
//! 2. backend/starmap_backend/links.rs 存在并注册为 qt_method。
//! 3. GraphController 有 addLink/updateLink/deleteLink/listLinks/createLinkWithPaths。
//! 4. InteractionController 有 linkArmed/connectArmed/beginLinkArmed/beginConnectFromMenu/cancelArmed。
//! 5. PathPlanner 用通用 planCrossLayerRelation（Edge/Link 共用，无第二套 LCA）。
//! 6. SceneContent 有 finishLink/commitLinkWithPaths/listLinksForSource。
//! 7. Canvas：hyperlinkDialog 已删，linkDialog 已建；菜单有"内部链接"和"连线"；
//!    signal 改名 visualFocusChanged（无 focusChanged 撞名）。
//! 8. StarMapEmbed：填充用 surface/surfaceContainerLow，边框用 inverseSurface，
//!    MultiEffect 圆形 mask，selectionRing 选中 ring，antialiasing。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::read_src;

const BRIDGE: &str = "src/starmap_bridge.rs";
const LINKS_BACKEND: &str = "src/backend/starmap_backend/links.rs";
const BACKEND: &str = "src/backend/starmap_backend.rs";
const GRAPH_CTL: &str = "qml/StarMapGraphController.qml";
const INTERACTION: &str = "qml/StarMapInteractionController.qml";
const PLANNER: &str = "qml/StarMapPathPlanner.js";
const CONTENT: &str = "qml/StarMapSceneContent.qml";
const CANVAS: &str = "qml/StarMapCanvas.qml";
const EMBED: &str = "qml/StarMapEmbed.qml";

#[test]
fn link_bridge_envelope_present() {
    let bridge = read_src(BRIDGE);
    // bridge 层 4 个 envelope 函数
    assert!(
        bridge.contains("pub fn add_starmap_link("),
        "bridge 缺 add_starmap_link"
    );
    assert!(
        bridge.contains("pub fn update_starmap_link("),
        "bridge 缺 update_starmap_link"
    );
    assert!(
        bridge.contains("pub fn delete_starmap_link("),
        "bridge 缺 delete_starmap_link"
    );
    assert!(
        bridge.contains("pub fn list_starmap_links("),
        "bridge 缺 list_starmap_links"
    );
    // link_id 用 lk_ 前缀（与 n_/e_/em_/hl_ 模式一致）
    assert!(bridge.contains("lk_{}"), "bridge link_id 未用 lk_ 前缀");
    // import Link DTO
    assert!(
        bridge.contains("StarMapLinkDto"),
        "bridge 未 import StarMapLinkDto"
    );
    assert!(
        bridge.contains("StarMapLinkPatchInputDto"),
        "bridge 未 import StarMapLinkPatchInputDto"
    );
}

#[test]
fn link_backend_module_present() {
    let links = read_src(LINKS_BACKEND);
    assert!(
        links.contains("fn add_starmap_link_json"),
        "links.rs 缺 add_starmap_link_json"
    );
    assert!(
        links.contains("fn list_starmap_links_json"),
        "links.rs 缺 list_starmap_links_json"
    );
    let backend = read_src(BACKEND);
    assert!(
        backend.contains("mod links"),
        "starmap_backend.rs 未注册 mod links"
    );
    assert!(
        backend.contains("add_starmap_link"),
        "starmap_backend.rs 未注册 add_starmap_link qt_method"
    );
}

#[test]
fn graph_controller_link_wrappers_present() {
    let gc = read_src(GRAPH_CTL);
    for f in [
        "function addLink(",
        "function updateLink(",
        "function deleteLink(",
        "function listLinks(",
        "function createLinkWithPaths(",
    ] {
        assert!(gc.contains(f), "GraphController 缺 {}", f);
    }
}

#[test]
fn interaction_armed_state_present() {
    let ic = read_src(INTERACTION);
    for tok in [
        "linkArmed",
        "connectArmed",
        "beginLinkArmed",
        "beginConnectFromMenu",
        "cancelArmed",
    ] {
        assert!(ic.contains(tok), "InteractionController 缺 {}", tok);
    }
}

#[test]
fn planner_uses_universal_relation_name() {
    let planner = read_src(PLANNER);
    assert!(
        planner.contains("function planCrossLayerRelation("),
        "PathPlanner 缺 planCrossLayerRelation"
    );
    assert!(
        !planner.contains("function planCrossLayerEdge("),
        "PathPlanner 仍残留 planCrossLayerEdge 定义"
    );
}

#[test]
fn scene_content_link_commit_present() {
    let sc = read_src(CONTENT);
    for f in [
        "function finishLink(",
        "function commitLinkWithPaths(",
        "function listLinksForSource(",
    ] {
        assert!(sc.contains(f), "SceneContent 缺 {}", f);
    }
    // 调用方用 planCrossLayerRelation
    assert!(
        sc.contains("planCrossLayerRelation"),
        "SceneContent 未用 planCrossLayerRelation"
    );
}

#[test]
fn canvas_link_dialog_and_menu_items_present() {
    let canvas = read_src(CANVAS);
    // hyperlinkDialog 已删
    assert!(
        !canvas.contains("hyperlinkDialog"),
        "Canvas 仍残留 hyperlinkDialog"
    );
    // linkDialog 已建
    assert!(canvas.contains("linkDialog"), "Canvas 缺 linkDialog");
    // 菜单项
    assert!(
        canvas.contains(qstr_internal_link()),
        "Canvas 菜单缺内部链接项"
    );
    assert!(canvas.contains(qstr_connect_line()), "Canvas 菜单缺连线项");
    // signal 改名
    assert!(
        canvas.contains("signal visualFocusChanged()"),
        "Canvas 未改名 visualFocusChanged"
    );
    assert!(
        !canvas.contains("signal focusChanged()"),
        "Canvas 仍残留 signal focusChanged()"
    );
}

// qsTr("内部链接") / qsTr("连线") 在源码里带引号，直接断言中文串出现即可。
fn qstr_internal_link() -> &'static str {
    "内部链接"
}
fn qstr_connect_line() -> &'static str {
    "连线"
}

#[test]
fn embed_visual_uses_surface_and_inverse_and_mask() {
    let embed = read_src(EMBED);
    // 填充用表面色（不再用 _accentSoft 作主填充）
    assert!(
        embed.contains("_surfaceLow")
            || embed.contains("dt.surfaceContainerLow")
            || embed.contains("dt.surface"),
        "Embed 未改用表面色填充"
    );
    // 边框用反色
    assert!(
        embed.contains("_inverseSurface") || embed.contains("dt.inverseSurface"),
        "Embed 未改用反色边框"
    );
    // 圆形 mask
    assert!(
        embed.contains("MultiEffect") || embed.contains("OpacityMask"),
        "Embed 未用 MultiEffect/OpacityMask 圆形 mask"
    );
    // 选中 ring
    assert!(embed.contains("selectionRing"), "Embed 缺 selectionRing");
    // antialiasing
    assert!(
        embed.contains("antialiasing: true"),
        "Embed 缺 antialiasing"
    );
    // 不再用 _accentSoft（应已删该属性或不再作主填充）
    assert!(
        !embed.contains("color: root.isSelected ? root._surfaceContainer : root._accentSoft"),
        "Embed 仍用旧 _accentSoft 填充"
    );
}
