//! Issue #801 评论 5895709352 — 子星图入口单一语义守卫。
//!
//! WHITE_BOX 验证策略：QML 无法在本仓库的 Rust 测试里实例化（组件依赖
//! `qml_resources` qrc 与 Rust 注册的上下文属性），因此按仓库既有惯例
//! （见 `issue762_comment5826175490_qml_wiring.rs` / `issue702_*`）读取 QML
//! 源码，确定性断言这次修复的接线不变量，防止回退：
//!
//! 1. `StarMapGraphController.buildModels()` 把带 `portal.destinationStarmapId`
//!    的旧 Node 归一到 `embedsModel`，不再作为普通 Node 下发——Canvas 以后只有
//!    一种子星图入口语义：Embed。
//! 2. `label` 不再把对象类型（"子星图"）当标题/占位文案，统一用 `qsTr("未命名")`。
//! 3. `StarMapEmbed.qml` 默认 `label` 为空。
//! 4. `StarMapNode.qml` / `StarMapCanvas.qml` 不再有 `isPortal` 语义，
//!    Node 双击统一走 `editNodeRequested`，不再特判 `portal.destinationStarmapId`
//!    下钻；下钻入口只剩 Embed。
//! 5. `computeEdgeRenders()` 把归一后的旧 portal 节点几何补进边锚点 layout，
//!    否则连到旧 portal 节点的边会因 `LocalNodeMissing`/`PortalMissing` 整条消失
//!    （行为契约由 `starmap_view::edge_render` 的两个单测固定）。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

const GRAPH_CONTROLLER: &str = "qml/StarMapGraphController.qml";
const CANVAS: &str = "qml/StarMapCanvas.qml";
const NODE: &str = "qml/StarMapNode.qml";
const EMBED: &str = "qml/StarMapEmbed.qml";

/// 去掉整行 `//` 注释，只留可执行语句。
///
/// 守卫断言的是"代码不再依赖某模式"，注释里说明历史原因提到该模式不算违规
/// （例如 "旧 portal Node 已归一到 Embed" 这句注释本身）。
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

/// 1. 旧 portal Node 在 buildModels() 里归一到 embedsModel，不再进 nodesModel。
#[test]
fn build_models_routes_legacy_portal_nodes_into_embeds() {
    let src = strip_line_comments(&read_src(GRAPH_CONTROLLER));
    let build = slice_between(&src, "function buildModels()", "function getNode(");

    assert!(
        build.contains("if (gn.portal && gn.portal.destinationStarmapId)"),
        "旧 portal Node 必须按 portal.destinationStarmapId 判定并归一，实际窗口:\n{build}"
    );
    assert!(
        build.contains("targetStarmapId: gn.portal.destinationStarmapId"),
        "归一条目必须保留 portal 的目标星图，实际窗口:\n{build}"
    );

    // 归一分支必须发生在 embedsModel 赋值之前（newEmbeds 提前声明），否则归一条目丢失。
    let portal_at = build
        .find("gn.portal")
        .expect("buildModels 必须有 portal 归一分支");
    let assign_at = build
        .find("embedsModel = newEmbeds")
        .expect("buildModels 必须把 newEmbeds 赋给 embedsModel");
    assert!(
        portal_at < assign_at,
        "portal 归一分支必须发生在 embedsModel = newEmbeds 之前"
    );

    // nodesModel 的 push 块不得再携带 portal 字段（两套语义的来源）。
    let nodes_push = slice_between(&build, "newNodes.push({", "});");
    assert!(
        !nodes_push.contains("portal"),
        "nodesModel 不得再下发 portal 字段，实际 push 块:\n{nodes_push}"
    );
}

/// 2. 名称 fallback 不再把对象类型当文案。
#[test]
fn sub_starmap_entries_do_not_use_object_type_as_label() {
    for rel in [GRAPH_CONTROLLER, EMBED] {
        let src = strip_line_comments(&read_src(rel));
        assert!(
            !src.contains("qsTr(\"子星图\")"),
            "{rel} 不得再把对象类型“子星图”当名称/占位文案"
        );
    }

    let controller = strip_line_comments(&read_src(GRAPH_CONTROLLER));
    assert!(
        controller.contains("label: gem.label || qsTr(\"未命名\")"),
        "Embed label fallback 必须是 qsTr(\"未命名\")"
    );

    let embed = strip_line_comments(&read_src(EMBED));
    assert!(
        embed.contains("property string label: \"\""),
        "StarMapEmbed 默认 label 必须为空字符串，不再预设类型文字"
    );
}

/// 3. StarMapNode 不再有 portal/isPortal 语义。
#[test]
fn node_component_has_no_portal_flag() {
    let src = strip_line_comments(&read_src(NODE));
    assert!(
        !src.contains("isPortal") && !src.contains("portal") && !src.contains("Portal"),
        "StarMapNode 不得再保留 isPortal/portal 语义"
    );
}

/// 4. Canvas 的 Node delegate 没有 isPortal，双击统一编辑。
#[test]
fn canvas_node_delegate_has_no_portal_special_case() {
    let src = strip_line_comments(&read_src(CANVAS));
    assert!(
        !src.contains("isPortal"),
        "StarMapCanvas 不得再给 Node 绑定 isPortal"
    );
    assert!(
        !src.contains("destinationStarmapId"),
        "StarMapCanvas 不得再按 portal.destinationStarmapId 特判下钻"
    );

    let node_double_click = function_window(&src, "onDoubleClicked: {", 220);
    assert!(
        node_double_click.contains("editNodeRequested(nd)"),
        "Node 双击必须统一走编辑，实际窗口:\n{node_double_click}"
    );
    assert!(
        !node_double_click.contains("drillDownRequested") && !node_double_click.contains("portal"),
        "Node 双击不得再下钻，实际窗口:\n{node_double_click}"
    );
}

