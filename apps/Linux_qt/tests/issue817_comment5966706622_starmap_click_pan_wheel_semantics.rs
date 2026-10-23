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
//! 2. `bgDragArea` 命中 node/embed 时放行给 delegate；childContent（未加载 /
//!    preview / shell）与 empty 一样进入全局 pan，超系统 dragThreshold 才开始，
//!    `onCanceled` / `resetInteraction` 会清 `pressHitKind` / `panStarted` 等本地状态。
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
//!
//! #929 更新：路由身份从字面量 "root" / 子层 pathKey 拼接改为 Core 导航路由的
//! `routeKey`（根 ID + 完整 segments）。活动画布由 StarMapCanvas 按 currentRoute
//! 解析后加载，父相机与进入上下文都按 routeKey 保存；`StarMapSceneContent` 只是
//! 只读微缩概览，不再参与路由身份。旧守卫
//! `root_scene_path_key_is_root_and_children_are_not` 随之改写为
//! `route_key_is_root_id_plus_full_segments`。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{count_occurrences, function_window, read_src, starmap_node_delegate_src};

const CANVAS: &str = "qml/StarMapCanvas.qml";
const EMBED: &str = "qml/StarMapEmbed.qml";
const ACTIVE_SCENE: &str = "qml/StarMapActiveScene.qml";
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

