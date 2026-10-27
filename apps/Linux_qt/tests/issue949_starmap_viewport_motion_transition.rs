//! Issue #949 — Linux_Qt 星图连续缩放镜头 + 子星图穿越过渡守卫。
//!
//! 诊断包（2026-10-18 18:42）记录多次 `starmap.scene_enter` / `scene_exit`：
//! Core 路由切换确实发生，问题在 QML 展示层 —— 镜头动作过硬（滚轮一档硬跳
//! 一个比例）、进入子图立即整屏替换完整画布，没有连续空间感。
//!
//! 本文件锁住修好之后的结构不变量，防止回退：
//!
//! 1. **演出状态唯一**：`StarMapViewportMotion.qml` 只保存展示快照
//!    （progress / fromCamera / toCamera / fromRoute / toRoute / 入口几何），
//!    只有一条时间线同时驱动镜头、圆环、节点展开；不写 Core、不做第二套导航。
//! 2. **连续缩放**：`zoomAt` 只记"累计目标 + 屏幕锚点"，显示镜头由**单一**
//!    补间从正在显示的帧插值（`panX(t)=anchorX-worldX*z(t)`）；捏合实时跟手；
//!    70% / 60% 穿越只按正在显示的投影直径判定，绝不从未来帧硬跳。
//! 3. **穿越不停机**：切换前冻结旧活动场景身份 / 镜头 / 入口几何，切换后
//!    旧画面层继续绘制（禁交互）直到圆环整圈离开可见视口；命中与业务始终按
//!    最新 Core route + 可见相机。
//! 4. **展开只改显示**：active scene / 概览用同一个 `detailReveal`（小圆点 →
//!    完整节点），不改 authored 坐标 / 图关系 / 选中 / 碰撞；仍是矢量绘制，
//!    没有 FBO / 纹理拉伸。
//! 5. **KDE 一致性**：时长派生自 Kirigami.Units（longDuration / shortDuration），
//!    缓动 OutCubic，不用弹簧；关闭动画 / 有效时长为 0 时直接落到最终状态。
//!
//! 复核（2026-10-18T12:12:29Z）后的四条硬约束：
//! 6. **锚点先于缩放**：捏合 / 关闭动画的直接缩放分支先用本次事件 / 质心坐标
//!    现算锚点的世界位置，再改 zoom / pan。
//! 7. **取消与自然完成分开**：settle 仅在演出确实 active 时生效、保留当前
//!    显示帧、不落 toCamera；只有 generation 匹配的 finished 回调才落 toCamera；
//!    `cancel()` 清掉落点，下一次进 / 退场不再被旧相机复位。
//! 8. **圆形可见区域**：圆内那一层（子场景 Loader）由平台 `Sujian.StarMapCircleClip`
//!    裁到 ringCenterX/Y/ringRadius 的圆内（公共 QSGClipNode 几何，stencil），
//!    裁剪半径带描边余量，只在过渡期启用；不是 Rectangle 遮挡、不是 FBO / 位图。
//!
//! 复查评论 6250468319 的定稿口径（本轮）：
//! 9. **一条 progress 派生两份进度**：contentProgress 只在前段（约 60%）用
//!    `1-(1-p/0.6)^3` 平滑到达目标（末端速度趋 0），圆环继续按完整 progress
//!    走到离开可见视口；detailReveal / outgoingReveal 跟随 contentProgress。
//! 10. **进场落点 = 可读镜头**：`from = 入口预览相机`，`to = 子图正常可读 /
//!     可交互镜头`（优先 cameraStack 已有镜头，否则作者尺度 100% + 内容包围盒
//!     居中）；不得再按"圆环撑出可见视口"的倍率反算内容落点。
//! 11. **两种出场两种落点**：滚轮出场目标 = 由当前显示子镜头按 entryContexts
//!     反推的父镜头（零跳变，不回 cameraStack 旧值）；按钮出场目标 =
//!     cameraStack 保存的父镜头，但从当前可见帧平滑过去。
//! 12. **出场圆环 = 入口圆在两个父帧下的投影**：起点 = 当前显示边界圆投影
//!     （与 60% 判定同一"正在显示的投影直径"几何；屏外则覆盖整个视口），
//!     终点 = 目标父帧下 Embed 圆壳投影；结束才更新 cameraStack、才销毁临时层。
//! 13. **旧画面层逐帧推**：进场父层相机 = parentCameraForChild(子层相机(t))，
//!     出场子层相机 = childCameraForParent(父层相机(t))；不做端点独立插值。
//!
//! 复核评论 6250932011（2026-10-18T14:13:40Z）的定稿口径（本轮）：
//! 14. **可读镜头不变**：`readableChildCamera` 仍是「再次进入用 cameraStack 保存
//!     的镜头；首次 zoom=1 + 内容包围盒居中」，不默认 fit 整图；进场仍是
//!     contentProgress 先停、圆环继续走的同一条进度线。
//! 15. **换层不吞滚轮**：`enterEmbedViaCore`（滚轮穿越那一路，trigger=zoom）/
//!     `exitCanvasViaCore(true)` 在 `prepareCanvasSwitch()` 触发换层之前保存本层
//!     没显示完的 `_zoomTargetLevel` 与鼠标屏幕锚点，按 entryContexts 的父子比例
//!     换算（子尺度的 parentPendingZoom × ratio / 父尺度的 childPendingZoom ÷ ratio）
//!     作为过渡目标；`beginCanvasTransition` 不再无条件把 pending 重置成落点。
//!     双击进入仍用可读镜头落点（评论问题 1 的口径不变）。
//! 16. **过渡期相机继续跟手**：演出进行中收到滚轮仍要改当前可见镜头（不冻结整个
//!     longDuration、不排队第二条动画）——`zoomAt` 调 `retargetViewportMotion()`
//!     从可见帧 + 同一鼠标锚点重定相机两端，唯一进度线用 `cameraProgress`（从重定
//!     那一刻起的剩余时间）继续推进；出场圆环端点随父相机两端一起重新投影。
//! 17. **按钮返回不继承滚轮 pending**：`exitCanvasViaCore(false)` 仍平滑回到
//!     cameraStack 保存的父镜头；70% 进入 / 60% 退出阈值不变。
//!
//! QML 组件依赖 qml_resources qrc 与 Rust 注册的上下文属性，无法在本仓库的
//! Rust 测试里实例化，因此按仓库既有惯例（issue801/issue814/issue817/issue822/
//! issue832 系列 WHITE_BOX 守卫）读取 QML 源码确定性断言。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{count_occurrences, function_window, linux_qt_root, read_src};

const MOTION: &str = "qml/StarMapViewportMotion.qml";
const CANVAS: &str = "qml/StarMapCanvas.qml";
const ACTIVE_SCENE: &str = "qml/StarMapActiveScene.qml";
const CONTENT: &str = "qml/StarMapSceneContent.qml";
const EMBED: &str = "qml/StarMapEmbed.qml";
const ROUTER: &str = "qml/StarMapInputRouter.qml";
const MAIN_RS: &str = "src/main.rs";
const BUILD_RS: &str = "build.rs";
/// Issue #949 复核（问题 4）：平台侧圆形裁剪容器。
const CLIP_RS: &str = "src/native/starmap_circle_clip.rs";
const NATIVE_MOD: &str = "src/native/mod.rs";

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

/// 取 `start_marker` 到其后最近 `end_marker` 之间的源码窗口（含两端）。
fn window_between(source: &str, start_marker: &str, end_marker: &str) -> String {
    let start = source
        .find(start_marker)
        .unwrap_or_else(|| panic!("源码必须包含 {start_marker}"));
    let rest = &source[start..];
    let end = rest
        .find(end_marker)
        .unwrap_or_else(|| panic!("{start_marker} 之后必须出现 {end_marker}"));
    rest[..end].to_string()
}

// ─────────────────────────────────────────────────────────────────────────
// 1. 演出组件：只有一个时间线，只存展示快照
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn viewport_motion_file_exists_and_is_registered_in_qrc_and_build() {
    assert!(
        linux_qt_root().join(MOTION).exists(),
        "Issue #949 要求新建 {MOTION}：星图穿越过渡的演出状态（唯一时间线）"
    );

    let main = strip_line_comments(&read_src(MAIN_RS));
    assert!(
        main.contains("qml/StarMapViewportMotion.qml"),
        "src/main.rs 必须把 qml/StarMapViewportMotion.qml 加进 qrc：\
         漏登记时开发目录碰巧能找到，打包后组件不存在"
    );
    let build = strip_line_comments(&read_src(BUILD_RS));
    assert!(
        build.contains("qml/StarMapViewportMotion.qml"),
        "build.rs 的 rerun-if-changed 必须包含 qml/StarMapViewportMotion.qml"
    );
}

