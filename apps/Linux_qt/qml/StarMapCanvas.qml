// =============================================================================
// StarMapCanvas.qml — 星图工作区（当前活动画布的独立镜头 + 唯一输入路由宿主）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：持有星图导航路由（root + segments）、当前活动画布 ID 与活动镜头；
//   只加载 currentStarmapId 的 StarMapActiveScene；菜单宿主、connect 预览
//   overlay、背景网格、唯一输入层宿主。
//
// Issue #929：独立画布协议（对齐 #902 Core 导航 + docs/starmap_viewport.md）。
//   - 任意时刻只有一个"活动画布"：本文件只创建一张 StarMapActiveScene
//     （currentStarmapId），不再把所有子画布递归嵌进根图。
//   - 导航身份 = Core 的 StarMapNavigationRoute（rootStarmapId + 完整 segments）。
//     进入走 enter_starmap_embed，退出走 exit_starmap_canvas，定位活动画布走
//     resolve_starmap_navigation。QML 不自己拼 targetStarmapId 当路由；
//     同一目标图的不同 Embed 入口由完整 route 区分。
//   - 按 routeKey（根 ID + 完整 segments）保存每一层进入前的父相机
//     {zoom, panX, panY}；不以子图 ID 做 key。
//   - 连续缩放穿越：入口投影直径按视口宽高双条件标尺达到 100% 进入子画布；
//     反向缩小到 ≤ 82% 恢复父镜头。进入只在放大方向评估，退出只在缩小
//     方向评估（任意层同规则）；双击 Embed 走同一个 enter 函数，不受方向过滤。
//     阈值只比较"指针瞄准（或已锁定）的那个入口"，zoom 数值本身绝不触发层级切换。
//   - 相机只有数值安全边界（1e-4 ~ 1e5），边界不是层级切换依据。
//   - Core 不保存任何相机 / 视口 / 缩放信息：本文件只在平台层做投影与渲染。
//
// Issue #832：原始输入只有一个主人 StarMapInputRouter。
//   本文件不挂背景 TapHandler / DragHandler / PinchHandler / WheelHandler /
//   MouseArea，只保留：相机数据与纯方法（screen/world 换算、panBy()、zoomAt()）、
//   hitTargetAtScreen() 唯一命中入口、菜单宿主和 connect 预览 overlay。
//
// Issue #987：节点内联编辑浮层由 Canvas 顶层 nodeEditOverlayHost 承载
//   （位于 StarMapInputRouter 之上）；编辑会话与草稿仍只属于活动 Scene。
//   Router 按编辑器实际矩形逐手势仲裁：框内输入归编辑器，框外先提交再继续。
//
// 约束：
//   - 纯渲染和输入宿主层，星图业务逻辑在 Core 与各画布的 StarMapGraphController
//   - 不读写 Core 的相机/视口字段；切换活动画布不改变任何 authored position
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
// Issue #949 复核：平台侧圆形裁剪容器（过渡期把子场景 Loader 裁到圆环内）。
import Sujian 1.0