/// 普通节点卡片 delegate 的 `nodeMouseTap` / `nodeTouchTap` 是 #817 的单击选中入口，
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
    // Issue #832：Node 不再挂选中 TapHandler；单击选中由唯一 Router 的
    // TapHandler 完成，仍必须是默认 NotExclusive（立即发 singleTapped）。
    let node = strip_line_comments(&starmap_node_delegate_src());
    assert!(
        !node.contains("TapHandler"),
        "节点卡片 delegate 不得再挂 TapHandler：输入只有一个主人（Router）"
    );

    let router = read_src("qml/StarMapInputRouter.qml");
    assert!(
        !router.contains("TapHandler.SingleTap | TapHandler.DoubleTap"),
        "Router 的选中 TapHandler 不得声明 exclusiveSignals: SingleTap | DoubleTap：\
         该组合会让 singleTapped 推迟到双击时间窗之后，单击选中不再立即响应"
    );
    assert!(
        !router.contains("exclusiveSignals:"),
        "Router 的 TapHandler 保持默认 NotExclusive，不引入任何 exclusiveSignals"
    );
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
    // Issue #832：Embed 不再挂选中 TapHandler；单击选中由唯一 Router 完成，
    // 命中几何来自 GraphController 的圆壳命中测试，不靠 PointerHandler acceptance。
    let embed = strip_line_comments(&read_src(EMBED));
    for forbidden in ["TapHandler", "containmentMask", "chromeLayer"] {
        assert!(
            !embed.contains(forbidden),
            "StarMapEmbed 不得再保留 chrome 输入层 {forbidden}：输入只有一个主人（Router）"
        );
    }

    let router = read_src("qml/StarMapInputRouter.qml");
    assert!(
        !router.contains("TapHandler.SingleTap | TapHandler.DoubleTap")
            && !router.contains("exclusiveSignals:"),
        "Router 的选中 TapHandler 必须保持默认 NotExclusive，单击选中立即响应"
    );
    let select = function_window(&router, "function selectHit(", 1000);
    assert!(
        select.contains("hit.owner.selectEmbed(hit.id)"),
        "embed 单击必须由 Router 选中，实际窗口:\n{select}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 2. bgDragArea 命中 childContent 不进 pan、pan 超阈值才开始、cancel/reset 清状态
// ─────────────────────────────────────────────────────────────────────────

/// Issue #832：唯一 Router 在按下时按递归命中固定手势归属：
/// 空白/连线按下进入 pan 候选，超拖动阈值才 `beginPan()`；
/// node/embed 按下先 pressPending，拖动阈值先到才转 move，触屏未长按滑动优先 pan；
/// 小尺寸子图（shell/preview）整体命中成父层 embed，不留死区。
/// Router.resetInteraction/cancelLocalState 会清本地手势现场。
#[test]
fn router_pan_gating_and_local_state_reset() {
    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));

    let press = function_window(&router, "function beginPress(", 2800);
    assert!(
        press.contains("emptyPressActive = true") && press.contains("hasGestureTarget(hit)"),
        "空白/连线按下只登记 pan 候选，node/embed 按下登记 pressPending，实际窗口:\n{press}"
    );
    assert!(
        press.contains("ic.beginPress(hit.kind, hit.id, hit.targetPath, hit.scenePathKey,"),
        "node/embed 按下必须登记完整命中身份，实际窗口:\n{press}"
    );

    let activated = function_window(&router, "function handleDragActivated(", 2000);
    assert!(
        activated.contains("if (ic.beginPan())") && activated.contains("emptyPressActive"),
        "空白按下只有越过拖动阈值才 beginPan，实际窗口:\n{activated}"
    );
    assert!(
        activated.contains("if (ic.pressPendingToPan())"),
        "触屏未长按滑动必须优先 pan，实际窗口:\n{activated}"
    );

    // reset 会清 Router 本地手势现场；左右各自清理，避免误抹另一键的手势。
    let cancel = function_window(&router, "function cancelLocalState(", 400);
    assert!(
        cancel.contains("resetLeftLocalState()") && cancel.contains("resetRightLocalState()"),
        "cancelLocalState 必须分别清左右本地手势现场，实际窗口:\n{cancel}"
    );
    let left_cancel = function_window(&router, "function resetLeftLocalState(", 400);
    assert!(
        left_cancel.contains("emptyPressActive = false")
            && left_cancel.contains("emptyLongPressArmed = false")
            && left_cancel.contains("panActive = false"),
        "resetLeftLocalState 必须清本地手势现场，实际窗口:\n{left_cancel}"
    );
    let canvas = read_src(CANVAS);
    let reset = function_window(&canvas, "function resetInteraction() {", 600);
    assert!(
        reset.contains("inputRouter.cancelAllGestures(\"reset\")"),
        "resetInteraction 必须走 Router 的统一手势取消入口，实际窗口:\n{reset}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. sceneWheel 只在 root 启用、bgDragArea 不自己处理 wheel
// ─────────────────────────────────────────────────────────────────────────

/// Issue #832：整棵星图只有一个全局相机，唯一 Router 的 WheelHandler 无条件
/// 接管滚轮并走 Canvas 的 `zoomAt` 统一缩放入口 + 显式 `event.accepted`，
/// 不再按场景身份开关、不按 childContent 分发到"最深子 Scene"，
/// 也不用 `event.accepted` 做二次分流。Canvas 自己不再挂 WheelHandler。
#[test]
fn router_wheel_is_the_only_wheel_entry() {
    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        !canvas.contains("WheelHandler"),
        "StarMapCanvas 不得再挂 WheelHandler：滚轮统一走唯一 Router"
    );

    let router = strip_line_comments(&read_src("qml/StarMapInputRouter.qml"));
    let wheel = function_window(&router, "id: wheelHandler", 2400);
    assert!(
        !wheel.contains("enabled:"),
        "wheelHandler 不得再按场景身份开关（只有根 Canvas 有相机），实际窗口:\n{wheel}"
    );
    assert!(
        wheel.contains("canvas.zoomAt("),
        "wheelHandler 必须走根 Canvas 的 zoomAt 统一缩放入口，实际窗口:\n{wheel}"
    );
    assert!(
        wheel.contains("blocking: false")
            && wheel.contains("event.accepted = false")
            && wheel.contains("event.accepted = true"),
        "wheelHandler 不得阻塞编辑器滚动，并须显式声明 accepted，实际窗口:\n{wheel}"
    );
    assert!(
        !wheel.contains("childContent"),
        "wheelHandler 不得再按 childContent 分发到最深子 Scene，实际窗口:\n{wheel}"
    );
    assert!(
        !wheel.contains("event.accepted"),
        "wheelHandler 不得再用 event.accepted 做二次分流（blocking 决定阻塞语义），实际窗口:\n{wheel}"
    );
}

