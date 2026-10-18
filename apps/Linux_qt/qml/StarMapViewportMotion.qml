// =============================================================================
// StarMapViewportMotion.qml — 星图穿越过渡的演出状态（唯一时间线）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 非可视组件）
// 职责：只保存"这次穿越怎么演"的展示快照，用一个时间线（progress 0→1）
//   同时驱动镜头、圆环（入口圆壳放大描边）和节点展开。它不是第二套导航 /
//   相机业务模型：
//   - 真正的 route 仍由 StarMapCanvas 走 Core 的
//     enter_starmap_embed / exit_starmap_canvas 取得并写进 currentRoute；
//   - 相机真相仍是 Canvas 的 zoomLevel / panX / panY 与 cameraStack；
//   - 本组件不持久化、不写 Core、不落盘，过渡结束即被清空（active=false）。
//
// 空间关系（与 enterEmbedViaCore / exitCanvasViaCore 的连续换算同一份）：
//   - 入口圆壳的世界几何恒定（直径 200），半径随镜头比例放大；
//   - 进场：embedScreenX/Y/Radius 是入口在**冻结的旧父镜头**下的屏幕几何
//     （圆环起点）；ringTarget* 是圆环的终点：圆心对齐视口中心，半径大到
//     整圈离开可见视口。子图内容只在前段（contentProgress）走到可读镜头
//     后停住，圆环继续按完整 progress 放大退场；
//   - 出场：exitRingStart* 是当前显示帧（父镜头由当前子镜头反推）下入口圆的
//     投影，exitRingEnd* 是目标父帧下入口圆的投影；父相机线性插值下端点圆
//     线性插值就是 Embed 圆壳投影，圆环与父相机共用同一条 progress；
//   - detailReveal 是显示层展开进度：概览小圆点 → 当前层完整节点；
//     进场 0→1，出场 1→0（退回概览圆点），跟随 contentProgress。
//
// KDE 一致性：时间从 Kirigami.Units.longDuration 派生，缓动 OutCubic，
//   不加弹簧；系统 / 应用关闭动画或有效时长为 0 时直接落到最终状态。

import QtQuick
import org.kde.kirigami.platform as Kirigami

