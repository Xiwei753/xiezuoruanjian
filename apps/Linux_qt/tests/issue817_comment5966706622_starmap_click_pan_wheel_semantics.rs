//! Issue #817 评论 5966706622 — 星图单击/空白拖动/滚轮缩放的输入归属守卫。
//!
//! 评论 5949494799 定下输入归属重做的三条主线，5953678540 复核后修正为：
//! 1. 单击选中必须立即响应：`StarMapNode.qml` / `StarMapEmbed.qml` 的 TapHandler
//!    不得再声明 `exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap`。
//!    Qt 6.5+ 对该组合的语义是把 singleTapped/doubleTapped 都推迟到双击时间窗之后，
//!    时间窗内连续点 3 次及以上两个信号都不发（诊断包里“十几次 pointer_press 才
//!    偶尔一次 selection_changed”正是这个行为）；恢复默认 `NotExclusive` 后
//!    单击在 release 当场发信号。
//! 2. 空白点击与空白拖动分开，且 pan 本地状态必须闭环：bgDragArea 在按下时固定
//!    本次鼠标手势归属（node/embedChrome/childContent 一律 `mouse.accepted = false`
//!    放行给对象/子 Scene），只有 empty 命中才在移动超过系统 dragThreshold 后进入
//!    pan；pressHitKind/panStarted 必须统一 reset（onCanceled +
//!    canvasArea.resetInteraction），实际位移必须同时确认
//!    `interaction.pointerMode === "pan"`，中键必须先检查 `beginPan()` 返回值。
//!    空白单击仍由 `bgMouseLeftTap` 立即清共享 selection。
//! 3. 滚轮缩放只由根 Scene 唯一处理：`StarMapCanvas.qml` 只能有一个 WheelHandler，
//!    `enabled: pathKey === "root"` + `blocking: true`，不得再按 hit.kind 分发到最深
//!    child Scene，也不得再有 `event.accepted` 二次分流；声明支持 TouchPad 时
//!    必须读 pixelDelta（平滑滚动只有 pixelDelta，angleDelta 为 0）。
//!    根 Scene 由 `StarMapWorkspace.qml` 显式传 `pathKey: "root"`，子 Scene 的
//!    pathKey 由 `StarMapEmbed.childScenePathKey` 拼成，永远不等于 "root"。
//!
//! QML 组件依赖 qml_resources qrc 与 Rust 注册的上下文属性，无法在本仓库的
//! Rust 测试里实例化，因此按仓库既有惯例（issue801/issue814 系列 WHITE_BOX
//! 测试）读取 QML 源码，确定性断言这次修复的接线不变量，防止回退。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{count_occurrences, function_window, read_src};

const CANVAS: &str = "qml/StarMapCanvas.qml";
const NODE: &str = "qml/StarMapNode.qml";
const EMBED: &str = "qml/StarMapEmbed.qml";
const WORKSPACE: &str = "qml/StarMapWorkspace.qml";

/// 去掉整行 `//` 注释，只留可执行语句。
///
/// 守卫断言的是“代码不得再依赖某模式”，注释里说明历史原因提到该模式不算违规
/// （例如 Canvas 注释里解释“滚轮已移到独立 WheelHandler”）。
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

/// 收集源码中所有 `decl {` 声明的完整块（按花括号深度匹配到对应 `}`）。
fn declaration_blocks(src: &str, decl: &str) -> Vec<String> {
    let needle = format!("{decl} {{");
    let mut blocks = Vec::new();
    let mut rest = src;
    while let Some(idx) = rest.find(&needle) {
        let body_start = idx + needle.len();
        let mut depth = 1usize;
        let mut end = body_start;
        for (i, c) in rest[body_start..].char_indices() {
            if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    end = body_start + i + 1;
                    break;
                }
            }
        }
        blocks.push(rest[idx..end].to_string());
        rest = &rest[end..];
    }
    blocks
}

/// 取 `block` 中 `if (!interaction.beginPan())` 之后紧邻的非空语句行（去缩进）。
///
/// 用于断言 beginPan 失败时真的 `return`，而不是只调用不检查返回值。
fn line_after(block: &str, marker: &str) -> String {
    let pos = block
        .find(marker)
        .unwrap_or_else(|| panic!("missing marker `{marker}`"));
    block[pos..]
        .lines()
        .skip(1)
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string()
}

