//! Issue #801 评论 5897793716 — 星图无级层级路径段守卫。
//!
//! WHITE_BOX 验证评论 5897793716 的两处修复：
//! 1. legacy portal 拉线端点用 EnterPortal 段（portalPath），不再用普通 nodePath。
//! 2. root starmap + 实例路径：下钻事件携带具体 segment（EnterEmbed/EnterPortal），
//!    Workspace 维护完整 currentPathSegments，下钻 append、返回 pop，
//!    currentStarmapId 不再充当完整层级身份。
//!
//! Issue #801 评论 5895878756 更新：legacyPortalNodeId 分流逻辑已从 Canvas 移到
//! StarMapGraphController，Canvas 只调用 graphController.embedConnectPath(instanceId)
//! 和 graphController.embedDrillSegment(instanceId)，不再直接访问 legacyPortalNodeId。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

const CANVAS: &str = "qml/StarMapCanvas.qml";
const GRAPH_CONTROLLER: &str = "qml/StarMapGraphController.qml";
const WORKSPACE: &str = "qml/StarMapWorkspace.qml";

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

/// 1. Controller embedConnectPath 对 legacy portal 构造 EnterPortal 段。
#[test]
fn controller_embed_connect_path_builds_enter_portal_segment() {
    let src = strip_line_comments(&read_src(GRAPH_CONTROLLER));
    let helper = slice_between(&src, "function embedConnectPath(instanceId)", "function embedDrillSegment(");
    assert!(
        helper.contains("\"enterPortal\""),
        "Controller embedConnectPath 必须构造 enterPortal 段，实际窗口:\n{helper}"
    );
    assert!(
        helper.contains("nodeId: embed.legacyPortalNodeId"),
        "embedConnectPath 段必须携带 nodeId，实际窗口:\n{helper}"
    );
    assert!(
        helper.contains("target: { type: \"starmap\" }"),
        "embedConnectPath target 必须是 starmap，实际窗口:\n{helper}"
    );
}

/// 2. Controller embedConnectPath 对 legacy portal 用 enterPortal，对正式 Embed 用 enterEmbed。
#[test]
fn controller_embed_connect_path_routes_legacy_portal() {
    let src = strip_line_comments(&read_src(GRAPH_CONTROLLER));
    let helper = slice_between(&src, "function embedConnectPath(instanceId)", "function embedDrillSegment(");
    assert!(
        helper.contains("legacyPortalNodeId"),
        "embedConnectPath 必须按 legacyPortalNodeId 分流，实际窗口:\n{helper}"
    );
    assert!(
        helper.contains("\"enterPortal\""),
        "embedConnectPath 必须对 legacy portal 构造 enterPortal 段，实际窗口:\n{helper}"
    );
    assert!(
        helper.contains("\"enterEmbed\""),
        "embedConnectPath 必须对正式 Embed 构造 enterEmbed 段，实际窗口:\n{helper}"
    );
}

/// 3. drillDownRequested 信号携带三个参数（starmapId, title, segment）。
#[test]
fn canvas_drill_down_signal_carries_segment() {
    let src = strip_line_comments(&read_src(CANVAS));
    assert!(
        src.contains("signal drillDownRequested(string starmapId, string title, var segment)"),
        "drillDownRequested 必须是三参数信号（含 segment），实际源码缺少该声明"
    );
}

