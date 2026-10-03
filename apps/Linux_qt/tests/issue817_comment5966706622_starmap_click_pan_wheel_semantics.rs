//! Issue #817 评论 5966706622 → 5967085589 → 5967339186 逐轮复核收窄后的
//! 星图输入归属守卫。
//!
//! 5966706622 那轮新增的源码字符串守卫把几条"当前实现细节"错误地锁成了
//! "永久架构规则"——全文件禁止 `exclusiveSignals`、规定 Canvas 永远只能有
//! 一个 WheelHandler、强制 `pixelDelta`。5967085589 复核要求收窄回真正有
//! 价值的三组不变量，并复用 `common/source_guard.rs` 的 helper，不在测试里
//! 自造简易 QML 解析器。5967339186 复核再收两点：
//! - Embed 的选中守卫从全文件扫描收窄到只检查负责 `root.clicked(instanceId)`
//!   的 `TapHandler {` 块，避免误伤标题/边框的右键菜单 TapHandler；
//! - 加回 `childScenePathKey` 必须真正传给子 Scene `pathKey` 的接线守卫，
//!   防止子 Scene 回退到默认 `pathKey: "root"` 而让滚轮在子 Scene 重新启用。
//!
//! 收窄后保留的守卫：
//! 1. Node/Embed 的选中 TapHandler 不得重新声明
//!    `exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap`。
//!    Qt 官方文档：默认 `NotExclusive` 立即发 single/double；`SingleTap | DoubleTap`
//!    会把两个信号都推迟到双击时间窗之后，3 连点以上两个信号都不发。只锁这个
//!    导致单击延迟的组合，不禁止 `exclusiveSignals` 的其他合法用法，也不全文件
//!    禁止 `exclusiveSignals`。Embed 守卫只围绕真正的选中回调
//!    `onSingleTapped: root.clicked(root.instanceId)` 收窄，不误伤右键菜单 handler。
//! 2. `bgDragArea` 命中 childContent 时不进入 pan（`mouse.accepted = false` 放行），
//!    pan 超过系统 dragThreshold 才开始，`onCanceled` / `resetInteraction` 会清
//!    `pressHitKind` / `panStarted` 等本地状态。
//! 3. `sceneWheel` 只在根 Scene 启用（`pathKey === "root"`）并 `blocking`，不按
//!    childContent 分发、不用 `event.accepted` 二次分流；`bgDragArea` 不再自己
//!    处理 wheel。不限制 Canvas 里 WheelHandler 的总数——以后新增不同用途的
//!    WheelHandler 不应无条件炸掉本守卫。根 Scene 唯一滚轮能够成立还依赖
//!    `StarMapEmbed` 把 `childScenePathKey` 真正传给子 Scene 的 `pathKey`，
//!    否则子 Scene 会回退到默认 `"root"` 而重新启用滚轮。
//!
//! QML 组件依赖 qml_resources qrc 与 Rust 注册的上下文属性，无法在本仓库的
//! Rust 测试里实例化，因此按仓库既有惯例（issue801/issue814 系列 WHITE_BOX
//! 测试）读取 QML 源码，确定性断言这次修复的接线不变量，防止回退。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

const CANVAS: &str = "qml/StarMapCanvas.qml";
const NODE: &str = "qml/StarMapNode.qml";
const EMBED: &str = "qml/StarMapEmbed.qml";
const CONTENT: &str = "qml/StarMapSceneContent.qml";

/// 去掉整行 `//` 注释，只留可执行语句。
/// 守卫断言"代码不再依赖某模式"，注释里作为历史说明提到该模式不算违规。
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

// ─────────────────────────────────────────────────────────────────────────
// 1. 选中 TapHandler 不得重新声明 SingleTap | DoubleTap
// ─────────────────────────────────────────────────────────────────────────

/// `StarMapNode.qml` 的 `nodeMouseTap` / `nodeTouchTap` 是 #817 的单击选中入口，
/// 不得重新声明 `exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap`。
///
/// Qt 官方文档（https://doc.qt.io/qt-6/qml-qtquick-taphandler.html#exclusiveSignals-prop）：
/// 默认 `NotExclusive` 立即发 singleTapped/doubleTapped；`SingleTap | DoubleTap`
/// 会把两个信号都推迟到双击时间窗之后，时间窗内连点 3 次及以上两个信号都不发
/// （诊断包里"十几次 pointer_press 才偶尔一次 selection_changed"正是这个行为）。
///
/// 只锁这个导致单击延迟的组合，不禁止 `exclusiveSignals` 的其他合法值，也不全文件
/// 禁止 `exclusiveSignals`——Node 的右键 TapHandler 等非选中 handler 不在本守卫范围。
#[test]
fn node_selection_tap_handlers_not_readd_single_tap_double_tap() {
    let node = read_src(NODE);
    for (marker, window) in [("id: nodeMouseTap", 300usize), ("id: nodeTouchTap", 250usize)] {
        let block = function_window(&node, marker, window);
        assert!(
            !block.contains("TapHandler.SingleTap | TapHandler.DoubleTap"),
            "{marker} 不得重新声明 exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap：\
             该组合会让 singleTapped 推迟到双击时间窗之后，单击选中不再立即响应，实际窗口:\n{block}"
        );
    }
}