// ─────────────────────────────────────────────────────────────────────────
// 1. 单击立即选中：TapHandler 恢复默认 NotExclusive
// ─────────────────────────────────────────────────────────────────────────

/// Node / Embed / Canvas 三个星图组件都不得再声明 exclusiveSignals。
///
/// `SingleTap | DoubleTap` 是 Qt 官方文档里唯一会让两个信号互相等待、
/// 并在 3 连点以上全部吞掉的组合；默认 NotExclusive 才是“单击当场选中”。
#[test]
fn star_map_tap_handlers_never_declare_exclusive_signals() {
    for rel in [NODE, EMBED, CANVAS] {
        let src = strip_line_comments(&read_src(rel));
        assert!(
            !src.contains("exclusiveSignals"),
            "{rel} 不得再声明 exclusiveSignals：恢复默认 NotExclusive 才能单击立即发 singleTapped"
        );
    }
}

/// Node 的鼠标/触屏 TapHandler 仍然完整接线 single/double/long press。
#[test]
fn node_tap_handlers_emit_single_click_immediately() {
    let src = strip_line_comments(&read_src(NODE));

    let mouse = function_window(&src, "id: nodeMouseTap", 700);
    assert!(
        mouse.contains("onSingleTapped: root.singleClicked()"),
        "nodeMouseTap 必须单击立即上抛 singleClicked，实际窗口:\n{mouse}"
    );
    assert!(
        mouse.contains("onDoubleTapped: root.doubleClicked()"),
        "nodeMouseTap 必须保留双击编辑，实际窗口:\n{mouse}"
    );
    assert!(
        mouse.contains("onLongPressed: root.mouseLongPressed()"),
        "nodeMouseTap 必须保留鼠标长按进 connect，实际窗口:\n{mouse}"
    );

    let touch = function_window(&src, "id: nodeTouchTap", 500);
    assert!(
        touch.contains("onSingleTapped: root.singleClicked()"),
        "nodeTouchTap 必须单击立即上抛 singleClicked，实际窗口:\n{touch}"
    );
    assert!(
        touch.contains("onLongPressed: root.touchLongPressed()"),
        "nodeTouchTap 必须保留触屏长按进 contextPending，实际窗口:\n{touch}"
    );
}

/// Embed 标题条 + 四条边框的每个鼠标/触屏 chrome handler 都统一
/// “一次点击立即 root.clicked(instanceId)”，不存在两套选择时序。
#[test]
fn embed_chrome_handlers_select_on_first_click() {
    let src = strip_line_comments(&read_src(EMBED));
    let immediate = count_occurrences(&src, "onSingleTapped: root.clicked(root.instanceId)");
    assert!(
        immediate >= 10,
        "title + 四条边框 × 鼠标/触屏共 10 处 chrome handler 必须一次点击立即 root.clicked(instanceId)，实际 {immediate} 处"
    );
}

