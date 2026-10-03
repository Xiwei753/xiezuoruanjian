// =============================================================================
// StarMapCanvas.qml — 星图画布（整棵星图唯一的 viewport / camera）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：全局相机（pan/zoom）、全局输入入口、根层内容容器、右键菜单与弹窗
//
// Issue #822：整棵星图只有一个全局视口。
//   panX / panY / zoomLevel 只在这里存在，WheelHandler / PinchHandler /
//   触屏 +/- 按钮也只在这里。鼠标停在任意深度的节点、子星图、孙星图上，
//   滚轮和捏合都调同一个 zoomAround()，只改根 zoomLevel/panX/panY。
//   子星图"看起来更大/更小"是 Deep Zoom 显示档位：每层内容做 local fit，
//   ownerEffectiveScale = globalZoom × 祖先 local fit，再用投影覆盖率决定
//   子内容是完整交互 / 轻量 preview / 只留外壳。档位绝不反写全局相机，
//   也不改 Embed 的 world 几何（见 StarMapEmbed / docs/starmap_viewport.md）。
//
//   递归的是"内容"不是"视口"：根层内容由 StarMapSceneContent 渲染，
//   子星图内容在 Embed 内部懒加载下一层 StarMapSceneContent。
//   节点/连线都画在各自层的局部坐标里，相机只作用于根层 Content 这一张 Item。
//
//   交互状态机也只有一个：StarMapInteractionController 由本文件创建一次，
//   整棵递归树共享。连线预览线也只有这一层 overlay 一条，不每层各画一条。
//
//   命中判断统一走根层内容的递归接口 hitTargetAtScene()：
//   它先查本层节点/Embed chrome，落在子星图内容区时递归进子层，
//   返回真正命中的那一层（owner/scenePathKey/starmapId/kind/id/targetPath）。
//   连线松手、右键空白新建、选中、pointer_press 日志全部走这一个入口，
//   不再只认识根层的 findNodeAt/findEmbedChromeAt。
//
// 约束：
//   - 纯渲染和输入层，星图业务逻辑委托给各层 StarMapGraphController
//   - 鼠标行为由共享 pointerMode 状态机明确驱动
//   - 使用 Canvas 进行自定义绘制
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: canvasArea
    clip: true

    // 根星图 ID。根层内容用它作为整棵递归树的 rootStarmapId。
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
    readonly property string errorMessage: rootContent ? rootContent.errorMessage : ""

    // ── 全局相机（唯一）──
    // Issue #814 评论 5935285879: 无限画布 — pan 两个方向都不设边界。
    // 改用"根层 Content 一张带 transform 的 Item + delegate 直接局部坐标"承载世界后，
    // 世界坐标可以是任意正负值，pan 不再需要 clamp。
    property real panX: 0
    property real panY: 0
    property real zoomLevel: 1.0

    // Issue #822 评论 5972215936: 相机范围只保留数值安全边界，不设产品硬天花板。
    // 与 docs/starmap_viewport.md 的 CAMERA_SCALE_MIN/MAX、ZOOM_FACTOR 同一口径：
    // 乘法步进保证各档手感一致，Deep Zoom 的 coverage 档位由用户能到的真实比例决定
    // （0.35~2.5 的旧硬上限会让 1080 高窗口里的子星图永远停在 preview）。
    readonly property real _cameraScaleMin: 1e-4
    readonly property real _cameraScaleMax: 1e5
    readonly property real _zoomFactor: 1.2

    function applyPan(nextX, nextY) {
        panX = nextX
        panY = nextY
    }

    // scene 坐标（整棵递归树的顶层 world 坐标）↔ 视口坐标。
    // 相机只有这一个，所以换算是全局唯一入口，不再有"每层各自的 zoomLevel"。
    function worldToScreenX(wx) { return panX + wx * zoomLevel }
    function worldToScreenY(wy) { return panY + wy * zoomLevel }
    function screenToWorldX(sx) { return (sx - panX) / zoomLevel }
    function screenToWorldY(sy) { return (sy - panY) / zoomLevel }

    // Issue #822: 滚轮和捏合共用同一个缩放入口，只改根 zoomLevel/panX/panY。
    // 鼠标停在节点/子星图/孙星图上都不影响：整棵树一起缩放。
    // 只夹数值安全范围（CAMERA_SCALE_MIN/MAX），不再有产品缩放上限。
    function zoomAround(screenX, screenY, nextZoom) {
        var target = Math.max(_cameraScaleMin, Math.min(_cameraScaleMax, nextZoom))
        var oldZoom = zoomLevel
        if (target === oldZoom)
            return
        zoomLevel = target
        applyPan(
            screenX - (screenX - panX) * (zoomLevel / oldZoom),
            screenY - (screenY - panY) * (zoomLevel / oldZoom)
        )
    }

    // 当前可见区域（scene 坐标矩形），逐层传给内容做子星图懒加载判定。
    readonly property var viewportSceneRect: ({
        x: screenToWorldX(0),
        y: screenToWorldY(0),
        width: width / (zoomLevel > 0 ? zoomLevel : 1),
        height: height / (zoomLevel > 0 ? zoomLevel : 1)
    })

    // DragHandler 的 activeTranslation 是 Qt scene 坐标增量。
    // 映射到本 Canvas local 再用；根层无祖先 scale 时退化成恒等。
    function sceneDeltaToCanvas(dx, dy) {
        var o = canvasArea.mapFromItem(null, 0, 0)
        var p = canvasArea.mapFromItem(null, dx, dy)
        return { x: p.x - o.x, y: p.y - o.y }
    }

    // Issue #801 评论 5894639734: +/- 触屏按钮按需显示，鼠标模式不常驻。
    property bool _touchInputActive: false

    // Issue #814 评论 5935346839: pan 手势起点记录，用于 pan_end 边界日志。
    property real _panBeginX: 0
    property real _panBeginY: 0

    // Issue #814 评论 5935346839: 星图交互边界日志统一入口。
    // pathKey 显式传参：现在是全局相机，没有"哪一层是 root"的隐含身份判断。
    function logInteraction(event, itemKind, itemId, fields, scenePathKey) {
        if (!starmapBackendRef) return
        var fj = fields ? JSON.stringify(fields) : ""
        starmapBackendRef.record_interaction(event, scenePathKey === undefined ? "root" : scenePathKey,
                                              starmapId, itemKind, itemId, fj)
    }

    // Issue #822: 统一命中判断入口 —— 递归命中测试，从根层内容开始往下钻。
    // 空白点击、拖动画布、pointer_press、右键菜单全部共用。
    function hitTargetAtScreen(sx, sy) {
        if (!rootContent) return null
        return rootContent.hitTargetAtScene(screenToWorldX(sx), screenToWorldY(sy))
    }

    // Issue #814 评论 5935346839: pointer_press 是完整手势的起点边界。
    // 统一由根节点上的 passive-grab PointHandler 观察 press：不抢事件，
    // 命中的对象照常拿到完整交互；hitKind/hitId 在按下当场递归命中重算。
    function logPointerPress(button, device, point) {
        var hit = hitTargetAtScreen(point.position.x, point.position.y)
        var kind = hit ? hit.kind : "empty"
        var hitId = hit ? hit.id : ""
        var hitPathKey = hit ? hit.scenePathKey : "root"
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

    // ---------------------------------------------------------------------------
    // Issue #822：整棵递归树唯一的交互状态机，就在这里创建一次。
    // 各层 StarMapSceneContent 拿到的都是同一个实例（经 Embed 原样下传）。
    // ---------------------------------------------------------------------------
    StarMapInteractionController {
        id: interaction
        // 长按阈值用系统的 mousePressAndHoldInterval，和平台其它长按一致。
        longPressInterval: Application.styleHints.mousePressAndHoldInterval > 0
                ? Application.styleHints.mousePressAndHoldInterval
                : 800
    }

    // Issue #822: 长按计时的 Timer 只能挂在 Item 下（InteractionController 是
    // QtObject，没有默认属性），所以放在这里，由 pressTimerActive 驱动。
    // Timer 到点只发 pressTimeout 信号；把 pressPending 提升成 move 还是 connect
    // 由手势归属层 StarMapSceneContent 判断，状态只被提升一次。
    Timer {
        id: pressLongPressTimer
        interval: interaction.longPressInterval
        repeat: false
        running: interaction.pressTimerActive
        onTriggered: {
            interaction.pressTimerActive = false
            interaction.pressTimeout()
        }
    }

    readonly property var sharedInteraction: interaction

    // 菜单辅助状态：菜单归属层就是被点中的那一层。
    property var menuOwnerContent: null
    property var selectedNodeForMenu: null
    property var selectedEdgeForMenu: null
    property var selectedEmbedForMenu: null
    // 右键命中点的 scene 坐标，创建类操作由归属层换算成局部坐标。
    property real contextMenuSceneX: 0
    property real contextMenuSceneY: 0

    // 新建对话框状态：先收集名字再写 Core（Issue #793 评论 5884923277）
    property string createMode: ""      // "node" / "starmap"

    // Signals
    signal nodeSelected(var node)
    signal edgeSelected(var edge)
    signal selectionCleared()

    // Issue #805 评论 5907045450 第 1 部分：删掉 drillDownRequested / drillUpRequested。
    // 递归渲染由 StarMapSceneContent 处理，Canvas 不再上抛层级切换请求。
    // Issue #822: 删掉 editNodeRequested / childEditNodeRequested ——
    // 节点标题改成节点自身的内联编辑，不再经过外部 Popup。

    function clearError() {
        if (rootContent) rootContent.clearError()
    }

    // Issue #798: 公开 reset 入口，供 Workspace 切图 / 不可见时清瞬时交互状态。
    function resetInteraction() {
        // Issue #798 评论 5892406254: reset 前若正在 move，edgeRenders 已被
        // transient 坐标更新。reset 后 delegate 回 canonical，edge cache 也要
        // 一起恢复 canonical，否则节点回去了线还停在拖动位置。
        interaction.reset()
        // Issue #817 评论 5953678540: 一起清 bgDragArea 的本地 pan 手势状态。
        bgDragArea.resetMouseGesture()
        if (rootContent) rootContent.refreshAllEdges()
    }

    // ---------------------------------------------------------------------------
    // 相机层 + 根层内容：整棵递归树的入口。
    // 相机平移/缩放只存在于 cameraLayer 这一张 Item 上（x/y/scale），
    // rootContent 只用 anchors.fill 占满相机层，绝不自己再拿 x/y 当平移 ——
    // 一个 Item 上不能同时有 anchors 和相机两套几何来源。
    // ---------------------------------------------------------------------------
    Item {
        id: cameraLayer
        x: canvasArea.panX
        y: canvasArea.panY
        width: canvasArea.width
        height: canvasArea.height
        scale: canvasArea.zoomLevel
        transformOrigin: Item.TopLeft

        StarMapSceneContent {
            id: rootContent
            anchors.fill: parent
            dt: canvasArea.dt
            rootStarmapId: canvasArea.starmapId
            pathSegments: []
            // Issue #822: 根层显式传 "root"，不再由默认值冒名顶替。
            scenePathKey: "root"
            starmapBackendRef: canvasArea.starmapBackendRef
            selectionController: canvasArea.selectionController
            interactionController: canvasArea.sharedInteraction
            globalZoom: canvasArea.zoomLevel
            depth: 0
            rootContent: rootContent
            rootViewportRect: canvasArea.viewportSceneRect
            menuHost: canvasArea

            onNodeSelected: function(node) { canvasArea.nodeSelected(node) }
            onEdgeSelected: function(edge) { canvasArea.edgeSelected(edge) }
            onSelectionCleared: canvasArea.selectionCleared()
        }
    }

    // Issue #822: 整棵递归树共享的选中状态控制器，由 Workspace 创建并传入。
    property var selectionController: null

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

                var gridSpacing = 50 * zoomLevel
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
    // 背景交互层：TapHandler 处理点击类，MouseArea 处理 pan 拖动，WheelHandler 缩放。
    // 桌面指针/触屏按 acceptedDevices 拆开：
    //   - 桌面指针（Mouse | TouchPad）：单击选中 / 拖动移动或平移 / 右键菜单
    //   - 触屏（TouchScreen）：长按弹菜单 / 滑动平移或拉线
    //
    // Issue #812: 桌面指针的 acceptedDevices 必须是 Mouse | TouchPad，
    // 不能只写 Mouse。acceptedDevices 是硬过滤，设备类型不匹配时
    // Handler 根本不参与这个事件。
    //
    // Issue #806 评论 5907045450: 这些 handler 直接挂在 canvasArea 上。
    // Qt 的事件会投递给「命中点所在最深 item」及其祖先链上的 handler，
    // 所以挂在根上就能覆盖整棵递归树，不必每层各挂一套。
    //
    // Issue #822: 这些 handler 不再问"当前是哪一层 Scene"，命中交给递归接口。
    // ---------------------------------------------------------------------------

    // 鼠标左键单击：递归命中后选中或清选区
    TapHandler {
        id: bgMouseLeftTap
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        acceptedButtons: Qt.LeftButton
        onSingleTapped: function(eventPoint) {
            _touchInputActive = false
            var hit = hitTargetAtScreen(eventPoint.position.x, eventPoint.position.y)
            if (!hit)
                return
            var sx = screenToWorldX(eventPoint.position.x)
            var sy = screenToWorldY(eventPoint.position.y)
            if (hit.kind === "edge") {
                hit.owner.selectEdge(hit.id)
                logInteraction("selection_changed", "edge", hit.id, {
                    "device": "mouse"
                }, hit.scenePathKey)
            } else if (hit.kind === "empty") {
                // Issue #822: 空白清选区，空子星图内部也走同一条路径。
                if (hit.owner) hit.owner.clearLayerSelection()
                logInteraction("selection_changed", "none", "", {
                    "device": "mouse"
                }, hit.scenePathKey)
            }
            // node / embed / childContent 由各自的 delegate 处理，这里不吞。
        }
    }

    // 触屏左键单击：同鼠标；长按在空白处打开归属层的背景菜单
    TapHandler {
        id: bgTouchLeftTap
        acceptedDevices: PointerDevice.TouchScreen
        acceptedButtons: Qt.LeftButton
        onSingleTapped: function(eventPoint) {
            _touchInputActive = true
            var hit = hitTargetAtScreen(eventPoint.position.x, eventPoint.position.y)
            if (!hit)
                return
            var sx = screenToWorldX(eventPoint.position.x)
            var sy = screenToWorldY(eventPoint.position.y)
            if (hit.kind === "edge") {
                hit.owner.selectEdge(hit.id)
                logInteraction("selection_changed", "edge", hit.id, {
                    "device": "touch"
                }, hit.scenePathKey)
            } else if (hit.kind === "empty") {
                if (hit.owner) hit.owner.clearLayerSelection()
                logInteraction("selection_changed", "none", "", {
                    "device": "touch"
                }, hit.scenePathKey)
            }
        }
        // TapHandler.longPressed 信号无参数，用 point.position 拿当前点。
        // 长按前先递归判命中：命中对象的长按归 delegate，这里不弹背景菜单。
        onLongPressed: {
            _touchInputActive = true
            var px = bgTouchLeftTap.point.position.x
            var py = bgTouchLeftTap.point.position.y
            var hit = hitTargetAtScreen(px, py)
            if (!hit || hit.kind !== "empty")
                return
            openBlankMenu(screenToWorldX(px), screenToWorldY(py), hit, px, py)
        }
    }

    // 右键单击：递归命中后开对应菜单。
    // Issue #822: 空白处右键的菜单归属层就是被点中的那一层，
    // 子星图内部空白不需要"进入"另一个页面。
    TapHandler {
        id: backgroundRightTap
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        acceptedButtons: Qt.RightButton
        onSingleTapped: function(eventPoint) {
            _touchInputActive = false
            var px = eventPoint.position.x
            var py = eventPoint.position.y
            var hit = hitTargetAtScreen(px, py)
            if (!hit)
                return
            var sx = screenToWorldX(px)
            var sy = screenToWorldY(py)
            if (hit.kind === "node") {
                hit.owner.selectNode(hit.id)
                menuOwnerContent = hit.owner
                selectedNodeForMenu = hit
                logInteraction("context_menu_open", "node", hit.id, {
                    "menuKind": "node",
                    "sceneX": sx,
                    "sceneY": sy
                }, hit.scenePathKey)
                nodeContextMenu.popup(px, py)
            } else if (hit.kind === "embed") {
                hit.owner.selectEmbed(hit.id)
                menuOwnerContent = hit.owner
                selectedEmbedForMenu = hit
                logInteraction("context_menu_open", "embed", hit.id, {
                    "menuKind": "embed",
                    "sceneX": sx,
                    "sceneY": sy
                }, hit.scenePathKey)
                embedContextMenu.popup(px, py)
            } else if (hit.kind === "edge") {
                hit.owner.selectEdge(hit.id)
                menuOwnerContent = hit.owner
                selectedEdgeForMenu = hit
                logInteraction("context_menu_open", "edge", hit.id, {
                    "menuKind": "edge",
                    "sceneX": sx,
                    "sceneY": sy
                }, hit.scenePathKey)
                edgeContextMenu.popup(px, py)
            } else {
                openBlankMenu(sx, sy, hit, px, py)
            }
        }
    }

    // 触屏拖动：空白处滑动 = 全局画布 pan；
    // 已经有 press 归属（connect/contextPending/move）时只更新共享状态机。
    DragHandler {
        id: bgTouchDrag
        acceptedDevices: PointerDevice.TouchScreen
        acceptedButtons: Qt.LeftButton
        target: null
        property real lastTx: 0
        property real lastTy: 0
        onActiveChanged: {
            if (active) {
                lastTx = 0
                lastTy = 0
                _touchInputActive = true
            }
        }
        onActiveTranslationChanged: {
            var dx = activeTranslation.x - lastTx
            var dy = activeTranslation.y - lastTy
            lastTx = activeTranslation.x
            lastTy = activeTranslation.y
            var mode = interaction.pointerMode
            if (mode === "connect" || mode === "contextPending" || mode === "move") {
                // 归属层自己换算 scene→局部坐标，这里只交原始 scene 位移。
                if (rootContent) rootContent.onSceneDragDelta(dx, dy)
                return
            }
            if (mode === "pressPending") {
                // 触屏按下在节点/Embed 上：位移先累计，够阈值才提升为 move。
                if (rootContent) rootContent.onSceneDragDelta(dx, dy)
                return
            }
            var cd = sceneDeltaToCanvas(dx, dy)
            applyPan(panX + cd.x, panY + cd.y)
        }
    }

    // Issue #822: 捏合缩放只有这一处，作用在全局相机上。
    // 不再有"捏合归某个子星图"的判断：整棵树只有一个视口。
    // 捏合比例相对手势起点，统一交给 zoomAround 做数值夹取 + 以中心缩放，
    // 不再自己维护第二套 0.35/2.5 夹取和 pan 公式。
    PinchHandler {
        id: canvasPinch
        acceptedDevices: PointerDevice.TouchScreen
        target: null
        property real _pinchStartZoom: 1.0
        onActiveChanged: {
            if (active) {
                _pinchStartZoom = zoomLevel
                _touchInputActive = true
            }
        }
        onActiveScaleChanged: {
            var cx = centroid.position.x
            var cy = centroid.position.y
            zoomAround(cx, cy, _pinchStartZoom * activeScale)
        }
    }

    // Issue #814 评论 5935346839: press 边界观察器。
    // PointHandler 只取 passive grab，不参与 exclusive grab 竞争。
    PointHandler {
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        acceptedButtons: Qt.LeftButton
        onActiveChanged: {
            if (active)
                canvasArea.logPointerPress("left", "mouse", point)
        }
    }
    PointHandler {
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        acceptedButtons: Qt.MiddleButton
        onActiveChanged: {
            if (active)
                canvasArea.logPointerPress("middle", "mouse", point)
        }
    }
    PointHandler {
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        acceptedButtons: Qt.RightButton
        onActiveChanged: {
            if (active)
                canvasArea.logPointerPress("right", "mouse", point)
        }
    }
    // 触屏 press 观察器，只记 pointer_press 边界日志，不参与手势所有权。
    // Issue #822: 不再有"哪个手指属于哪个子星图"的判断——只有一个全局视口，
    // 触屏手势不需要按层让出。
    PointHandler {
        acceptedDevices: PointerDevice.TouchScreen
        acceptedButtons: Qt.NoButton
        onActiveChanged: {
            if (active)
                canvasArea.logPointerPress("left", "touch", point)
        }
    }

    // Issue #817 评论 5949494799: 背景 pan 拖动改为 press-time 手势归属 + 拖动阈值。
    // 按下时先用递归命中判断：node/embed/childContent 时 mouse.accepted = false
    // 让事件穿透给对应对象或子层内容；只有 empty/edge 才可能平移画布。
    // 中键直接平移。
    MouseArea {
        id: bgDragArea
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton | Qt.MiddleButton
        hoverEnabled: true

        property string pressHitKind: ""
        property real pressX: 0
        property real pressY: 0
        property real lastX: 0
        property real lastY: 0
        property bool panStarted: false

        // Issue #817 评论 5953678540: 统一清理 pan 手势本地状态。
        function resetMouseGesture() {
            pressHitKind = ""
            pressX = 0
            pressY = 0
            lastX = 0
            lastY = 0
            panStarted = false
        }

        onPressed: function(mouse) {
            _touchInputActive = false
            var hit = hitTargetAtScreen(mouse.x, mouse.y)

            if (mouse.button === Qt.LeftButton) {
                // 命中 node/embed/childContent 时不接受事件，
                // 让对应对象/子层内容处理；本层不进入 pan。
                if (hit && (hit.kind === "node" || hit.kind === "embed" || hit.kind === "childContent")) {
                    mouse.accepted = false
                    return
                }
                pressHitKind = hit ? hit.kind : "empty"
                pressX = mouse.x
                pressY = mouse.y
                lastX = mouse.x
                lastY = mouse.y
                panStarted = false
                return
            }

            // 中键直接进入 pan（不依赖长按/阈值）
            if (mouse.button === Qt.MiddleButton) {
                if (!interaction.beginPan())
                    return

                pressHitKind = "empty"
                panStarted = true
                lastX = mouse.x
                lastY = mouse.y
                _panBeginX = panX
                _panBeginY = panY
                logInteraction("pan_begin", "empty", "", {
                    "startPanX": panX,
                    "startPanY": panY,
                    "button": "middle",
                    "device": "mouse"
                })
            }
        }

        onPositionChanged: function(mouse) {
            if (pressHitKind !== "empty")
                return

            // 左键且尚未 panStarted：检查是否超过拖动阈值
            if (!panStarted && (mouse.buttons & Qt.LeftButton)) {
                var dx0 = mouse.x - pressX
                var dy0 = mouse.y - pressY
                if (Math.hypot(dx0, dy0) < bgMouseLeftTap.dragThreshold)
                    return

                if (!interaction.beginPan())
                    return

                panStarted = true
                lastX = mouse.x
                lastY = mouse.y
                _panBeginX = panX
                _panBeginY = panY
                logInteraction("pan_begin", "empty", "", {
                    "startPanX": panX,
                    "startPanY": panY,
                    "device": "mouse"
                })
                return
            }

            // panStarted（中键直接 true，或左键已超阈值）：继续 pan
            if (panStarted && interaction.pointerMode === "pan") {
                var dx = mouse.x - lastX
                var dy = mouse.y - lastY
                applyPan(panX + dx, panY + dy)
                lastX = mouse.x
                lastY = mouse.y
            }
        }

        onReleased: function(mouse) {
            // 只在 panStarted 时结束 pan。
            // 没有超过阈值就是普通点击，由 bgMouseLeftTap 处理选中/清选。
            if (panStarted) {
                interaction.endPan()
                logInteraction("pan_end", "empty", "", {
                    "startPanX": _panBeginX,
                    "startPanY": _panBeginY,
                    "endPanX": panX,
                    "endPanY": panY,
                    "device": "mouse"
                })
                panStarted = false
            }
            pressHitKind = ""
        }

        // 系统取消抓取时也要结束 pan 并清本地状态。
        onCanceled: {
            if (interaction.pointerMode === "pan")
                interaction.endPan()
            resetMouseGesture()
        }
    }

    // Issue #822: 滚轮缩放只由根 Canvas 唯一处理，没有 enabled: pathKey === "root"
    // 这种"按场景身份决定谁能缩放"的判断——已经不存在子 Canvas 了。
    // 鼠标停在第三层子星图内部，变化的也只是根 zoomLevel/panX/panY。
    WheelHandler {
        id: sceneWheel
        target: null
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        blocking: true

        onWheel: function(event) {
            _touchInputActive = false

            var delta = event.angleDelta.y !== 0
                ? event.angleDelta.y / 120
                : event.pixelDelta.y / 120.0

            if (delta === 0)
                return

            var oldZoom = zoomLevel
            // 乘法步进：每格滚轮 ×/÷ _zoomFactor，各档手感一致、不设产品上限，
            // 只由 zoomAround 夹数值安全范围（docs/starmap_viewport.md）。
            var newZoom = oldZoom * Math.pow(_zoomFactor, delta)
            if (newZoom === oldZoom)
                return

            var mx = point.position.x
            var my = point.position.y

            zoomAround(mx, my, newZoom)

            logInteraction("zoom_wheel", "scene", starmapId, {
                "oldZoom": oldZoom,
                "newZoom": zoomLevel,
                "screenX": mx,
                "screenY": my
            })
        }
    }

    // 连线预览线：整棵星图只有这一条，在根 Canvas 的 overlay 上。
    // 坐标是 scene 坐标（整棵递归树的顶层 world 坐标），用全局相机换算。
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
            function onPointerModeChanged() { connectPreview.requestPaint() }
        }

        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, width, height)
            if (interaction.pointerMode !== "connect")
                return
            if (interaction.connectFromKind === "")
                return
            ctx.save()
            ctx.translate(panX, panY)
            ctx.scale(zoomLevel, zoomLevel)
            ctx.beginPath()
            ctx.moveTo(interaction.connectFromSceneX, interaction.connectFromSceneY)
            ctx.lineTo(interaction.connectMouseX, interaction.connectMouseY)
            ctx.strokeStyle = _accent
            ctx.lineWidth = 2
            ctx.stroke()
            ctx.restore()
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

        AppButton {
            dt: canvasArea.dt
            text: qsTr("+")
            onClicked: zoomAround(width / 2, height / 2, zoomLevel * _zoomFactor)
        }

        AppButton {
            dt: canvasArea.dt
            text: qsTr("−")
            onClicked: zoomAround(width / 2, height / 2, zoomLevel / _zoomFactor)
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

        AppText {
            dt: canvasArea.dt
            anchors.centerIn: parent
            text: canvasArea.errorMessage
            color: _onError
            font.pointSize: dt.bodyPt
        }
        MouseArea {
            anchors.fill: parent
            onClicked: canvasArea.clearError()
        }
    }

    // Issue #801 评论 5895310100: 触屏长按菜单视觉层（不抢 pointer grab）。
    // 此组件纯视觉，无任何 TapHandler/MouseArea/Handler。
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

            AppText {
                dt: canvasArea.dt
                Layout.fillWidth: true
                text: touchContextPreview.previewKind === "embed" ? qsTr("编辑名称") : qsTr("编辑")
                color: _textPrimary
                font.pointSize: dt.labelPt
                leftPadding: 4
            }
            AppText {
                dt: canvasArea.dt
                Layout.fillWidth: true
                text: qsTr("移动")
                color: _textPrimary
                font.pointSize: dt.labelPt
                leftPadding: 4
            }
            AppText {
                dt: canvasArea.dt
                Layout.fillWidth: true
                text: qsTr("删除")
                color: _textPrimary
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
    // 菜单入口：归属层由递归命中决定，坐标是 scene 坐标。
    // ---------------------------------------------------------------------------
    function openBlankMenu(sceneX, sceneY, hit, screenX, screenY) {
        menuOwnerContent = hit ? hit.owner : null
        selectedNodeForMenu = null
        selectedEdgeForMenu = null
        selectedEmbedForMenu = null
        contextMenuSceneX = sceneX
        contextMenuSceneY = sceneY
        logInteraction("context_menu_open", "empty", "", {
            "menuKind": "bg",
            "sceneX": sceneX,
            "sceneY": sceneY
        }, hit ? hit.scenePathKey : "root")
        bgContextMenu.popup(screenX, screenY)
    }

    function noteMouseInteracted() {
        _touchInputActive = false
    }

    // 触屏长按视觉层：scene 坐标 → 屏幕坐标后显示。
    function showTouchPreview(kind, sceneX, sceneY) {
        touchContextPreview.show(kind, worldToScreenX(sceneX), worldToScreenY(sceneY))
    }

    function hideTouchPreview() {
        touchContextPreview.hide()
    }

    // 触屏长按松手：弹出真正可点击的菜单，归属层就是长按对象的归属层。
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
            contentItem: AppText {
                dt: canvasArea.dt
                text: bgMenuItem1.text
                color: bgMenuItem1.hovered ? _accent : _textPrimary
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
            contentItem: AppText {
                dt: canvasArea.dt
                text: bgMenuItem2.text
                color: bgMenuItem2.hovered ? _accent : _textPrimary
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
            contentItem: AppText {
                dt: canvasArea.dt
                text: nodeMenuItemEdit.text
                color: nodeMenuItemEdit.hovered ? _accent : _textPrimary
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
            contentItem: AppText {
                dt: canvasArea.dt
                text: nodeMenuItemMove.text
                color: nodeMenuItemMove.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItemMove.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                // 菜单已确定目标，直接进 move，不经过 pressPending 仲裁。
                if (selectedNodeForMenu && menuOwnerContent) {
                    var item = menuOwnerContent.hitNode(selectedNodeForMenu.id)
                    if (!item)
                        return
                    interaction.beginMove(item.id, menuOwnerContent.scenePathKey, item.x, item.y)
                }
            }
        }

        MenuItem {
            id: nodeMenuItemDelete
            text: qsTr("删除")
            contentItem: AppText {
                dt: canvasArea.dt
                text: nodeMenuItemDelete.text
                color: nodeMenuItemDelete.hovered ? _danger : _textPrimary
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
            contentItem: AppText {
                dt: canvasArea.dt
                text: edgeMenuItem1.text
                color: edgeMenuItem1.hovered ? _accent : _textPrimary
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
            contentItem: AppText {
                dt: canvasArea.dt
                text: edgeMenuItem2.text
                color: edgeMenuItem2.hovered ? _danger : _textPrimary
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
            contentItem: AppText {
                dt: canvasArea.dt
                text: embedMenuItemRename.text
                color: embedMenuItemRename.hovered ? _accent : _textPrimary
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
            contentItem: AppText {
                dt: canvasArea.dt
                text: embedMenuItemMove.text
                color: embedMenuItemMove.hovered ? _accent : _textPrimary
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
                    interaction.beginEmbedMove(item.instanceId, menuOwnerContent.scenePathKey, item.x, item.y)
                }
            }
        }

        MenuItem {
            id: embedMenuItemDelete
            text: qsTr("删除")
            contentItem: AppText {
                dt: canvasArea.dt
                text: embedMenuItemDelete.text
                color: embedMenuItemDelete.hovered ? _danger : _textPrimary
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

            AppText {
                dt: canvasArea.dt
                text: renameDialog.targetType === "embed" ? qsTr("修改子星图名称")
                     : qsTr("修改连线标签")
                font.pointSize: dt.fontLgPt
                font.bold: true
                color: _textPrimary
            }

            TextField {
                id: renameInput
                Layout.fillWidth: true
                height: 36
                color: _textPrimary
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
                    contentItem: AppText {
                        dt: canvasArea.dt
                        text: cancelBtn.text
                        color: _textSecondary
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
                    contentItem: AppText {
                        dt: canvasArea.dt
                        text: confirmBtn.text
                        color: _onPrimary
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

            AppText {
                dt: canvasArea.dt
                text: createMode === "starmap" ? qsTr("新建子星图") : qsTr("新建节点")
                font.pointSize: dt.fontLgPt
                font.bold: true
                color: _textPrimary
            }

            TextField {
                id: createInput
                Layout.fillWidth: true
                height: 36
                color: _textPrimary
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
                    contentItem: AppText {
                        dt: canvasArea.dt
                        text: createCancelBtn.text
                        color: _textSecondary
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
                    contentItem: AppText {
                        dt: canvasArea.dt
                        text: createConfirmBtn.text
                        color: _onPrimary
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
            // Issue #822: 归属层自己把 scene 坐标换算成本层局部坐标，
            // 空的子星图可以直接新建内容，不用"进入"另一个页面。
            if (createMode === "node")
                menuOwnerContent.createNodeWithName(name, contextMenuSceneX, contextMenuSceneY)
            else if (createMode === "starmap")
                menuOwnerContent.createSubStarmapWithName(name, contextMenuSceneX, contextMenuSceneY)
            close()
        }
    }
}