/// #929 取代 #822：不再有字面量 `scenePathKey: "root"`，也不再有子层
/// `parentPathKey + "/embed_" + instanceId` 拼接。路由身份 = `routeKey`
/// （根 ID + 完整 segments）：同一目标图的不同 Embed 入口路由不同、各自独立；
/// 活动画布身份与父相机 / 进入上下文都按 routeKey 保存。
#[test]
fn route_key_is_root_id_plus_full_segments() {
    let canvas = read_src(CANVAS);
    assert!(
        canvas.contains("readonly property string routeKey: routeKeyOf(currentRoute)"),
        "StarMapCanvas 必须以 routeKey 作为路由身份"
    );
    let key_fn = function_window(&canvas, "function routeKeyOf(route)", 700);
    assert!(
        key_fn.contains("route.rootStarmapId")
            && key_fn.contains("\"enterEmbed:\" + (s.instanceId")
            && key_fn.contains("\"enterPortal:\" + (s.nodeId"),
        "routeKey 必须由根 ID + 完整 segments（enterEmbed/enterPortal）拼成，实际窗口:\n{key_fn}"
    );

    // 父相机 / 进入上下文按 routeKey 保存，退出按当前 routeKey 的上下文反推。
    assert!(
        canvas.contains("property var cameraStack: ({})")
            && canvas.contains("storeCamera(routeKey, cameraSnapshot())"),
        "父相机必须按 routeKey 保存（同一目标图的不同入口各自独立）"
    );
    let exit_fn = function_window(&canvas, "function maybeExitCanvas()", 900);
    assert!(
        exit_fn.contains("entryContexts[routeKey]") && exit_fn.contains("exitCanvasViaCore(true)"),
        "退出必须按当前 routeKey 的进入上下文反推父镜头，实际窗口:\n{exit_fn}"
    );
    // 进入 / 退出阈值是入口投影直径占视口短边：70% 进，60% 出。
    assert!(
        canvas.contains("readonly property real _enterRatio: 0.70")
            && canvas.contains("readonly property real _exitRatio: 0.60"),
        "进入/退出阈值必须是 70% / 60%（入口投影直径占视口短边）"
    );

    // 活动画布身份用完整 routeKey，不是字面量 "root"。
    assert!(
        canvas.contains("scenePathKey: canvasArea.routeKey"),
        "活动画布身份必须传完整 routeKey"
    );
    assert!(
        !canvas.contains("scenePathKey: \"root\""),
        "不得再写死 scenePathKey: \"root\""
    );

    let scene = read_src(ACTIVE_SCENE);
    assert!(
        scene.contains("required property string scenePathKey"),
        "StarMapActiveScene 的 scenePathKey 必须是 required property"
    );
    assert!(
        !scene.contains("property string scenePathKey: \"root\""),
        "StarMapActiveScene 的 scenePathKey 不得带 \"root\" 默认值，否则活动画布会冒充根画布"
    );
    assert!(
        scene.contains("pathKey: scene.scenePathKey"),
        "活动画布的 GraphController 必须显式接收完整 routeKey 作为 pathKey"
    );
    assert!(
        scene.contains("graphController.pathKey = scene.scenePathKey"),
        "切换画布时必须显式落 pathKey 再加载图，不能只靠绑定求值顺序读到上一张画布"
    );

    // 只读微缩概览不参与路由身份。
    let content = read_src(CONTENT);
    assert!(
        !content.contains("scenePathKey"),
        "只读微缩概览不得再持有 scenePathKey"
    );
}