/// Canvas 的 Node / Embed delegate 把首次点击直接路由到选中。
#[test]
fn canvas_delegates_route_first_click_to_selection() {
    let src = strip_line_comments(&read_src(CANVAS));

    let node_click = function_window(&src, "onSingleClicked: {", 400);
    assert!(
        node_click.contains("graphController.selectNode(nodeData.id)"),
        "Node delegate 单击必须当场 selectNode，实际窗口:\n{node_click}"
    );

    let embed_click = function_window(&src, "onClicked: function(instId) {", 400);
    assert!(
        embed_click.contains("graphController.selectEmbed(instId)"),
        "Embed delegate 单击必须当场 selectEmbed，实际窗口:\n{embed_click}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 2. 空白点击清选中 + 空白拖动按阈值转 pan
// ─────────────────────────────────────────────────────────────────────────

/// 空白单击仍由 bgMouseLeftTap 清共享 selection，且不吞对象/子场景命中。
#[test]
fn blank_click_still_clears_selection() {
    let src = strip_line_comments(&read_src(CANVAS));
    let bg_tap = function_window(&src, "id: bgMouseLeftTap", 2000);

    assert!(
        bg_tap.contains("clearSelection()"),
        "空白单击必须调用 clearSelection() 清共享选中，实际窗口:\n{bg_tap}"
    );
    assert!(
        bg_tap.contains("findNodeAt(mx, my)")
            && bg_tap.contains("findEmbedChromeAt(mx, my)")
            && bg_tap.contains("findEmbedContentAt(mx, my)"),
        "bgMouseLeftTap 必须先按 Node/Embed chrome/child content 命中提前返回，实际窗口:\n{bg_tap}"
    );
    assert!(
        bg_tap.contains("graphController.hitTestEdge(mx, my)")
            && bg_tap.contains("graphController.selectEdge(clickedEdge.id)"),
        "边命中时空白点击必须选中边而不是清空，实际窗口:\n{bg_tap}"
    );
}

/// hitPointerAtScreen 是 press-time 归属的唯一命中入口，childContent 有独立 kind。
#[test]
fn hit_helper_is_single_source_of_truth() {
    let src = strip_line_comments(&read_src(CANVAS));

    let hit_fn = function_window(&src, "function hitPointerAtScreen(", 900);
    for kind in ["node", "embedChrome", "childContent", "edge", "empty"] {
        assert!(
            hit_fn.contains(&format!("kind: \"{kind}\"")),
            "hitPointerAtScreen 必须返回 {kind} 命中种类，实际窗口:\n{hit_fn}"
        );
    }

    let press_log = function_window(&src, "function logPointerPress(", 700);
    assert!(
        press_log.contains("hitPointerAtScreen(point.position.x, point.position.y)"),
        "logPointerPress 必须复用 hitPointerAtScreen，实际窗口:\n{press_log}"
    );
}

/// bgDragArea 在按下时固定手势归属，左键按下不直接 beginPan。
#[test]
fn bg_drag_area_records_press_time_ownership() {
    let src = strip_line_comments(&read_src(CANVAS));
    let drag_area = declaration_blocks(&src, "MouseArea")
        .into_iter()
        .find(|block| block.contains("id: bgDragArea"))
        .expect("StarMapCanvas 必须有 bgDragArea MouseArea");

    assert!(
        drag_area.contains("property string pressHitKind: \"\""),
        "bgDragArea 必须有 pressHitKind press-time 归属状态"
    );
    assert!(
        drag_area.contains("property bool panStarted: false"),
        "bgDragArea 必须有 panStarted 本地状态"
    );

    // 左键分支：命中对象/子场景必须放行，且不得当场 beginPan。
    let left = slice_between(
        &drag_area,
        "if (mouse.button === Qt.LeftButton) {",
        "if (mouse.button === Qt.MiddleButton) {",
    );
    assert!(
        left.contains("hit.kind === \"node\"")
            && left.contains("hit.kind === \"embedChrome\"")
            && left.contains("hit.kind === \"childContent\""),
        "左键按下必须先判 node/embedChrome/childContent 归属，实际窗口:\n{left}"
    );
    assert!(
        left.contains("mouse.accepted = false"),
        "命中对象/子场景时必须 mouse.accepted = false 让事件穿透，实际窗口:\n{left}"
    );
    assert!(
        !left.contains("beginPan("),
        "左键按下不得直接 beginPan：必须等移动超过 dragThreshold，实际窗口:\n{left}"
    );
}

/// 中键分支必须先检查 beginPan() 返回值，失败直接 return。
#[test]
fn middle_press_requires_successful_begin_pan() {
    let src = strip_line_comments(&read_src(CANVAS));
    let drag_area = declaration_blocks(&src, "MouseArea")
        .into_iter()
        .find(|block| block.contains("id: bgDragArea"))
        .expect("StarMapCanvas 必须有 bgDragArea MouseArea");

    let middle = slice_between(
        &drag_area,
        "if (mouse.button === Qt.MiddleButton) {",
        "onPositionChanged: function(mouse) {",
    );
    assert!(
        middle.contains("if (!interaction.beginPan())"),
        "中键必须先检查 beginPan() 返回值，实际窗口:\n{middle}"
    );
    assert_eq!(
        line_after(&middle, "if (!interaction.beginPan())"),
        "return",
        "beginPan() 失败（move/connect 等非 idle 状态）必须直接 return，不得制造 panStarted=true / pointerMode!=pan 的矛盾状态"
    );
    assert!(
        middle.contains("panStarted = true"),
        "只有 beginPan() 成功后才允许设置 panStarted=true，实际窗口:\n{middle}"
    );
}

/// 左键移动超过 dragThreshold 才转 pan；实际位移必须同时确认 pointerMode。
#[test]
fn pan_position_updates_require_threshold_and_pan_mode() {
    let src = strip_line_comments(&read_src(CANVAS));
    let drag_area = declaration_blocks(&src, "MouseArea")
        .into_iter()
        .find(|block| block.contains("id: bgDragArea"))
        .expect("StarMapCanvas 必须有 bgDragArea MouseArea");

    let pos_changed = slice_between(
        &drag_area,
        "onPositionChanged: function(mouse) {",
        "onReleased: function(mouse) {",
    );
    assert!(
        pos_changed.contains("if (pressHitKind !== \"empty\")"),
        "非 empty 归属（edge 等）不得拖动画布，实际窗口:\n{pos_changed}"
    );
    assert!(
        pos_changed.contains("if (!panStarted && (mouse.buttons & Qt.LeftButton))"),
        "左键必须先卡在“尚未 panStarted”分支里等拖动阈值，实际窗口:\n{pos_changed}"
    );
    assert!(
        pos_changed.contains("bgMouseLeftTap.dragThreshold"),
        "拖动阈值必须复用背景 TapHandler 的系统 dragThreshold，不得自写像素常量，实际窗口:\n{pos_changed}"
    );
    assert!(
        pos_changed.contains("Math.hypot(dx0, dy0)"),
        "拖动阈值必须按直线距离判定，实际窗口:\n{pos_changed}"
    );
    assert_eq!(
        line_after(&pos_changed, "if (!interaction.beginPan())"),
        "return",
        "超过阈值后 beginPan() 失败（非 idle）必须直接 return"
    );
    assert!(
        pos_changed.contains("if (panStarted && interaction.pointerMode === \"pan\")"),
        "实际位移必须同时确认 panStarted 且 pointerMode === \"pan\"，防止 reset/cancel 后本地脏状态继续拖动画布，实际窗口:\n{pos_changed}"
    );

    let released = slice_between(&drag_area, "onReleased: function(mouse) {", "onCanceled:");
    assert!(
        released.contains("if (panStarted) {") && released.contains("interaction.endPan()"),
        "onReleased 只应在 panStarted 时结束 pan，未超过阈值交给 bgMouseLeftTap，实际窗口:\n{released}"
    );
}

/// pan 本地状态有统一 reset 链：onCanceled + canvasArea.resetInteraction。
#[test]
fn pan_local_state_has_reset_chain() {
    let src = strip_line_comments(&read_src(CANVAS));
    let drag_area = declaration_blocks(&src, "MouseArea")
        .into_iter()
        .find(|block| block.contains("id: bgDragArea"))
        .expect("StarMapCanvas 必须有 bgDragArea MouseArea");

    let reset_fn = slice_between(&drag_area, "function resetMouseGesture() {", "onPressed:");
    for cleared in [
        "pressHitKind = \"\"",
        "pressX = 0",
        "pressY = 0",
        "lastX = 0",
        "lastY = 0",
        "panStarted = false",
    ] {
        assert!(
            reset_fn.contains(cleared),
            "resetMouseGesture 必须清 `{cleared}`，实际窗口:\n{reset_fn}"
        );
    }

    let canceled = function_window(&drag_area, "onCanceled:", 200);
    assert!(
        canceled.contains("if (interaction.pointerMode === \"pan\")")
            && canceled.contains("interaction.endPan()")
            && canceled.contains("resetMouseGesture()"),
        "onCanceled 必须结束 pan 并清本地状态，实际窗口:\n{canceled}"
    );

    let reset_interaction = function_window(&src, "function resetInteraction() {", 700);
    assert!(
        reset_interaction.contains("interaction.reset()")
            && reset_interaction.contains("bgDragArea.resetMouseGesture()"),
        "canvasArea.resetInteraction 必须同时清交互状态机与 bgDragArea 本地 pan 状态，实际窗口:\n{reset_interaction}"
    );

    // 切图 / 隐藏两条触发路径必须仍然调用 resetInteraction()。
    let starmap_changed = function_window(&src, "onStarmapIdChanged:", 200);
    assert!(
        starmap_changed.contains("resetInteraction()"),
        "切图（onStarmapIdChanged）必须触发 resetInteraction，实际窗口:\n{starmap_changed}"
    );
    assert!(
        src.contains("onVisibleChanged: { if (!visible) resetInteraction() }"),
        "窗口隐藏（onVisibleChanged）必须触发 resetInteraction"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. 滚轮缩放只由根 Scene 唯一处理
// ─────────────────────────────────────────────────────────────────────────

/// StarMapCanvas 只能有一个 WheelHandler，且就是根 Scene 唯一入口。
#[test]
fn wheel_has_single_root_scene_entry() {
    let src = strip_line_comments(&read_src(CANVAS));

    let wheels = declaration_blocks(&src, "WheelHandler");
    assert_eq!(
        wheels.len(),
        1,
        "StarMapCanvas.qml 只能有一个 WheelHandler（根 Scene 唯一滚轮入口），实际 {} 个",
        wheels.len()
    );
    assert_eq!(
        count_occurrences(&src, "onWheel"),
        1,
        "StarMapCanvas.qml 只能有一处 onWheel（bgDragArea 的第二条滚轮入口必须保持删除）"
    );

    let wheel = &wheels[0];
    assert!(
        wheel.contains("id: sceneWheel"),
        "唯一 WheelHandler 必须是 sceneWheel，实际:\n{wheel}"
    );
    assert!(
        wheel.contains("enabled: pathKey === \"root\""),
        "滚轮缩放必须只由根 Scene 处理（enabled: pathKey === \"root\"），实际:\n{wheel}"
    );
    assert!(
        wheel.contains("blocking: true"),
        "根 Scene 处理后必须 blocking，实际:\n{wheel}"
    );
    assert!(
        wheel.contains("acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad"),
        "滚轮必须覆盖鼠标与触控板，实际:\n{wheel}"
    );
    assert!(
        wheel.contains("event.angleDelta.y") && wheel.contains("event.pixelDelta.y"),
        "声明支持 TouchPad 时必须同时读 pixelDelta（平滑滚动只有 pixelDelta），实际:\n{wheel}"
    );
    assert!(
        wheel.contains("logInteraction(\"zoom_wheel\""),
        "滚轮缩放必须记录 zoom_wheel 边界日志，实际:\n{wheel}"
    );
    assert!(
        !wheel.contains("childContent"),
        "滚轮不得再按 childContent 分发到最深子 Scene，实际:\n{wheel}"
    );
    assert!(
        !wheel.contains("event.accepted"),
        "滚轮不得再用 event.accepted 做二次分流，blocking 决定阻塞语义，实际:\n{wheel}"
    );

    for rel in [NODE, EMBED] {
        let other = strip_line_comments(&read_src(rel));
        assert!(
            !other.contains("WheelHandler") && !other.contains("onWheel"),
            "{rel} 不得再出现滚轮入口"
        );
    }
}

/// 根 Scene 的 pathKey 由 Workspace 显式传 "root"，子 Scene 永远不是 "root"。
#[test]
fn root_scene_path_key_is_root_and_children_are_not() {
    let workspace = strip_line_comments(&read_src(WORKSPACE));
    assert!(
        workspace.contains("pathKey: \"root\""),
        "StarMapWorkspace 必须给根 StarMapScene 传 pathKey: \"root\""
    );

    let embed = strip_line_comments(&read_src(EMBED));
    assert!(
        embed.contains("parentPathKey + \"/embed_\" + instanceId"),
        "子 Scene pathKey 必须由父 pathKey + \"/embed_\" + instanceId 拼成，永远不等于 \"root\""
    );
    assert!(
        embed.contains("\"pathKey\": childScenePathKey"),
        "StarMapEmbed 必须把 childScenePathKey 传给子 Scene"
    );
}
