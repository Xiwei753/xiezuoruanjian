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
//   - 连续缩放穿越：入口投影直径占当前视口短边 ≥ 70% 进入子画布；
//     反向缩小到 ≤ 60% 恢复父镜头。进入只在放大方向评估，退出只在缩小
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
// 约束：
//   - 纯渲染和输入宿主层，星图业务逻辑在 Core 与各画布的 StarMapGraphController
//   - 不读写 Core 的相机/视口字段；切换活动画布不改变任何 authored position
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: canvasArea
    clip: true

    // 根星图 ID：整条导航路由的起点（Core 的 rootStarmapId）。
    property string starmapId: ""
    required property var dt

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

    readonly property string errorMessage:
        routeError.length > 0
        ? routeError
        : (activeScene() ? activeScene().errorMessage : "")

    // ── 活动镜头（当前活动画布的唯一相机）──
    // 屏幕坐标 = pan + world × zoom；pan 是世界原点的屏幕位置。
    property real zoomLevel: 1.0
    property real panX: 0
    property real panY: 0
    // Issue #822 评论 5972215936: 只保留数值安全边界，不设产品硬天花板。
    // 边界只是防止浮点/图形管线爆掉，绝不用来触发层级切换。
    readonly property real _cameraScaleMin: 1e-4
    readonly property real _cameraScaleMax: 1e5
    readonly property real _zoomFactor: 1.2

    // routeKey -> { zoom, panX, panY }：每一层进入前保存的父相机。
    property var cameraStack: ({})
    // childRouteKey -> 进入上下文（父/子比例、入口世界几何），退场反推父镜头用。
    property var entryContexts: ({})
    // 本次连续缩放瞄准的入口（routeKey/instanceId）。只比较这个入口，
    // 不会因为鼠标停在别处缩放而进入别的子图。
    property string _zoomEntryKey: ""
    // 捏合穿越补偿：Router 的捏合是"捏合开始 zoom × 累计比例"的绝对值，
    // 画布切换后把这一次绝对值换算成新画布上的连续比例。
    property real _pinchSwitchBaseZoom: -1
    property real _pinchSwitchCanvasZoom: 1
    property bool _initialized: false

    // 画布穿越阈值：入口投影直径占当前视口短边。
    readonly property real _enterRatio: 0.70
    readonly property real _exitRatio: 0.60

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

    // Core 定位当前活动画布：route 解析失败不回退到裸目标 ID。
    function applyRoute(route) {
        var normalized = normalizeRoute(route)
        if (!normalized) {
            currentRoute = { rootStarmapId: "", segments: [] }
            currentStarmapId = ""
            routeError = ""
            return
        }
        currentRoute = normalized
        if (!starmapBackendRef) {
            currentStarmapId = normalized.segments.length === 0 ? normalized.rootStarmapId : ""
            routeError = ""
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
        cameraStack = ({})
        entryContexts = ({})
        _zoomEntryKey = ""
        _pinchSwitchBaseZoom = -1
        _pinchSwitchCanvasZoom = 1
        var route = { rootStarmapId: starmapId, segments: [] }
        applyRoute(route)
        applyCamera(defaultCamera())
        storeCamera(routeKeyOf(route), cameraSnapshot())
        visualFocusChanged()
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
        if (_initialized && currentRoute.rootStarmapId !== "")
            applyRoute(currentRoute)
    }

    // 相机任何变化都写回当前 route 的相机槽（同一张画布再次进入时可用）。
    onZoomLevelChanged: storeCamera(routeKey, cameraSnapshot())
    onPanXChanged: storeCamera(routeKey, cameraSnapshot())
    onPanYChanged: storeCamera(routeKey, cameraSnapshot())

    // ── 纯方法：平移 / 缩放 / 坐标换算 ──
    function applyPan(nextX, nextY) {
        panX = nextX
        panY = nextY
    }

    function panBy(dx, dy) {
        applyPan(panX + dx, panY + dy)
    }

    // world ↔ 屏幕换算：优先用活动场景的投影（视口中心相对原点），
    // 没有活动场景时用同一数学口径的直接式。
    function worldToScreenX(wx) {
        var scene = activeScene()
        if (scene)
            return scene.worldToScreenX(wx)
        return panX + wx * zoomLevel
    }
    function worldToScreenY(wy) {
        var scene = activeScene()
        if (scene)
            return scene.worldToScreenY(wy)
        return panY + wy * zoomLevel
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

    // 唯一缩放入口：滚轮、捏合、+/- 按钮都走这里，只改当前活动画布的镜头。
    // 随后按"指针瞄准的入口"评估画布穿越（70% 进 / 60% 出），zoom 数值本身
    // 不参与判断。穿越按缩放方向过滤：只有放大才评估进场，只有缩小才评估退场；
    // 缩小经过入口不进入子画布，任意层同样规则（双击不受方向过滤）。
    function zoomAt(screenX, screenY, nextZoom) {
        var requested = nextZoom
        if (interaction && interaction.pointerMode === "pinch" && _pinchSwitchBaseZoom > 0)
            requested = _pinchSwitchCanvasZoom * (nextZoom / _pinchSwitchBaseZoom)
        var target = Math.max(_cameraScaleMin, Math.min(_cameraScaleMax, requested))
        var oldZoom = zoomLevel
        if (target === oldZoom)
            return
        var zoomingIn = target > oldZoom
        var zoomingOut = target < oldZoom
        zoomLevel = target
        applyPan(
            screenX - (screenX - panX) * (zoomLevel / oldZoom),
            screenY - (screenY - panY) * (zoomLevel / oldZoom)
        )
        var switched = -1
        if (zoomingIn) {
            // 放大：任意层只评估进场（根→子、子→孙同理）。
            switched = maybeEnterEmbedAt(screenX, screenY)
        } else if (zoomingOut) {
            // 缩小：任意层只评估退场，不评估进场。
            switched = maybeExitCanvas()
        }
        if (switched > 0 && interaction && interaction.pointerMode === "pinch") {
            // 基准配对（#929 新评论）：nextZoom 是 PinchHandler 在旧基准下的累计值，
            // 尚未转换到新画布；enterEmbedViaCore/exitCanvasViaCore 已把 zoomLevel
            // 设为新画布 zoom。若这里记 zoomLevel 作基准，下一帧
            // requested = switched * (nextZoom / switched) = nextZoom，跨画布尺度
            // 换算失效、刚穿越就跳变。父→子、子→父、子→孙每次新穿越都重设该配对。
            _pinchSwitchBaseZoom = nextZoom
            _pinchSwitchCanvasZoom = switched
        }
    }

    function viewportShortSide() {
        return Math.min(width, height)
    }

    // 覆盖率口径：入口投影直径 / 当前视口短边。
    function coverageOfScreenPx(px) {
        var short = viewportShortSide()
        return short > 0 ? px / short : 0
    }

    // 缩放后：只比较指针瞄准的那个入口。没瞄准任何入口就不进入任何子图。
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
        var projectedPx = Math.min(emb.width, emb.height) * zoomLevel
        if (coverageOfScreenPx(projectedPx) < _enterRatio)
            return -1
        return enterEmbedViaCore(emb.instanceId, "zoom")
    }

    // 反向缩小：按进入上下文反推父入口的投影直径，≤ 60% 时恢复父画布。
    // 双击可能在远低于 70% 的入口上进入：退场阈值取 min(60%, 进入时的覆盖率)，
    // 保证"至少要从进入状态继续缩小"才会退场，不会一进子图就被弹回父图。
    function maybeExitCanvas() {
        if (focusIsRoot)
            return -1
        var ctx = entryContexts[routeKey]
        if (!ctx || !(ctx.ratio > 0))
            return -1
        var virtualParentZoom = zoomLevel / ctx.ratio
        var coverageNow = coverageOfScreenPx(ctx.embedDiameter * virtualParentZoom)
        var entryCoverage = ctx.entryCoverage > 0 ? ctx.entryCoverage : _exitRatio
        if (coverageNow >= entryCoverage)
            return -1
        if (coverageNow > Math.min(_exitRatio, entryCoverage))
            return -1
        return exitCanvasViaCore(true)
    }

    // 进入子画布（双击、连续缩放都用这一个入口）。返回新画布的 zoom，失败 -1。
    function enterEmbedViaCore(instanceId, trigger) {
        var scene = activeScene()
        if (!scene || !starmapBackendRef || !instanceId)
            return -1
        var info = scene.embedEntryInfo(instanceId)
        if (!info)
            return -1
        var parentZoom = zoomLevel
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
        // 连续性：子图 world→屏幕比例 = 概览的 fit 比例（进入后画面与概览同尺寸）；
        // 子图视口中心由父画布视口中心映射过去，入口切画布前后停在同一屏幕位置。
        var k = (info.fitScale > 0 && parentZoom > 0) ? info.fitScale / parentZoom : 1.0
        var boundsCX = info.bounds ? (info.bounds.minX + info.bounds.maxX) / 2 : 0
        var boundsCY = info.bounds ? (info.bounds.minY + info.bounds.maxY) / 2 : 0
        var childZoom = info.fitScale > 0 ? info.fitScale : parentZoom
        childZoom = Math.max(_cameraScaleMin, Math.min(_cameraScaleMax, childZoom))
        var parentCenterX = screenToWorldX(width / 2)
        var parentCenterY = screenToWorldY(height / 2)
        var childCenterX = boundsCX + (parentCenterX - info.centerX) * k
        var childCenterY = boundsCY + (parentCenterY - info.centerY) * k
        var childCamera = {
            zoom: childZoom,
            panX: width / 2 - childCenterX * childZoom,
            panY: height / 2 - childCenterY * childZoom
        }
        storeEntryContext(childKey, {
            parentRouteKey: routeKey,
            instanceId: instanceId,
            ratio: k,
            embedDiameter: Math.min(info.width, info.height),
            embedCenterX: info.centerX,
            embedCenterY: info.centerY,
            boundsCenterX: boundsCX,
            boundsCenterY: boundsCY,
            // 进入瞬间入口的投影覆盖率：双击在很小的入口上进入时可能低于 60%，
            // 退场阈值必须再受它约束（见 maybeExitCanvas），否则一进子图、
            // 甚至继续放大都会立刻被退场判定弹回父图。
            entryCoverage: coverageOfScreenPx(Math.min(info.width, info.height) * parentZoom)
        })
        storeCamera(routeKey, cameraSnapshot())
        storeCamera(childKey, childCamera)
        prepareCanvasSwitch()
        applyRoute(newRoute)
        applyCamera(childCamera)
        logInteraction("scene_enter", "embed", instanceId, {
            "trigger": trigger || "",
            "zoomEntryKey": _zoomEntryKey,
            "childRouteKey": childKey,
            "childStarmapId": res.data.location.currentStarmapId,
            "parentZoom": parentZoom,
            "childZoom": childZoom
        })
        visualFocusChanged()
        return childZoom
    }

    // 退出当前画布，回到 Core 返回的父 route。continuous=true 时按当前镜头
    // 反推连续父镜头（缩放穿越用）；false 时恢复进入前的父镜头（返回按钮用）。
    function exitCanvasViaCore(continuous) {
        if (focusIsRoot || !starmapBackendRef)
            return -1
        var childKey = routeKey
        var childZoom = zoomLevel
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
        var cam = cameraStack[parentKey]
        if (continuous && ctx && ctx.ratio > 0) {
            var parentZoom = Math.max(_cameraScaleMin,
                                      Math.min(_cameraScaleMax, childZoom / ctx.ratio))
            var childCenterX = screenToWorldX(width / 2)
            var childCenterY = screenToWorldY(height / 2)
            var parentCenterX = ctx.embedCenterX + (childCenterX - ctx.boundsCenterX) / ctx.ratio
            var parentCenterY = ctx.embedCenterY + (childCenterY - ctx.boundsCenterY) / ctx.ratio
            cam = {
                zoom: parentZoom,
                panX: width / 2 - parentCenterX * parentZoom,
                panY: height / 2 - parentCenterY * parentZoom
            }
        }
        if (!cam)
            cam = defaultCamera()
        storeCamera(parentKey, cam)
        prepareCanvasSwitch()
        applyRoute(parentRoute)
        applyCamera(cam)
        logInteraction("scene_exit", "scene", "", {
            "parentRouteKey": parentKey,
            "continuous": !!continuous,
            "childZoom": childZoom,
            "parentZoom": cam.zoom
        })
        visualFocusChanged()
        return cam.zoom
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
    // 状态，让同一次捏合继续作用在新画布上。
    function prepareCanvasSwitch() {
        setInlineEditingKey("")
        hideTouchPreview()
        if (inputRouter)
            inputRouter.cancelLocalState()
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
    function hitTargetAtScreen(sx, sy) {
        var scene = activeScene()
        if (!scene)
            return null
        return scene.hitTargetAtWorld(screenToWorldX(sx), screenToWorldY(sy))
    }

    // Issue #814 评论 5935346839: pointer_press 是完整手势的起点边界。
    function logPointerPress(button, device, point) {
        var hit = hitTargetAtScreen(point.position.x, point.position.y)
        var kind = hit ? hit.kind : "empty"
        var hitId = hit ? hit.id : ""
        var hitPathKey = hit ? hit.scenePathKey : routeKey
        logInteraction("pointer_press", kind, hitId, {
            "button": button,
            "device": device,
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
            inputRouter.cancelLocalState()
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
    // 活动画布：本文件只加载 currentStarmapId 的一张 StarMapActiveScene。
    // 相机只是注入给它的两个标量（zoom / pan），内容自己按投影实时绘制。
    // ---------------------------------------------------------------------------
    Loader {
        id: activeSceneLoader
        anchors.fill: parent
        z: 1
        active: canvasArea.currentStarmapId !== ""
        sourceComponent: activeSceneComponent
    }

    Component {
        id: activeSceneComponent
        StarMapActiveScene {
            dt: canvasArea.dt
            starmapId: canvasArea.currentStarmapId
            rootStarmapId: canvasArea.currentRoute.rootStarmapId
            pathSegments: canvasArea.currentRoute.segments
            scenePathKey: canvasArea.routeKey
            starmapBackendRef: canvasArea.starmapBackendRef
            selectionController: canvasArea.selectionController
            interactionController: canvasArea.sharedInteraction
            menuHost: canvasArea
            zoomLevel: canvasArea.zoomLevel
            panX: canvasArea.panX
            panY: canvasArea.panY

            onNodeSelected: function(node) { canvasArea.nodeSelected(node) }
            onEdgeSelected: function(edge) { canvasArea.edgeSelected(edge) }
            onSelectionCleared: canvasArea.selectionCleared()
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
                var gridSpacing = worldSpacing * zoomLevel
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