#[test]
fn viewport_motion_exposes_display_snapshots_only() {
    let src = strip_line_comments(&read_src(MOTION));
    for field in [
        "property real progress: 0",
        "property bool active: false",
        "property bool entering: true",
        "property var fromCamera: null",
        "property var toCamera: null",
        "property var fromRoute: null",
        "property var toRoute: null",
        "property real embedCenterX: 0",
        "property real embedDiameter: 0",
        "property var childBounds: null",
        "property real fitScale: 0",
    ] {
        assert!(
            src.contains(field),
            "演出组件必须只暴露展示快照 {field}（不是第二套导航 / 相机模型）"
        );
    }
    assert!(
        src.contains("function beginEnter(fromCamera, toCamera, embedGeometry, parentRoute, childRoute)")
            && src.contains("function beginExit(fromCamera, toCamera, embedGeometry, childRoute, parentRoute)"),
        "进 / 退场只允许 beginEnter / beginExit 两个入口存展示快照"
    );

    // 不写 Core、不持久化、不做第二套路由。
    for forbidden in [
        "enter_starmap_embed",
        "exit_starmap_canvas",
        "resolve_starmap_navigation",
        "record_interaction",
        "storeCamera",
        "currentRoute",
    ] {
        assert!(
            !src.contains(forbidden),
            "演出组件不得出现 {forbidden}：真正 route 仍由 Canvas 走 Core，\
             演出结束即销毁临时层，不持久化、不写 Core"
        );
    }
}

#[test]
fn viewport_motion_has_one_timeline_for_camera_ring_and_reveal() {
    let src = strip_line_comments(&read_src(MOTION));
    assert_eq!(
        count_occurrences(&src, "NumberAnimation {"),
        1,
        "镜头 / 圆环 / 节点展开必须共用一个时间线，不给三部分各开一套时钟"
    );
    let timeline = function_window(&src, "property NumberAnimation timeline:", 500);
    assert!(
        timeline.contains("property: \"progress\"")
            && timeline.contains("easing.type: Easing.OutCubic"),
        "唯一时间线必须驱动 progress 且用 OutCubic（不加弹簧反弹），实际窗口:\n{timeline}"
    );
    assert!(
        !src.contains("SpringAnimation")
            && !src.contains("Easing.OutBack")
            && !src.contains("Easing.InBack"),
        "星图过渡不得引入弹簧 / 回弹缓动"
    );

    // 三部分都由同一个 progress 派生；内容态跟随 contentProgress。
    assert!(
        src.contains("readonly property real ringRadius:")
            && src.contains("readonly property real contentProgress:")
            && src.contains("detailReveal: entering ? contentProgress : 1 - contentProgress")
            && src.contains("outgoingReveal: entering ? 1 : 1 - contentProgress"),
        "圆环半径与节点展开都必须按同一个 progress 派生出场方向，且内容态跟随 contentProgress"
    );
    assert!(
        src.contains("contentSpan: 0.6")
            && src.contains("1 - Math.pow(1 - Math.min(1, progress / contentSpan), 3)"),
        "contentProgress 必须在前段（约 60%）平滑到达 1：1-(1-p/0.6)^3，末端速度趋 0"
    );
    // 数值语义：p=0.6 时内容已经到 1（之后不再变化），圆环仍按完整 progress
    // 继续走到整圈离开视口（p=1）。
    let content = |p: f64| 1.0 - (1.0 - (p / 0.6).min(1.0)).powi(3);
    assert!(
        (content(0.6) - 1.0).abs() < 1e-9,
        "p=0.6 时内容必须恰好到 1"
    );
    assert!(
        content(0.5) < 1.0 && (content(1.0) - content(0.6)).abs() < 1e-9,
        "内容先到 1 后停住，不再跟圆环继续放大"
    );
    assert!(
        src.contains("ringVisible: active && progress < 1"),
        "圆环必须继续按完整 progress 走到整圈离开可见视口才停止绘制"
    );
}