/// 4. Canvas 双击 Embed 调用 graphController.embedDrillSegment，Controller 按 legacyPortalNodeId 分流。
#[test]
fn canvas_embed_double_click_calls_controller_drill_segment() {
    let canvas_src = strip_line_comments(&read_src(CANVAS));
    let embed_double_click =
        function_window(&canvas_src, "onDoubleClicked: function(tgtStarmapId) {", 600);
    assert!(
        embed_double_click.contains("graphController.embedDrillSegment(ed.instanceId)"),
        "双击 Embed 必须调用 graphController.embedDrillSegment，实际窗口:\n{embed_double_click}"
    );
    assert!(
        !embed_double_click.contains("legacyPortalNodeId"),
        "Canvas 双击不得再直接访问 legacyPortalNodeId，实际窗口:\n{embed_double_click}"
    );
    assert!(
        embed_double_click.contains("drillDownRequested(tgtStarmapId, ed.label || qsTr(\"未命名\"), segment)"),
        "双击必须把 segment 传给 drillDownRequested 第三参数，实际窗口:\n{embed_double_click}"
    );

    // Controller embedDrillSegment 按 legacyPortalNodeId 分流
    let ctrl_src = strip_line_comments(&read_src(GRAPH_CONTROLLER));
    let drill_helper = slice_between(&ctrl_src, "function embedDrillSegment(instanceId)", "function findEmbedAt(");
    assert!(
        drill_helper.contains("\"enterPortal\"") && drill_helper.contains("\"enterEmbed\""),
        "Controller embedDrillSegment 必须包含 enterPortal/enterEmbed 分流，实际窗口:\n{drill_helper}"
    );
}

/// 5. Workspace 声明 currentPathSegments property。
#[test]
fn workspace_declares_current_path_segments() {
    let src = strip_line_comments(&read_src(WORKSPACE));
    assert!(
        src.contains("property var currentPathSegments: []"),
        "Workspace 必须声明 currentPathSegments property（初始空数组），实际源码缺少"
    );
}

/// 6. enterChildStarmap 接收 segment 并 append 到 currentPathSegments。
#[test]
fn workspace_enter_child_starmap_appends_segment() {
    let src = strip_line_comments(&read_src(WORKSPACE));
    let func = slice_between(&src, "function enterChildStarmap(", "function returnToParentStarmap(");
    assert!(
        func.contains("segment"),
        "enterChildStarmap 必须接收 segment 参数，实际窗口:\n{func}"
    );
    assert!(
        func.contains("currentPathSegments.concat([segment])"),
        "enterChildStarmap 必须 append segment 到 currentPathSegments，实际窗口:\n{func}"
    );
    assert!(
        func.contains("segment: segment"),
        "栈元素必须保存 segment 字段，实际窗口:\n{func}"
    );
}

/// 7. returnToParentStarmap pop currentPathSegments。
#[test]
fn workspace_return_to_parent_pops_segment() {
    let src = strip_line_comments(&read_src(WORKSPACE));
    // returnToParentStarmap 是文件最后一个 function，用 function_window 最可靠。
    let func = function_window(&src, "function returnToParentStarmap(", 600);
    assert!(
        func.contains("currentPathSegments.slice(0, currentPathSegments.length - 1)"),
        "returnToParentStarmap 必须 pop currentPathSegments，实际窗口:\n{func}"
    );
}

/// 8. onStarmapIdChanged 重置 currentPathSegments 为空。
#[test]
fn workspace_on_starmap_id_changed_resets_path_segments() {
    let src = strip_line_comments(&read_src(WORKSPACE));
    let handler = function_window(&src, "onStarmapIdChanged: {", 200);
    assert!(
        handler.contains("currentPathSegments = []"),
        "onStarmapIdChanged 必须重置 currentPathSegments 为空，实际窗口:\n{handler}"
    );
}

/// 9. onDrillDownRequested handler 把 segment 传给 enterChildStarmap。
#[test]
fn workspace_drill_down_handler_forwards_segment() {
    let src = strip_line_comments(&read_src(WORKSPACE));
    let handler = function_window(&src, "onDrillDownRequested: function(smId, smTitle", 200);
    assert!(
        handler.contains("segment"),
        "onDrillDownRequested handler 必须接收 segment，实际窗口:\n{handler}"
    );
    assert!(
        handler.contains("enterChildStarmap(smId, smTitle, segment)"),
        "handler 必须把 segment 传给 enterChildStarmap，实际窗口:\n{handler}"
    );
}
