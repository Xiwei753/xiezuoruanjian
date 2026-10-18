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

    // 三部分都由同一个 progress 派生。
    assert!(
        src.contains("readonly property real ringRadius:")
            && src.contains("readonly property real detailReveal:")
            && src.contains("entering ? progress : 1 - progress"),
        "圆环半径与节点展开都必须按同一个 progress 派生出场方向"
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
            && zoom.contains("Math.max(_cameraScaleMin, Math.min(_cameraScaleMax, requested))"),
        "zoomAt 仍必须是唯一缩放入口并只夹数值安全范围，实际窗口:\n{zoom}"
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
    let zoom = function_window(&src, "function zoomAt(", 2600);
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
    let wheel = function_window(&router, "id: wheelHandler", 900);
    assert!(
        wheel.contains("var oldZoom = canvas.zoomTargetLevel")
            && wheel.contains("oldZoom * Math.pow(canvas._zoomFactor, delta)"),
        "滚轮必须乘在累计目标上（一档一档连续累积，显示镜头随后追上去），实际窗口:\n{wheel}"
    );
    assert!(
        wheel.contains("canvas.zoomAt(point.position.x, point.position.y, newZoom)")
            && wheel.contains("blocking: true"),
        "滚轮仍必须走唯一的 zoomAt 入口且保持 blocking，实际窗口:\n{wheel}"
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

    let enter = function_window(&src, "function enterEmbedViaCore(", 3200);
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

    let exit = function_window(&src, "function exitCanvasViaCore(", 2400);
    assert!(
        exit.contains("starmapBackendRef.exit_starmap_canvas(")
            && exit.contains("var cam = cameraStack[parentKey]")
            && exit.contains("beginCanvasTransition(false, childStarmapId, childRoute, childCamera, cam,"),
        "退出必须走 Core 路由并启动反向演出，实际窗口:\n{exit}"
    );
}

#[test]
fn transition_moves_border_and_content_in_the_same_progress() {
    let src = read_src(CANVAS);
    let begin = function_window(&src, "function beginCanvasTransition(", 2200);
    assert!(
        begin.contains("viewportMotion.beginEnter(") && begin.contains("viewportMotion.beginExit("),
        "进 / 退场都必须经同一个演出组件启动，实际窗口:\n{begin}"
    );
    assert!(
        begin.contains("_zoomTargetLevel = incomingCam.zoom"),
        "切画布后滚轮累计目标必须在新画布尺度上重新起步，实际窗口:\n{begin}"
    );

    let frame = function_window(&src, "function applyMotionFrame(", 900);
    assert!(
        frame.contains("applyCamera(cameraLerp(viewportMotion.fromCamera, viewportMotion.toCamera, p))"),
        "活动画布镜头必须逐帧按同一个 progress 插值（命中与显示同源），实际窗口:\n{frame}"
    );
    assert!(
        frame.contains("cameraLerp(outgoingFromCamera, outgoingToCamera, p)"),
        "旧画面层必须用同一份父子对应连续运动（不硬切），实际窗口:\n{frame}"
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
    let settle = function_window(&src, "function settleViewportMotion(", 800);
    assert!(
        settle.contains("viewportMotion.cancel()")
            && settle.contains("applyCamera(to)")
            && settle.contains("resetZoomRound()"),
        "新输入必须从当前已显示帧收尾旧演出，实际窗口:\n{settle}"
    );

    let router = strip_line_comments(&read_src(ROUTER));
    assert_eq!(
        count_occurrences(&router, "canvas.cancelViewportMotion()"),
        2,
        "左键 / 右键按下都要先让演出落到最终状态（命中按最新 route + 可见帧）"
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
fn overview_content_scales_dots_with_the_same_reveal() {
    let src = strip_line_comments(&read_src(CONTENT));
    assert!(
        src.contains("property real detailReveal: 1.0"),
        "只读概览必须接收同一个展开进度（两层圆点同步长大 / 收回）"
    );
    let dots = function_window(&src, "readonly property real _overviewDotRadius:", 300);
    assert!(
        dots.contains("Math.min(width, height) * 0.018")
            && dots.contains("Math.max(0, Math.min(1, detailReveal))"),
        "概览圆点半径必须按展开进度缩放（不改 authored 坐标），实际窗口:\n{dots}"
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