Item {
    id: canvasArea
    clip: true

    // 根星图 ID：整条导航路由的起点（Core 的 rootStarmapId）。
    property string starmapId: ""
    required property var dt
    required property color canvasBackgroundColor

    readonly property color _primary: dt.primary
    readonly property color _onPrimary: dt.onPrimary
    readonly property color _accent: dt.accent
    readonly property color _accentSoft: dt.accentSoft
    readonly property color _accentHover: dt.accentHover
    readonly property color _border: dt.border
    readonly property color _card: dt.card
    readonly property color _danger: dt.danger
    readonly property color _dangerContainer: dt.dangerContainer
    readonly property color _surfaceContainer: dt.surfaceContainer
    readonly property color _textPrimary: dt.textPrimary
    readonly property color _textSecondary: dt.textSecondary
    readonly property color _textMuted: dt.textMuted
    readonly property color _error: dt.error
    readonly property color _onError: dt.onError
    readonly property color _scrim: dt.scrim
    readonly property int _radiusXs: dt.radiusXs
    readonly property int _radiusSm: dt.radiusSm
    readonly property int _dialogRadius: dt.dialogRadius

    property var starmapBackendRef: null
    property string annotationProjectId: ""
    // Issue #814 评论 5935285879: 整棵星图共享的唯一选中状态控制器。
    property var selectionController: null

    // ── 导航路由（Core 唯一真相）──
    // currentRoute = { rootStarmapId, segments: StarMapPathSegmentDto[] }。
    property var currentRoute: ({ rootStarmapId: "", segments: [] })
    // 由 Core 解析出的当前活动画布 ID。
    property string currentStarmapId: ""
    // 路由解析 / 画布穿越的失败信息（error banner 显示）。
    property string routeError: ""
    // 路由身份：根 ID + 完整 segments。父相机 / 进入上下文都按它保存。
    readonly property string routeKey: routeKeyOf(currentRoute)
    // 是否在根画布。返回按钮据此决定"退出星图工作区"还是"返回父星图"。
    readonly property bool focusIsRoot:
        !currentRoute.segments || currentRoute.segments.length === 0
    readonly property string focusScenePathKey: routeKey
    signal visualFocusChanged()
    // Issue #1014：星图导航树按完整 route 切换活动画布；进入 / 退出子星图时
    // 把当前路由回传给 Workspace，由树同步选中与展开路径。
    signal routeChanged(string rootStarmapId, string segmentsJson, string routeKey)
    // 画布内新建/移除图对象后通知左栏刷新当前分支。
    signal treeStructureChanged()
    // 树对象定位的最终结果；异步等待 Scene ready 后才会发出。
    signal treeObjectRevealFinished(bool success, string objectKey)
    // Issue #982：活动画布图数据/几何可用，供全局搜索按真实就绪时机定位。
    signal sceneGeometryReady(string starmapId, string routeKey)
    signal annotationActionRequested(string action, var targetRef, string title)

    readonly property string errorMessage:
        routeError.length > 0
        ? routeError
        : (activeScene() ? activeScene().errorMessage : "")

    // ── 活动镜头（当前活动画布的唯一相机）──
    // 屏幕坐标 = pan + world × zoom；pan 是世界原点的屏幕位置。
    property real zoomLevel: 1.0
    property real panX: 0
    property real panY: 0
    // 相机比例不设产品上下限：滚轮、按钮、捏合都连续作用在同一台活动镜头上。
    // 只有 Number 真正上溢/下溢时才做数值规范化，规范化不触发任何层级切换。
    readonly property real _zoomFactor: 1.2

    function normalizedZoom(value, fallback) {
        if (value === Infinity)
            return Number.MAX_VALUE
        if (!isFinite(value) || value <= 0)
            return fallback > 0 && isFinite(fallback) ? fallback : 1.0
        return value
    }

    // 相机比例可以连续增大/缩小，但世界坐标乘比例、pan 加减都必须在 Number
    // 可表示范围内收口。超过范围的点本来就在屏幕外，饱和到有限端点不会改变
    // 可见画面，也不会产生 NaN/Infinity 的 pan 或路径坐标。
    function finiteProduct(a, b) {
        var value = a * b
        if (isFinite(value))
            return value
        return (a < 0) !== (b < 0) ? -Number.MAX_VALUE : Number.MAX_VALUE
    }

    function finiteSum(a, b) {
        var value = a + b
        if (isFinite(value))
            return value
        return value === Infinity ? Number.MAX_VALUE : -Number.MAX_VALUE
    }

    function finiteDifference(a, b) {
        var value = a - b
        if (isFinite(value))
            return value
        return value === Infinity ? Number.MAX_VALUE : -Number.MAX_VALUE
    }

    function finiteQuotient(a, b) {
        if (!isFinite(b) || b === 0)
            return 0
        var value = a / b
        if (isFinite(value))
            return value
        return value === Infinity ? Number.MAX_VALUE : -Number.MAX_VALUE
    }

    // routeKey -> { zoom, panX, panY }：每一层进入前保存的父相机。
    property var cameraStack: ({})
    // childRouteKey -> 进入上下文（父/子比例、入口世界几何），退场反推父镜头用。
    property var entryContexts: ({})
    // 最近一次鼠标命中的入口（routeKey/instanceId），用于记录实际 scene_enter。
    property string _zoomEntryKey: ""
    // 捏合穿越补偿：Router 的捏合是"捏合开始 zoom × 累计比例"的绝对值，
    // 画布切换后把这一次绝对值换算成新画布上的连续比例。
    property real _pinchSwitchBaseZoom: -1
    property real _pinchSwitchCanvasZoom: 1
    property bool _initialized: false
    // 等待活动 Scene ready 后完成的树对象定位请求；不能把“尚未就绪”
    // 当成定位失败而立即清掉左栏高亮。
    property var pendingTreeReveal: null

    // 画布穿越阈值使用同一宽高双条件标尺，进入与退出基于同一比值。
    readonly property real _enterRatio: 1.0
    readonly property real _exitRatio: 0.82

    // ── 路由工具 ──
    function routeKeyOf(route) {
        if (!route)
            return ""
        var segs = route.segments || []
        var parts = []
        for (var i = 0; i < segs.length; i++) {
            var s = segs[i]
            if (s.type === "enterPortal")
                parts.push("enterPortal:" + (s.nodeId || ""))
            else
                parts.push("enterEmbed:" + (s.instanceId || ""))
        }
        return (route.rootStarmapId || "") + "|" + parts.join("/")
    }

    function normalizeRoute(raw) {
        if (!raw || !raw.rootStarmapId)
            return null
        var segs = []
        var rawSegs = raw.segments || []
        for (var i = 0; i < rawSegs.length; i++) {
            var s = rawSegs[i]
            segs.push({
                type: s.type,
                instanceId: s.instanceId ? s.instanceId : null,
                nodeId: s.nodeId ? s.nodeId : null
            })
        }
        return { rootStarmapId: raw.rootStarmapId, segments: segs }
    }

    function activeScene() {
        return activeSceneLoader.item
    }

    function annotationRef(kind, objectId) {
        if (!annotationProjectId || !objectId) return null
        return { kind: kind, projectId: annotationProjectId, objectId: String(objectId) }
    }

    function requestAnnotationAction(action, kind, objectId, title) {
        var target = annotationRef(kind, objectId)
        if (target) annotationActionRequested(action, target, String(title || ""))
    }

    // Issue #984：离开活动画布前统一提交节点编辑。
    // 所有会替换 currentStarmapId / 销毁活动 Scene 的导航入口都必须先过这里；
    // 保存失败时留在原画布和编辑器，不允许继续 applyRoute。
    function commitPendingNodeEditBeforeNavigation() {
        var scene = activeScene()
        if (!scene || typeof scene.flushPendingNodeEdit !== "function")
            return true
        if (scene.flushPendingNodeEdit())
            return true
        routeError = qsTr("节点保存失败，请重试")
        return false
    }

    // 对 Workspace / 壳层暴露同一个离开前保存入口。
    function flushPendingNodeEdit() {
        return commitPendingNodeEditBeforeNavigation()
    }

    // Core 定位当前活动画布：route 解析失败不回退到裸目标 ID。
    function applyRoute(route) {
        var normalized = normalizeRoute(route)
        if (!normalized) {
            currentRoute = { rootStarmapId: "", segments: [] }
            currentStarmapId = ""
            routeError = ""
            emitRouteChanged(null)
            return
        }
        currentRoute = normalized
        if (!starmapBackendRef) {
            currentStarmapId = normalized.segments.length === 0 ? normalized.rootStarmapId : ""
            routeError = ""
            emitRouteChanged(normalized)
            return
        }
        var res = starmapBackendRef.resolve_starmap_navigation(
                    normalized.rootStarmapId, JSON.stringify(normalized.segments))
        if (res && res.success === true && res.data && res.data.currentStarmapId) {
            routeError = ""
            currentStarmapId = res.data.currentStarmapId
        } else {
            routeError = qsTr("解析星图导航路由失败")
                    + ((res && res.errorCode) ? " (" + res.errorCode + ")" : "")
            // 根层 route 没有穿越段，退化身份就是根图；子层解析失败则不加载。
            currentStarmapId = normalized.segments.length === 0 ? normalized.rootStarmapId : ""
        }
        emitRouteChanged(normalized)
    }

    function segmentsJsonOf(route) {
        if (!route || !route.segments)
            return "[]"
        var segments = []
        for (var i = 0; i < route.segments.length; ++i) {
            var segment = route.segments[i]
            if (segment.type === "enterPortal") {
                segments.push({
                    type: "enterPortal",
                    instanceId: null,
                    nodeId: segment.nodeId ? segment.nodeId : null
                })
            } else {
                segments.push({
                    type: "enterEmbed",
                    instanceId: segment.instanceId ? segment.instanceId : null,
                    nodeId: null
                })
            }
        }
        return JSON.stringify(segments)
    }

    function emitRouteChanged(route) {
        var normalized = normalizeRoute(route)
        if (!normalized || !normalized.rootStarmapId) {
            routeChanged("", "[]", "")
            return
        }
        routeChanged(normalized.rootStarmapId,
                     segmentsJsonOf(normalized),
                     routeKeyOf(normalized))
    }

    // 左栏 + 入口：在当前活动画布创建节点或子星图，不再创建独立根星图。
    function openCreateDialog(mode) {
        if (mode !== "node" && mode !== "starmap")
            return
        menuOwnerContent = activeScene()
        if (!menuOwnerContent)
            return
        createDialog.open(mode, width / 2, height / 2)
    }

    function treeRevealMatches(pending) {
        return pending
            && String(currentRoute.rootStarmapId || "") === String(pending.rootStarmapId || "")
            && routeKey === String(pending.routeKey || "")
    }

    function cancelPendingTreeReveal() {
        if (!pendingTreeReveal)
            return
        var objectKey = String(pendingTreeReveal.objectKey || "")
        pendingTreeReveal = null
        treeObjectRevealFinished(false, objectKey)
    }

    // 活动 Scene 由 Loader 异步加载；只有 graphData 就绪且 route 完全匹配
    // 时才执行命中/选中/居中。等待期间保留 pending，不误报失败。
    function completeTreeReveal() {
        if (!pendingTreeReveal)
            return false
        var pending = pendingTreeReveal
        if (!treeRevealMatches(pending)) {
            cancelPendingTreeReveal()
            return null
        }
        var scene = activeScene()
        if (!scene || !scene.ready)
            return false
        if (String(scene.scenePathKey || "") !== String(pending.routeKey || "")
                || String(scene.starmapId || "") !== String(currentStarmapId || ""))
            return false
        var objectId = String(pending.objectId || "")
        var kind = String(pending.kind || "")
        var item = kind === "embed" ? scene.hitEmbed(objectId) : scene.hitNode(objectId)
        if (!item) {
            // Scene 已 ready 却找不到对象，说明目标已失效；结束等待并回报失败。
            pendingTreeReveal = null
            treeObjectRevealFinished(false, String(pending.objectKey || ""))
            return null
        }
        if (kind === "embed")
            scene.selectEmbed(objectId)
        else
            scene.selectNode(objectId)
        var center = scene.itemCenterScene(kind === "embed" ? "embed" : "node", objectId)
        if (center) {
            applyPan(finiteDifference(width / 2, finiteProduct(center.x, zoomLevel)),
                     finiteDifference(height / 2, finiteProduct(center.y, zoomLevel)))
        }
        pendingTreeReveal = null
        treeObjectRevealFinished(true, String(pending.objectKey || ""))
        return true
    }

    // Issue #1018：树中普通对象行定位。先进入该对象所在画布，再等待 Scene
    // ready 后命中、选中并居中；不复用会强制回根图的 revealSearchTarget()。
    // 返回值只表示“已接受定位请求”，最终结果由 treeObjectRevealFinished 发出。
    function revealTreeObject(rootStarmapId, segmentsJson, kind, objectId, objectKey) {
        if (!rootStarmapId || !objectId)
            return false
        if (kind !== "node" && kind !== "portal" && kind !== "embed")
            return false
        var segments = []
        try {
            segments = JSON.parse(String(segmentsJson || "[]"))
        } catch (e) {
            return false
        }
        if (!Array.isArray(segments))
            return false
        var targetSegments = segments
        if (kind === "embed" || kind === "portal")
            targetSegments = segments.slice(0, Math.max(0, segments.length - 1))
        var targetRoute = normalizeRoute({
            rootStarmapId: String(rootStarmapId),
            segments: targetSegments
        })
        if (!targetRoute)
            return false
        cancelPendingTreeReveal()
        if (!navigateToTreeRoute(rootStarmapId, JSON.stringify(targetSegments),
                                 routeKeyOf(targetRoute)))
            return false
        pendingTreeReveal = {
            rootStarmapId: String(rootStarmapId),
            routeKey: routeKeyOf(targetRoute),
            kind: String(kind),
            objectId: String(objectId),
            objectKey: String(objectKey || "")
        }
        // null 表示 Scene 已就绪但目标已失效；false 表示还在等待 ready。
        return completeTreeReveal() !== null
    }

    // Issue #1014：树导航入口。先让 Core 解析 root + 完整 segments，解析失败
    // 不切换画布；成功时先提交当前节点编辑，再复用 applyRoute / cameraStack。
    function navigateToTreeRoute(rootStarmapId, segmentsJson, routeKey) {
        if (!rootStarmapId)
            return false
        var parsedSegments = []
        try {
            parsedSegments = JSON.parse(String(segmentsJson || "[]"))
        } catch (e) {
            return false
        }
        if (!Array.isArray(parsedSegments))
            return false
        var route = normalizeRoute({
            rootStarmapId: String(rootStarmapId),
            segments: parsedSegments
        })
        if (!route)
            return false
        var targetKey = routeKeyOf(route)
        if (routeKey && String(routeKey) !== targetKey)
            return false
        if (starmapBackendRef) {
            var resolved = starmapBackendRef.resolve_starmap_navigation(
                        route.rootStarmapId, JSON.stringify(route.segments))
            if (!resolved || resolved.success !== true || !resolved.data
                    || !resolved.data.currentStarmapId) {
                routeError = qsTr("解析星图导航路由失败")
                        + ((resolved && resolved.errorCode) ? " (" + resolved.errorCode + ")" : "")
                return false
            }
        }
        if (targetKey === routeKey && currentStarmapId !== "")
            return true
        if (!commitPendingNodeEditBeforeNavigation())
            return false
        settleViewportMotion()
        prepareCanvasSwitch()
        applyRoute(route)
        var cachedCamera = cameraStack[targetKey]
        applyCamera(cachedCamera ? cachedCamera : defaultCamera())
        resetZoomRound()
        storeCamera(targetKey, cameraSnapshot())
        visualFocusChanged()
        return true
    }

    function defaultCamera() {
        return { zoom: 1.0, panX: 0, panY: 0 }
    }

    function cameraSnapshot() {
        return { zoom: zoomLevel, panX: panX, panY: panY }
    }

    function storeCamera(key, cam) {
        if (!key)
            return
        var next = {}
        for (var k in cameraStack)
            next[k] = cameraStack[k]
        next[key] = { zoom: cam.zoom, panX: cam.panX, panY: cam.panY }
        cameraStack = next
    }

    function storeEntryContext(key, ctx) {
        if (!key)
            return
        var next = {}
        for (var k in entryContexts)
            next[k] = entryContexts[k]
        next[key] = ctx
        entryContexts = next
    }

    function applyCamera(cam) {
        if (!cam)
            return
        zoomLevel = cam.zoom
        panX = cam.panX
        panY = cam.panY
    }

    // 切回根图：清所有层相机，镜头回默认。
    function resetToRoot() {
        cancelPendingTreeReveal()
        if (!commitPendingNodeEditBeforeNavigation())
            return false
        settleViewportMotion()
        cameraStack = ({})
        entryContexts = ({})
        _zoomEntryKey = ""
        _pinchSwitchBaseZoom = -1
        _pinchSwitchCanvasZoom = 1
        var route = { rootStarmapId: starmapId, segments: [] }
        applyRoute(route)
        applyCamera(defaultCamera())
        resetZoomRound()
        storeCamera(routeKeyOf(route), cameraSnapshot())
        visualFocusChanged()
        return true
    }

    Component.onCompleted: {
        _initialized = true
        resetToRoot()
    }
    onStarmapIdChanged: {
        if (_initialized)
            resetToRoot()
    }
    onStarmapBackendRefChanged: {
        // 后端晚于 starmapId 注入时补一次路由解析。
        // 若此时已有活动画布正在编辑，也必须先保存成功再替换 Scene。
        if (_initialized && currentRoute.rootStarmapId !== ""
                && commitPendingNodeEditBeforeNavigation())
            applyRoute(currentRoute)
    }

    // 路由变化后旧的树定位请求已经失效，必须清掉，不能拿新画布误命中。
    onRouteChanged: {
        if (pendingTreeReveal && !treeRevealMatches(pendingTreeReveal))
            cancelPendingTreeReveal()
    }

    // 相机任何变化都写回当前 route 的相机槽（同一张画布再次进入时可用）。
    onZoomLevelChanged: storeCamera(routeKey, cameraSnapshot())
    onPanXChanged: storeCamera(routeKey, cameraSnapshot())
    onPanYChanged: storeCamera(routeKey, cameraSnapshot())

    // ── 纯方法：平移 / 缩放 / 坐标换算 ──
    function applyPan(nextX, nextY) {
        panX = isFinite(nextX) ? nextX : (nextX === Infinity ? Number.MAX_VALUE : -Number.MAX_VALUE)
        panY = isFinite(nextY) ? nextY : (nextY === Infinity ? Number.MAX_VALUE : -Number.MAX_VALUE)
    }

    function panBy(dx, dy) {
        applyPan(finiteSum(panX, dx), finiteSum(panY, dy))
    }

    // world ↔ 屏幕换算：优先用活动场景的投影（视口中心相对原点），
    // 没有活动场景时用同一数学口径的直接式。
    function worldToScreenX(wx) {
        var scene = activeScene()
        if (scene)
            return scene.worldToScreenX(wx)
        return finiteSum(panX, finiteProduct(wx, zoomLevel))
    }
    function worldToScreenY(wy) {
        var scene = activeScene()
        if (scene)
            return scene.worldToScreenY(wy)
        return finiteSum(panY, finiteProduct(wy, zoomLevel))
    }
    function screenToWorldX(sx) {
        var scene = activeScene()
        if (scene)
            return scene.screenToWorldX(sx)
        return (sx - panX) / (zoomLevel > 0 ? zoomLevel : 1)
    }
    function screenToWorldY(sy) {
        var scene = activeScene()
        if (scene)
            return scene.screenToWorldY(sy)
        return (sy - panY) / (zoomLevel > 0 ? zoomLevel : 1)
    }

    readonly property real _edgeLineWorldWidth: 2
    readonly property real _edgeScreenStrokeWidth:
        Math.max(1.0, Math.min(_edgeLineWorldWidth * zoomLevel, 6.0))

    // Issue #949：连续缩放的演出状态。只属于当前显示帧，不是第二套相机模型：
    //   - _zoomAnchorX/Y 是本轮缩放的屏幕锚点（滚轮 / 捏合 / 按钮的指针位置）；
    //   - _zoomRoundWorldX/Y 是锚点的 world 位置，用"正在显示的帧"现算，
    //     于是 panX(t)=anchorX-worldX*z(t)、panY(t)=anchorY-worldY*z(t)，
    //     鼠标指向的对象在整段动画里不会滑走；
    //   - _zoomTargetLevel 是滚轮累计出的目标比例（一档一档连续累积，而不是
    //     每次从还没走完的显示帧重新硬跳）；换层时按 entryContexts 的父子比例
    //     换算到新层尺度（Issue #949 复核评论 6250932011），过渡期滚轮继续重定它；
    //   - 补间只有一个：连续滚轮只重定它，不排队堆积互相打断的补间。
    property real _zoomAnchorX: 0
    property real _zoomAnchorY: 0
    property real _zoomRoundWorldX: 0
    property real _zoomRoundWorldY: 0
    property real _zoomTargetLevel: 1.0
    // 滚轮累计的目标比例（下一档从它继续乘，滚轮手感连续）。
    readonly property real zoomTargetLevel: _zoomTargetLevel
    // 只由缩放补间写的显示帧值；变化时推进显示镜头。
    property real zoomTweenValue: 1.0

    // 唯一缩放入口：滚轮、捏合、+/- 按钮都走这里，只改当前活动画布的镜头。
    // Issue #949：这里只记"累计目标 + 屏幕锚点"，显示镜头由单一补间从**正在
    // 显示的**帧连续插值；随后按"指针瞄准的入口"评估画布穿越，
    // zoom 数值本身不参与判断，判定一律用正在显示的投影直径：放大只评估进场、
    // 缩小只评估退场（缩小经过入口不进入子画布），任意层同规则，双击不受方向过滤。
    function zoomAt(screenX, screenY, nextZoom) {
        var pinchInput = interaction && interaction.pointerMode === "pinch"
        if (viewportMotion.active && pinchInput) {
            // 捏合实时跟手：先收尾演出（保留当前正在显示的帧），再从这一帧继续。
            // Issue #949 复核（问题 1/2）：配对基准随之锚定到**当前显示帧**的
            // zoom —— 演出被打断时显示帧还在预览与落点之间，用落点当基准会跳。
            settleViewportMotion()
            if (_pinchSwitchBaseZoom > 0)
                _pinchSwitchCanvasZoom = zoomLevel
        }
        var requested = nextZoom
        if (pinchInput && _pinchSwitchBaseZoom > 0)
            requested = _pinchSwitchCanvasZoom * (nextZoom / _pinchSwitchBaseZoom)
        var target = normalizedZoom(requested, zoomLevel)
        var oldZoom = zoomLevel
        if (target === oldZoom && !zoomTween.running)
            return
        var zoomingIn = target > oldZoom
        var zoomingOut = target < oldZoom
        _zoomTargetLevel = target
        if (!zoomingIn && !zoomingOut) {
            stopZoomTween()
            return
        }
        if (viewportMotion.active) {
            // 演出进行中（滚轮）：不能冻结整个 longDuration —— 继续改这台可见
            // 相机：把新目标按"当前显示帧 + 本次鼠标屏幕锚点"算出来，沿同一条
            // 演出进度线从可见帧连续推进（不排队第二条动画）。
            retargetViewportMotion(screenX, screenY)
            return
        }
        if (pinchInput || zoomTweenDuration() <= 0) {
            // 捏合实时跟手 / 关闭动画：不插值，本帧直接落到目标帧，穿越按正在
            // 显示的投影直径评估（放大只进场、缩小只退场）。
            // Issue #949 复核（问题 1）：必须**先**用本次事件 / 质心坐标把锚点
            // 的世界位置从"正在显示的帧"现算出来，再改 zoom / pan。沿用上一次
            // resetZoomRound() 写入的锚点会让触点一偏离屏幕中心就整张图飞走
            //（例如屏宽 1000、初始 panX=0、锚点 world=500，在 x=100 捏合到 2 时
            // panX 应为 100-100×2=-100，不是 100-500×2=-900）。
            stopZoomTween()
            anchorRoundAt(screenX, screenY)
            zoomLevel = target
            applyPan(finiteDifference(screenX, finiteProduct(_zoomRoundWorldX, target)),
                     finiteDifference(screenY, finiteProduct(_zoomRoundWorldY, target)))
            var switched = -1
            if (zoomingIn) {
                switched = maybeEnterEmbedAt(screenX, screenY)
            } else if (zoomingOut) {
                switched = maybeExitCanvas()
            }
            if (switched > 0 && pinchInput) {
                // 基准配对（#929）：nextZoom 是旧基准累计值；记新画布 zoomLevel
                // 会让下一帧 requested = switched * (next / switched) 换算失效。
                _pinchSwitchBaseZoom = nextZoom
                _pinchSwitchCanvasZoom = switched
            }
            return
        }
        startZoomTween(target, screenX, screenY)
    }

    // 显示镜头每帧的唯一推进：先确定 z(t)，再按本轮锚点的世界位置反推 pan，
    // 然后只用**正在显示**的投影直径评估画布穿越（放大只进场、缩小只退场）。
    // 命中与穿越因此永远与看得见的画面一致，绝不从尚未显示的未来缩放值跳过。
    function applyDisplayedZoom(target, screenX, screenY) {
        target = normalizedZoom(target, zoomLevel)
        var previous = zoomLevel
        if (target === previous)
            return -1
        zoomLevel = target
        applyPan(finiteDifference(screenX, finiteProduct(_zoomRoundWorldX, target)),
                 finiteDifference(screenY, finiteProduct(_zoomRoundWorldY, target)))
        var zoomingIn = target > previous
        var zoomingOut = target < previous
        var switched = -1
        if (zoomingIn) {
            switched = maybeEnterEmbedAt(screenX, screenY)
        } else if (zoomingOut) {
            switched = maybeExitCanvas()
        }
        return switched
    }

    // 锚点世界位置：用"当前显示帧"现算一次。演出 / 切画布之后锚点不会指向
    // 错误的世界坐标（滚轮记锚点、补间起步前都走它）。
    function anchorRoundAt(screenX, screenY) {
        var z = zoomLevel > 0 ? zoomLevel : 1
        _zoomAnchorX = screenX
        _zoomAnchorY = screenY
        _zoomRoundWorldX = finiteQuotient(finiteDifference(screenX, panX), z)
        _zoomRoundWorldY = finiteQuotient(finiteDifference(screenY, panY), z)
    }

    // 同一屏幕锚点在给定 zoom 下推出来的相机（锚点 world 位置用"当前显示帧"现算）：
    // 未完成滚轮目标的跨画布换算、过渡期滚轮重定都走它（Issue #949 复核评论
    // 6250932011 问题 2/3）。
    function anchoredCameraAt(zoom, screenX, screenY) {
        var normalized = normalizedZoom(zoom, zoomLevel)
        var z = zoomLevel > 0 ? zoomLevel : 1
        var worldX = finiteQuotient(finiteDifference(screenX, panX), z)
        var worldY = finiteQuotient(finiteDifference(screenY, panY), z)
        return { zoom: normalized,
                 panX: finiteDifference(screenX, finiteProduct(worldX, normalized)),
                 panY: finiteDifference(screenY, finiteProduct(worldY, normalized)) }
    }

    // 过渡期滚轮：从**当前可见帧**连续推进到新目标（Issue #949 复核评论
    // 6250932011 问题 3）。终点 = 累计滚轮目标在当前新层尺度上、仍锚在同一鼠标
    // 屏幕位置的那台相机；两端交给唯一演出进度线（retargetCamera），相机沿
    // "重定那一刻起的剩余时间"从可见帧继续追，不冻结、不排队第二条动画。
    // 出场圆环端点随父相机两端重新投影，保证"圆环 = 可见镜头下入口圆的投影"。
    function retargetViewportMotion(screenX, screenY) {
        if (!viewportMotion.active || !(_zoomTargetLevel > 0))
            return
        var fromCam = cameraSnapshot()
        var toCam = anchoredCameraAt(_zoomTargetLevel, screenX, screenY)
        viewportMotion.retargetCamera(fromCam, toCam, viewportMotion.progress)
        if (!viewportMotion.entering) {
            var startGeo = embedScreenGeometry(fromCam, viewportMotion.embedCenterX,
                                               viewportMotion.embedCenterY,
                                               viewportMotion.embedDiameter)
            var endGeo = embedScreenGeometry(toCam, viewportMotion.embedCenterX,
                                             viewportMotion.embedCenterY,
                                             viewportMotion.embedDiameter)
            viewportMotion.exitRingStartX = startGeo.screenX
            viewportMotion.exitRingStartY = startGeo.screenY
            viewportMotion.exitRingStartRadius = startGeo.screenRadius
            viewportMotion.exitRingEndX = endGeo.screenX
            viewportMotion.exitRingEndY = endGeo.screenY
            viewportMotion.exitRingEndRadius = endGeo.screenRadius
        }
        anchorRoundAt(screenX, screenY)
        applyMotionFrame()
    }

    // 缩放补间：只有这一条，每次重定起点（当前显示帧）与目标，不排队。
    NumberAnimation {
        id: zoomTween
        target: canvasArea
        property: "zoomTweenValue"
        easing.type: Easing.OutCubic
    }

    onZoomTweenValueChanged: applyDisplayedZoom(zoomTweenValue, _zoomAnchorX, _zoomAnchorY)

    function startZoomTween(target, screenX, screenY) {
        anchorRoundAt(screenX, screenY)
        var duration = zoomTweenDuration()
        if (duration <= 0) {
            applyDisplayedZoom(target, screenX, screenY)
            return
        }
        zoomTween.stop()
        zoomTween.from = zoomLevel
        zoomTween.to = target
        zoomTween.duration = duration
        zoomTween.restart()
    }

    function stopZoomTween() {
        if (zoomTween.running)
            zoomTween.stop()
    }

    // KDE 一致性：缩放用 shortDuration 量级；关闭动画时 0（直接落到目标帧）。
    function zoomTweenDuration() {
        if (!viewportMotion.animationsEnabled)
            return 0
        return Kirigami.Units.shortDuration
    }

    // 切画布 / 复位后，本轮缩放的锚点与目标属于旧画布：整体复位到新显示帧。
    function resetZoomRound() {
        stopZoomTween()
        _zoomTargetLevel = zoomLevel
        _zoomAnchorX = width / 2
        _zoomAnchorY = height / 2
        _zoomRoundWorldX = screenToWorldX(width / 2)
        _zoomRoundWorldY = screenToWorldY(height / 2)
    }

    // ---------------------------------------------------------------------------
    // Issue #949：画布穿越的过渡演出（只影响显示层）
    //   - 唯一时间线在 StarMapViewportMotion 里；本文件只按同一个 progress
    //     逐帧推进镜头（applyCamera），命中因此永远换算自"看得见的当前镜头"；
    //   - route / 相机真相仍是 currentRoute + 镜头标量 + cameraStack；演出不写
    //     Core、不新建第二套导航，也不为动画另建图数据；
    //   - 旧活动场景在过渡期间以冻结身份 / 镜头继续绘制（只绘制、禁交互），
    //     圆壳描边由真实 Embed 随镜头继续放大，直到整圈离开可见视口才结束
    //     显示：阈值切换瞬间绝不把入场的圆壳描边关掉。
    // ---------------------------------------------------------------------------
    StarMapViewportMotion {
        id: viewportMotion
        viewportWidth: canvasArea.width
        viewportHeight: canvasArea.height
        onProgressChanged: canvasArea.applyMotionFrame()
        onFinished: function(entering, generation) {
            canvasArea.finishViewportMotion(entering, generation)
        }
    }

    // 过渡期间的旧画面层身份 + 镜头（Loader 直接绑这些标量，逐帧刷新）。
    // 旧画面的镜头不再做端点独立插值：每帧由 applyMotionFrame 从当前
    // 子镜头 / 父镜头按同一份父子对应推出来。
    property var outgoingRoute: null
    property string outgoingStarmapId: ""
    property real outgoingZoom: 1
    property real outgoingPanX: 0
    property real outgoingPanY: 0
    // 本次过渡用的父子镜头对应（entryContexts 里的那一份快照）。
    property var motionContext: null
    // 防晚到回调：每次启动新演出都 ++，旧时间线的 finished 不再生效。
    property int motionGeneration: 0

    readonly property string outgoingRouteKey: outgoingRoute ? routeKeyOf(outgoingRoute) : ""

    // 显示层派生：圆环放大描边只画在"圆内"的那一层（进场 = 活动子画布，
    // 出场 = 旧子画面层）；母图侧的真实圆壳由 transitionEmbedInstanceId 标出。
    readonly property bool activeSceneIsInsideRing:
        viewportMotion.active && viewportMotion.entering
    readonly property bool outgoingSceneIsInsideRing:
        viewportMotion.active && !viewportMotion.entering
    readonly property real activeDetailReveal:
        viewportMotion.active && viewportMotion.entering ? viewportMotion.detailReveal : 1
    readonly property real outgoingDetailReveal:
        viewportMotion.active ? viewportMotion.outgoingReveal : 1
    readonly property string motionFocusInstanceId:
        viewportMotion.active ? viewportMotion.instanceId : ""

    // 父 ↔ 子镜头对应（与 enterEmbedViaCore / exitCanvasViaCore 的连续换算同一份
    // 比例）：子镜头 = 父镜头 × ratio，子视口中心 = 父视口中心在子图 bounds 坐标
    // 里的对应点。进出场共用它，双击进入和滚轮穿越因此是同一份空间关系。
    function childCameraForParent(parentCam, ctx) {
        if (!parentCam || !ctx || !(ctx.ratio > 0))
            return parentCam
        var zoom = normalizedZoom(parentCam.zoom * ctx.ratio, parentCam.zoom)
        var screenX = finiteSum(parentCam.panX, finiteProduct(ctx.embedCenterX, parentCam.zoom))
        var screenY = finiteSum(parentCam.panY, finiteProduct(ctx.embedCenterY, parentCam.zoom))
        return { zoom: zoom,
                 panX: finiteDifference(screenX, finiteProduct(ctx.boundsCenterX, zoom)),
                 panY: finiteDifference(screenY, finiteProduct(ctx.boundsCenterY, zoom)) }
    }

    function parentCameraForChild(childCam, ctx) {
        if (!childCam || !ctx || !(ctx.ratio > 0))
            return childCam
        var zoom = normalizedZoom(childCam.zoom / ctx.ratio, childCam.zoom)
        var screenX = finiteSum(childCam.panX, finiteProduct(ctx.boundsCenterX, childCam.zoom))
        var screenY = finiteSum(childCam.panY, finiteProduct(ctx.boundsCenterY, childCam.zoom))
        return { zoom: zoom,
                 panX: finiteDifference(screenX, finiteProduct(ctx.embedCenterX, zoom)),
                 panY: finiteDifference(screenY, finiteProduct(ctx.embedCenterY, zoom)) }
    }

    function cameraLerp(from, to, p) {
        if (!from)
            return to
        if (!to)
            return from
        return { zoom: finiteSum(from.zoom, finiteProduct(finiteDifference(to.zoom, from.zoom), p)),
                 panX: finiteSum(from.panX, finiteProduct(finiteDifference(to.panX, from.panX), p)),
                 panY: finiteSum(from.panY, finiteProduct(finiteDifference(to.panY, from.panY), p)) }
    }

    // 子图正常可读 / 可交互的镜头（进场落点，Issue #949 复核评论 6250468319，
    // 口径经复核评论 6250932011（问题 1）确认保持）：
    //   1. 优先沿用该子图已有的合法镜头（同一 routeKey 的相机槽，曾经进去过），
    //      回到用户上次看到的位置和倍率；
    //   2. 否则用进入时的合理默认：作者尺度 100%（节点卡片按标题 / 正文度量，
    //      正文预览保持可读）并把子图内容包围盒居中——不从"圆环需要撑出可见视口"的倍数
    //      反算，也不默认 fit 整图（星图可无限扩展，节点距离太远时 fit 全图会
    //      让节点小得难读）；空图按 boundsCenter 0 的原默认中心处理。
    function readableChildCamera(childKey, context) {
        var saved = childKey ? cameraStack[childKey] : null
        if (saved && saved.zoom > 0)
            return { zoom: saved.zoom, panX: saved.panX, panY: saved.panY }
        var zoom = 1.0
        var cx = context ? context.boundsCenterX : 0
        var cy = context ? context.boundsCenterY : 0
        return { zoom: zoom, panX: width / 2 - cx * zoom, panY: height / 2 - cy * zoom }
    }

    // 入口圆壳在给定镜头下的屏幕几何（世界直径恒定 200，屏幕半径随镜头放大；
    // 进场时圆环从这里起步，出场时缩回这里后由真实圆壳接管）。
    function embedScreenGeometry(cam, centerX, centerY, diameter) {
        var z = cam && cam.zoom > 0 ? cam.zoom : 1
        return { screenX: finiteSum(cam.panX, finiteProduct(centerX, z)),
                 screenY: finiteSum(cam.panY, finiteProduct(centerY, z)),
                 screenRadius: Math.max(diameter, 1) * z / 2 }
    }

    // 启动一次过渡：route / 相机真相已由调用方确定，这里只准备展示快照。
    // outgoing* = 旧活动场景（冻结身份，只绘制、禁交互），
    // viewportMotion = 新活动画布从 fromCamera 连续插值到 toCamera。
    // Issue #949 复核评论 6250468319：进出场都是**真正的镜头连续变换**——
    //   - 进场：新子场景 from = 入口预览相机（子图内容按父图当前显示帧里
    //     Embed 圆壳的投影映射），to = 调用方给的子图目标镜头（没有未完成滚轮
    //     目标时就是可读镜头，不从"圆环需要撑出视口"的倍率反算）；内容只在前段
    //     （contentProgress）走到目标，圆环继续按完整 progress 放大退场；旧父画面层
    //     不做端点独立插值，而是每帧用 parentCameraForChild(子镜头(t)) 推同一份对应。
    //     每帧子镜头 = cameraLerp(预览, 可读, contentProgress)：to 是包围盒
    //     居中的可读镜头时等价于 pan(t)=ringCenter(cp)−W0×zoom(cp)（W0 =
    //     入口圆心对应的子图世界点），内容锚点沿"入口原屏幕位置 → 视口中心"
    //     的直线移动，不会先把新子图摆到全屏中心再播放动画。
    //   - 出场：新父场景 from = 从当前子镜头反推的父镜头（零跳变）；to = 滚轮
    //     时由未完成目标换算出的父镜头（相机继续缩小），按钮 = 进入前保存的
    //     父镜头；旧子画面层每帧用 childCameraForParent(父镜头(t)) 推；圆环
    //     起点 / 终点就是入口圆在这两个父帧下的投影（父相机线性插值下端点圆
    //     线性插值 = Embed 投影，共用同一个相机参数）。
    //   - Issue #949 复核评论 6250932011（问题 2）：未完成滚轮目标由调用方换算好
    //     传进 incomingCam，这里只把屏幕锚点固定到同一鼠标位置（没有 pending
    //     时回落视口中心），不再无条件重置掉它。
    function beginCanvasTransition(entering, outgoingStarmapIdIn, outgoingRouteIn,
                                   outgoingCam, incomingCam, geometry, context,
                                   pendingAnchor) {
        viewportMotion.generation = ++motionGeneration
        stopZoomTween()
        outgoingStarmapId = outgoingStarmapIdIn
        outgoingRoute = outgoingRouteIn
        motionContext = context
        // Issue #949 复核评论 6250932011（问题 2）：incomingCam 已由调用方按
        // 父子比例换算好（本层还有没显示完的滚轮目标时 = 换算到新层尺度的目标
        // 本身，不是落点），这里不再无条件重置成落点；屏幕锚点也沿用处理滚轮
        // 时的同一鼠标位置，后续滚轮与收尾都从这台可见镜头继续推。
        _zoomTargetLevel = incomingCam.zoom
        _zoomAnchorX = pendingAnchor ? pendingAnchor.x : width / 2
        _zoomAnchorY = pendingAnchor ? pendingAnchor.y : height / 2
        if (entering) {
            // 进场：起点 = 入口内同一片坐标（几何精确的概览 fit 映射），
            // 落点 = 调用方给的子图目标镜头（可读镜头 / 换算后的滚轮目标）；
            // 圆环从入口在旧父镜头下的屏幕几何放大到整圈离开视口。
            var previewCam = context && context.ratio > 0
                    ? childCameraForParent(outgoingCam, context) : incomingCam
            viewportMotion.beginEnter(previewCam, incomingCam, geometry, outgoingRouteIn,
                                      normalizeRoute(currentRoute))
        } else {
            // 出场：新活动画布是父图，从"当前子内容对应的父镜头"接上（滚轮
            // 目标 = 未完成滚轮目标换算出的父镜头，相机继续缩小；按钮再平滑
            // 走到 cameraStack 保存的父镜头）；圆环起点 = 这一帧下入口圆的
            // 投影，终点 = 目标父帧下入口圆投影。
            var parentStartCam = context && context.ratio > 0
                    ? parentCameraForChild(outgoingCam, context) : outgoingCam
            var g = geometry || {}
            var exitGeometry = {}
            for (var k in g)
                exitGeometry[k] = g[k]
            if (context && context.ratio > 0 && g.diameter > 0) {
                var startGeo = embedScreenGeometry(parentStartCam, g.centerX, g.centerY, g.diameter)
                exitGeometry.startScreenX = startGeo.screenX
                exitGeometry.startScreenY = startGeo.screenY
                exitGeometry.startScreenRadius = startGeo.screenRadius
            } else {
                // 没有进入上下文的退化路径：圆环从"整圈覆盖可见视口"收回，
                // 与进场镜像，至少不出现零跳变。
                exitGeometry.startScreenX = viewportMotion.ringTargetX
                exitGeometry.startScreenY = viewportMotion.ringTargetY
                exitGeometry.startScreenRadius = viewportMotion.ringCornerDistance
                        + Math.max(g.screenRadius || 0, 8)
            }
            viewportMotion.beginExit(parentStartCam, incomingCam, exitGeometry,
                                      outgoingRouteIn, normalizeRoute(currentRoute))
        }
        if (!viewportMotion.active) {
            // 关闭动画 / 有效时长为 0：直接落在最终状态，不留半截演出。
            finishViewportMotion(entering)
            return
        }
        applyMotionFrame()
        // 锚点的 world 位置用新层第一帧现算：滚轮 / 收尾从这台可见镜头继续推。
        anchorRoundAt(_zoomAnchorX, _zoomAnchorY)
        logInteraction(entering ? "viewport_enter" : "viewport_exit", "scene", "", {
            "instanceId": geometry ? geometry.instanceId : "",
            "duration": viewportMotion.effectiveDuration,
            "toRouteKey": routeKey
        })
    }

    // 过渡每帧：活动画布镜头按唯一演出进度线推进（默认进场 contentProgress
    // ——内容先停；出场 progress；过渡期滚轮重定后按 cameraProgress 从可见帧
    // 继续追），旧画面层不做端点独立插值，而是每帧用同一份父子镜头对应从
    // **当前子镜头 / 当前父镜头**推出来——圆壳、内部内容、镜头因此始终对齐
    //（Issue #949 复核评论 6250468319 / 6250932011）。
    function applyMotionFrame() {
        if (!viewportMotion.active)
            return
        var previousZoom = zoomLevel
        var camProgress = viewportMotion.cameraProgress
        if (viewportMotion.entering) {
            var childCam = cameraLerp(viewportMotion.fromCamera, viewportMotion.toCamera,
                                      camProgress)
            applyCamera(childCam)
            if (outgoingStarmapId && motionContext) {
                var parentCam = parentCameraForChild(childCam, motionContext)
                outgoingZoom = parentCam.zoom
                outgoingPanX = parentCam.panX
                outgoingPanY = parentCam.panY
            }
        } else {
            var nextParentCam = cameraLerp(viewportMotion.fromCamera, viewportMotion.toCamera,
                                           camProgress)
            applyCamera(nextParentCam)
            if (outgoingStarmapId && motionContext) {
                var nextChildCam = childCameraForParent(nextParentCam, motionContext)
                outgoingZoom = nextChildCam.zoom
                outgoingPanX = nextChildCam.panX
                outgoingPanY = nextChildCam.panY
            }
        }
        checkTransitionThresholds(previousZoom)
    }

    // Issue #949 复核评论 6251370089：动画进行中滚轮仍按当前投影阈值换层。
    // 过渡期滚轮只把相机重锚到可见帧（retargetViewportMotion），这里在每帧相机
    // 推进之后按**上一帧/本帧实际显示的 zoom** 的方向重跑既有的 maybeEnterEmbedAt
    // / maybeExitCanvas 判定——不得按尚未显示的 pending 目标判定，方向由显示帧的
    // 变化决定。只对收到过滚轮重定的演出生效（userRetargeted）：按钮返回 /
    // 双击进入这类纯演出自身的运动不是滚动意图，不参与换层判定。
    // 命中后走的就是现有路径：maybeExit / maybeEnter 内部先 settle 取消本过渡、
    // 保留这一刻的镜头与鼠标锚点，再走 Core route + beginCanvasTransition 开启
    // 新演出；新演出起点 _begin 会复位 userRetargeted，所以"相机更新、方向阈值
    // 判定、Core 切换"这条链在一次启动里只发生一次，不会在 applyMotionFrame
    // 启动下一次演出期间重复触发自己。同方向尚未越界时两个 maybe* 均返回 -1，
    // 演出照常继续（只 retarget 相机）。
    function checkTransitionThresholds(previousZoom) {
        if (!viewportMotion.active || !viewportMotion.userRetargeted)
            return
        if (!(previousZoom > 0) || !(zoomLevel > 0))
            return
        var delta = zoomLevel - previousZoom
        if (Math.abs(delta) <= previousZoom * 1e-6)
            return
        if (delta > 0)
            maybeEnterEmbedAt(_zoomAnchorX, _zoomAnchorY)
        else
            maybeExitCanvas()
    }

    // 过渡自然收尾（唯一时间线跑到 progress=1 的 finished 回调）：落最终镜头、
    // 销毁临时层，不持久化、不写 Core。Issue #949 复核评论 6250932011（问题 2）：
    // 新层目标在换层时已按父子比例换算好（过渡期间滚轮还会继续重定它），这里
    // 只用新层自己的值兜底续走，不再拿旧层倍率 / 落点覆盖它。晚到的旧时间线
    // （generation 不匹配）直接忽略，不许把用户拉回上一层。
    // Issue #949 复核（问题 2）：只有这一条路径（自然完成）才允许 applyCamera
    // (toCamera)；cameraStack 由 applyCamera 触发的 onZoomLevelChanged /
    // onPanXChanged / onPanYChanged 写回**真实落点**。
    function finishViewportMotion(enteringFlag, generation) {
        if (generation !== undefined && generation !== motionGeneration)
            return
        var to = viewportMotion.toCamera
        if (to)
            applyCamera(to)
        var pending = _zoomTargetLevel
        var anchorX = _zoomAnchorX
        var anchorY = _zoomAnchorY
        viewportMotion.cancel()
        outgoingStarmapId = ""
        outgoingRoute = null
        motionContext = null
        resetZoomRound()
        if (pending > 0 && pending !== zoomLevel && zoomTweenDuration() > 0) {
            _zoomTargetLevel = pending
            startZoomTween(pending, anchorX, anchorY)
        }
    }

    // 用户在新一帧输入（捏合继续 / 点按）或在切画布前收尾旧演出：取消语义。
    // Issue #949 复核（问题 2）：
    //   - 只有 viewportMotion.active **确实为真**时才处理本轮的演出；inactive
    //     时直接返回，不得动正常用户镜头（旧落点 toCamera 已在 cancel() 里清掉，
    //     再次进 / 退场不会再被上一次的落点复位）；
    //   - 取消 / 反向输入不落 toCamera（那也是跳到未来最终帧）：保留
    //     cameraSnapshot() 的当前显示帧，停止旧时间线、清旧 outgoing，把演出
    //     期间累计的滚轮目标从这一帧续走；
    //   - 自然完成走 finishViewportMotion（generation 匹配的 finished 回调）。
    function settleViewportMotion() {
        if (!viewportMotion.active)
            return
        motionGeneration++
        var pending = _zoomTargetLevel
        var anchorX = _zoomAnchorX
        var anchorY = _zoomAnchorY
        viewportMotion.cancel()
        outgoingStarmapId = ""
        outgoingRoute = null
        motionContext = null
        resetZoomRound()
        if (pending > 0 && pending !== zoomLevel && zoomTweenDuration() > 0) {
            _zoomTargetLevel = pending
            startZoomTween(pending, anchorX, anchorY)
        }
    }

    // 立即结束过渡（供输入入口在按下时调用；不改 route / Core）。
    function cancelViewportMotion() {
        settleViewportMotion()
    }

    function enterDiameterPx() {
        return Math.max(width * 0.70, height * 0.90)
    }

    // 入口投影直径 / 同一宽高双条件阈值；窗口 resize 后按真实尺寸计算。
    function entryScaleForDiameter(px) {
        var threshold = enterDiameterPx()
        return threshold > 0 ? px / threshold : 0
    }

    // 缩放后：只比较当前活动画布上鼠标锚点命中的入口。
    function maybeEnterEmbedAt(screenX, screenY) {
        var scene = activeScene()
        if (!scene || !scene.ready) {
            _zoomEntryKey = ""
            return -1
        }
        var emb = scene.embedAtWorld(screenToWorldX(screenX), screenToWorldY(screenY))
        if (!emb) {
            _zoomEntryKey = ""
            return -1
        }
        _zoomEntryKey = routeKey + "/" + emb.instanceId
        var projectedPx = finiteProduct(Math.min(emb.width, emb.height), zoomLevel)
        if (entryScaleForDiameter(projectedPx) < _enterRatio) {
            return -1
        }
        return enterEmbedViaCore(emb.instanceId, "zoom")
    }

    // 反向缩小：按进入上下文反推父入口投影，≤ 82% 时恢复父画布。
    // 双击可能在远低于进入阈值的入口上进入：退场阈值取 min(82%, 进入时的比例)，
    // 保证"至少要从进入状态继续缩小"才会退场，不会一进子图就被弹回父图。
    function maybeExitCanvas() {
        if (focusIsRoot)
            return -1
        var ctx = entryContexts[routeKey]
        if (!ctx || !(ctx.ratio > 0))
            return -1
        var virtualParentZoom = finiteQuotient(zoomLevel, ctx.ratio)
        var coverageNow = entryScaleForDiameter(finiteProduct(ctx.embedDiameter, virtualParentZoom))
        var entryCoverage = ctx.entryCoverage > 0 ? ctx.entryCoverage : _exitRatio
        if (coverageNow >= entryCoverage)
            return -1
        if (coverageNow > Math.min(_exitRatio, entryCoverage))
            return -1
        return exitCanvasViaCore(true)
    }

    // 进入子画布（双击、连续缩放都用这一个入口）。返回新画布的落点 zoom，失败 -1。
    // Issue #949：切换前先冻结旧活动场景身份 / 镜头 / 入口几何，切换后启动
    // 过渡演出 —— 旧父画面在位停留（只绘制），子画布从入口内同一片坐标钻进去、
    // 前段展开到目标镜头（没有未完成滚轮目标时 = 可读镜头），圆壳描边继续放大
    // 到整圈离开可见视口。复核评论 6250932011（问题 2）：滚轮穿越（trigger=zoom）
    // 若有没显示完的滚轮目标，先按 ctx.ratio 换算到子图尺度当过渡目标，不吞档；
    // 双击（double_click）是显式进入动作，仍用可读镜头落点（问题 1 的口径）。
    function enterEmbedViaCore(instanceId, trigger) {
        // 过渡还没收尾就继续进入孙图：先从当前已显示帧收尾，再冻结新快照。
        settleViewportMotion()
        var scene = activeScene()
        if (!scene || !starmapBackendRef || !instanceId)
            return -1
        // Issue #984：切换画布前先保存未提交的节点编辑；保存失败就中止切换，
        // 不能让 scene / model 重建把输入草稿静默丢掉。
        if (!scene.flushPendingNodeEdit()) {
            routeError = qsTr("节点保存失败，请重试")
            return -1
        }
        var info = scene.embedEntryInfo(instanceId)
        if (!info)
            return -1
        var zoomEntryKeyUsed = _zoomEntryKey
        var parentZoom = zoomLevel
        var projectedDiameterPx = Math.min(info.width, info.height) * parentZoom
        var enterThresholdPx = enterDiameterPx()
        var entryScale = entryScaleForDiameter(projectedDiameterPx)
        var parentCamera = cameraSnapshot()
        var parentStarmapId = currentStarmapId
        var parentRoute = normalizeRoute(currentRoute)
        // Issue #949 复核评论 6250932011（问题 2）：换层前保存本层还没显示完的
        // 滚轮目标和屏幕锚点（本层尺度），跨进子图那一路（trigger=zoom）按
        // childTargetZoom = parentPendingZoom × ctx.ratio 换算，避免同类吞滚轮；
        // 双击（double_click）是显式进入动作，仍用可读镜头落点（问题 1）。
        var parentPendingZoom = _zoomTargetLevel
        var pendingAnchorX = _zoomAnchorX
        var pendingAnchorY = _zoomAnchorY
        var hasPendingZoom = trigger === "zoom" && parentPendingZoom > 0
                && Math.abs(parentPendingZoom - parentZoom) > 1e-9 * parentZoom
        // 入口在"正在显示的"旧镜头下的屏幕几何：圆环起点、"入口内同一片坐标"
        // 的换算来源。只能用当前显示帧，不能用尚未显示的目标帧。
        var entryGeometry = {
            instanceId: instanceId,
            centerX: info.centerX,
            centerY: info.centerY,
            diameter: Math.min(info.width, info.height),
            screenX: worldToScreenX(info.centerX),
            screenY: worldToScreenY(info.centerY),
            screenRadius: Math.min(info.width, info.height) * parentZoom / 2,
            bounds: info.bounds,
            fitScale: info.fitScale
        }
        var res = starmapBackendRef.enter_starmap_embed(
                    currentRoute.rootStarmapId,
                    JSON.stringify(currentRoute.segments),
                    instanceId)
        if (!res || res.success !== true || !res.data || !res.data.route
                || !res.data.location || !res.data.location.currentStarmapId) {
            routeError = qsTr("进入子星图失败")
                    + ((res && res.errorCode) ? " (" + res.errorCode + ")" : "")
            logInteraction("scene_enter_failed", "embed", instanceId, {
                "trigger": trigger || "",
                "errorCode": (res && res.errorCode) ? res.errorCode : ""
            })
            return -1
        }
        var newRoute = normalizeRoute(res.data.route)
        if (!newRoute)
            return -1
        var childKey = routeKeyOf(newRoute)
        // 连续性：子图 world→屏幕比例 = 概览的 fit 比例，并且入口圆心对应的
        // 那一小片子图坐标正好停在入口原来的屏幕位置（childCameraForParent 与
        // 概览 fit 投影是同一份几何）——这是过渡的**起点相机**。
        var context = {
            parentRouteKey: routeKey,
            instanceId: instanceId,
            ratio: (info.fitScale > 0 && parentZoom > 0) ? info.fitScale / parentZoom : 1.0,
            embedDiameter: Math.min(info.width, info.height),
            embedCenterX: info.centerX,
            embedCenterY: info.centerY,
            boundsCenterX: info.bounds ? (info.bounds.minX + info.bounds.maxX) / 2 : 0,
            boundsCenterY: info.bounds ? (info.bounds.minY + info.bounds.maxY) / 2 : 0,
            // 进入瞬间入口的投影比例：双击在很小的入口上进入时可能低于 82%，
            // 退场阈值必须再受它约束（见 maybeExitCanvas），否则一进子图、
            // 甚至继续放大都会立刻被退场判定弹回父图。
            entryCoverage: entryScale
        }
        // Issue #949 复核评论 6250468319：落点 = 子图正常可读 / 可交互镜头
        // （优先沿用该子图已有的合法镜头，否则作者尺度 100% + 内容包围盒
        // 居中）。不再按"圆环要撑出可见视口"的倍率反算——入口很小时那会把
        // 节点放大几十倍、最后根本没法看。
        var childCamera = readableChildCamera(childKey, context)
        // 有没显示完的滚轮目标：过渡目标 = 换算到子图尺度、仍锚在同一鼠标屏幕
        // 位置的那台相机（相机跟着用户的滚轮走，不冻结、不吞档）；没有才用
        // 可读镜头当落点。
        var childTargetCamera = hasPendingZoom
                ? childCameraForParent(
                      anchoredCameraAt(parentPendingZoom, pendingAnchorX, pendingAnchorY), context)
                : childCamera
        storeEntryContext(childKey, context)
        storeCamera(routeKey, cameraSnapshot())
        storeCamera(childKey, childCamera)
        prepareCanvasSwitch()
        applyRoute(newRoute)
        applyCamera(childCamera)
        beginCanvasTransition(true, parentStarmapId, parentRoute, parentCamera,
                              childTargetCamera, entryGeometry, context,
                              hasPendingZoom ? { x: pendingAnchorX, y: pendingAnchorY } : null)
        logInteraction("scene_enter", "embed", instanceId, {
            "trigger": trigger || "",
            "zoomEntryKey": zoomEntryKeyUsed,
            "childRouteKey": childKey,
            "childStarmapId": res.data.location.currentStarmapId,
            "parentZoom": parentZoom,
            "childZoom": childCamera.zoom,
            "childTargetZoom": childTargetCamera.zoom,
            "frozenScreenRadius": entryGeometry.screenRadius,
            "viewportWidth": width,
            "viewportHeight": height,
            "projectedDiameterPx": projectedDiameterPx,
            "enterDiameterPx": enterThresholdPx
        })
        visualFocusChanged()
        return childCamera.zoom
    }

    // 退出当前画布，回到 Core 返回的父 route。continuous=true 时按当前镜头
    // 反推连续父镜头（缩放穿越用）；false 时恢复进入前的父镜头（返回按钮用）。
    // Issue #949 复核评论 6250468319 / 6250932011：两种出场是两种镜头落点，
    // 但都从**当前正在显示的帧**接上——
    //   - 滚轮：落点 = 未走完的滚轮目标按 entryContexts 换算出的父镜头
    //     （parentTargetZoom = clamp(childPendingZoom / ratio)，仍锚在同一鼠标
    //     屏幕位置）；没有未完成目标时就是由当前子镜头反推的父镜头本身。沿用户
    //     正在看的这一帧连续缩小，不得跨 82% 边界瞬间回到进入前保存的父镜头，
    //     也不得冻结相机把后续滚轮丢给过渡结束后的补间；
    //   - 按钮：落点 = cameraStack 保存的父镜头，不接滚轮未完成目标，但从当前
    //     可见帧平滑移动 / 缩放过去（用户拖到多远都不会直接跳回）。
    // 圆环起点 = 当前帧入口圆的投影（与退出阈值判定同一"正在显示的投影直径"
    // 几何），终点 = 目标父帧下入口圆投影；圆内子图 / 圆外父图按同一帧接上，
    // 结束才由 applyCamera 更新 cameraStack、才销毁临时层。
    function exitCanvasViaCore(continuous) {
        if (focusIsRoot || !starmapBackendRef)
            return -1
        // Issue #984：退出画布同样不能丢掉未提交的节点编辑。
        var active = activeScene()
        if (active && !active.flushPendingNodeEdit()) {
            routeError = qsTr("节点保存失败，请重试")
            return -1
        }
        // 过渡还没收尾就立即反向缩小：先从当前已显示帧收尾，再冻结新快照。
        settleViewportMotion()
        var childKey = routeKey
        var childZoom = zoomLevel
        var childCamera = cameraSnapshot()
        var childStarmapId = currentStarmapId
        var childRoute = normalizeRoute(currentRoute)
        // Issue #949 复核评论 6250932011（问题 2）：换层前保存子图这轮还没显示完
        // 的滚轮目标与屏幕锚点（子图尺度）。prepareCanvasSwitch() 会
        // resetZoomRound()，不先存就会被丢掉。
        var childPendingZoom = _zoomTargetLevel
        var pendingAnchorX = _zoomAnchorX
        var pendingAnchorY = _zoomAnchorY
        var res = starmapBackendRef.exit_starmap_canvas(JSON.stringify(currentRoute))
        if (!res || res.success !== true || !res.data || !res.data.rootStarmapId) {
            routeError = qsTr("返回父星图失败")
                    + ((res && res.errorCode) ? " (" + res.errorCode + ")" : "")
            logInteraction("scene_exit_failed", "scene", "", {
                "errorCode": (res && res.errorCode) ? res.errorCode : ""
            })
            return -1
        }
        var parentRoute = normalizeRoute(res.data)
        if (!parentRoute)
            return -1
        var parentKey = routeKeyOf(parentRoute)
        var ctx = entryContexts[childKey]
        // 起始父帧：当前子镜头按同一份父子对应反推（与退出阈值同几何）。
        var parentStartCam = ctx && ctx.ratio > 0
                ? parentCameraForChild(childCamera, ctx) : childCamera
        // 目标父帧（Issue #949 复核评论 6250932011 问题 1/2）：滚轮退出时若还有
        // 没显示完的滚轮目标，先把屏幕锚点上的目标子相机换算到父图尺度
        // （parentTargetZoom = clamp(childPendingZoom / ctx.ratio)，仍锚在同一屏
        // 幕位置），过渡期间相机继续缩小、不冻结、不吞档；没有未完成目标时就是
        // 反推帧本身（零跳变）。按钮返回（continuous=false）不接滚轮未完成目标，
        // 仍平滑回到进入前保存的父镜头（问题 4）。
        var parentTargetCam = parentStartCam
        var pendingAnchor = null
        if (!continuous) {
            var cam = cameraStack[parentKey]
            parentTargetCam = cam ? { zoom: cam.zoom, panX: cam.panX, panY: cam.panY }
                                  : defaultCamera()
        } else if (childPendingZoom > 0
                   && Math.abs(childPendingZoom - childZoom) > 1e-9 * childZoom) {
            var pendingCam = anchoredCameraAt(childPendingZoom, pendingAnchorX, pendingAnchorY)
            parentTargetCam = ctx && ctx.ratio > 0
                    ? parentCameraForChild(pendingCam, ctx) : pendingCam
            pendingAnchor = { x: pendingAnchorX, y: pendingAnchorY }
        }
        // 入口世界几何 + 终点屏幕几何（目标父帧下入口圆的投影 = 真实圆壳落点）。
        var exitGeometry = ctx && ctx.embedDiameter > 0
                ? embedScreenGeometry(parentTargetCam, ctx.embedCenterX, ctx.embedCenterY,
                                      ctx.embedDiameter)
                : null
        if (exitGeometry) {
            exitGeometry.instanceId = ctx.instanceId
            exitGeometry.centerX = ctx.embedCenterX
            exitGeometry.centerY = ctx.embedCenterY
            exitGeometry.diameter = ctx.embedDiameter
            exitGeometry.bounds = ctx.bounds
        }
        prepareCanvasSwitch()
        applyRoute(parentRoute)
        beginCanvasTransition(false, childStarmapId, childRoute, childCamera,
                              parentTargetCam, exitGeometry, ctx, pendingAnchor)
        logInteraction("scene_exit", "scene", "", {
            "parentRouteKey": parentKey,
            "continuous": !!continuous,
            "childZoom": childZoom,
            "parentZoom": parentTargetCam.zoom,
            "pendingCarried": !!pendingAnchor
        })
        visualFocusChanged()
        return parentTargetCam.zoom
    }

    // 双击 Embed：与连续缩放穿越同一个 enter 函数。
    function focusEmbed(hit) {
        if (!hit || hit.kind !== "embed" || !hit.owner)
            return
        if (hit.owner !== activeScene() || hit.scenePathKey !== routeKey)
            return
        enterEmbedViaCore(hit.id, "double_click")
    }

    // 左上"返回父星图"：走 Core 退出路由，恢复进入前的父镜头。
    // 已是根画布返回 false（Workspace 据此退出星图工作区）。
    function focusParentScene() {
        if (focusIsRoot)
            return false
        return exitCanvasViaCore(false) > 0
    }

    // 切画布前清掉旧画布上的瞬时业务手势与编辑汇总键；捏合穿越时保留 pinch
    // 状态，让同一次捏合继续作用在新画布上。Issue #949：旧画面层与它的时间线
    // 一起收尾，新画布的显示镜头从它自己的帧重新起步。
    function prepareCanvasSwitch() {
        settleViewportMotion()
        resetZoomRound()
        _zoomEntryKey = ""
        setInlineEditingKey("")
        hideTouchPreview()
        if (inputRouter)
            inputRouter.cancelAllGestures("scene_switch")
        if (interaction.pointerMode === "pinch") {
            interaction.cancelArmed()
        } else {
            interaction.reset()
        }
    }

    // 当前可见区域（活动画布的 world 坐标矩形）。
    readonly property var viewportSceneRect: ({
        x: screenToWorldX(0),
        y: screenToWorldY(0),
        width: width / (zoomLevel > 0 ? zoomLevel : 1),
        height: height / (zoomLevel > 0 ? zoomLevel : 1)
    })

    // DragHandler 的 activeTranslation 是 Qt scene 坐标增量。
    // 映射到本 Canvas local 再用；无祖先 scale 时退化成恒等。
    function sceneDeltaToCanvas(dx, dy) {
        var o = canvasArea.mapFromItem(null, 0, 0)
        var p = canvasArea.mapFromItem(null, dx, dy)
        return { x: p.x - o.x, y: p.y - o.y }
    }

    // Issue #801 评论 5894639734: +/- 触屏按钮按需显示，鼠标模式不常驻。
    property bool _touchInputActive: false
    function notePointerDevice(isTouch) {
        _touchInputActive = isTouch
    }

    // Issue #832：内联编辑汇总键（活动画布经 noteInlineEditing 汇总到这里）。
    property string inlineEditingKey: ""
    function setInlineEditingKey(key) {
        inlineEditingKey = key
    }

    // Issue #987：编辑器实际投影矩形命中。编辑器已送到 Canvas 顶层宿主，
    // 但坐标语义仍以活动 Scene 为准：先把 Canvas 局部点映射进 Scene 局部，
    // 再问编辑器自己的矩形。编辑会话无效时永远返回 false。
    function isInsideNodeEditor(sx, sy) {
        var scene = activeScene()
        if (!scene || typeof scene.editorContainsScenePoint !== "function")
            return false
        var p = scene.mapFromItem(canvasArea, sx, sy)
        return scene.editorContainsScenePoint(p.x, p.y)
    }

    // 编辑卡保持在节点原位；只在激活时最小平移相机，让展开矩形进入视口安全区。
    function ensureNodeEditVisible() {
        var scene = activeScene()
        if (!scene || typeof scene.nodeEditExpandedRect !== "function")
            return false
        var rect = scene.nodeEditExpandedRect()
        if (!rect || rect.width <= 0 || rect.height <= 0)
            return false
        var topLeft = scene.mapToItem(canvasArea, rect.x, rect.y)
        var bottomRight = scene.mapToItem(canvasArea,
                                          rect.x + rect.width,
                                          rect.y + rect.height)
        var left = Math.min(topLeft.x, bottomRight.x)
        var right = Math.max(topLeft.x, bottomRight.x)
        var top = Math.min(topLeft.y, bottomRight.y)
        var bottom = Math.max(topLeft.y, bottomRight.y)
        var margin = Math.max(0, Math.min(12, Math.min(width, height) / 2))
        var dx = left < margin ? margin - left
                               : (right > width - margin ? width - margin - right : 0)
        var dy = top < margin ? margin - top
                              : (bottom > height - margin ? height - margin - bottom : 0)
        if (dx !== 0 || dy !== 0)
            applyPan(finiteSum(panX, dx), finiteSum(panY, dy))
        return true
    }

    // 直接读活动 Scene 的会话事实，不依赖可能晚一拍的汇总键。
    function hasActiveNodeEdit() {
        var scene = activeScene()
        return !!scene && scene.hasActiveNodeEdit === true
    }

    // Issue #987：编辑框外的新手势开始前，同步提交唯一编辑会话。
    // 失败返回 false：Router 必须把本次手势整体判为 blocked，保留编辑器与草稿。
    function commitNodeEditForOutsideGesture(sx, sy, button, gestureId) {
        var scene = activeScene()
        if (!scene || scene.editingNodeId === "")
            return true
        if (typeof scene.commitActiveNodeEdit !== "function")
            return false
        var editingId = scene.editingNodeId
        var didCommit = scene.commitActiveNodeEdit() === true
        var ended = scene.editingNodeId === ""
        var ok = didCommit && ended
        logInteraction(ok ? "node_edit_handoff" : "node_edit_handoff_failed",
                       "node", editingId, {
                           "success": ok,
                           "sessionEnded": ended,
                           "screenX": sx,
                           "screenY": sy,
                           "gesture": gestureId,
                           "button": button
                       }, scene.scenePathKey || routeKey)
        return ok
    }

    // Issue #814 评论 5935346839: 星图交互边界日志统一入口。
    function logInteraction(event, itemKind, itemId, fields, scenePathKey) {
        if (!starmapBackendRef)
            return
        var fj = fields ? JSON.stringify(fields) : ""
        starmapBackendRef.record_interaction(
                    event,
                    scenePathKey === undefined ? routeKey : scenePathKey,
                    currentStarmapId !== "" ? currentStarmapId : starmapId,
                    itemKind, itemId, fj)
    }

    // ── Issue #832：唯一命中入口 ──
    // 命中只发生在当前活动画布上；命中身份带该画布的 routeKey。
    // Issue #982：把全局搜索命中的星图对象带到镜头中心并选中。
    // 搜索索引只覆盖根星图对象，因此这里始终按根画布定位。
    function revealSearchTarget(target) {
        var scene = activeScene()
        if (!target || !scene)
            return false
        if (!target.nodeId && !target.edgeId && !target.embedId
                && !target.linkId)
            return false
        if (!focusIsRoot) {
            if (!resetToRoot())
                return false
        } else if (!commitPendingNodeEditBeforeNavigation()) {
            return false
        }
        scene = activeScene()
        if (!scene)
            return false

        var kind = ""
        var itemId = ""
        if (target.nodeId) {
            kind = "node"
            itemId = target.nodeId
        } else if (target.edgeId) {
            kind = "edge"
            itemId = target.edgeId
        } else if (target.embedId) {
            kind = "embed"
            itemId = target.embedId
        } else if (target.linkId) {
            kind = "link"
            itemId = target.linkId
        }

        // Link 是挂在 node 或 embed 上的附属对象，没有独立几何。
        // 定位时选中并居中其宿主，避免“打开了星图但什么也没高亮”。
        if (kind === "link") {
            var owner = scene.linkOwner(itemId)
            if (!owner)
                return false
            kind = owner.kind
            itemId = owner.id
        }

        var item = kind === "node" ? scene.hitNode(itemId)
            : kind === "edge" ? scene.hitEdge(itemId)
            : scene.hitEmbed(itemId)
        if (!item)
            return false

        if (kind === "node")
            scene.selectNode(itemId)
        else if (kind === "edge")
            scene.selectEdge(itemId)
        else
            scene.selectEmbed(itemId)

        var center = scene.itemCenterScene(kind, itemId)
        if (center) {
            applyPan(finiteDifference(width / 2, finiteProduct(center.x, zoomLevel)),
                     finiteDifference(height / 2, finiteProduct(center.y, zoomLevel)))
        }
        return center !== null
    }

    function hitTargetAtScreen(sx, sy) {
        var scene = activeScene()
        if (!scene)
            return null
        return scene.hitTargetAtWorld(screenToWorldX(sx), screenToWorldY(sy))
    }

    // Issue #814 评论 5935346839: pointer_press 是完整手势的起点边界。
    function logPointerPress(button, device, point, gestureId) {
        var hit = hitTargetAtScreen(point.position.x, point.position.y)
        var kind = hit ? hit.kind : "empty"
        var hitId = hit ? hit.id : ""
        var hitPathKey = hit ? hit.scenePathKey : routeKey
        logInteraction("pointer_press", kind, hitId, {
            "button": button,
            "device": device,
            "gesture": gestureId !== undefined ? gestureId : 0,
            "screenX": point.position.x,
            "screenY": point.position.y,
            "sceneX": screenToWorldX(point.position.x),
            "sceneY": screenToWorldY(point.position.y),
            "panX": panX,
            "panY": panY,
            "zoomLevel": zoomLevel
        }, hitPathKey)
    }

    // ── 交互状态机：整张活动画布唯一实例 ──
    StarMapInteractionController {
        id: interaction
        longPressInterval: Application.styleHints.mousePressAndHoldInterval > 0
                ? Application.styleHints.mousePressAndHoldInterval
                : 800
    }

    readonly property var sharedInteraction: interaction

    Connections {
        target: interaction
        // 捏合结束（pointerMode 离开 pinch）时清掉穿越补偿基准。
        function onPointerModeChanged() {
            if (interaction.pointerMode !== "pinch") {
                canvasArea._pinchSwitchBaseZoom = -1
                canvasArea._pinchSwitchCanvasZoom = 1
            }
        }
    }

    // 菜单辅助状态：菜单归属就是当前活动画布。
    property var menuOwnerContent: null
    property var selectedNodeForMenu: null
    property var selectedEdgeForMenu: null
    property var selectedEmbedForMenu: null
    // 右键命中点的 world 坐标，创建类操作由活动画布直接使用。
    property real contextMenuSceneX: 0
    property real contextMenuSceneY: 0

    // 新建对话框状态：先收集名字再写 Core（Issue #793 评论 5884923277）
    property string createMode: ""      // "node" / "starmap"

    // Signals
    signal nodeSelected(var node)
    signal edgeSelected(var edge)
    signal selectionCleared()

    function clearError() {
        routeError = ""
        var scene = activeScene()
        if (scene)
            scene.clearError()
    }

    // Issue #798: 公开 reset 入口，供 Workspace 切图 / 不可见时清瞬时交互状态。
    function resetInteraction() {
        interaction.reset()
        if (inputRouter)
            inputRouter.cancelAllGestures("reset")
        hideTouchPreview()
        var scene = activeScene()
        if (scene)
            scene.refreshAllEdges()
    }

    // Issue #834：connectArmed（菜单"连线"发起）时 Router 的 HoverHandler 调这里
    // 刷新预览；活动场景内部就是命中主人。
    function refreshConnectPreview() {
        var scene = activeScene()
        if (scene)
            scene.refreshConnectPreview()
    }

    // ---------------------------------------------------------------------------
    // Issue #949：过渡期间的旧画面层。冻结的 route / 镜头（不再交互、不再是
    // activeScene()），只负责把"切换前一帧"留在屏幕上，直到圆环整圈离开
    // 可见视口；过渡结束（或用户新一帧输入）立即销毁，不持久化任何状态。
    // 复核（问题 4）：出场时它是"圆内"的那一层（z 在上、被圆形裁剪），
    // 圆外显示新的父画面；圆形裁剪只在过渡期启用，结束即回落到直接矢量渲染。
    // ---------------------------------------------------------------------------
    StarMapCircleClip {
        id: outgoingSceneClip
        anchors.fill: parent
        z: viewportMotion.entering ? 0 : 2
        clipEnabled: canvasArea.outgoingSceneIsInsideRing
        clipCenterX: viewportMotion.ringCenterX
        clipCenterY: viewportMotion.ringCenterY
        clipRadius: viewportMotion.ringClipRadius

        Loader {
            id: outgoingSceneLoader
            anchors.fill: parent
            active: canvasArea.outgoingStarmapId !== "" && viewportMotion.active
            sourceComponent: outgoingSceneComponent
        }
    }

    Component {
        id: outgoingSceneComponent
        StarMapActiveScene {
            dt: canvasArea.dt
            canvasBackgroundColor: canvasArea.canvasBackgroundColor
            starmapId: canvasArea.outgoingStarmapId
            rootStarmapId: canvasArea.outgoingRoute
                    ? canvasArea.outgoingRoute.rootStarmapId : ""
            pathSegments: canvasArea.outgoingRoute ? canvasArea.outgoingRoute.segments : []
            scenePathKey: canvasArea.outgoingRouteKey
            starmapBackendRef: canvasArea.starmapBackendRef
            selectionController: canvasArea.selectionController
            interactionController: canvasArea.sharedInteraction
            menuHost: canvasArea
            interactive: false
            zoomLevel: canvasArea.outgoingZoom
            panX: canvasArea.outgoingPanX
            panY: canvasArea.outgoingPanY
            detailReveal: canvasArea.outgoingDetailReveal
            transitionEmbedId: canvasArea.motionFocusInstanceId
            ringVisible: canvasArea.outgoingSceneIsInsideRing && viewportMotion.ringVisible
            ringCenterX: viewportMotion.ringCenterX
            ringCenterY: viewportMotion.ringCenterY
            ringRadius: viewportMotion.ringRadius
            ringStrokeWidth: viewportMotion.ringStrokeWidth
            ringBackgroundProgress: canvasArea.outgoingSceneIsInsideRing
                    ? 1 - viewportMotion.progress : 0
        }
    }

    // ---------------------------------------------------------------------------
    // 活动画布：本文件只加载 currentStarmapId 的一张 StarMapActiveScene。
    // 相机只是注入给它的两个标量（zoom / pan），内容自己按投影实时绘制。
    // Issue #949 复核（问题 4）：进场时它是"圆内"的那一层（被圆形裁剪），
    // 圆外持续显示旧父画面；裁剪只在过渡期启用，结束即停用（仍是直接矢量渲染）。
    // ---------------------------------------------------------------------------
    StarMapCircleClip {
        id: activeSceneClip
        anchors.fill: parent
        z: 1
        clipEnabled: canvasArea.activeSceneIsInsideRing
        clipCenterX: viewportMotion.ringCenterX
        clipCenterY: viewportMotion.ringCenterY
        clipRadius: viewportMotion.ringClipRadius

        Loader {
            id: activeSceneLoader
            anchors.fill: parent
            active: canvasArea.currentStarmapId !== ""
            sourceComponent: activeSceneComponent
            onLoaded: {
                if (item && item.ready) {
                    canvasArea.sceneGeometryReady(String(item.starmapId || ""), canvasArea.routeKey)
                    canvasArea.completeTreeReveal()
                }
            }
        }

        Connections {
            target: activeSceneLoader.item
            ignoreUnknownSignals: true
            function onReadyChanged() {
                var scene = activeSceneLoader.item
                if (scene && scene.ready) {
                    canvasArea.sceneGeometryReady(String(scene.starmapId || ""), canvasArea.routeKey)
                    canvasArea.completeTreeReveal()
                }
            }
        }
    }

    Component {
        id: activeSceneComponent
        StarMapActiveScene {
            dt: canvasArea.dt
            canvasBackgroundColor: canvasArea.canvasBackgroundColor
            starmapId: canvasArea.currentStarmapId
            rootStarmapId: canvasArea.currentRoute.rootStarmapId
            pathSegments: canvasArea.currentRoute.segments
            scenePathKey: canvasArea.routeKey
            starmapBackendRef: canvasArea.starmapBackendRef
            selectionController: canvasArea.selectionController
            interactionController: canvasArea.sharedInteraction
            menuHost: canvasArea
            // Issue #987：编辑器的视觉父级送到 Router 之上；草稿与编辑会话仍只属于本 Scene。
            editOverlayHost: nodeEditOverlayHost
            zoomLevel: canvasArea.zoomLevel
            panX: canvasArea.panX
            panY: canvasArea.panY
            // Issue #949：过渡显示层（只影响绘制，不改 authored 坐标 / 图关系）。
            detailReveal: canvasArea.activeDetailReveal
            transitionEmbedId: canvasArea.motionFocusInstanceId
            ringVisible: canvasArea.activeSceneIsInsideRing && viewportMotion.ringVisible
            ringCenterX: viewportMotion.ringCenterX
            ringCenterY: viewportMotion.ringCenterY
            ringRadius: viewportMotion.ringRadius
            ringStrokeWidth: viewportMotion.ringStrokeWidth
            ringBackgroundProgress: canvasArea.activeSceneIsInsideRing
                    ? viewportMotion.progress : 0

            onNodeSelected: function(node) { canvasArea.nodeSelected(node) }
            onEdgeSelected: function(edge) { canvasArea.edgeSelected(edge) }
            onSelectionCleared: canvasArea.selectionCleared()
            onStructureChanged: canvasArea.treeStructureChanged()
        }
    }

    // Background Grid
    Rectangle {
        anchors.fill: parent
        color: "transparent"
        opacity: 0.3

        Canvas {
            id: gridCanvas
            anchors.fill: parent
            onPaint: {
                var ctx = getContext("2d")
                ctx.clearRect(0, 0, width, height)
                ctx.fillStyle = _textMuted

                // 网格自身做 LOD：相机放开到 1e-4 后，50 × zoomLevel 会掉到亚像素，
                // 双重循环按 1/zoom² 爆炸。按 5 倍档抬 world 间距，屏幕上实际画出来的
                // 间距永远 >= 16px，循环次数只跟屏幕尺寸有关。
                if (!(zoomLevel > 0))
                    return
                var worldSpacing = 50
                var gridSpacing = finiteProduct(worldSpacing, zoomLevel)
                while (gridSpacing < 16) {
                    worldSpacing *= 5
                    gridSpacing = worldSpacing * zoomLevel
                }
                var startX = (panX % gridSpacing)
                var startY = (panY % gridSpacing)

                for (var x = startX; x < width; x += gridSpacing) {
                    for (var y = startY; y < height; y += gridSpacing) {
                        ctx.fillRect(x - 1, y - 1, 2, 2)
                    }
                }
            }
            Connections {
                target: canvasArea
                function onPanXChanged() { gridCanvas.requestPaint() }
                function onPanYChanged() { gridCanvas.requestPaint() }
                function onZoomLevelChanged() { gridCanvas.requestPaint() }
            }
        }
    }

    // ---------------------------------------------------------------------------
    // Issue #832：唯一的原始输入层，透明铺在活动画布内容之上。
    // ---------------------------------------------------------------------------
    StarMapInputRouter {
        id: inputRouter
        anchors.fill: parent
        canvas: canvasArea
        z: 10
    }

    // ---------------------------------------------------------------------------
    // Issue #987：节点编辑器视觉宿主。位于输入路由之上（z:20），只负责层级与
    // 坐标，不保存草稿、不复制编辑器；编辑器对象仍由活动 Scene 声明和销毁。
    // 子项的 z 再大也不能越过父项与兄弟项的堆叠关系，因此不能让编辑器继续
    // 留在 activeSceneClip(z:1) 里对抗 Router(z:10)。
    // ---------------------------------------------------------------------------
    Item {
        id: nodeEditOverlayHost
        anchors.fill: parent
        z: 20
    }

    // 连线预览线：唯一 overlay，坐标是活动画布 world 坐标，用当前镜头投影。
    Canvas {
        id: connectPreview
        anchors.fill: parent
        z: 3

        Connections {
            target: canvasArea
            function onPanXChanged() { connectPreview.requestPaint() }
            function onPanYChanged() { connectPreview.requestPaint() }
            function onZoomLevelChanged() { connectPreview.requestPaint() }
        }
        Connections {
            target: interaction
            function onConnectMouseXChanged() { connectPreview.requestPaint() }
            function onConnectMouseYChanged() { connectPreview.requestPaint() }
            function onConnectFromSceneXChanged() { connectPreview.requestPaint() }
            function onConnectFromSceneYChanged() { connectPreview.requestPaint() }
            function onConnectPreviewEndXChanged() { connectPreview.requestPaint() }
            function onConnectPreviewEndYChanged() { connectPreview.requestPaint() }
            function onPointerModeChanged() { connectPreview.requestPaint() }
        }

        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, width, height)
            if (interaction.pointerMode !== "connect")
                return
            if (interaction.connectFromKind === "")
                return
            ctx.beginPath()
            // 预览起点/终点都取"与正式边同源"的可见端点（悬停合法目标时贴边界）。
            ctx.moveTo(canvasArea.worldToScreenX(interaction.connectFromSceneX),
                       canvasArea.worldToScreenY(interaction.connectFromSceneY))
            ctx.lineTo(canvasArea.worldToScreenX(interaction.connectPreviewEndX),
                       canvasArea.worldToScreenY(interaction.connectPreviewEndY))
            ctx.strokeStyle = _accent
            ctx.lineWidth = canvasArea._edgeScreenStrokeWidth
            ctx.stroke()
        }
    }

    // Issue #801 评论 5894639734: 触屏缩放 +/- 按钮（右下角浮层）。
    RowLayout {
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.rightMargin: 16
        anchors.bottomMargin: 16
        spacing: 8
        z: 50
        visible: _touchInputActive

        ToolButton {
            icon.name: "zoom-in"
            // 缩放锚点是画布中心：中心若瞄准子星图入口，连续放大同样会穿越。
            onClicked: zoomAt(canvasArea.width / 2, canvasArea.height / 2,
                              zoomLevel * _zoomFactor)
        }

        ToolButton {
            icon.name: "zoom-out"
            onClicked: zoomAt(canvasArea.width / 2, canvasArea.height / 2,
                              zoomLevel / _zoomFactor)
        }
    }

    Rectangle {
        id: errorBanner
        width: parent.width - 32
        height: 40
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 16
        anchors.horizontalCenter: parent.horizontalCenter
        color: _error
        radius: _radiusSm
        visible: errorMessage.length > 0
        z: 100

        Label {
            anchors.centerIn: parent
            text: canvasArea.errorMessage
            color: _onError
        }
        MouseArea {
            anchors.fill: parent
            onClicked: canvasArea.clearError()
        }
    }

    // Issue #801 评论 5895310100: 触屏长按菜单视觉层（不抢 pointer grab）。
    Item {
        id: touchContextPreview
        visible: false
        z: 60

        property string previewKind: ""   // "node" / "embed"
        property real anchorX: 0
        property real anchorY: 0

        x: anchorX - width / 2
        y: anchorY + 8
        width: 150
        height: 120

        Rectangle {
            anchors.fill: parent
            color: _card
            border.color: _border
            border.width: 1
            radius: _radiusSm
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 8
            spacing: 4

            Label {
                Layout.fillWidth: true
                text: touchContextPreview.previewKind === "embed" ? qsTr("编辑名称") : qsTr("编辑")
                color: Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                leftPadding: 4
            }
            Label {
                Layout.fillWidth: true
                text: qsTr("移动")
                color: Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                leftPadding: 4
            }
            Label {
                Layout.fillWidth: true
                text: qsTr("删除")
                color: Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                leftPadding: 4
            }
        }

        function show(kind, sx, sy) {
            previewKind = kind
            anchorX = sx
            anchorY = sy
            visible = true
        }

        function hide() {
            visible = false
        }
    }

    // ---------------------------------------------------------------------------
    // 菜单入口：归属就是当前活动画布，坐标是 world 坐标。
    // ---------------------------------------------------------------------------
    function openBlankMenu(sceneX, sceneY, hit, screenX, screenY) {
        menuOwnerContent = hit ? hit.owner : activeScene()
        selectedNodeForMenu = null
        selectedEdgeForMenu = null
        selectedEmbedForMenu = null
        contextMenuSceneX = sceneX
        contextMenuSceneY = sceneY
        logInteraction("context_menu_open", "empty", "", {
            "menuKind": "bg",
            "sceneX": sceneX,
            "sceneY": sceneY
        }, hit ? hit.scenePathKey : routeKey)
        bgContextMenu.popup(screenX, screenY)
    }

    // 右键命中分发：node / embed / edge 开对应菜单；空白开当前画布的新建菜单。
    function openHitContextMenu(hit, screenX, screenY) {
        var sx = screenToWorldX(screenX)
        var sy = screenToWorldY(screenY)
        if (hit.kind === "node") {
            hit.owner.selectNode(hit.id)
            menuOwnerContent = hit.owner
            selectedNodeForMenu = hit
            logInteraction("context_menu_open", "node", hit.id, {
                "menuKind": "node",
                "sceneX": sx,
                "sceneY": sy
            }, hit.scenePathKey)
            nodeContextMenu.popup(screenX, screenY)
        } else if (hit.kind === "embed") {
            var embed = hit.owner.hitEmbed(hit.id)
            if (!embed)
                return
            hit.owner.selectEmbed(hit.id)
            menuOwnerContent = hit.owner
            selectedEmbedForMenu = embed
            logInteraction("context_menu_open", "embed", hit.id, {
                "menuKind": "embed",
                "sceneX": sx,
                "sceneY": sy
            }, hit.scenePathKey)
            embedContextMenu.popup(screenX, screenY)
        } else if (hit.kind === "edge") {
            hit.owner.selectEdge(hit.id)
            menuOwnerContent = hit.owner
            selectedEdgeForMenu = hit
            logInteraction("context_menu_open", "edge", hit.id, {
                "menuKind": "edge",
                "sceneX": sx,
                "sceneY": sy
            }, hit.scenePathKey)
            edgeContextMenu.popup(screenX, screenY)
        } else {
            openBlankMenu(sx, sy, hit, screenX, screenY)
        }
    }

    // 触屏长按视觉层：world 坐标 → 屏幕坐标后显示。
    function showTouchPreview(kind, sceneX, sceneY) {
        touchContextPreview.show(kind, worldToScreenX(sceneX), worldToScreenY(sceneY))
    }

    function hideTouchPreview() {
        touchContextPreview.hide()
    }

    // 触屏长按松手：弹出真正可点击的菜单，归属就是当前活动画布。
    function showLongPressMenu(kind, id, sceneX, sceneY, owner) {
        menuOwnerContent = owner
        if (kind === "node") {
            selectedNodeForMenu = owner.hitNode(id)
            if (!selectedNodeForMenu)
                return
            owner.selectNode(id)
            nodeContextMenu.popup(worldToScreenX(sceneX), worldToScreenY(sceneY))
        } else if (kind === "embed") {
            selectedEmbedForMenu = owner.hitEmbed(id)
            if (!selectedEmbedForMenu)
                return
            owner.selectEmbed(id)
            embedContextMenu.popup(worldToScreenX(sceneX), worldToScreenY(sceneY))
        }
    }

    // Context Menus
    Menu {
        id: bgContextMenu

        background: Rectangle {
            implicitWidth: 150
            color: _card
            border.color: _border
            border.width: 1
            radius: _radiusSm
        }

        MenuItem {
            id: bgMenuItem1
            text: qsTr("新建节点")
            contentItem: Label {
                text: bgMenuItem1.text
                color: bgMenuItem1.hovered ? _accent : Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                font.bold: true
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: bgMenuItem1.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: createDialog.open("node", contextMenuSceneX, contextMenuSceneY)
        }

        MenuItem {
            id: bgMenuItem2
            text: qsTr("新建子星图")
            contentItem: Label {
                text: bgMenuItem2.text
                color: bgMenuItem2.hovered ? _accent : Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                font.bold: true
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: bgMenuItem2.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: createDialog.open("starmap", contextMenuSceneX, contextMenuSceneY)
        }

        MenuSeparator {}
        MenuItem {
            text: qsTr("标签与引用")
            onTriggered: requestAnnotationAction("tags", "starMap", currentStarmapId, "")
        }
        MenuItem {
            text: qsTr("添加超链接")
            onTriggered: requestAnnotationAction("link", "starMap", currentStarmapId, "")
        }
        MenuItem {
            text: qsTr("查看引用")
            onTriggered: requestAnnotationAction("viewReferences", "starMap", currentStarmapId, "")
        }
    }

    Menu {
        id: nodeContextMenu

        background: Rectangle {
            implicitWidth: 150
            color: _card
            border.color: _border
            border.width: 1
            radius: _radiusSm
        }

        // Issue #822: 节点标题编辑就在节点框内（TextInput 内联），
        // 这里只负责把光标放进节点框，不再打开外部编辑 Popup。
        MenuItem {
            id: nodeMenuItemEdit
            text: qsTr("编辑")
            contentItem: Label {
                text: nodeMenuItemEdit.text
                color: nodeMenuItemEdit.hovered ? _accent : Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItemEdit.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedNodeForMenu && menuOwnerContent)
                    menuOwnerContent.beginInlineEdit(selectedNodeForMenu.id)
            }
        }

        MenuSeparator {}
        MenuItem {
            text: qsTr("标签与引用")
            onTriggered: requestAnnotationAction("tags", "starMapNode",
                                                   selectedNodeForMenu ? selectedNodeForMenu.id : "", "")
        }
        MenuItem {
            text: qsTr("添加超链接")
            onTriggered: requestAnnotationAction("link", "starMapNode",
                                                   selectedNodeForMenu ? selectedNodeForMenu.id : "", "")
        }
        MenuItem {
            text: qsTr("登记名称")
            onTriggered: requestAnnotationAction("registerName", "starMapNode",
                selectedNodeForMenu ? selectedNodeForMenu.id : "",
                selectedNodeForMenu ? selectedNodeForMenu.title : "")
        }
        MenuItem {
            text: qsTr("查看引用")
            onTriggered: requestAnnotationAction("viewReferences", "starMapNode",
                                                   selectedNodeForMenu ? selectedNodeForMenu.id : "", "")
        }

        MenuItem {
            id: nodeMenuItemMove
            text: qsTr("移动")
            contentItem: Label {
                text: nodeMenuItemMove.text
                color: nodeMenuItemMove.hovered ? _accent : Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItemMove.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                // 菜单已确定目标，直接进 move，不经过 pressPending 仲裁；
                // 完整身份（kind/id/targetPath/scenePathKey）与手势路径同源。
                if (selectedNodeForMenu && menuOwnerContent) {
                    var item = menuOwnerContent.hitNode(selectedNodeForMenu.id)
                    if (!item)
                        return
                    interaction.beginMove("node", item.id,
                                          menuOwnerContent.nodePath(item.id),
                                          menuOwnerContent.scenePathKey,
                                          item.x, item.y)
                }
            }
        }

        MenuItem {
            id: nodeMenuItemLink
            text: qsTr("内部链接")
            contentItem: Label {
                text: nodeMenuItemLink.text
                color: nodeMenuItemLink.hovered ? _accent : Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItemLink.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                // Issue #834：节点内部链接菜单（StarMapLink，内部跳转）。
                if (selectedNodeForMenu && menuOwnerContent)
                    linkDialog.open("node", selectedNodeForMenu.id, menuOwnerContent,
                                    menuOwnerContent.nodePath(selectedNodeForMenu.id))
            }
        }

        MenuItem {
            id: nodeMenuItemConnect
            text: qsTr("连线")
            contentItem: Label {
                text: nodeMenuItemConnect.text
                color: nodeMenuItemConnect.hovered ? _accent : Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItemConnect.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                // Issue #834：菜单发起连线。source 已知，进 connectArmed 等点 target。
                if (selectedNodeForMenu && menuOwnerContent) {
                    var center = menuOwnerContent.itemCenterScene("node", selectedNodeForMenu.id)
                    if (center)
                        interaction.beginConnectFromMenu("node", selectedNodeForMenu.id,
                                menuOwnerContent.nodePath(selectedNodeForMenu.id),
                                menuOwnerContent.scenePathKey, center.x, center.y)
                }
            }
        }

        MenuItem {
            id: nodeMenuItemDelete
            text: qsTr("删除")
            contentItem: Label {
                text: nodeMenuItemDelete.text
                color: nodeMenuItemDelete.hovered ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItemDelete.hovered ? _dangerContainer : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedNodeForMenu && menuOwnerContent)
                    menuOwnerContent.deleteNode(selectedNodeForMenu.id)
            }
        }
    }

    Menu {
        id: edgeContextMenu

        background: Rectangle {
            implicitWidth: 150
            color: _card
            border.color: _border
            border.width: 1
            radius: _radiusSm
        }

        MenuItem {
            id: edgeMenuItem1
            text: qsTr("重命名连线")
            contentItem: Label {
                text: edgeMenuItem1.text
                color: edgeMenuItem1.hovered ? _accent : Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: edgeMenuItem1.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedEdgeForMenu) {
                    renameDialog.open("edge", selectedEdgeForMenu.id, selectedEdgeForMenu.label || "")
                }
            }
        }

        MenuItem {
            id: edgeMenuItem2
            text: qsTr("删除连线")
            contentItem: Label {
                text: edgeMenuItem2.text
                color: edgeMenuItem2.hovered ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: edgeMenuItem2.hovered ? _dangerContainer : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedEdgeForMenu && menuOwnerContent)
                    menuOwnerContent.deleteEdge(selectedEdgeForMenu.id)
            }
        }
    }

    // Issue #796 评论 5886483653: Embed 右键菜单：编辑名称 / 移动 / 删除
    Menu {
        id: embedContextMenu

        background: Rectangle {
            implicitWidth: 150
            color: _card
            border.color: _border
            border.width: 1
            radius: _radiusSm
        }

        MenuItem {
            id: embedMenuItemRename
            text: qsTr("编辑名称")
            contentItem: Label {
                text: embedMenuItemRename.text
                color: embedMenuItemRename.hovered ? _accent : Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: embedMenuItemRename.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedEmbedForMenu) {
                    renameDialog.open("embed", selectedEmbedForMenu.instanceId, selectedEmbedForMenu.label || "")
                }
            }
        }

        MenuSeparator {}
        MenuItem {
            text: qsTr("标签与引用")
            onTriggered: requestAnnotationAction("tags", "starMapEmbed",
                selectedEmbedForMenu ? selectedEmbedForMenu.instanceId : "", "")
        }
        MenuItem {
            text: qsTr("添加超链接")
            onTriggered: requestAnnotationAction("link", "starMapEmbed",
                selectedEmbedForMenu ? selectedEmbedForMenu.instanceId : "", "")
        }
        MenuItem {
            text: qsTr("登记名称")
            onTriggered: requestAnnotationAction("registerName", "starMapEmbed",
                selectedEmbedForMenu ? selectedEmbedForMenu.instanceId : "",
                selectedEmbedForMenu ? selectedEmbedForMenu.label : "")
        }
        MenuItem {
            text: qsTr("查看引用")
            onTriggered: requestAnnotationAction("viewReferences", "starMapEmbed",
                selectedEmbedForMenu ? selectedEmbedForMenu.instanceId : "", "")
        }

        MenuItem {
            id: embedMenuItemMove
            text: qsTr("移动")
            contentItem: Label {
                text: embedMenuItemMove.text
                color: embedMenuItemMove.hovered ? _accent : Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: embedMenuItemMove.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedEmbedForMenu && menuOwnerContent) {
                    var item = menuOwnerContent.hitEmbed(selectedEmbedForMenu.instanceId)
                    if (!item)
                        return
                    interaction.beginMove("embed", item.instanceId,
                                          menuOwnerContent.embedPath(item.instanceId),
                                          menuOwnerContent.scenePathKey,
                                          item.x, item.y)
                }
            }
        }

        MenuItem {
            id: embedMenuItemLink
            text: qsTr("内部链接")
            contentItem: Label {
                text: embedMenuItemLink.text
                color: embedMenuItemLink.hovered ? _accent : Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: embedMenuItemLink.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                // Issue #834：子星图入口内部链接菜单（StarMapLink，内部跳转）。
                if (selectedEmbedForMenu && menuOwnerContent)
                    linkDialog.open("embed", selectedEmbedForMenu.instanceId, menuOwnerContent,
                                    menuOwnerContent.embedPath(selectedEmbedForMenu.instanceId))
            }
        }

        MenuItem {
            id: embedMenuItemConnect
            text: qsTr("连线")
            contentItem: Label {
                text: embedMenuItemConnect.text
                color: embedMenuItemConnect.hovered ? _accent : Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: embedMenuItemConnect.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                // Issue #834：菜单发起连线。source 已知，进 connectArmed 等点 target。
                if (selectedEmbedForMenu && menuOwnerContent) {
                    var center = menuOwnerContent.itemCenterScene("embed", selectedEmbedForMenu.instanceId)
                    if (center)
                        interaction.beginConnectFromMenu("embed", selectedEmbedForMenu.instanceId,
                                menuOwnerContent.embedPath(selectedEmbedForMenu.instanceId),
                                menuOwnerContent.scenePathKey, center.x, center.y)
                }
            }
        }

        MenuItem {
            id: embedMenuItemDelete
            text: qsTr("删除")
            contentItem: Label {
                text: embedMenuItemDelete.text
                color: embedMenuItemDelete.hovered ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: embedMenuItemDelete.hovered ? _dangerContainer : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedEmbedForMenu && menuOwnerContent)
                    menuOwnerContent.deleteEmbed(selectedEmbedForMenu.instanceId)
            }
        }
    }

    // Issue #796 评论 5886483653: 弹窗改 Qt Quick Controls Popup。
    Popup {
        id: renameDialog
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        width: 300
        height: 160
        anchors.centerIn: Overlay.overlay
        Overlay.modal: Rectangle { color: Qt.rgba(0, 0, 0, 0.32) }
        background: Rectangle {
            color: _card
            border.color: _border
            border.width: 1.5
            radius: _dialogRadius
        }

        property string targetType: "" // "edge" / "embed"
        property string targetId: ""
        property string initialText: ""

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 16

            Label {
                text: renameDialog.targetType === "embed" ? qsTr("修改子星图名称")
                     : qsTr("修改连线标签")
                font.pointSize: dt.fontLgPt
                font.bold: true
                color: Kirigami.Theme.textColor
            }

            TextField {
                id: renameInput
                Layout.fillWidth: true
                Layout.preferredHeight: 36
                color: Kirigami.Theme.textColor
                font.pointSize: dt.bodyPt
                focus: renameDialog.visible
                text: renameDialog.initialText

                background: Rectangle {
                    color: _surfaceContainer
                    border.color: renameInput.activeFocus ? _accent : _border
                    border.width: 1.5
                    radius: _radiusXs
                }

                Keys.onReturnPressed: renameDialog.confirm()
                Keys.onEscapePressed: renameDialog.close()
            }

            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 12

                Button {
                    id: cancelBtn
                    text: qsTr("取消")
                    onClicked: renameDialog.close()
                    contentItem: Label {
                        text: cancelBtn.text
                        color: Kirigami.Theme.disabledTextColor
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: cancelBtn.hovered ? _surfaceContainer : "transparent"
                        border.color: _border
                        radius: _radiusXs
                    }
                }

                Button {
                    id: confirmBtn
                    text: qsTr("确定")
                    onClicked: renameDialog.confirm()
                    contentItem: Label {
                        text: confirmBtn.text
                        color: Kirigami.Theme.highlightedTextColor
                        font.bold: true
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: confirmBtn.hovered ? _accentHover : _accent
                        radius: _radiusXs
                    }
                }
            }
        }

        function open(type, id, text) {
            targetType = type
            targetId = id
            initialText = text
            renameInput.text = text
            visible = true
            renameInput.forceActiveFocus()
        }

        function close() {
            visible = false
        }

        function confirm() {
            // Issue #822: 连线标签和子星图名称仍然用这个弹窗；
            // 节点标题已改为节点内联编辑，不再走这里。
            if (!menuOwnerContent) {
                close()
                return
            }
            if (targetType === "edge") {
                menuOwnerContent.updateEdgeLabel(targetId, renameInput.text)
            } else if (targetType === "embed") {
                menuOwnerContent.updateEmbedLabel(targetId, renameInput.text)
            }
            close()
        }
    }

    // Issue #793 评论 5884923277: 新建节点/子星图 Dialog
    Popup {
        id: createDialog
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        width: 300
        height: 160
        anchors.centerIn: Overlay.overlay
        Overlay.modal: Rectangle { color: Qt.rgba(0, 0, 0, 0.32) }
        background: Rectangle {
            color: _card
            border.color: _border
            border.width: 1.5
            radius: _dialogRadius
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 16

            Label {
                text: createMode === "starmap" ? qsTr("新建子星图") : qsTr("新建节点")
                font.pointSize: dt.fontLgPt
                font.bold: true
                color: Kirigami.Theme.textColor
            }

            TextField {
                id: createInput
                Layout.fillWidth: true
                Layout.preferredHeight: 36
                color: Kirigami.Theme.textColor
                font.pointSize: dt.bodyPt
                placeholderText: qsTr("名称")
                focus: createDialog.visible
                text: ""

                background: Rectangle {
                    color: _surfaceContainer
                    border.color: createInput.activeFocus ? _accent : _border
                    border.width: 1.5
                    radius: _radiusXs
                }

                Keys.onReturnPressed: createDialog.confirm()
                Keys.onEscapePressed: createDialog.close()
            }

            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 12

                Button {
                    id: createCancelBtn
                    text: qsTr("取消")
                    onClicked: createDialog.close()
                    contentItem: Label {
                        text: createCancelBtn.text
                        color: Kirigami.Theme.disabledTextColor
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: createCancelBtn.hovered ? _surfaceContainer : "transparent"
                        border.color: _border
                        radius: _radiusXs
                    }
                }

                Button {
                    id: createConfirmBtn
                    text: qsTr("确定")
                    onClicked: createDialog.confirm()
                    contentItem: Label {
                        text: createConfirmBtn.text
                        color: Kirigami.Theme.highlightedTextColor
                        font.bold: true
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: createConfirmBtn.hovered ? _accentHover : _accent
                        radius: _radiusXs
                    }
                }
            }
        }

        function open(mode, sceneX, sceneY) {
            createMode = mode
            contextMenuSceneX = sceneX
            contextMenuSceneY = sceneY
            createInput.text = ""
            visible = true
            createInput.forceActiveFocus()
        }

        function close() {
            visible = false
        }

        function confirm() {
            var name = createInput.text.trim()
            if (name.length === 0 || !menuOwnerContent) {
                close()
                return
            }
            // Issue #929：归属画布自己按 world 坐标新建；活动画布是无限画布，
            // 不做圆内安全区 clamp。
            if (createMode === "node")
                menuOwnerContent.createNodeWithName(name, contextMenuSceneX, contextMenuSceneY)
            else if (createMode === "starmap")
                menuOwnerContent.createSubStarmapWithName(name, contextMenuSceneX, contextMenuSceneY)
            close()
            // 结构刷新由 GraphController 在真实持久化成功后发出的
            // structureChanged 统一触发，失败时不误报。
        }
    }

    // ---------------------------------------------------------------------------
    // Issue #834：内部链接弹窗（StarMapLink，内部跳转）。
    // 列出该 source 已有的内部链接，可删除；"选择目标"进入 linkArmed，
    // 下一次点 node/embed 是 target（不填 URI，不是外部超链接）。
    // ---------------------------------------------------------------------------
    Popup {
        id: linkDialog
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        width: 360
        height: 320
        anchors.centerIn: Overlay.overlay
        Overlay.modal: Rectangle { color: Qt.rgba(0, 0, 0, 0.32) }
        background: Rectangle {
            color: _card
            border.color: _border
            border.width: 1.5
            radius: _dialogRadius
        }

        property string sourceKind: ""    // "node" / "embed"
        property string sourceId: ""
        property var sourceOwner: null    // 命中层 StarMapActiveScene
        property var sourcePath: null     // StarMapTargetPathDto
        property var linkItems: []        // 已有内部链接列表

        function refreshItems() {
            if (sourceOwner && sourcePath)
                linkItems = sourceOwner.listLinksForSource(sourcePath)
            else
                linkItems = []
        }

        // target 路径摘要：node 显示 nodeId，starmap(embed) 显示"子星图"。
        function targetSummary(item) {
            var link = item && item.link ? item.link : null
            if (!link || !link.target)
                return ""
            var t = link.target.target
            if (!t)
                return ""
            return t.type === "node" ? (t.nodeId || "") : qsTr("子星图")
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 12

            Label {
                text: qsTr("内部链接")
                font.pointSize: dt.fontLgPt
                font.bold: true
                color: Kirigami.Theme.textColor
            }

            Label {
                text: linkDialog.linkItems.length > 0
                      ? qsTr("已链接到：")
                      : qsTr("尚无内部链接，选择目标开始建立。")
                color: Kirigami.Theme.disabledTextColor
                font.pointSize: dt.labelPt
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
            }

            // 已有链接列表（每条：摘要 + 删除按钮）
            ListView {
                id: linkList
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: linkDialog.linkItems
                spacing: 6
                delegate: RowLayout {
                    width: linkList.width
                    spacing: 8

                    Label {
                        Layout.fillWidth: true
                        text: linkDialog.targetSummary(modelData)
                                + (modelData.link.label ? "（" + modelData.link.label + "）" : "")
                        color: Kirigami.Theme.textColor
                        font.pointSize: dt.bodyPt
                        elide: Text.ElideRight
                    }

                    Button {
                        id: linkReassignBtn
                        text: qsTr("重新指定")
                        onClicked: {
                            // 重新指定这条 Link 的 target：进入 linkArmed 并记住
                            // existingLinkId + oldHostOwner，选新 target 后 finishLink
                            // 走 update。
                            if (modelData.hostOwner && modelData.link.linkId) {
                                interaction.beginLinkArmed(
                                        linkDialog.sourceKind,
                                        linkDialog.sourceId,
                                        linkDialog.sourcePath,
                                        linkDialog.sourceOwner.scenePathKey,
                                        modelData.link.linkId,
                                        modelData.hostOwner,
                                        modelData.link.label || "")
                                linkDialog.close()
                            }
                        }
                        contentItem: Label {
                            text: linkReassignBtn.text
                            color: Kirigami.Theme.disabledTextColor
                            font.pointSize: dt.labelPt
                        }
                        background: Rectangle {
                            color: linkReassignBtn.hovered ? _surfaceContainer : "transparent"
                            border.color: _border
                            radius: _radiusXs
                        }
                    }

                    Button {
                        id: linkDelBtn
                        text: qsTr("删除")
                        onClicked: {
                            // 删除必须调这条 Link 自己的 hostOwner.deleteLink，
                            // 不能固定 sourceOwner。
                            if (modelData.hostOwner && modelData.link.linkId) {
                                modelData.hostOwner.deleteLink(modelData.link.linkId)
                                linkDialog.refreshItems()
                            }
                        }
                        contentItem: Label {
                            text: linkDelBtn.text
                            color: Kirigami.Theme.negativeTextColor
                            font.pointSize: dt.labelPt
                        }
                        background: Rectangle {
                            color: linkDelBtn.hovered ? _dangerContainer : "transparent"
                            border.color: _border
                            radius: _radiusXs
                        }
                    }
                }
            }

            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 12

                Button {
                    id: linkCancelBtn
                    text: qsTr("取消")
                    onClicked: linkDialog.close()
                    contentItem: Label {
                        text: linkCancelBtn.text
                        color: Kirigami.Theme.disabledTextColor
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: linkCancelBtn.hovered ? _surfaceContainer : "transparent"
                        border.color: _border
                        radius: _radiusXs
                    }
                }

                Button {
                    id: linkPickTargetBtn
                    text: qsTr("选择目标")
                    onClicked: {
                        // 关闭弹窗，进入 linkArmed：下一次点 node/embed 是 target。
                        if (linkDialog.sourceOwner && linkDialog.sourcePath) {
                            interaction.beginLinkArmed(linkDialog.sourceKind,
                                    linkDialog.sourceId, linkDialog.sourcePath,
                                    linkDialog.sourceOwner.scenePathKey)
                        }
                        linkDialog.close()
                    }
                    contentItem: Label {
                        text: linkPickTargetBtn.text
                        color: Kirigami.Theme.highlightedTextColor
                        font.bold: true
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: linkPickTargetBtn.hovered ? _accentHover : _accent
                        radius: _radiusXs
                    }
                }
            }
        }

        function open(kind, id, owner, sourcePath) {
            sourceKind = kind
            sourceId = id
            sourceOwner = owner
            linkDialog.sourcePath = sourcePath
            refreshItems()
            visible = true
        }

        function close() {
            visible = false
        }
    }
}
