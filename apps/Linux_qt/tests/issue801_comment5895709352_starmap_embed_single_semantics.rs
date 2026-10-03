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
//! 6. Issue #801 评论 5896594591: 旧 portal 操作身份分流到 Node API。
//! 7. Issue #801 评论 5895785633: 类型标签移除必须彻底——`StarMapNode` 不得再保留
//!    `kind` 属性和 `getKindLabel`/`getKindColor` 死代码，`StarMapCanvas` 不再下发
//!    `kind`；Embed 卡片文本直接绑定 `label`，渲染层不回落类型名，
//!    空 label 由 Controller 统一兜底为 `qsTr("未命名")`。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

const GRAPH_CONTROLLER: &str = "qml/StarMapGraphController.qml";
const CANVAS: &str = "qml/StarMapCanvas.qml";
const CONTENT: &str = "qml/StarMapSceneContent.qml";
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

    // Issue #822：Node delegate 已从根 Canvas 搬到递归层 Content；
    // 双击不再上抛 editNodeRequested 去开外部 Popup，而是直接进入节点自身的 TextInput。
    let content = strip_line_comments(&read_src(CONTENT));
    let double_click = function_window(&content, "onDoubleClicked: content.beginInlineEdit(", 200);
    assert!(
        double_click.contains("content.beginInlineEdit(nodeData.id)"),
        "Node 双击必须进入本层节点的内联编辑，实际窗口:\n{double_click}"
    );
    assert!(
        !content.contains("editNodeRequested"),
        "内联编辑后不得再保留 editNodeRequested 外部 Popup 通道"
    );
}