#[test]
fn viewport_motion_duration_follows_kde_and_skips_when_disabled() {
    let src = read_src(MOTION);
    assert!(
        src.contains("import org.kde.kirigami.platform as Kirigami"),
        "演出时长必须派生自 KDE（Kirigami.Units），需要 platform import"
    );
    let duration = function_window(&src, "readonly property int effectiveDuration:", 500);
    assert!(
        duration.contains("Kirigami.Units.longDuration") && duration.contains("return 0"),
        "进 / 退场用 longDuration 量级，关闭动画 / 有效时长为 0 时归一到 0，实际窗口:\n{duration}"
    );
    assert!(
        count_occurrences(&src, "_onTimelineFinished()") >= 2
            && src.contains("if (effectiveDuration <= 0)")
            && src.contains("// 系统关闭动画 / 有效时长为 0：直接完成到 1（不留半截演出）。"),
        "关闭动画 / 有效时长为 0 时必须直接完成到 1"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 2. 连续缩放：目标与显示帧分开，命中仍按可见镜头
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn zoom_target_and_anchor_are_recorded_without_queueing_tweens() {
    let src = read_src(CANVAS);
    let zoom = function_window(&src, "function zoomAt(", 2600);
    assert!(
        zoom.contains("var requested = nextZoom")
            && zoom.contains("var target = normalizedZoom(requested, zoomLevel)"),
        "zoomAt 仍必须是唯一缩放入口并只做 Number 边界规范化，实际窗口:\n{zoom}"
    );
    assert!(
        zoom.contains("_zoomTargetLevel = target"),
        "滚轮 / 按钮只更新累计目标（不从还没走完的显示帧重新硬跳），实际窗口:\n{zoom}"
    );
    assert!(
        zoom.contains("var zoomingIn = target > oldZoom")
            && zoom.contains("var zoomingOut = target < oldZoom"),
        "zoomAt 必须区分放大 / 缩小方向，实际窗口:\n{zoom}"
    );

    // 每帧推进：先确定 z(t)，再按锚点世界位置反推 pan；穿越只按显示帧评估。
    let display = function_window(&src, "function applyDisplayedZoom(", 900);
    assert!(
        display.contains("zoomLevel = target")
            && display.contains("applyPan(screenX - _zoomRoundWorldX * target,")
            && display.contains("screenY - _zoomRoundWorldY * target)"),
        "每帧必须用 panX(t)=anchorX-worldX*z(t) 反推 pan（鼠标指向的对象不滑走），实际窗口:\n{display}"
    );
    assert!(
        display.contains("var previous = zoomLevel")
            && display.contains("var zoomingIn = target > previous")
            && display.contains("if (zoomingIn)")
            && display.contains("switched = maybeEnterEmbedAt(screenX, screenY)")
            && display.contains("else if (zoomingOut)")
            && display.contains("switched = maybeExitCanvas()"),
        "穿越判定必须只用正在显示的投影直径（放大只进场、缩小只退场），实际窗口:\n{display}"
    );

    // 锚点的 world 位置永远用"正在显示的帧"现算。
    let anchor = function_window(&src, "function anchorRoundAt(", 400);
    assert!(
        anchor.contains("(screenX - panX) / z") && anchor.contains("(screenY - panY) / z"),
        "锚点世界位置必须由当前显示帧现算，实际窗口:\n{anchor}"
    );
}

#[test]
fn canvas_has_exactly_one_zoom_tween_retargeted_from_the_displayed_frame() {
    let src = strip_line_comments(&read_src(CANVAS));
    assert_eq!(
        count_occurrences(&src, "id: zoomTween"),
        1,
        "连续滚轮只允许重定同一个补间，不排队堆积互相打断的补间"
    );
    let tween = function_window(&src, "function startZoomTween(", 700);
    assert!(
        tween.contains("zoomTween.from = zoomLevel")
            && tween.contains("zoomTween.to = target")
            && tween.contains("zoomTween.restart()"),
        "补间起点永远是当前显示帧，实际窗口:\n{tween}"
    );
    assert!(
        src.contains("function stopZoomTween("),
        "补间必须能被显式收尾（切画布 / 新输入都不留半截）"
    );
    let duration = function_window(&src, "function zoomTweenDuration(", 400);
    assert!(
        duration.contains("Kirigami.Units.shortDuration"),
        "缩放用 shortDuration 量级（KDE 一致性），实际窗口:\n{duration}"
    );
}

#[test]
fn pinch_stays_realtime_and_keeps_the_929_pairing() {
    let src = read_src(CANVAS);
    let zoom = function_window(&src, "function zoomAt(", 3600);
    assert!(
        zoom.contains("var pinchInput = interaction && interaction.pointerMode === \"pinch\"")
            && zoom.contains("if (pinchInput || zoomTweenDuration() <= 0)"),
        "捏合必须走「本帧直接落到目标帧」（不能每帧额外延迟），实际窗口:\n{zoom}"
    );
    assert!(
        zoom.contains("_pinchSwitchBaseZoom = nextZoom")
            && zoom.contains("_pinchSwitchCanvasZoom = switched")
            && !zoom.contains("_pinchSwitchBaseZoom = zoomLevel"),
        "捏合穿越后必须以本帧 nextZoom 重设基准配对，实际窗口:\n{zoom}"
    );

    let router = strip_line_comments(&read_src(ROUTER));
    assert!(
        router.contains("canvas.zoomAt(centroid.position.x, centroid.position.y,")
            && router.contains("_pinchStartZoom * activeScale"),
        "捏合仍必须实时把累计比例交给同一个 zoomAt 入口"
    );
}

#[test]
fn wheel_accumulates_on_the_target_not_on_the_displayed_frame() {
    let router = strip_line_comments(&read_src(ROUTER));
    let wheel = function_window(&router, "id: wheelHandler", 2400);
    assert!(
        wheel.contains("var oldZoom = canvas.zoomTargetLevel")
            && wheel.contains("oldZoom * Math.pow(canvas._zoomFactor, delta)"),
        "滚轮必须乘在累计目标上（一档一档连续累积，显示镜头随后追上去），实际窗口:\n{wheel}"
    );
    assert!(
        wheel.contains("canvas.zoomAt(point.position.x, point.position.y, newZoom)")
            && wheel.contains("blocking: false")
            && wheel.contains("event.accepted = true"),
        "滚轮仍必须走唯一的 zoomAt 入口，并显式接管框外 accepted，实际窗口:\n{wheel}"
    );
    assert!(
        !wheel.contains("enabled:"),
        "wheelHandler 不得有任何 enabled 门控（鼠标停在哪一层都走同一个 zoomAt），\
         实际窗口:\n{wheel}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. 穿越不停机：旧画面在位 + 新画面同帧展开
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn canvas_freezes_the_outgoing_scene_before_switching_the_route() {
    let src = strip_line_comments(&read_src(CANVAS));
    assert!(
        src.contains("id: outgoingSceneLoader") && src.contains("id: outgoingSceneComponent"),
        "过渡必须同时实例化旧画面层（冻结 route / 镜头，只绘制）"
    );
    let outgoing = function_window(&src, "id: outgoingSceneComponent", 1400);
    assert!(
        outgoing.contains("starmapId: canvasArea.outgoingStarmapId")
            && outgoing.contains("scenePathKey: canvasArea.outgoingRouteKey")
            && outgoing.contains("zoomLevel: canvasArea.outgoingZoom")
            && outgoing.contains("panX: canvasArea.outgoingPanX")
            && outgoing.contains("panY: canvasArea.outgoingPanY"),
        "旧画面层必须用冻结的身份 / 镜头绘制，实际窗口:\n{outgoing}"
    );
    assert!(
        outgoing.contains("interactive: false"),
        "旧画面层禁交互：命中 / 菜单 / 编辑都不走它，实际窗口:\n{outgoing}"
    );
    assert!(
        !outgoing.contains("hitTargetAtWorld") && !outgoing.contains("worldToScreenX"),
        "旧画面层不得自带第二套命中 / 投影入口"
    );

    let enter = function_window(&src, "function enterEmbedViaCore(", 5200);
    assert!(
        enter.contains("var parentCamera = cameraSnapshot()")
            && enter.contains("var parentRoute = normalizeRoute(currentRoute)")
            && enter.contains("screenX: worldToScreenX(info.centerX)")
            && enter.contains("screenY: worldToScreenY(info.centerY)"),
        "切换前必须冻结旧活动场景身份 / 旧镜头 / 入口在显示帧上的屏幕几何，实际窗口:\n{enter}"
    );
    assert!(
        enter.contains("starmapBackendRef.enter_starmap_embed(")
            && enter.contains("storeCamera(routeKey, cameraSnapshot())")
            && enter.contains("storeCamera(childKey, childCamera)"),
        "进入仍必须走 Core 导航路由并按 routeKey 保存父镜头，实际窗口:\n{enter}"
    );
    assert!(
        enter.contains("beginCanvasTransition(true, parentStarmapId, parentRoute, parentCamera,"),
        "取得新 route / camera 后必须启动同一份演出，实际窗口:\n{enter}"
    );

    let exit = function_window(&src, "function exitCanvasViaCore(", 3600);
    assert!(
        exit.contains("starmapBackendRef.exit_starmap_canvas(")
            && exit.contains("var cam = cameraStack[parentKey]")
            && exit
                .contains("beginCanvasTransition(false, childStarmapId, childRoute, childCamera,"),
        "退出必须走 Core 路由并启动反向演出，实际窗口:\n{exit}"
    );
}

#[test]
fn transition_moves_border_and_content_in_the_same_progress() {
    let src = read_src(CANVAS);
    let begin = function_window(&src, "function beginCanvasTransition(", 3600);
    assert!(
        begin.contains("viewportMotion.beginEnter(") && begin.contains("viewportMotion.beginExit("),
        "进 / 退场都必须经同一个演出组件启动，实际窗口:\n{begin}"
    );
    // 复核 6250932011（问题 2）：pending 由调用方按父子比例换算好传进 incomingCam，
    // 这里不再无条件重置成落点；锚点固定在同一鼠标屏幕位置。
    assert!(
        begin.contains("_zoomTargetLevel = incomingCam.zoom")
            && begin.contains("_zoomAnchorX = pendingAnchor ? pendingAnchor.x : width / 2")
            && begin.contains("_zoomAnchorY = pendingAnchor ? pendingAnchor.y : height / 2"),
        "切画布后的滚轮累计目标必须是调用方换算好的新层目标，锚点沿用同一鼠标位置，实际窗口:\n{begin}"
    );

    let frame = function_window(&src, "function applyMotionFrame(", 1400);
    assert!(
        frame.contains("var camProgress = viewportMotion.cameraProgress")
            && count_occurrences(
                &frame,
                "cameraLerp(viewportMotion.fromCamera, viewportMotion.toCamera,",
            ) == 2,
        "进场（内容先到、圆环后到）与出场（连续缩回）的相机都按同一条演出进度线的 \
         cameraProgress 推进（默认进场 contentProgress / 出场 progress，滚轮重定后从可见帧继续），\
         实际窗口:\n{frame}"
    );
    assert!(
        frame.contains("parentCameraForChild(childCam, motionContext)")
            && frame.contains("childCameraForParent(nextParentCam, motionContext)"),
        "旧画面层必须逐帧用同一份父子对应推（不做端点独立插值），实际窗口:\n{frame}"
    );

    assert!(
        src.contains("function finishViewportMotion(") && src.contains("function settleViewportMotion("),
        "过渡必须能被正常收尾（落最终镜头 + 销毁临时层）与立即收尾（新输入重定目标）"
    );
    let finish = function_window(&src, "function finishViewportMotion(", 1200);
    assert!(
        finish.contains("viewportMotion.cancel()")
            && finish.contains("outgoingStarmapId = \"\"")
            && finish.contains("resetZoomRound()"),
        "演出结束必须销毁临时层并把本轮缩放复位，实际窗口:\n{finish}"
    );

    // 父子镜头对应只有一份（进出场共用），双击进入和滚轮穿越用同一空间关系。
    let pair = function_window(&src, "function childCameraForParent(", 600);
    assert!(
        pair.contains("parentCam.zoom * ctx.ratio")
            && pair.contains("screenX - ctx.boundsCenterX * zoom"),
        "子镜头必须与概览 fit 投影同一份几何（入口内同一片坐标），实际窗口:\n{pair}"
    );
}

#[test]
fn new_input_settles_the_transition_and_late_finish_cannot_pull_back() {
    let src = read_src(CANVAS);
    assert!(
        src.contains("property int motionGeneration: 0")
            && src.contains("viewportMotion.generation = ++motionGeneration")
            && src.contains("if (generation !== undefined && generation !== motionGeneration)"),
        "每次启动 / 收尾演出都要推进 generation，晚到的旧回调不得改状态"
    );
    // Issue #949 复核（问题 2）：取消 / 打断是独立路径 —— inactive 时是 no-op，
    // active 时保留当前显示帧，绝不无条件 applyCamera(toCamera)（跳到未来最终帧）。
    let settle = function_window(&src, "function settleViewportMotion(", 1400);
    assert!(
        settle.contains("if (!viewportMotion.active)")
            && settle.contains("viewportMotion.cancel()")
            && settle.contains("resetZoomRound()"),
        "新输入必须先判断演出确实在播，再从当前已显示帧收尾旧演出，实际窗口:\n{settle}"
    );
    assert!(
        !settle.contains("applyCamera(to)")
            && !settle.contains("viewportMotion.toCamera"),
        "取消 / 打断路径不得落 toCamera（保留 cameraSnapshot() 的当前显示帧），实际窗口:\n{settle}"
    );
    let finish = function_window(&src, "function finishViewportMotion(", 1200);
    assert!(
        finish.contains("var to = viewportMotion.toCamera")
            && finish.contains("applyCamera(to)"),
        "自然完成（generation 匹配的 finished 回调）才落 toCamera，实际窗口:\n{finish}"
    );

    let router = strip_line_comments(&read_src(ROUTER));
    assert_eq!(
        count_occurrences(&router, "canvas.cancelViewportMotion()"),
        2,
        "左键 / 右键按下都要先让演出落到当前显示帧（命中按最新 route + 可见帧）"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 4. 展开只改显示：圆点 → 完整节点，仍是矢量
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn active_scene_reveals_dots_into_full_nodes_without_touching_authored_coords() {
    let src = strip_line_comments(&read_src(ACTIVE_SCENE));
    assert!(
        src.contains("property real detailReveal: 1.0")
            && src.contains("readonly property real _revealDotRadius:")
            && src.contains("Math.min(width, height) * 0.018"),
        "活动画布必须有显示层展开进度与概览同款圆点口径"
    );
    assert!(
        src.contains("width: nodeData.width * scene._safeZoom * scene._reveal")
            && src.contains("width: embedData.width * scene._safeZoom * scene._reveal"),
        "节点 / 入口的屏幕尺寸必须随展开进度从圆点长成完整几何"
    );
    assert!(
        src.contains("x: scene.worldToScreenX(worldX)")
            && src.contains("y: scene.worldToScreenY(worldY)"),
        "展开只改屏幕显示尺寸：位置仍来自 authored 坐标的投影"
    );

    // 仍然是矢量绘制：没有 FBO / 纹理拉伸 / 递归子视口。
    for forbidden in [
        "layer.enabled",
        "ShaderEffectSource",
        "MultiEffect",
        "grabToImage",
        "StarMapSceneContent",
    ] {
        assert!(
            !src.contains(forbidden),
            "星图过渡不得引入位图 / FBO / 递归子视口：{forbidden}"
        );
    }
}

#[test]
fn overview_content_projects_real_geometry_without_dot_tiers() {
    let src = strip_line_comments(&read_src(CONTENT));
    assert!(
        src.contains("property real detailReveal: 1.0"),
        "只读概览仍接收视口过渡进度属性（不再据此换形状）"
    );
    assert!(
        src.contains("readonly property var projectedNodes:")
            && src.contains("readonly property var projectedEmbeds:")
            && src.contains("width: size.width * fitScale")
            && src.contains("radius: modelData.diameter / 2"),
        "概览必须直接投影真实卡片 / 入口几何（不改 authored 坐标）"
    );
    assert!(
        !src.contains("_overviewDotRadius")
            && !src.contains("readonly property string nodesPath")
            && !src.contains("readonly property string embedsPath"),
        "概览不得按倍率换成小圆点 / 摘要卡"
    );
    assert!(
        src.contains("readonly property real fitScale:") && src.contains("readonly property var childBounds:"),
        "概览的 fitScale / childBounds 必须保留（进入连续性靠它）"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 5. 圆壳：同一个圆在放大，切换瞬间不许关掉描边
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn embed_keeps_its_shell_until_the_ring_leaves_the_viewport() {
    let src = strip_line_comments(&read_src(EMBED));
    assert!(
        src.contains("property real revealProgress: -1")
            && src.contains("readonly property bool revealOnScreen: revealActive && revealProgress < 1"),
        "圆壳必须按过渡进度判断「还在不在可见视口里」，实际源码缺少"
    );
    assert!(
        src.contains("visible: !root.revealActive || root.revealOnScreen"),
        "圆壳只在整圈离开可见视口（进度 1）之前由过渡层接手，实际源码缺少"
    );
    assert!(
        !src.contains("_enterRatio") && !src.contains("_exitRatio") && !src.contains("coverage"),
        "Embed 不得用覆盖率阈值决定圆壳可见性：覆盖率 70% 的切换瞬间不许把描边关掉"
    );
    assert!(
        src.contains("readonly property real _diameter: Math.min(width, height)")
            && !src.contains("_embedDefaultWidth") && !src.contains("_embedDefaultHeight"),
        "圆壳世界几何恒定（正圆，屏幕直径由父镜头投影）"
    );

    // 放大描边从"入口在旧父镜头下的屏幕几何"起步（与圆壳同一位置、同一半径），
    // 圆心对齐视口中心、半径一直到整圈离开可见视口才停。
    let motion = strip_line_comments(&read_src(MOTION));
    assert!(
        motion.contains("embedScreenRadius + (ringTargetRadius - embedScreenRadius) * progress")
            && motion.contains("ringTargetRadius: ringCornerDistance + ringMargin")
            && motion.contains("ringCornerDistance:"),
        "过渡圆环必须与同一个圆壳同几何起步、放大到整圈离开可见视口"
    );
    assert!(
        motion.contains("embedScreenX + (ringTargetX - embedScreenX) * progress")
            && motion.contains("ringTargetX: viewportWidth / 2"),
        "过渡圆环圆心必须向视口中心对齐"
    );

    let scene = strip_line_comments(&read_src(ACTIVE_SCENE));
    assert!(
        scene.contains("property string transitionEmbedId: \"\"")
            && scene.contains("revealProgress: embedData.instanceId === scene.transitionEmbedId"),
        "活动画布必须把「继续放大的那个圆壳」标出来并透给 Embed"
    );
    assert!(
        scene.contains("id: transitionRing") && scene.contains("border.color: scene.dt.inverseSurface"),
        "过渡圆环（同一圆壳的放大描边）必须由活动场景按同一进度绘制"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 6. 命中 / 业务不被迟滞的显示镜头带偏
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn hit_testing_follows_the_visible_camera_only() {
    let src = strip_line_comments(&read_src(CANVAS));
    let entry = function_window(&src, "function hitTargetAtScreen(", 400);
    assert!(
        entry.contains("scene.hitTargetAtWorld(screenToWorldX(sx), screenToWorldY(sy))"),
        "命中仍必须从「正在显示的镜头」换算，实际窗口:\n{entry}"
    );
    assert_eq!(
        count_occurrences(&src, "function hitTargetAtScreen("),
        1,
        "过渡不得新增第二个命中入口"
    );
    assert!(
        !src.contains("outgoingScene.hit"),
        "旧画面层不参与命中：业务目标始终是最新 Core route"
    );

    // 穿越阈值仍然只看"正在显示的"投影直径。
    let enter = function_window(&src, "function maybeEnterEmbedAt(", 800);
    assert!(
        enter.contains("var projectedPx = Math.min(emb.width, emb.height) * zoomLevel"),
        "进场判定必须用正在显示的投影直径，实际窗口:\n{enter}"
    );
    let exit = function_window(&src, "function maybeExitCanvas(", 900);
    assert!(
        exit.contains("var virtualParentZoom = zoomLevel / ctx.ratio"),
        "退场判定必须用正在显示的投影直径反推父入口，实际窗口:\n{exit}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 7. 复核 2026-10-18T12:12:29Z：四处确定的逻辑错误
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn direct_zoom_branches_anchor_before_changing_zoom() {
    // 问题 1：捏合 / 关闭动画的直接缩放分支必须先用**本次事件 / 质心坐标**
    // 把锚点的世界位置从正在显示的帧现算出来，再改 zoom / pan；沿用上一次
    // resetZoomRound() / 滚轮写入的锚点会让触点一偏离屏幕中心就整张图飞走。
    let src = read_src(CANVAS);
    let direct = window_between(
        &src,
        "if (pinchInput || zoomTweenDuration() <= 0) {",
        "startZoomTween(target, screenX, screenY)",
    );
    let anchor_at = direct
        .find("anchorRoundAt(screenX, screenY)")
        .unwrap_or_else(|| panic!("直接缩放分支必须用本次坐标重算锚点，实际窗口:\n{direct}"));
    let zoom_set = direct
        .find("zoomLevel = target")
        .unwrap_or_else(|| panic!("直接缩放分支必须落到目标帧，实际窗口:\n{direct}"));
    assert!(
        anchor_at < zoom_set,
        "必须先 anchorRoundAt(screenX, screenY) 再改 zoomLevel（顺序反了就是 800px 跳动），实际窗口:\n{direct}"
    );
    assert!(
        direct.contains("applyPan(screenX - _zoomRoundWorldX * target,")
            && direct.contains("screenY - _zoomRoundWorldY * target)"),
        "锚点必须是本次坐标现算出的世界位置，实际窗口:\n{direct}"
    );
    assert!(
        direct.contains("switched = maybeEnterEmbedAt(screenX, screenY)")
            && direct.contains("switched = maybeExitCanvas()"),
        "直接落到目标帧后仍按正在显示的投影直径评估穿越，实际窗口:\n{direct}"
    );

    // #929 捏合跨 route 倍率基准配对保留；演出被捏合打断时基准锚定当前显示帧。
    let zoom = function_window(&src, "function zoomAt(", 3600);
    assert!(
        zoom.contains("_pinchSwitchBaseZoom = nextZoom")
            && zoom.contains("_pinchSwitchCanvasZoom = switched")
            && !zoom.contains("_pinchSwitchBaseZoom = zoomLevel"),
        "捏合穿越后必须以本帧 nextZoom 重设基准配对，实际窗口:\n{zoom}"
    );
    assert!(
        zoom.contains("if (viewportMotion.active && pinchInput)")
            && zoom.contains("_pinchSwitchCanvasZoom = zoomLevel"),
        "捏合继续打断演出时必须把换换算基准锚定到当前显示帧（不是上一次落点），实际窗口:\n{zoom}"
    );
}

#[test]
fn motion_cancel_clears_the_landing_snapshot() {
    // 问题 2：cancel() 必须清掉 toCamera，否则 active=false 之后旧落点仍挂在
    // 组件上，下一次进 / 退场开始时的 settle 会把用户镜头强行复位。
    let src = strip_line_comments(&read_src(MOTION));
    let cancel = function_window(&src, "function cancel()", 700);
    assert!(
        cancel.contains("timeline.stop()")
            && cancel.contains("active = false")
            && cancel.contains("toCamera = null"),
        "cancel() 必须停时间线并把落点清空，实际窗口:\n{cancel}"
    );
}

#[test]
fn enter_transition_moves_from_entry_preview_to_the_readable_camera() {
    // 复查 6250468319：from = 入口预览相机（父图当前显示帧里 Embed 圆壳的投影
    // 映射），to = 子图正常可读 / 可交互镜头；不再使用"内容 = 预览倍率 × 圆环
    // 退场半径 / 原半径"的落点公式（入口很小时会把节点放大几十倍）。
    let canvas = strip_line_comments(&read_src(CANVAS));
    let begin = function_window(&canvas, "function beginCanvasTransition(", 3000);
    assert!(
        begin.contains("var previewCam = context && context.ratio > 0")
            && begin.contains("? childCameraForParent(outgoingCam, context) : incomingCam")
            && begin.contains("viewportMotion.beginEnter(previewCam, incomingCam, geometry")
            && !begin.contains("viewportMotion.beginEnter(incomingCam, incomingCam"),
        "进场起点必须是与入口预览同一份几何的预览相机，落点另算，实际窗口:\n{begin}"
    );
    assert!(
        !begin.contains("outgoingToCamera"),
        "旧父画面层不做端点独立插值（每帧由 parentCameraForChild(子层相机(t)) 推），实际窗口:\n{begin}"
    );

    let enter = function_window(&canvas, "function enterEmbedViaCore(", 6200);
    assert!(
        enter.contains("var childCamera = readableChildCamera(childKey, context)"),
        "进场落点必须取子图可读镜头，实际窗口:\n{enter}"
    );
    assert!(
        enter.contains("storeCamera(childKey, childCamera)"),
        "进入后 cameraStack 记的必须是这个可读落点相机，实际窗口:\n{enter}"
    );
    for forbidden in [
        "discTargetRadiusFor",
        "discTarget",
        "discStart",
        "landingZoom",
        "previewCamera",
    ] {
        assert!(
            !enter.contains(forbidden),
            "进场不得再从圆圈需要撑出屏幕的倍数反算落点（{forbidden}），实际窗口:\n{enter}"
        );
    }

    // 可读镜头的两条来源：优先该子图已有的合法镜头，否则作者尺度 100% +
    // 内容包围盒居中。
    let readable = function_window(&canvas, "function readableChildCamera(", 900);
    assert!(
        readable.contains("var saved = childKey ? cameraStack[childKey] : null")
            && readable.contains("if (saved && saved.zoom > 0)")
            && readable.contains("return { zoom: saved.zoom, panX: saved.panX, panY: saved.panY }"),
        "可读镜头必须优先沿用该子图已有的合法镜头，实际窗口:\n{readable}"
    );
    assert!(
        readable.contains("var zoom = 1.0")
            && readable.contains("panX: width / 2 - cx * zoom")
            && readable.contains("panY: height / 2 - cy * zoom"),
        "没有既有镜头时用作者尺度 100% + 内容包围盒居中的合理默认，实际窗口:\n{readable}"
    );
    // 复核 6250932011 问题 1：保持本行为——首次 zoom=1 + 包围盒居中，不默认
    // fit 整图（星图可无限扩展，节点距离太远时 fit 全图会让节点小得难读）。
    assert!(
        !readable.contains("fit") && !readable.contains("ratio"),
        "可读镜头不得引入 fit 整图 / 按父子比例反算内容倍率的行为，实际窗口:\n{readable}"
    );

    // 圆环"离开可见视口"的公式只剩进场圆环用，不再被人拿去反算内容倍率。
    let motion = strip_line_comments(&read_src(MOTION));
    assert!(
        motion.contains("ringTargetRadius: ringCornerDistance + ringMargin")
            && !motion.contains("function discTargetRadiusFor(screenRadius)")
            && !canvas.contains("discTargetRadiusFor"),
        "落点倍率公式（discTargetRadiusFor）必须删除，圆环目标半径只归圆环自己"
    );
}

#[test]
fn exits_connect_from_the_currently_displayed_frame() {
    // 复查 6250468319：滚轮退出 = 沿当前显示帧连续缩小（目标父镜头 = 反推
    // 帧，不得回到进入前保存的父镜头）；按钮退出 = 目标仍是保存的父镜头，
    // 但从当前可见帧平滑移动 / 缩放过去。
    let canvas = strip_line_comments(&read_src(CANVAS));
    let exit = function_window(&canvas, "function exitCanvasViaCore(", 4200);
    assert!(
        exit.contains("var parentStartCam = ctx && ctx.ratio > 0")
            && exit.contains("? parentCameraForChild(childCamera, ctx) : childCamera"),
        "出场起始父帧必须由当前正在显示的子镜头按 entryContexts 反推，实际窗口:\n{exit}"
    );
    assert!(
        exit.contains("var parentTargetCam = parentStartCam")
            && exit.contains("if (!continuous) {")
            && exit.contains("var cam = cameraStack[parentKey]")
            && exit.contains(
                "parentTargetCam = cam ? { zoom: cam.zoom, panX: cam.panX, panY: cam.panY }"
            )
            && exit.contains(": defaultCamera()"),
        "没有未完成滚轮目标时滚轮出场目标 = 反推帧本身（零跳变）；按钮出场目标 = cameraStack \
         保存的父镜头；有未完成目标时见 layer_switch_carries 守卫，实际窗口:\n{exit}"
    );
    assert!(
        exit.contains("beginCanvasTransition(false, childStarmapId, childRoute, childCamera,")
            && exit.contains("parentTargetCam, exitGeometry, ctx, pendingAnchor)"),
        "出场必须把目标父帧 / 入口几何 / 同一鼠标锚点交给同一份演出，实际窗口:\n{exit}"
    );
    // 结束才更新 cameraStack：起始帧写入由 applyCamera 逐帧完成，这里不预写。
    assert!(
        !exit.contains("storeCamera(parentKey"),
        "出场不得在开始时预写 cameraStack（结束才更新），实际窗口:\n{exit}"
    );
    assert!(
        !exit.contains("applyCamera(cam)") && !exit.contains("applyCamera(parentTargetCam)"),
        "出场不得先把活动镜头跳到目标帧（那是演出终点，由 applyMotionFrame 落），实际窗口:\n{exit}"
    );
}

#[test]
fn exit_ring_spans_the_entry_circle_between_the_two_parent_frames() {
    // 复查 6250468319：圆环起点 = 子图当前显示边界圆投影（与 60% 判定同一
    // "正在显示的投影直径"几何；屏外则覆盖整个视口），终点 = 目标父帧下
    // Embed 圆壳投影。父相机线性插值下端点圆线性插值 = Embed 投影，所以
    // 圆环与父相机共用同一个 progress。
    let motion = strip_line_comments(&read_src(MOTION));
    for field in [
        "property real exitRingStartX: 0",
        "property real exitRingStartY: 0",
        "property real exitRingStartRadius: 0",
        "property real exitRingEndX: 0",
        "property real exitRingEndY: 0",
        "property real exitRingEndRadius: 0",
    ] {
        assert!(
            motion.contains(field),
            "演出组件必须保存出场圆环端点 {field}"
        );
    }
    // 最小替换（复核 6250932011 问题 3）：出场圆环改跟 cameraProgress —— 它默认
    // 就是 progress（未被打断时行为不变），过渡期滚轮重定相机后仍与父相机同参，
    // 保证"圆环 = 可见镜头下入口圆的投影"不脱节。
    assert!(
        motion.contains("exitRingStartX + (exitRingEndX - exitRingStartX) * cameraProgress")
            && motion.contains(
                "exitRingStartRadius + (exitRingEndRadius - exitRingStartRadius) * cameraProgress"
            ),
        "出场圆环必须与父相机共用同一个参数在两端之间线性插值（= 父相机插值下的 Embed 投影）"
    );
    let begin_exit = function_window(&motion, "function beginExit(", 1600);
    assert!(
        begin_exit.contains(
            "_begin(false, fromCamera, toCamera, embedGeometry, childRoute, parentRoute)"
        ) && begin_exit.contains("exitRingStartX = g.startScreenX")
            && begin_exit.contains("exitRingStartRadius = g.startScreenRadius"),
        "出场圆环端点必须经 beginExit 一次性写入，实际窗口:\n{begin_exit}"
    );

    let canvas = strip_line_comments(&read_src(CANVAS));
    let transition = function_window(&canvas, "function beginCanvasTransition(", 3000);
    assert!(
        transition
            .contains("embedScreenGeometry(parentStartCam, g.centerX, g.centerY, g.diameter)")
            && transition.contains("exitGeometry.startScreenX = startGeo.screenX")
            && transition.contains("exitGeometry.startScreenRadius = startGeo.screenRadius"),
        "出场圆环起点必须由当前帧父镜头下的入口圆投影给出，实际窗口:\n{transition}"
    );

    let exit = function_window(&canvas, "function exitCanvasViaCore(", 4200);
    assert!(
        exit.contains("embedScreenGeometry(parentTargetCam, ctx.embedCenterX, ctx.embedCenterY,")
            && exit.contains("exitGeometry.diameter = ctx.embedDiameter"),
        "出场圆环终点必须是目标父帧下入口圆（真实 Embed 圆壳）的投影，实际窗口:\n{exit}"
    );

    // 同一个「正在显示的投影直径」几何：60% 判定用虚拟父镜头反推投影直径。
    let maybe_exit = function_window(&canvas, "function maybeExitCanvas(", 900);
    assert!(
        maybe_exit.contains("var virtualParentZoom = zoomLevel / ctx.ratio")
            && maybe_exit.contains("ctx.embedDiameter * virtualParentZoom"),
        "退场 60% 判定仍必须用正在显示的投影直径反推父入口，实际窗口:\n{maybe_exit}"
    );
}

#[test]
fn transition_ring_clips_the_inside_scene_via_platform_circle_clip() {
    // 问题 4：圆环只画边不行 —— 圆内那一层（子场景 Loader）的可见区域必须
    // 由平台圆形裁剪容器裁到 viewportMotion.ringCenterX/Y/ringRadius 的圆内；
    // 圆外持续显示另一层；只过渡期启用，结束回到直接矢量渲染。
    let canvas = strip_line_comments(&read_src(CANVAS));
    assert!(
        canvas.contains("import Sujian 1.0"),
        "圆形裁剪容器是平台注册的 QML 类型，Canvas 必须 import Sujian 1.0"
    );
    assert_eq!(
        count_occurrences(&canvas, "StarMapCircleClip {"),
        2,
        "活动画布与旧画面层两个 Loader 都要有圆形裁剪容器"
    );
    let active_clip = function_window(&canvas, "id: activeSceneClip", 800);
    assert!(
        active_clip.contains("clipEnabled: canvasArea.activeSceneIsInsideRing")
            && active_clip.contains("clipCenterX: viewportMotion.ringCenterX")
            && active_clip.contains("clipCenterY: viewportMotion.ringCenterY")
            && active_clip.contains("clipRadius: viewportMotion.ringClipRadius")
            && active_clip.contains("id: activeSceneLoader"),
        "进场时「圆内」的新子场景 Loader 必须被裁到圆环圆内（同一 progress），实际窗口:\n{active_clip}"
    );
    let outgoing_clip = function_window(&canvas, "id: outgoingSceneClip", 800);
    assert!(
        outgoing_clip.contains("clipEnabled: canvasArea.outgoingSceneIsInsideRing")
            && outgoing_clip.contains("clipCenterX: viewportMotion.ringCenterX")
            && outgoing_clip.contains("clipRadius: viewportMotion.ringClipRadius")
            && outgoing_clip.contains("id: outgoingSceneLoader"),
        "出场时「圆内」的旧子场景 Loader 必须反向裁到圆环圆内，实际窗口:\n{outgoing_clip}"
    );
    assert!(
        canvas.contains("activeSceneIsInsideRing:")
            && canvas.contains("viewportMotion.active && viewportMotion.entering")
            && canvas.contains("outgoingSceneIsInsideRing:")
            && canvas.contains("viewportMotion.active && !viewportMotion.entering"),
        "圆形裁剪必须只在过渡期对「圆内那一层」启用"
    );
    // 裁剪半径只给圆环外半条描边留 1~2 逻辑像素余量：不改圆环大小 /
    // 世界尺寸 / 70% / 60% 穿越阈值（复查 6250468319）。
    let motion = strip_line_comments(&read_src(MOTION));
    assert!(
        motion.contains("ringClipRadius: ringRadius + ringStrokeWidth / 2 + 1"),
        "裁剪半径必须 = ringRadius + 描边宽/2 + 1~2 逻辑像素"
    );

    // 平台 item：native 模块登记 + main.rs qml_register_type + 公共 QSGClipNode
    // 几何实现（不依赖私有头，也不往 Core 加 UI 坐标 / 状态）。
    let main = strip_line_comments(&read_src(MAIN_RS));
    assert!(
        main.contains("qml_register_type::<StarMapCircleClip>(c\"Sujian\", 1, 0, c\"StarMapCircleClip\")"),
        "平台圆形裁剪容器必须在加载 QML 之前注册进 Sujian 1.0"
    );
    assert!(
        read_src(NATIVE_MOD).contains("pub mod starmap_circle_clip;"),
        "native/mod.rs 必须声明 starmap_circle_clip 模块"
    );
    let clip_raw = read_src(CLIP_RS);
    let clip = strip_line_comments(&clip_raw);
    assert!(
        clip.contains("pub struct StarMapCircleClip")
            && clip.contains("qt_base_class!(trait QQuickItem)")
            && clip.contains("ALIAS clipEnabled WRITE set_clip_enabled"),
        "平台组件必须是 QQuickItem 子类并暴露 clipEnabled / 圆心 / 半径"
    );
    assert!(
        clip.contains("obj->setClip(true)")
            && clip.contains("QSGNode::ClipNodeType")
            && clip.contains("setIsRectangular(false)")
            && clip.contains("QSGGeometry::DrawTriangleStrip")
            && clip.contains("markVertexDataDirty"),
        "圆形可见区域必须落在场景图裁剪节点的自定义圆形几何上（stencil），不是矩形裁剪"
    );
    assert!(
        clip.contains("update_paint_node")
            && clip.contains("fn geometry_changed"),
        "几何参数必须在渲染线程同步阶段应用（update_paint_node），尺寸变化后重新应用"
    );
    for forbidden in [
        "layer.enabled",
        "ShaderEffectSource",
        "MultiEffect",
        "grabToImage",
        "FramebufferObject",
    ] {
        assert!(
            !clip.contains(forbidden) && !canvas.contains(forbidden),
            "圆形裁剪不得退回位图 / FBO / 纹理放大：{forbidden}"
        );
    }
    assert!(
        clip_raw.contains("qquickitem_p.h") && clip_raw.contains("QQuickDefaultClipNode"),
        "必须写明为什么不用私有头 / QQuickDefaultClipNode 的路径与取舍"
    );
    assert!(
        !clip.contains("QQuickItemPrivate")
            && !clip.contains("qquickitem_p.h")
            && !clip.contains("qquickclipnode_p.h"),
        "平台裁剪组件不得依赖本机不存在的 Qt 私有头"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 8. 复核评论 6250932011：换层不吞滚轮 + 过渡期相机继续跟手
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn layer_switch_carries_the_unfinished_wheel_target_across_the_ratio() {
    // 问题 1/2：换层前先保存本层没显示完的滚动目标与鼠标屏幕锚点，按
    // entryContexts 的父子比例换算到目标层尺度（子尺度 ×ratio / 父尺度 ÷ratio，
    // 由 childCameraForParent / parentCameraForChild 的 clamp 一并完成），
    // 不得直接重置成落点 incomingCam.zoom。
    let canvas = strip_line_comments(&read_src(CANVAS));

    // 换算公式本体：子镜头 = 父镜头 × ratio、父镜头 = 子镜头 ÷ ratio，都带 clamp。
    assert!(
        canvas.contains(
            "var zoom = normalizedZoom(parentCam.zoom * ctx.ratio, parentCam.zoom)"
        ) && canvas.contains(
            "var zoom = normalizedZoom(childCam.zoom / ctx.ratio, childCam.zoom)"
        ),
        "跨画布换算必须走 childCameraForParent / parentCameraForChild 的 ×ratio / ÷ratio（带 Number 规范化）"
    );

    // 进场：换层前保存父层 pending + 锚点，换算到子图尺度当过渡目标。
    let enter = function_window(&canvas, "function enterEmbedViaCore(", 6600);
    assert!(
        enter.contains("var parentPendingZoom = _zoomTargetLevel")
            && enter.contains("var pendingAnchorX = _zoomAnchorX")
            && enter.contains("var pendingAnchorY = _zoomAnchorY")
            && enter.contains("var hasPendingZoom = trigger === \"zoom\" && parentPendingZoom > 0")
            && enter.contains("var childTargetCamera = hasPendingZoom"),
        "滚轮穿越（trigger=zoom）必须在 prepareCanvasSwitch() 前保存父层未完成滚轮目标与屏幕锚点，\
         双击仍走可读镜头落点，实际窗口:\n{enter}"
    );
    assert!(
        enter.contains("anchoredCameraAt(parentPendingZoom, pendingAnchorX, pendingAnchorY)")
            && enter.contains(": childCamera")
            && enter.contains("childTargetCamera, entryGeometry, context,")
            && enter.contains("hasPendingZoom ? { x: pendingAnchorX, y: pendingAnchorY } : null"),
        "进场过渡目标必须是换算后的滚轮目标（没有 pending 才用可读镜头），并把同一鼠标锚点交给演出，\
         实际窗口:\n{enter}"
    );
    assert!(
        enter.contains("childCameraForParent(") && enter.contains("anchoredCameraAt("),
        "子图目标 = childTargetZoom = parentPendingZoom × ratio，且仍锚在同一屏幕位置，实际窗口:\n{enter}"
    );

    // 出场（滚轮）：换层前保存子层 pending + 锚点，换算到父图尺度当过渡目标。
    let exit = function_window(&canvas, "function exitCanvasViaCore(", 4600);
    assert!(
        exit.contains("var childPendingZoom = _zoomTargetLevel")
            && exit.contains("var pendingAnchorX = _zoomAnchorX")
            && exit.contains("var pendingAnchorY = _zoomAnchorY"),
        "滚轮出场必须在 prepareCanvasSwitch() 前保存子图未完成滚轮目标与屏幕锚点，实际窗口:\n{exit}"
    );
    assert!(
        exit.contains("} else if (childPendingZoom > 0")
            && exit.contains("anchoredCameraAt(childPendingZoom, pendingAnchorX, pendingAnchorY)")
            && exit.contains("? parentCameraForChild(pendingCam, ctx) : pendingCam")
            && exit.contains("pendingAnchor = { x: pendingAnchorX, y: pendingAnchorY }"),
        "滚轮出场目标 = parentTargetZoom = clamp(childPendingZoom / ctx.ratio) 且锚在同一屏幕位置，\
         实际窗口:\n{exit}"
    );
    assert!(
        exit.contains("parentTargetCam, exitGeometry, ctx, pendingAnchor)"),
        "换算后的目标与同一锚点必须交给同一份演出，实际窗口:\n{exit}"
    );
}

#[test]
fn button_exit_does_not_inherit_the_wheel_pending_and_thresholds_are_unchanged() {
    // 问题 4：点击返回（continuous=false）不接滚轮未完成目标，仍平滑回到
    // cameraStack 保存的父镜头；70% 进入 / 60% 退出的阈值规则不变。
    let canvas = strip_line_comments(&read_src(CANVAS));
    let exit = function_window(&canvas, "function exitCanvasViaCore(", 4600);
    let button_at = exit
        .find("if (!continuous) {")
        .unwrap_or_else(|| panic!("按钮返回分支必须存在，实际窗口:\n{exit}"));
    let pending_at = exit
        .find("} else if (childPendingZoom > 0")
        .unwrap_or_else(|| {
            panic!("滚轮 pending 换算分支必须是按钮分支的 else if，实际窗口:\n{exit}")
        });
    assert!(
        button_at < pending_at,
        "按钮返回必须优先判定且不继承滚轮 pending（pending 只在 continuous 分支里接），实际窗口:\n{exit}"
    );
    let button_branch = window_between(&exit, "if (!continuous) {", "} else if");
    assert!(
        button_branch.contains("var cam = cameraStack[parentKey]")
            && button_branch.contains(": defaultCamera()")
            && !button_branch.contains("pendingAnchor = {"),
        "按钮返回落点仍必须是 cameraStack 保存的父镜头（没有则默认镜头），且不得接滚轮 pending，\
         实际窗口:\n{button_branch}"
    );

    assert!(
        canvas.contains("readonly property real _enterRatio: 0.70")
            && canvas.contains("readonly property real _exitRatio: 0.60"),
        "70% 进入 / 60% 退出的阈值规则不变"
    );
    let maybe_enter = function_window(&canvas, "function maybeEnterEmbedAt(", 800);
    assert!(
        maybe_enter.contains("coverageOfScreenPx(projectedPx) < _enterRatio"),
        "进场阈值仍只按正在显示的投影直径，实际窗口:\n{maybe_enter}"
    );
    let maybe_exit = function_window(&canvas, "function maybeExitCanvas(", 900);
    assert!(
        maybe_exit.contains("Math.min(_exitRatio, entryCoverage)"),
        "退场阈值仍受 60% 与进入时覆盖率的较小值约束（不因本轮改动放宽 / 收紧），实际窗口:\n{maybe_exit}"
    );
}

#[test]
fn wheel_during_transition_keeps_the_visible_camera_following() {
    // 问题 3：过渡过程中收到新的滚轮，必须继续改**当前显示**的缩放镜头
    // （不冻结整个 longDuration、等过渡结束再追赶），圆环 / 父子画面跟着这台
    // 可见镜头更新；仍沿用唯一演出进度线，不排队多条动画。
    let canvas = strip_line_comments(&read_src(CANVAS));
    let zoom = function_window(&canvas, "function zoomAt(", 3800);
    let active_branch = window_between(
        &zoom,
        "if (viewportMotion.active) {",
        "if (pinchInput || zoomTweenDuration() <= 0) {",
    );
    assert!(
        active_branch.contains("retargetViewportMotion(screenX, screenY)"),
        "演出进行中收到滚轮必须继续推动可见相机（从可见帧重定目标），实际分支:\n{active_branch}"
    );
    assert!(
        !active_branch.contains("anchorRoundAt(screenX, screenY)"),
        "只记锚点、等演出结束再追赶的旧冻结行为必须删除（相机要跟着用户的滚轮走），\
         实际分支:\n{active_branch}"
    );

    // 重定本体：起点 = 当前可见帧，终点 = 同一鼠标锚点上的累计目标；
    // 参数从重定那一刻起的剩余时间继续走（进度线不重启）。
    let retarget = function_window(&canvas, "function retargetViewportMotion(", 1300);
    assert!(
        retarget.contains("var fromCam = cameraSnapshot()")
            && retarget
                .contains("var toCam = anchoredCameraAt(_zoomTargetLevel, screenX, screenY)")
            && retarget
                .contains("viewportMotion.retargetCamera(fromCam, toCam, viewportMotion.progress)")
            && retarget.contains("applyMotionFrame()"),
        "过渡期滚轮必须以可见帧为起点、累计目标为终点从当前进度继续推进，实际窗口:\n{retarget}"
    );
    assert!(
        !retarget.contains("startZoomTween")
            && !retarget.contains(".restart()")
            && !retarget.contains("NumberAnimation"),
        "过渡期滚轮不得另起一条动画 / 补间（仍只用唯一演出进度线），实际窗口:\n{retarget}"
    );
    // 出场圆环端点随父相机两端一起重新投影（圆环 = 可见镜头下入口圆的投影）。
    assert!(
        retarget.contains("viewportMotion.exitRingStartRadius = startGeo.screenRadius")
            && retarget.contains("viewportMotion.exitRingEndRadius = endGeo.screenRadius"),
        "出场圆环端点必须随重定后的父相机两端重新投影，实际窗口:\n{retarget}"
    );

    // 锚点 world 用当前显示帧现算（不得拿尚未渲染的未来帧）。
    let anchor = function_window(&canvas, "function anchoredCameraAt(", 500);
    assert!(
        anchor.contains("(screenX - panX) / z") && anchor.contains("(screenY - panY) / z"),
        "过渡期滚轮锚点的世界位置必须由当前显示帧现算，实际窗口:\n{anchor}"
    );

    // 唯一时间线仍是唯一：Canvas 只有一个缩放补间、演出组件只有一个进度线；
    // 重定只改相机两端与 cameraProgress 参数，不重启进度。
    assert_eq!(
        count_occurrences(&canvas, "NumberAnimation {"),
        1,
        "过渡期滚轮不得给 Canvas 增加第二条动画（不排队互相打断的补间）"
    );
    let motion = strip_line_comments(&read_src(MOTION));
    assert_eq!(
        count_occurrences(&motion, "NumberAnimation {"),
        1,
        "相机重定必须复用唯一演出进度线，不给圆环 / 相机各开一套时钟"
    );
    assert!(
        motion.contains("property real retargetAt: -1")
            && motion.contains("return entering ? contentProgress : progress")
            && motion.contains("(progress - retargetAt) / (1 - retargetAt)")
            && motion.contains("function retargetCamera(fromCameraIn, toCameraIn, atProgress)")
            && motion.contains("fromCamera = snapshotOf(fromCameraIn)")
            && motion.contains("toCamera = snapshotOf(toCameraIn)"),
        "演出组件必须提供 cameraProgress（默认内容先停 / 完整 progress，重定后从剩余时间继续）"
    );
    assert_eq!(
        count_occurrences(&motion, "retargetAt = -1"),
        2,
        "新一次演出与 cancel() 都必须把重定参数复位（不把上一轮的相位带到下一轮）"
    );
    // 相机参数默认值仍是 contentProgress —— 4 条口径里的「内容先停、圆环继续」不变。
    let cam_progress = function_window(&motion, "readonly property real cameraProgress:", 500);
    assert!(
        cam_progress.contains("return entering ? contentProgress : progress")
            && !cam_progress.contains("contentSpan"),
        "未被打断时相机参数必须就是进场的 contentProgress / 出场的完整 progress，实际窗口:\n{cam_progress}"
    );
}

#[test]
fn motion_frames_rerun_crossing_checks_only_after_wheel_retarget() {
    // Issue #949 复核评论 6251370089：动画进行中反向滚轮跨 70%/60% 立即换层。
    // retargetViewportMotion 只重锚相机；每帧推进后必须按**上一帧 / 本帧实际
    // 显示**的 zoom 方向重跑既有 maybeEnterEmbedAt / maybeExitCanvas，而不是按
    // 尚未显示的 pending 目标判定；命中后由既有路径 settle 取消本过渡、保留
    // 这一帧镜头与鼠标锚点，再走 Core route + beginCanvasTransition。
    let canvas = strip_line_comments(&read_src(CANVAS));
    let frame = window_between(
        &canvas,
        "function applyMotionFrame(",
        "function checkTransitionThresholds(",
    );
    assert!(
        frame.contains("var previousZoom = zoomLevel")
            && frame.contains("checkTransitionThresholds(previousZoom)"),
        "每帧必须先记下上一帧的显示 zoom，再在相机推进后按这一帧的显示 zoom 判定，\
         实际窗口:\n{frame}"
    );
    let camera_at = frame
        .find("applyCamera(")
        .expect("applyMotionFrame 必须更新相机");
    let check_at = frame
        .find("checkTransitionThresholds(previousZoom)")
        .expect("applyMotionFrame 必须在相机推进后重跑穿越判定");
    assert!(
        check_at > camera_at,
        "阈值判定必须发生在相机推进之后（用刚显示的这一帧，而不是未来的目标帧），\
         实际窗口:\n{frame}"
    );
    assert_eq!(
        count_occurrences(&frame, "checkTransitionThresholds("),
        1,
        "applyMotionFrame 每帧只允许通过唯一的 checkTransitionThresholds 判定一次"
    );
    assert_eq!(
        count_occurrences(&canvas, "checkTransitionThresholds("),
        2,
        "阈值判定只能由 applyMotionFrame 调用（另一个是函数定义本身），不新增第二处入口"
    );

    let check = window_between(
        &canvas,
        "function checkTransitionThresholds(",
        "function finishViewportMotion(",
    );
    assert!(
        check.contains("!viewportMotion.active || !viewportMotion.userRetargeted"),
        "只有收到过滚轮重定的演出才逐帧重跑穿越判定（按钮返回 / 双击进入这类纯演出\
         自身的运动不是滚动意图），实际窗口:\n{check}"
    );
    assert!(
        check.contains("var delta = zoomLevel - previousZoom")
            && check.contains("if (delta > 0)")
            && check.contains("maybeEnterEmbedAt(_zoomAnchorX, _zoomAnchorY)")
            && check.contains("maybeExitCanvas()"),
        "方向由显示帧变化决定：放大只评估进场、缩小只评估退场；复用既有 \
         maybeEnterEmbedAt / maybeExitCanvas（沿用同一鼠标锚点），实际窗口:\n{check}"
    );
    for forbidden in [
        "enter_starmap_embed",
        "exit_starmap_canvas",
        "applyCamera(",
        "startZoomTween",
        "settleViewportMotion(",
        "beginCanvasTransition(",
    ] {
        assert!(
            !check.contains(forbidden),
            "阈值判定不得直接写 Core / 落镜头 / 自己起演出：换层必须交给既有 \
             maybeEnterEmbedAt / maybeExitCanvas（内部先 settle 保留这一帧），\
             实际窗口:\n{check}"
        );
    }

    // 演出组件：重定标记由 retargetCamera 置位，新演出与取消都复位——保证
    // 「相机更新、方向阈值判定、Core 切换」在一次启动里只发生一次。
    let motion = strip_line_comments(&read_src(MOTION));
    assert!(
        motion.contains("property bool userRetargeted: false"),
        "演出组件必须提供滚轮重定标记，供 Canvas 每帧判定是否允许换层"
    );
    let retarget = function_window(&motion, "function retargetCamera(", 700);
    assert!(
        retarget.contains("userRetargeted = true"),
        "滚轮重定必须点亮该标记（这是「过渡期滚轮意图」的唯一来源），实际窗口:\n{retarget}"
    );
    assert_eq!(
        count_occurrences(&motion, "userRetargeted = false"),
        2,
        "新一次演出（_begin）与 cancel() 都必须复位标记，防止同一条演出重复判定自己"
    );
}