/// 4b. Embed 双击是唯一下钻入口，title fallback 不用类型名。
#[test]
fn canvas_embed_delegate_is_the_only_drill_down_entry() {
    let src = strip_line_comments(&read_src(CANVAS));
    let embed_double_click =
        function_window(&src, "onDoubleClicked: function(tgtStarmapId) {", 400);
    assert!(
        embed_double_click
            .contains("drillDownRequested(tgtStarmapId, ed.label || qsTr(\"未命名\"))"),
        "Embed 双击必须上抛 drillDownRequested 且 title fallback 用“未命名”，\
         实际窗口:\n{embed_double_click}"
    );
    assert!(
        !embed_double_click.contains("子星图"),
        "Embed 双击不得再用类型名当 title，实际窗口:\n{embed_double_click}"
    );
}

/// 5. 归一到 Embeds 的旧 portal 节点仍要出现在边锚点 layout 里。
#[test]
fn edge_layout_still_covers_normalized_legacy_portal_nodes() {
    let src = strip_line_comments(&read_src(GRAPH_CONTROLLER));
    let compute = slice_between(
        &src,
        "function computeEdgeRenders(moveOverride)",
        "function hitTestEdge",
    );

    assert!(
        compute.contains("if (!pn.portal || !pn.portal.destinationStarmapId) continue"),
        "computeEdgeRenders 必须识别归一的旧 portal 节点，实际窗口:\n{compute}"
    );
    assert!(
        compute.contains("getEmbed(\"legacy-portal:\" + pn.id)"),
        "旧 portal 节点的显示几何必须按归一前缀 key 取，实际窗口:\n{compute}"
    );
    assert!(
        compute.contains("nodePos.push({ id: pn.id"),
        "旧 portal 节点几何必须补进边锚点 layout，实际窗口:\n{compute}"
    );
}

/// 6. Issue #801 评论 5896594591: 旧 portal 操作身份分流。
/// 归一条目必须保存真实 Node ID（legacyPortalNodeId），instanceId 加前缀仅作 UI key；
/// updateEmbed/deleteEmbed/commitEmbedMove 必须按 legacyPortalNodeId 分流到 Node API；
/// Canvas 拉线端点对 legacy portal 用 nodePath，不构造 EnterEmbed。
#[test]
fn legacy_portal_operations_route_to_node_api() {
    let controller = strip_line_comments(&read_src(GRAPH_CONTROLLER));
    let canvas = strip_line_comments(&read_src(CANVAS));

    // buildModels 归一条目保存真实身份
    let build = slice_between(&controller, "function buildModels()", "function getNode(");
    assert!(
        build.contains("legacyPortalNodeId: gn.id"),
        "buildModels 归一条目必须保存 legacyPortalNodeId: gn.id，实际窗口:\n{build}"
    );
    assert!(
        build.contains("instanceId: \"legacy-portal:\" + gn.id"),
        "buildModels 归一条目 instanceId 必须加 legacy-portal: 前缀，实际窗口:\n{build}"
    );

    // updateEmbed 分流到 update_starmap_node
    let update_embed = slice_between(&controller, "function updateEmbed(instanceId, patch)", "function deleteEmbed(");
    assert!(
        update_embed.contains("legacyPortalNodeId") && update_embed.contains("update_starmap_node"),
        "updateEmbed 必须按 legacyPortalNodeId 分流到 update_starmap_node，实际窗口:\n{update_embed}"
    );
    assert!(
        update_embed.contains("nodePatch.title = patch.label"),
        "updateEmbed 分流必须把 label patch 转成 Node title，实际窗口:\n{update_embed}"
    );

    // deleteEmbed 分流到 delete_starmap_node
    let delete_embed = slice_between(&controller, "function deleteEmbed(instanceId)", "function createSubStarmapAt(");
    assert!(
        delete_embed.contains("legacyPortalNodeId") && delete_embed.contains("delete_starmap_node"),
        "deleteEmbed 必须按 legacyPortalNodeId 分流到 delete_starmap_node，实际窗口:\n{delete_embed}"
    );

    // commitEmbedMove 分流到 update_starmap_node
    let commit_move = slice_between(&controller, "function commitEmbedMove(instanceId, nx, ny)", "function computeEdgeRenders(");
    assert!(
        commit_move.contains("legacyPortalNodeId") && commit_move.contains("update_starmap_node"),
        "commitEmbedMove 必须按 legacyPortalNodeId 分流到 update_starmap_node，实际窗口:\n{commit_move}"
    );

    // Canvas 有 embedConnectPath helper，legacy portal 用 nodePath
    assert!(
        canvas.contains("function embedConnectPath(embed)"),
        "StarMapCanvas 必须有 embedConnectPath helper"
    );
    let helper = slice_between(&canvas, "function embedConnectPath(embed)", "function createEdgeWithPaths(");
    assert!(
        helper.contains("legacyPortalNodeId") && helper.contains("nodePath(embed.legacyPortalNodeId)"),
        "embedConnectPath 必须对 legacy portal 用 nodePath，实际窗口:\n{helper}"
    );

    // 4 个拉线调用点用 embedConnectPath 而非裸 embedPath
    // （connect 松手 / 鼠标长按 / 触屏长按 / connect 松手命中 embed）
    let connect_count = canvas.matches("embedConnectPath(").count();
    assert!(
        connect_count >= 4,
        "Canvas 必须有至少 4 处 embedConnectPath 调用（拉线端点分流），实际: {connect_count}"
    );
}