/// 4b. Issue #822：子星图不再有"下钻进入另一页"这个入口，也就不需要下钻信号。
/// 子星图内容就是本层的递归 Content，永远在同一个全局视口里就地展开；
/// 命中与菜单都走递归命中入口，Embed 只是一种内容容器语义。
#[test]
fn no_starmap_drill_down_entry_remains() {
    for src_name in [CANVAS, CONTENT, EMBED] {
        let src = strip_line_comments(&read_src(src_name));
        assert!(
            !src.contains("drillDown") && !src.contains("drillUp"),
            "{src_name} 不得再有下钻/回退入口，子星图是就地展开的递归内容"
        );
    }
    // 递归命中入口能返回 Embed 内容命中，说明子星图内部直接可命中
    let content = strip_line_comments(&read_src(CONTENT));
    let hit = function_window(&content, "function hitTargetAtScene(", 3000);
    assert!(
        hit.contains("\"childContent\""),
        "递归命中必须能命中子星图内容区，实际窗口:\n{hit}"
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
/// Canvas 拉线端点对 legacy portal 用 portalPath（EnterPortal 段），不构造 EnterEmbed。
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
    let update_embed = slice_between(
        &controller,
        "function updateEmbed(instanceId, patch)",
        "function deleteEmbed(",
    );
    assert!(
        update_embed.contains("legacyPortalNodeId") && update_embed.contains("update_starmap_node"),
        "updateEmbed 必须按 legacyPortalNodeId 分流到 update_starmap_node，实际窗口:\n{update_embed}"
    );
    assert!(
        update_embed.contains("nodePatch.title = patch.label"),
        "updateEmbed 分流必须把 label patch 转成 Node title，实际窗口:\n{update_embed}"
    );

    // deleteEmbed 分流到 delete_starmap_node
    let delete_embed = slice_between(
        &controller,
        "function deleteEmbed(instanceId)",
        "function createSubStarmapAt(",
    );
    assert!(
        delete_embed.contains("legacyPortalNodeId") && delete_embed.contains("delete_starmap_node"),
        "deleteEmbed 必须按 legacyPortalNodeId 分流到 delete_starmap_node，实际窗口:\n{delete_embed}"
    );

    // commitEmbedMove 分流到 update_starmap_node
    let commit_move = slice_between(
        &controller,
        "function commitEmbedMove(instanceId, nx, ny)",
        "function computeEdgeRenders(",
    );
    assert!(
        commit_move.contains("legacyPortalNodeId") && commit_move.contains("update_starmap_node"),
        "commitEmbedMove 必须按 legacyPortalNodeId 分流到 update_starmap_node，实际窗口:\n{commit_move}"
    );

    // Issue #822：路径 DTO 的拼装搬到了递归层 Content，根 Canvas 不再拼路径，
    // 也不再直接访问 legacyPortalNodeId（该身份只由 Controller 封装）。
    assert!(
        !canvas.contains("function embedConnectPath(")
            && !canvas.contains("function portalPath(")
            && !canvas.contains("function embedPath(")
            && !canvas.contains("legacyPortalNodeId"),
        "StarMapCanvas 不得再拼 Embed 目标路径或直接访问 legacyPortalNodeId"
    );
    // Content 有 embedPath()，只用 Controller 的 embedPathSegment 分流旧 portal 身份
    let content = strip_line_comments(&read_src(CONTENT));
    let embed_path = function_window(&content, "function embedPath(instanceId)", 600);
    assert!(
        embed_path.contains("graphController.embedPathSegment(instanceId)"),
        "Content 的 embedPath 必须通过 graphController.embedPathSegment 分流，实际窗口:\n{embed_path}"
    );
    assert!(
        !content.contains("legacyPortalNodeId") && !content.contains("enterPortal"),
        "Content 不得自行判断 legacyPortalNodeId 或直接构造 enterPortal 段"
    );
    // Controller 只保留唯一入口 embedPathSegment：旧 portal → enterPortal，新 Embed → enterEmbed
    assert!(
        controller.contains("function embedPathSegment(instanceId)"),
        "StarMapGraphController 必须有 embedPathSegment(instanceId) 函数"
    );
    let ctrl_helper = slice_between(
        &controller,
        "function embedPathSegment(instanceId)",
        "function createNode(",
    );
    assert!(
        ctrl_helper.contains("legacyPortalNodeId")
            && ctrl_helper.contains("enterPortal")
            && ctrl_helper.contains("enterEmbed"),
        "Controller embedPathSegment 必须按 legacyPortalNodeId 分流 enterPortal/enterEmbed，实际窗口:\n{ctrl_helper}"
    );
    assert!(
        !controller.contains("function embedConnectPath(")
            && !controller.contains("function embedDrillSegment("),
        "Controller 只保留 embedPathSegment 一个路径 segment 入口"
    );
}

/// 7. Issue #801 评论 5895785633: 类型标签移除必须彻底——
/// 节点组件不再保留 `kind` 属性与 `getKindLabel`/`getKindColor` 死代码，
/// Canvas 的 Node delegate 不再下发 `kind`。
#[test]
fn node_component_has_no_type_label_leftovers() {
    let node = strip_line_comments(&read_src(NODE));
    assert!(
        !node.contains("property string kind"),
        "StarMapNode 不得再保留无消费者的 kind 属性"
    );
    assert!(
        !node.contains("getKindLabel") && !node.contains("getKindColor"),
        "StarMapNode 不得再保留类型标签 helper（getKindLabel/getKindColor）"
    );

    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        !canvas.contains("nodeData.kind"),
        "StarMapCanvas 的 Node delegate 不得再向 StarMapNode 下发 kind"
    );
}

/// 8. Issue #801 评论 5895785633: 空 Embed 不得把类型名“子星图”当标题显示——
/// 卡片文本直接绑定 `label`，渲染层不做任何字符串 fallback；
/// 未命名兜底统一在 Controller 的 `label: gem.label || qsTr("未命名")` 完成。
#[test]
fn embed_card_renders_empty_label_without_type_fallback() {
    let embed = strip_line_comments(&read_src(EMBED));
    assert!(
        embed.contains("text: root.label"),
        "Embed 卡片文本必须直接绑定 root.label"
    );
    let label_render = function_window(&embed, "text: root.label", 120);
    assert!(
        !label_render.contains("||") && !label_render.contains("子星图"),
        "label 渲染不得回落类型名或做字符串 fallback，实际窗口:\n{label_render}"
    );
}