QtObject {
    id: motion

    // ── 演出状态：唯一时间线（0 → 1）──
    property real progress: 0
    // 是否正在演出。route / 相机真相不在这里，只决定"要不要画过渡层"。
    property bool active: false
    // 方向：true = 进入子星图（父图在位停留，圆环放大）；false = 返回父星图。
    property bool entering: true

    // ── 展示快照（beginEnter / beginExit 只存这些，不再推导业务状态）──
    // 新活动画布的镜头：进场 = 入口内同一片坐标（概览 fit 映射）→ 子图正常
    // 可读 / 可交互镜头；出场 = 当前子内容对应的父镜头 → 目标父镜头。
    // 每帧由 Canvas 按进度插值（旧画面层由父 / 子相机对应逐帧推，不在这里）。
    property var fromCamera: null
    property var toCamera: null
    // 旧活动场景的身份：过渡期间它继续按冻结的身份 / 镜头绘制。
    property var fromRoute: null
    property var toRoute: null

    // 视口尺寸：圆环"整圈离开可见视口"的口径。
    property real viewportWidth: 0
    property real viewportHeight: 0

    // 入口世界几何 + 母图侧屏幕几何（起点圆环）。
    property string instanceId: ""
    property real embedCenterX: 0
    property real embedCenterY: 0
    property real embedDiameter: 0
    property real embedScreenX: 0
    property real embedScreenY: 0
    property real embedScreenRadius: 0
    // 子图 authored 包围盒与概览 fit 比例（"入口内同一片坐标"的换算来源）。
    property var childBounds: null
    property real fitScale: 0

    // 演出结束（Canvas 据此清掉旧画面层）。generation 由 Canvas 在每次启动时
    // 写入：晚到的旧时间线带着旧 generation，不允许再把画面拉回上一层。
    signal finished(bool entering, int generation)
    property int generation: 0

    // ── 圆环目标：圆心对齐视口中心，半径大到整圈离开可见视口 ──
    readonly property real ringTargetX: viewportWidth / 2
    readonly property real ringTargetY: viewportHeight / 2
    readonly property real ringMargin: Math.max(embedScreenRadius, 8)
    readonly property real ringCornerDistance:
        Math.hypot(Math.max(ringTargetX, viewportWidth - ringTargetX),
                   Math.max(ringTargetY, viewportHeight - ringTargetY))
    readonly property real ringTargetRadius: ringCornerDistance + ringMargin

    // 出场圆环：起点 = 当前显示帧（父镜头由当前子镜头按 entryContexts 反推）
    // 下入口圆的投影，终点 = 目标父帧下入口圆的投影。父相机线性插值时端点圆
    // 线性插值 == Embed 圆壳投影，所以圆环与父相机共用同一个 progress。
    property real exitRingStartX: 0
    property real exitRingStartY: 0
    property real exitRingStartRadius: 0
    property real exitRingEndX: 0
    property real exitRingEndY: 0
    property real exitRingEndRadius: 0

    // ── 每帧按同一条进度算出的圆环几何（屏幕口径）──
    // 进场：入口圆壳 → 视口中心 + 整圈出视口（screen-space 放大路径，跟 progress）。
    // 出场：当前帧入口圆投影 → 目标父帧入口圆投影（父相机线性插值下的 Embed
    // 投影），与父相机共用同一个参数（cameraProgress，默认就是 progress；过渡期
    // 滚轮重定相机后仍与相机严格同参，圆环不许和可见镜头脱节）。
    readonly property real ringCenterX:
        entering ? embedScreenX + (ringTargetX - embedScreenX) * progress
                 : exitRingStartX + (exitRingEndX - exitRingStartX) * cameraProgress
    readonly property real ringCenterY:
        entering ? embedScreenY + (ringTargetY - embedScreenY) * progress
                 : exitRingStartY + (exitRingEndY - exitRingStartY) * cameraProgress
    readonly property real ringRadius:
        entering ? embedScreenRadius + (ringTargetRadius - embedScreenRadius) * progress
                 : exitRingStartRadius + (exitRingEndRadius - exitRingStartRadius) * cameraProgress
    // 进场：半径到目标时整圈已在视口外，此刻才停止绘制；出场：缩小到落回母图圆壳
    // 之前一直画（落地后由母图真实的 Embed 圆壳接管，同一位置同一半径）。
    readonly property bool ringVisible: active && progress < 1
    readonly property real ringStrokeWidth: 2
    // 圆形裁剪半径：只给圆环外半条描边留 1~2 逻辑像素余量，不改圆环大小 /
    // 世界尺寸 / 70% / 60% 阈值（Issue #949 复核评论 6250468319）。
    readonly property real ringClipRadius: ringRadius + ringStrokeWidth / 2 + 1

    // ── 内容进度：只驱动内容（子镜头 / 圆点展开），前段（约 60%）平滑到达
    //    目标镜头后停住；末端速度趋 0（1-(1-p/0.6)^3），停下的瞬间不跳速。
    //    圆环继续按完整 progress 走到离开视口，共用同一条时间线。──
    readonly property real contentSpan: 0.6
    readonly property real contentProgress:
        1 - Math.pow(1 - Math.min(1, progress / contentSpan), 3)

    // ── 相机参数（Issue #949 复核评论 6250932011 问题 3）：默认进场跟随
    //    contentProgress（内容先停）、出场跟随 progress；过渡期收到滚动滚轮时
    //    Canvas 调 retargetCamera() 把相机重锚到那一刻的可见帧、终点换成新目标，
    //    参数改为"从重定进度起的剩余时间"——仍沿这一条进度线继续推进，不重启
    //    进度、不排队第二条动画，圆环（出场）与相机严格同参。──
    property real retargetAt: -1
    readonly property real cameraProgress: {
        if (retargetAt < 0)
            return entering ? contentProgress : progress
        if (retargetAt >= 1)
            return 1
        return Math.max(0, Math.min(1, (progress - retargetAt) / (1 - retargetAt)))
    }

    // ── 节点展开进度（只影响显示层，跟随 contentProgress）──
    readonly property real detailReveal: entering ? contentProgress : 1 - contentProgress
    // 旧画面层的展开进度：进场时旧父图仍是完整节点，出场时它缩回概览圆点。
    readonly property real outgoingReveal: entering ? 1 : 1 - contentProgress

    // 应用侧开关：宿主（StarMapCanvas）可用它整体关掉演出。
    property bool animationsEnabled: true

    // 演出时长：KDE 长时长量级。Kirigami.Units 的时长已经带系统动画速度
    // 因子（KDE 关闭动画时为 0），这里只把"关闭动画 / 有效时长为 0"归一到 0，
    // 让 _begin 直接完成到 1。
    readonly property int effectiveDuration: {
        if (!animationsEnabled)
            return 0
        var d = Kirigami.Units.longDuration
        return d > 0 ? d : 0
    }

    // ── 唯一时间线：一个 NumberAnimation on progress，三部分共用 ──
    property NumberAnimation timeline: NumberAnimation {
        target: motion
        property: "progress"
        from: 0
        to: 1
        duration: motion.effectiveDuration
        easing.type: Easing.OutCubic
        onFinished: motion._onTimelineFinished()
    }

    function _onTimelineFinished() {
        if (!active)
            return
        progress = 1
        active = false
        finished(entering, generation)
    }

    function snapshotOf(camera) {
        if (!camera)
            return null
        return { zoom: camera.zoom, panX: camera.panX, panY: camera.panY }
    }

    // 进场：fromCamera / toCamera 是新活动画布（子图）自己的起止显示镜头，
    // embedGeometry = 入口世界几何 + 它在旧父镜头下的屏幕几何（圆环起点）；
    // 旧父画面的冻结镜头由调用方持有。只存快照，不推导任何业务状态。
    function beginEnter(fromCamera, toCamera, embedGeometry, parentRoute, childRoute) {
        _begin(true, fromCamera, toCamera, embedGeometry, parentRoute, childRoute)
    }

    // 出场：fromCamera / toCamera 是新活动画布（父图）自己的起止显示镜头，
    // embedGeometry 给入口世界几何 + 圆环的起点 / 终点屏幕几何：
    //   - startScreen*：当前显示帧（由当前子镜头反推的父镜头）下入口圆的投影；
    //   - screen*：目标父帧（滚轮 = 反推帧 / 换算后的滚轮目标，按钮 = 恢复的父镜头）
    //     下入口圆投影。
    function beginExit(fromCamera, toCamera, embedGeometry, childRoute, parentRoute) {
        _begin(false, fromCamera, toCamera, embedGeometry, childRoute, parentRoute)
    }

    // 过渡期滚轮重定（Issue #949 复核评论 6250932011 问题 3）：起点 = 处理滚轮
    // 那一刻的可见帧，终点 = 换算后的新目标；进度线本身不重启（圆环 / 展开仍按
    // 原 progress 走完），相机从可见帧连续追。出场圆环端点由 Canvas 随两端一起
    // 重新投影，保证"圆环 = 可见镜头下入口圆的投影"不脱节。
    function retargetCamera(fromCameraIn, toCameraIn, atProgress) {
        if (!active)
            return
        fromCamera = snapshotOf(fromCameraIn)
        toCamera = snapshotOf(toCameraIn)
        retargetAt = Math.max(0, Math.min(1, atProgress))
    }

    function _begin(direction, outgoingCamera, incomingCamera, embedGeometry, outgoingRoute, incomingRoute) {
        timeline.stop()
        entering = direction
        fromCamera = snapshotOf(outgoingCamera)
        toCamera = snapshotOf(incomingCamera)
        fromRoute = outgoingRoute
        toRoute = incomingRoute
        var g = embedGeometry || {}
        instanceId = g.instanceId || ""
        embedCenterX = g.centerX || 0
        embedCenterY = g.centerY || 0
        embedDiameter = g.diameter || 0
        embedScreenX = g.screenX || 0
        embedScreenY = g.screenY || 0
        embedScreenRadius = g.screenRadius || 0
        exitRingStartX = g.startScreenX !== undefined ? g.startScreenX : embedScreenX
        exitRingStartY = g.startScreenY !== undefined ? g.startScreenY : embedScreenY
        exitRingStartRadius = g.startScreenRadius !== undefined
                ? g.startScreenRadius : embedScreenRadius
        exitRingEndX = embedScreenX
        exitRingEndY = embedScreenY
        exitRingEndRadius = embedScreenRadius
        childBounds = g.bounds || null
        fitScale = g.fitScale || 0
        // 新一次演出从默认参数起步（进场 contentProgress / 出场 progress）。
        retargetAt = -1
        active = true
        if (effectiveDuration <= 0) {
            // 系统关闭动画 / 有效时长为 0：直接完成到 1（不留半截演出）。
            _onTimelineFinished()
            return
        }
        progress = 0
        timeline.restart()
    }

    // 取消演出（切图 / 复位 / 收尾）：不写任何业务状态，也不留落点。
    // Issue #949 复核（问题 2）：必须清掉 toCamera —— 否则 active=false 之后
    // 旧落点仍然挂在组件上，下一次进 / 退场开始时被 settle 误当成"本轮的最终
    // 帧"把用户镜头强行复位。停止时间线不会发出 finished，晚到的回调另由
    // Canvas 侧的 generation 兜住。
    function cancel() {
        timeline.stop()
        active = false
        progress = 0
        retargetAt = -1
        fromCamera = null
        toCamera = null
    }
}