/// `StarMapEmbed.qml` 负责标题/边框选中的 TapHandler 不得重新声明
/// `exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap`。
///
/// Embed 的选中 TapHandler 一次点击立即 `root.clicked(instanceId)`。只锁这些
/// 选中 handler 里导致单击延迟的 `SingleTap | DoubleTap` 组合，不禁止
/// `exclusiveSignals` 的其他合法值，也不全文件禁止——标题/四条边框的右键菜单
/// TapHandler（`root.rightClicked` / `root.contextMenuRequested`）不是 #817 的
/// 选中入口，以后若合法使用 `exclusiveSignals` 不应被本守卫误报。
///
/// 收窄方式：遍历每个 `onSingleTapped: root.clicked(root.instanceId)` 选中回调，
/// 回溯到所属 `TapHandler {` 块，只检查这些块里是否出现 `SingleTap | DoubleTap`。
/// 不做 QML 解析，只围绕真正的选中回调收窄。
#[test]
fn embed_chrome_selection_tap_handlers_not_readd_single_tap_double_tap() {
    let embed = read_src(EMBED);
    let marker = "onSingleTapped: root.clicked(root.instanceId)";
    let mut start = 0usize;
    let mut found = 0usize;

    while let Some(rel) = embed[start..].find(marker) {
        let pos = start + rel;
        let before = &embed[..pos];

        let handler_start = before
            .rfind("TapHandler {")
            .expect("选中回调前必须存在 TapHandler");

        let block = &embed[handler_start..pos + marker.len()];

        assert!(
            !block.contains("TapHandler.SingleTap | TapHandler.DoubleTap"),
            "负责 root.clicked(instanceId) 的选中 TapHandler 不得恢复\
             exclusiveSignals: TapHandler.SingleTap | TapHandler.DoubleTap：\
             恢复默认 NotExclusive 才能一次点击立即 root.clicked(instanceId)，实际块:\n{block}"
        );

        found += 1;
        start = pos + marker.len();
    }

    // 标题(鼠标+触屏=2) + 四条边框(4×2=8) = 10 个选中回调。
    assert!(
        found >= 10,
        "必须至少找到 10 个 root.clicked(instanceId) 选中回调\
         （标题 + 四条边框 × 鼠标/触屏），实际找到 {found} 个"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 2. bgDragArea 命中 childContent 不进 pan、pan 超阈值才开始、cancel/reset 清状态
// ─────────────────────────────────────────────────────────────────────────

/// `bgDragArea` 在按下时按 `hitPointerAtScreen` 固定手势归属：命中
/// node/embedChrome/childContent 时 `mouse.accepted = false` 放行给对象/子 Scene，
/// 不进入 pan；只有 empty 归属才在左键移动超过系统 dragThreshold 后 beginPan。
/// `onCanceled` 与 `resetInteraction` 都清 `pressHitKind` / `panStarted` 等本地状态，
/// 避免脏状态继续拖动画布。
#[test]
fn bg_drag_area_pan_gating_and_local_state_reset() {
    let src = read_src(CANVAS);
    let bg = function_window(&src, "id: bgDragArea", 6000);

    // 命中 childContent 时不进入 pan：放行给子 Scene。
    assert!(
        bg.contains("hit.kind === \"childContent\"") && bg.contains("mouse.accepted = false"),
        "bgDragArea 命中 childContent 时必须 mouse.accepted = false 放行，不进入 pan，实际窗口:\n{bg}"
    );

    // pan 超过系统 dragThreshold 才开始：不得自写像素常量。
    assert!(
        bg.contains("if (!panStarted && (mouse.buttons & Qt.LeftButton))"),
        "左键必须先卡在尚未 panStarted 的分支里等拖动阈值，实际窗口:\n{bg}"
    );
    assert!(
        bg.contains("bgMouseLeftTap.dragThreshold") && bg.contains("Math.hypot"),
        "拖动阈值必须复用 bgMouseLeftTap 的系统 dragThreshold 并按直线距离判定，实际窗口:\n{bg}"
    );

    // cancel/reset 清本地 pan 状态。
    assert!(
        bg.contains("onCanceled:") && bg.contains("resetMouseGesture"),
        "onCanceled 必须调用 resetMouseGesture 清本地状态，实际窗口:\n{bg}"
    );
    assert!(
        bg.contains("pressHitKind = \"\"") && bg.contains("panStarted = false"),
        "resetMouseGesture 必须清 pressHitKind 与 panStarted，实际窗口:\n{bg}"
    );

    // canvasArea.resetInteraction 也清 bgDragArea 本地状态。
    let reset = function_window(&src, "function resetInteraction() {", 600);
    assert!(
        reset.contains("bgDragArea.resetMouseGesture()"),
        "resetInteraction 必须同时清 bgDragArea 本地 pan 状态，实际窗口:\n{reset}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. sceneWheel 只在 root 启用、bgDragArea 不自己处理 wheel
// ─────────────────────────────────────────────────────────────────────────

/// Issue #822：整棵星图只有一个全局相机，`sceneWheel` 无条件接管滚轮并走
/// `zoomAround` 统一缩放入口 + `blocking: true`，不再按场景身份开关、
/// 不按 childContent 分发到"最深子 Scene"，也不用 `event.accepted` 做二次分流。
/// `bgDragArea` 不再自己处理 wheel。
///
/// 不限制 `StarMapCanvas.qml` 里 WheelHandler 的总数——以后新增完全不同用途的
/// WheelHandler 不应无条件炸掉本守卫；只检查 `sceneWheel` 自身与 `bgDragArea`。
#[test]
fn scene_wheel_only_root_and_bg_drag_area_has_no_wheel() {
    // 注释里会提到被删掉的 `enabled: pathKey === "root"` 作为历史说明，
    // 断言的是可执行语句，所以先剥掉整行注释。
    let src = strip_line_comments(&read_src(CANVAS));
    let wheel = function_window(&src, "id: sceneWheel", 1000);

    // Issue #822：全树只剩一个全局视口，不再有子 Scene/子 Canvas，
    // 因此 sceneWheel 不再需要按场景身份开关——鼠标停在任何一层节点或子星图上，
    // 滚轮都调同一个 zoomAround，只改根 zoomLevel/panX/panY。
    assert!(
        !wheel.contains("enabled:"),
        "sceneWheel 不得再按场景身份开关（只有根 Canvas 有相机），实际窗口:\n{wheel}"
    );
    assert!(
        wheel.contains("zoomAround("),
        "sceneWheel 必须走根 Canvas 的 zoomAround 统一缩放入口，实际窗口:\n{wheel}"
    );
    assert!(
        wheel.contains("blocking: true"),
        "sceneWheel 处理后必须 blocking，实际窗口:\n{wheel}"
    );
    assert!(
        !wheel.contains("childContent"),
        "sceneWheel 不得再按 childContent 分发到最深子 Scene，实际窗口:\n{wheel}"
    );
    assert!(
        !wheel.contains("event.accepted"),
        "sceneWheel 不得再用 event.accepted 做二次分流（blocking 决定阻塞语义），实际窗口:\n{wheel}"
    );

    // 用下一个 handler 作右边界，避免固定长度窗口越界吃到 sceneWheel 的 onWheel。
    let bg = {
        let start = src.find("id: bgDragArea").unwrap_or_else(|| panic!("missing bgDragArea"));
        let end = src[start..]
            .find("id: sceneWheel")
            .map(|i| start + i)
            .unwrap_or_else(|| panic!("missing sceneWheel after bgDragArea"));
        &src[start..end]
    };
    assert!(
        !bg.contains("onWheel"),
        "bgDragArea 不得再自己处理 wheel（滚轮已移到 sceneWheel），实际窗口:\n{bg}"
    );
}

/// Issue #822：根层的 `scenePathKey` 由根 Canvas 显式写死 `"root"`，
/// 子层的 `scenePathKey` 由父 pathKey + `"/embed_" + instanceId` 拼成并在创建时一次给全。
///
/// `StarMapSceneContent.scenePathKey` 是 required property 且**没有** `"root"` 默认值，
/// 因此不存在"子层先冒充根层再修正"的中间态——这正是诊断包里
/// `depth=1 + pathKey=root` 的根因。若有人给 scenePathKey 加回默认值，这条守卫立刻失败。
#[test]
fn root_scene_path_key_is_root_and_children_are_not() {
    let canvas = read_src(CANVAS);
    assert!(
        canvas.contains("scenePathKey: \"root\""),
        "根 StarMapSceneContent 必须由 StarMapCanvas 显式传 scenePathKey: \"root\""
    );

    let content = read_src(CONTENT);
    assert!(
        content.contains("required property string scenePathKey"),
        "StarMapSceneContent 的 scenePathKey 必须是 required property"
    );
    assert!(
        !content.contains("property string scenePathKey: \"root\""),
        "StarMapSceneContent 的 scenePathKey 不得带 \"root\" 默认值，否则子层会冒充根层"
    );

    let embed = read_src(EMBED);
    assert!(
        embed.contains("parentPathKey + \"/embed_\" + instanceId"),
        "子 Content 的 scenePathKey 必须由父 pathKey + \"/embed_\" + instanceId 拼成，永远不等于 \"root\""
    );
    assert!(
        embed.contains("\"scenePathKey\": childContentPathKey"),
        "StarMapEmbed 必须在创建子 Content 时一次给全 scenePathKey，子层不得回退到 root"
    );
}
