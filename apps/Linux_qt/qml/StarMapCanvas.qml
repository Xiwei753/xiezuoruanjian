// =============================================================================
// StarMapCanvas.qml — 星图画布
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：星图可视化渲染、平移/缩放交互、节点选中/编辑/连线/移动、右键菜单
// 约束：
//   - 纯渲染和交互层，业务逻辑委托给 StarMapGraphController
//   - 鼠标行为由 pointerMode 状态机明确驱动，不再用单一 MouseArea 猜行为
//   - 节点只上抛点击类信号，决定权交回 Canvas
//   - 使用 Canvas 进行自定义绘制
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: canvasArea
    clip: true

    property string starmapId: ""
    required property var dt

    // Issue #801 评论 5894035036: 层级路径栈已移到 Workspace。
    // Canvas 的 starmapId 是只读输入绑定（由 Workspace.currentStarmapId 驱动），
    // 不再在内部赋值 starmapId，也不再维护 starmapPathStack。

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
    property string errorMessage: graphController.errorMessage

    // View transform properties
    // Issue #814 评论 5935285879: 无限画布 — pan 两个方向都不设边界。
    // 旧实现把 pan clamp 到 ≤ 0 是因为 container 用 width/height = canvas/zoomLevel
    // 的有限矩形承载世界，panX > 0 会让可见世界坐标出现负值落在 container 矩形外。
    // 改用始终覆盖视口的 sceneLayer + delegate 直接世界→屏幕映射后，世界坐标
    // 可以是任意正负值，pan 不再需要 clamp。
    property real panX: 0
    property real panY: 0
    property real zoomLevel: 1.0

    function applyPan(nextX, nextY) {
        panX = nextX
        panY = nextY
    }

    // Issue #814 评论 5935285879: 世界坐标 ↔ 屏幕坐标统一换算入口。
    // 节点/Embed delegate 直接用世界坐标映射到屏幕坐标，背景交互统一走
    // screenToWorld*，不再各处分散手写 (x - panX) / zoomLevel。
    function worldToScreenX(wx) { return panX + wx * zoomLevel }
    function worldToScreenY(wy) { return panY + wy * zoomLevel }
    function screenToWorldX(sx) { return (sx - panX) / zoomLevel }
    function screenToWorldY(sy) { return (sy - panY) / zoomLevel }

    // Issue #814 评论 5947740838: 递归子星图坐标统一映射。
    // Qt DragHandler(target:null) 的 activeTranslation 是 scene 坐标增量，
    // 不是本层 Canvas 坐标。嵌套 Scene 处在父 Embed 缩放下时，只除本层 zoomLevel
    // 会漏掉祖先 scale，节点按祖先缩放比例漂移。统一用 mapFromItem(null,...)
    // 把 scene 坐标映射到本 Canvas local，再除 zoomLevel 转 world。
    // 根层无祖先 scale 时 mapFromItem(null) 退化成恒等，行为不变。
    function sceneToCanvas(sx, sy) {
        return canvasArea.mapFromItem(null, sx, sy)
    }
    function sceneDeltaToCanvas(dx, dy) {
        var o = canvasArea.mapFromItem(null, 0, 0)
        var p = canvasArea.mapFromItem(null, dx, dy)
        return { x: p.x - o.x, y: p.y - o.y }
    }
    function sceneDeltaToWorld(dx, dy) {
        var d = sceneDeltaToCanvas(dx, dy)
        return { x: d.x / zoomLevel, y: d.y / zoomLevel }
    }
    function sceneToWorld(sx, sy) {
        var p = sceneToCanvas(sx, sy)
        return { x: screenToWorldX(p.x), y: screenToWorldY(p.y) }
    }

    // Issue #801 评论 5894035036: PinchHandler 以手势中心缩放的起点记录。
    property real _pinchStartZoom: 1.0
    property real _pinchStartPanX: 0
    property real _pinchStartPanY: 0

    // Issue #805 评论 5907045450 第 1/2 部分：删掉"下钻换整页 graph"模型。
    // 不再有 canDrillUp / drillUpRequested / drillDownRequested。
    // 递归渲染由 StarMapScene + Embed 内部 Loader 处理。
    // 滚轮/Pinch 缩到最小时只 clamp，不再触发返回父层。

    // Issue #805 评论 5907045450 第 2 部分：递归渲染上下文。
    // rootStarmapId / pathSegments 传给 Embed delegate，Embed 的 contentViewport
    // 用这些构造子 Scene 的 pathSegments。
    property string rootStarmapId: ""
    property var pathSegments: []
    // Issue #805 评论 5908703621 问题 1：本 Scene 实例的唯一 key，
    // 传给 Embed delegate 用于构造 child Scene 的 pathKey。
    property string pathKey: "root"

    // Issue #814 评论 5935285879: 整棵递归树共享的选中状态控制器。
    // 由 Workspace 创建并逐层下传，子 Scene 沿用同一个实例。
    // Node/Embed/Edge 的 isSelected 全部从 selectionController.matches 派生。
    property var selectionController: null

    // Issue #801 评论 5894639734: +/- 触屏按钮按需显示，鼠标模式不常驻。
    // 第一次收到 TouchScreen 事件时显示，切回 Mouse 时隐藏。
    property bool _touchInputActive: false

    // Issue #814 评论 5935346839: pan 手势起点记录，用于 pan_end 边界日志。
    property real _panBeginX: 0
    property real _panBeginY: 0

    // Issue #814 评论 5946795049: press-time 手势所有权。
    // 旧实现（5946366104）在 onActiveChanged 之后才用 findEmbedContentAt 判所有权，
    // 且单指 drag / 双指 pinch 共用一个 ownership bool。问题：
    //   1. DragHandler 没有 point 属性（只有 centroid），在 DragHandler 上读 point 得 undefined。
    //   2. active===true 表示 Handler 已取得 exclusive grab，此时 return 只能让父层不动，
    //      不能把抓取还给 child，会出现"按在子星图里手势没反应"。
    //   3. 单指转双指时两个 Handler 切换 active，共用 bool 会互相清掉对方的所有权。
    // 新方案：用两个 passive PointHandler（touchOwnerA/touchOwnerB）在 touch press 时
    // 记录前两根手指各自属于哪个 child Embed（instanceId，空串=背景/非 content）。
    // 父层 bgTouchDrag / canvasPinch 在取得 exclusive grab 之前就通过 enabled 让出，
    // ownership 在 press 时固定，不随手势中位置变化而改判。
    property string _touchOwnerA: ""
    property string _touchOwnerB: ""

    // Issue #814 评论 5946795049: 屏幕坐标 → 该点所属 child Embed instanceId（空串=不属于任何 child content）。
    function childOwnerAtScreen(sx, sy) {
        var em = findEmbedContentAt(screenToWorldX(sx), screenToWorldY(sy))
        return em ? em.instanceId : ""
    }

    // Issue #814 评论 5946795049: pinch 归 child 当且仅当两根手指都在同一个 child Embed 内。
    // 一根在 child、一根在外时 child 无法独占两点，父 pinch 仍工作（避免"碰到子星图就无法缩放"死区）。
    readonly property bool _pinchBelongsToChild:
        touchOwnerA.active && touchOwnerB.active
        && _touchOwnerA !== ""
        && _touchOwnerA === _touchOwnerB

    // Issue #814 评论 5935346839: 星图交互边界日志统一入口。
    // 只在手势边界（press/release/begin/end/popup）调用，不进热路径。
    // starmapBackendRef 为 null 时静默跳过（不报错）。
    function logInteraction(event, itemKind, itemId, fields) {
        if (!starmapBackendRef) return
        var fj = fields ? JSON.stringify(fields) : ""
        starmapBackendRef.record_interaction(event, pathKey, starmapId, itemKind, itemId, fj)
    }

    // Issue #817 评论 5949494799: 统一命中判断入口。
    // logPointerPress()、空白点击、拖动画布、滚轮全部共用，
    // 不再各处分散手写 findNodeAt/findEmbedChromeAt/findEmbedContentAt/hitTestEdge。
    function hitPointerAtScreen(sx, sy) {
        var wx = screenToWorldX(sx)
        var wy = screenToWorldY(sy)

        var node = findNodeAt(wx, wy)
        if (node)
            return { kind: "node", id: node.id }

        var chrome = findEmbedChromeAt(wx, wy)
        if (chrome)
            return { kind: "embedChrome", id: chrome.instanceId }

        var content = findEmbedContentAt(wx, wy)
        if (content)
            return { kind: "childContent", id: content.instanceId }

        var edge = graphController.hitTestEdge(wx, wy)
        if (edge)
            return { kind: "edge", id: edge.id }

        return { kind: "empty", id: "" }
    }

    // Issue #814 评论 5935346839: pointer_press 是完整手势的起点边界。
    // 不能挂在背景 MouseArea.onPressed 上：按到 Node/Embed 时对象的 TapHandler
    // 先取得 exclusive grab，背景 MouseArea 根本收不到 press，而"按在对象上
    // 没反应"恰恰是最需要诊断的场景。统一由根节点上的 passive-grab PointHandler
    // 观察 press：不抢事件，命中的对象照常拿到完整交互；hitKind/hitId 在按下
    // 当场按世界坐标重算，能直接区分"坐标换算错"和"事件路由断"。
    function logPointerPress(button, device, point) {
        var hit = hitPointerAtScreen(point.position.x, point.position.y)
        logInteraction("pointer_press", hit.kind, hit.id, {
            "button": button,
            "device": device,
            "screenX": point.position.x,
            "screenY": point.position.y,
            "worldX": screenToWorldX(point.position.x),
            "worldY": screenToWorldY(point.position.y),
            "panX": panX,
            "panY": panY,
            "zoomLevel": zoomLevel
        })
    }

    // ---------------------------------------------------------------------------
    // 鼠标手势状态已拆到 StarMapInteractionController（interaction）：
    //   pointerMode / connectFrom* / connectMouseX/Y / pressedNodeId / pressedEmbedId
    // Canvas 只通过 interaction.* 读写瞬时状态，图操作仍留在 Canvas/GraphController。
    // ---------------------------------------------------------------------------

    // 上下文菜单辅助状态
    property var selectedNodeForMenu: null
    property var selectedEdgeForMenu: null
    // Issue #796 评论 5886483653: Embed 右键菜单辅助状态
    property var selectedEmbedForMenu: null
    property real contextMenuWorldX: 0
    property real contextMenuWorldY: 0

    // 新建对话框状态：先收集名字再写 Core（Issue #793 评论 5884923277）
    property string createMode: ""      // "node" / "starmap"
    property real createWorldX: 0
    property real createWorldY: 0

    // Signals
    signal nodeSelected(var node)
    signal edgeSelected(var edge)
    signal selectionCleared()
    signal editNodeRequested(var node)
    // Issue #805 评论 5908703621 问题 5：child Scene 经 Embed 冒泡上来的节点编辑请求，
    // 已带正确的 owner 上下文（ownerStarmapId/ownerPathKey），原样转发给 Scene 不重新包装。
    // Issue #805 评论 5912394108：ownerScene(var) 携带真正拥有该节点的子 Scene 引用，
    // 原样转发保持指向不变，Workspace 据此直接回写到对应子 Scene 的 Controller。
    signal childEditNodeRequested(var ownerScene, string ownerStarmapId, string ownerPathKey, var node)
    // Issue #805 评论 5907045450 第 1 部分：删掉 drillDownRequested / drillUpRequested。
    // 递归渲染由 StarMapScene 处理，Canvas 不再上抛层级切换请求。

    // Issue #798: Canvas 自身 starmapId 改变时清瞬时交互状态并重新加载，
    // 不可见 / 离开工作区时也 reset，避免旧 move/connect 状态泄漏。
    // Issue #801 评论 5894035036: starmapId 现在是只读输入，由 Workspace 驱动；
    // 切图后 resetInteraction + loadGraph 让新图正确加载。
    onStarmapIdChanged: {
        resetInteraction()
        if (starmapId.length > 0) loadGraph()
    }
    onVisibleChanged: { if (!visible) resetInteraction() }

    // Issue #798: 渲染层直接读 graphController 的模型，不再在 Canvas 维护副本。
    // graphController 是当前星图 canonical scene model 的唯一持有者。

    StarMapGraphController {
        id: graphController
        starmapId: canvasArea.starmapId
        starmapBackendRef: canvasArea.starmapBackendRef
        // Issue #814 评论 5935285879: 传 pathKey 和共享 selectionController 给 Controller，
        // selectNode/selectEdge/selectEmbed/clearSelection 据此调 selectionController。
        pathKey: canvasArea.pathKey
        selectionController: canvasArea.selectionController
        onGraphChanged: edgeCanvas.requestPaint()
        onSelectionCleared: canvasArea.selectionCleared()
        onNodeSelected: function(node) { canvasArea.nodeSelected(node) }
        onEdgeSelected: function(edge) { canvasArea.edgeSelected(edge) }
        onErrorMessageChanged: canvasArea.errorMessage = graphController.errorMessage
    }

    // Issue #798: 瞬时手势状态机（pan/connect/move），不读写 Core，不保存节点数据。
    StarMapInteractionController { id: interaction }

    function clearError() { graphController.clearError() }

    // Issue #798: 公开 reset 入口，供 Workspace 切图 / 不可见时清瞬时交互状态。
    function resetInteraction() {
        // Issue #798 评论 5892406254: reset 前若正在 move，edgeRenders 已被
        // transient 坐标更新。reset 后 delegate 回 canonical，edge cache 也要
        // 一起恢复 canonical，否则节点回去了线还停在拖动位置。
        var wasMove = interaction.pointerMode === "move"
        interaction.reset()
        // Issue #817 评论 5953678540: 一起清 bgDragArea 的本地 pan 手势状态，
        // 避免切图/窗口隐藏后 panStarted/pressHitKind 拖留。
        bgDragArea.resetMouseGesture()
        if (wasMove) {
            graphController.computeEdgeRenders(null)
            edgeCanvas.requestPaint()
        }
    }

    // Issue #801 评论 5894035036: drillDown / drillUp / isAtRootStarmap / starmapTitle
    // 已移到 Workspace。层级栈由 Workspace 持有，Canvas 只通过 drillDownRequested /
    // drillUpRequested 信号上抛请求。父级标题由 Workspace 的 currentStarmapTitle 维护，
    // 不再从 graphController 反查（graphController 没有 starmapTitle 属性）。

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
    // 背景交互层：TapHandler 处理点击类，MouseArea 处理 pan 拖动与滚轮
    // TapHandler 与 MouseArea 共存：Handler 独立收到 tap/longPress 信号
    // Issue #801 评论 5894035036: 桌面指针/触屏按 acceptedDevices 拆开：
    //   - 桌面指针空白长按无操作（用右键打开菜单）
    //   - 触屏空白长按打开背景菜单
    //   - 触屏未长按在节点上滑动 → 画布 pan（节点没挂触屏 DragHandler，事件穿透）
    //   - 触屏长按后移动 → 更新 connect 坐标
    //
    // Issue #812: 桌面指针的 acceptedDevices 必须是 Mouse | TouchPad，
    // 不能只写 Mouse。Qt 的 acceptedDevices 是硬过滤，设备类型不匹配时
    // Handler 根本不参与这个事件；而 Wayland 的桌面 pointer 路径不能可靠把
    // 实际硬件还原成 Mouse，于是只写 Mouse 会让实体鼠标的单击/右键/拖动
    // 全部被静默丢弃。画布平移用的是没有设备过滤的 MouseArea，所以会留下
    // "能拖动画布、但左右键都点不动" 的半套状态。
    // 约定：桌面语义（单击选中/双击编辑/右键菜单/直接拖动）= Mouse | TouchPad；
    // 触屏语义（长按/滑动）= TouchScreen，两者不混。
    //
    // Issue #806 评论 5907045450: 这些 handler 必须直接挂在 canvasArea 上。
    // 之前它们被包在一个独立的 sibling Item（bgInteractionLayer）里，而 Qt 的
    // 事件只投递给「命中点所在最深 item」及其祖先链上的 handler，兄弟节点收不到。
    // 结果空白处的右键菜单、滚轮缩放、拖拽平移全部失效。
    // ---------------------------------------------------------------------------

    // 鼠标左键单击：边选中或清选区
    // Issue #796 评论 5886483653: 命中顺序统一成 Node/Embed → Edge → 空白，
    // 不让画布背景先吞掉对象点击。
    TapHandler {
        id: bgMouseLeftTap
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        acceptedButtons: Qt.LeftButton
        onSingleTapped: function(eventPoint) {
            _touchInputActive = false
            var mx = screenToWorldX(eventPoint.position.x)
            var my = screenToWorldY(eventPoint.position.y)
            if (findNodeAt(mx, my)) {
                return
            }
            if (findEmbedChromeAt(mx, my)) {
                return
            }
            // Issue #814 评论 5946366104: 子星图 contentViewport 内的点击归 child Scene，
            // 父层不得当成空白 clearSelection() 吞掉子场景选中。
            if (findEmbedContentAt(mx, my)) {
                return
            }
            var clickedEdge = graphController.hitTestEdge(mx, my)
            if (clickedEdge) {
                graphController.selectEdge(clickedEdge.id)
                // Issue #814 评论 5935346839: selection_changed 边界日志（edge）。
                logInteraction("selection_changed", "edge", clickedEdge.id, {
                    "device": "mouse"
                })
            } else {
                clearSelection()
                // Issue #814 评论 5935346839: selection_changed 边界日志（none）。
                logInteraction("selection_changed", "none", "", {
                    "device": "mouse"
                })
            }
        }
        // 鼠标空白长按无操作（鼠标用右键打开菜单）。
        onLongPressed: {
        }
    }

    // 触屏左键单击：边选中或清选区；长按打开背景菜单
    TapHandler {
        id: bgTouchLeftTap
        acceptedDevices: PointerDevice.TouchScreen
        acceptedButtons: Qt.LeftButton
        onSingleTapped: function(eventPoint) {
            _touchInputActive = true
            var mx = screenToWorldX(eventPoint.position.x)
            var my = screenToWorldY(eventPoint.position.y)
            if (findNodeAt(mx, my)) {
                return
            }
            if (findEmbedChromeAt(mx, my)) {
                return
            }
            // Issue #814 评论 5946366104: 子星图 contentViewport 内的点击归 child Scene。
            if (findEmbedContentAt(mx, my)) {
                return
            }
            var clickedEdge = graphController.hitTestEdge(mx, my)
            if (clickedEdge) {
                graphController.selectEdge(clickedEdge.id)
                // Issue #814 评论 5935346839: selection_changed 边界日志（edge, touch）。
                logInteraction("selection_changed", "edge", clickedEdge.id, {
                    "device": "touch"
                })
            } else {
                clearSelection()
                // Issue #814 评论 5935346839: selection_changed 边界日志（none, touch）。
                logInteraction("selection_changed", "none", "", {
                    "device": "touch"
                })
            }
        }
        // Issue #801 评论 5894035036: 触屏空白长按打开背景菜单。
        // TapHandler.longPressed 信号无参数，用 point.position 拿当前点
        // （TapHandler 继承自 SinglePointHandler，有 point 属性）。
        // Issue #801 评论 5894639734: 长按前先判命中，节点/Embed/边上的长按
        // 不弹背景菜单（Qt TapHandler 是 passive grab，背景和对象 Handler 会
        // 同时观察同一个 press，不能假设背景自动收不到）。
        onLongPressed: {
            _touchInputActive = true
            var px = bgTouchLeftTap.point.position.x
            var py = bgTouchLeftTap.point.position.y
            var wx = screenToWorldX(px)
            var wy = screenToWorldY(py)

            if (findNodeAt(wx, wy)) return
            if (findEmbedChromeAt(wx, wy)) return
            if (findEmbedContentAt(wx, wy)) return
            if (graphController.hitTestEdge(wx, wy)) return

            contextMenuWorldX = wx
            contextMenuWorldY = wy
            // Issue #814 评论 5935346839: context_menu_open 边界日志（bg）。
            logInteraction("context_menu_open", "empty", "", {
                "menuKind": "bg",
                "worldX": wx,
                "worldY": wy
            })
            bgContextMenu.popup(px, py)
        }
    }

    // 右键单击：边菜单或画布菜单
    // Issue #796 评论 5886483653: 命中顺序 Node/Embed → Edge → 空白。
    TapHandler {
        id: backgroundRightTap
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        acceptedButtons: Qt.RightButton
        onSingleTapped: function(eventPoint) {
            _touchInputActive = false
            var mx = screenToWorldX(eventPoint.position.x)
            var my = screenToWorldY(eventPoint.position.y)
            if (findNodeAt(mx, my)) {
                return
            }
            if (findEmbedChromeAt(mx, my)) {
                return
            }
            // Issue #814 评论 5946366104: 子星图 contentViewport 内的右键归 child Scene。
            if (findEmbedContentAt(mx, my)) {
                return
            }
            var clickedEdge = graphController.hitTestEdge(mx, my)
            if (clickedEdge) {
                selectedEdgeForMenu = clickedEdge
                // Issue #814 评论 5935346839: context_menu_open 边界日志（edge）。
                logInteraction("context_menu_open", "edge", clickedEdge.id, {
                    "menuKind": "edge",
                    "worldX": mx,
                    "worldY": my
                })
                edgeContextMenu.popup(eventPoint.position.x, eventPoint.position.y)
            } else {
                contextMenuWorldX = mx
                contextMenuWorldY = my
                // Issue #814 评论 5935346839: context_menu_open 边界日志（bg, 右键）。
                logInteraction("context_menu_open", "empty", "", {
                    "menuKind": "bg",
                    "worldX": mx,
                    "worldY": my
                })
                bgContextMenu.popup(eventPoint.position.x, eventPoint.position.y)
            }
        }
    }

    // 触屏背景拖动：触屏未长按在节点上滑动 → 画布 pan；
    // 触屏长按后移动 → 更新 connect 坐标，超过阈值转 connect。
    // Issue #801 评论 5894035036: 节点没挂触屏 DragHandler，事件穿透到背景。
    DragHandler {
        id: bgTouchDrag
        acceptedDevices: PointerDevice.TouchScreen
        acceptedButtons: Qt.LeftButton
        target: null
        // Issue #814 评论 5946795049: press-time ownership — 任一手指落在 child content 内时
        // 父层 drag 不参与，由 child Scene 独占。enabled 在 exclusive grab 之前生效。
        enabled: _touchOwnerA === "" && _touchOwnerB === ""
        property real lastTx: 0
        property real lastTy: 0
        // Issue #801 评论 5895310100: 标记当前 move 由触屏发起，
        // 用于 onLeftReleased 区分鼠标 move（nodeDragHandler 驱动）和触屏 move（bgTouchDrag 驱动），
        // 避免两者重复 commit。
        property bool _wasTouchMove: false
        // Issue #814 评论 5935346839: 标记当前手势是触屏画布 pan，
        // 用于在拖动手势真正开始时记 pan_begin、结束时记 pan_end。
        property bool _wasTouchPan: false
        onActiveChanged: {
            if (active) {
                lastTx = 0
                lastTy = 0
                _touchInputActive = true
                // 触屏在 move 模式下开始拖动 → 标记，commit 由 bgTouchDrag 独占
                if (interaction.pointerMode === "move") {
                    _wasTouchMove = true
                }
                // Issue #814 评论 5935346839: 触屏画布 pan 的 begin 边界
                // （鼠标 pan_begin 在 bgDragArea.onPressed 记）。
                if (interaction.pointerMode === "idle") {
                    _wasTouchPan = true
                    _panBeginX = panX
                    _panBeginY = panY
                    logInteraction("pan_begin", "empty", "", {
                        "startPanX": panX,
                        "startPanY": panY,
                        "device": "touch"
                    })
                }
            } else {
                if (_wasTouchPan) {
                    // Issue #814 评论 5935346839: 触屏画布 pan 的 end 边界。
                    logInteraction("pan_end", "empty", "", {
                        "startPanX": _panBeginX,
                        "startPanY": _panBeginY,
                        "endPanX": panX,
                        "endPanY": panY,
                        "device": "touch"
                    })
                    _wasTouchPan = false
                }
                // Issue #801 评论 5895310100: 触屏 move 手势结束 → 提交位置。
                // 桌面指针 move 不走 bgTouchDrag（acceptedDevices 限定 TouchScreen），
                // 其 commit 由 Node/Embed 的 onLeftReleased 负责。
                if (_wasTouchMove && interaction.pointerMode === "move") {
                    var _touchCommitOk = false
                    if (interaction.pressedNodeId !== "") {
                        _touchCommitOk = graphController.commitNodeMove(interaction.pressedNodeId, interaction.moveX, interaction.moveY)
                    } else if (interaction.pressedEmbedId !== "") {
                        _touchCommitOk = graphController.commitEmbedMove(interaction.pressedEmbedId, interaction.moveX, interaction.moveY)
                    }
                    // Issue #814 评论 5935346839: move_end 边界日志（touch）。
                    var _mk = interaction.pressedNodeId !== "" ? "node" : (interaction.pressedEmbedId !== "" ? "embed" : "")
                    var _mid = interaction.pressedNodeId !== "" ? interaction.pressedNodeId : interaction.pressedEmbedId
                    logInteraction("move_end", _mk, _mid, {
                        "toX": interaction.moveX,
                        "toY": interaction.moveY,
                        "commitSuccess": _touchCommitOk,
                        "device": "touch"
                    })
                    interaction.endMove()
                    graphController.computeEdgeRenders(null)
                    edgeCanvas.requestPaint()
                }
                _wasTouchMove = false
            }
        }
        onActiveTranslationChanged: {
            var rawDx = activeTranslation.x - lastTx
            var rawDy = activeTranslation.y - lastTy
            lastTx = activeTranslation.x
            lastTy = activeTranslation.y
            // Issue #814 评论 5947740838: activeTranslation 是 scene 增量，
            // 映射到本 Canvas local 再用。根层无祖先 scale 时退化成恒等。
            var cd = sceneDeltaToCanvas(rawDx, rawDy)
            if (interaction.pointerMode === "idle") {
                // 触屏未长按滑动 = 画布 pan
                applyPan(panX + cd.x, panY + cd.y)
            } else if (interaction.pointerMode === "contextPending") {
                // 触屏长按后移动，更新 connect 坐标（世界坐标）
                interaction.connectMouseX += cd.x / zoomLevel
                interaction.connectMouseY += cd.y / zoomLevel
                // 移动总距离超过阈值则转 connect（阈值用 scene 原始位移判定）
                if (Math.sqrt(activeTranslation.x * activeTranslation.x + activeTranslation.y * activeTranslation.y) > interaction._moveThreshold) {
                    if (interaction.contextPendingToConnect()) {
                        // Issue #814 评论 5935346839: connect_begin 边界日志
                        // （触屏长按后拖过阈值转连线；鼠标连接在 onMouseLongPressed 记）。
                        logInteraction("connect_begin", interaction.connectFromKind, interaction.connectFromId, {
                            "kind": interaction.connectFromKind,
                            "fromId": interaction.connectFromId,
                            "fromX": interaction.connectMouseX,
                            "fromY": interaction.connectMouseY,
                            "device": "touch"
                        })
                    }
                    // Issue #801 评论 5895310100: 继续移动变连线，关闭长按菜单视觉层
                    touchContextPreview.hide()
                }
                edgeCanvas.requestPaint()
            } else if (interaction.pointerMode === "connect") {
                interaction.updateConnect(interaction.connectMouseX + cd.x / zoomLevel, interaction.connectMouseY + cd.y / zoomLevel)
                edgeCanvas.requestPaint()
            } else if (interaction.pointerMode === "move") {
                // Issue #801 评论 5895310100: 触屏菜单"移动"后再拖 → 更新 transient 坐标。
                // Node/Embed 的 DragHandler 限定 Mouse，触屏拖动穿透到背景层，
                // 由 bgTouchDrag 统一驱动 move。delegate 的 x/y binding 自动跟随 moveX/moveY。
                interaction.updateMove(interaction.moveX + cd.x / zoomLevel, interaction.moveY + cd.y / zoomLevel)
                graphController.computeEdgeRenders(currentMoveOverride())
                edgeCanvas.requestPaint()
            }
        }
    }

    // Issue #801 评论 5894035036: 触屏双指 Pinch 缩放。
    PinchHandler {
        id: canvasPinch
        acceptedDevices: PointerDevice.TouchScreen
        target: null
        // Issue #814 评论 5946795049: 两根手指都在同一个 child Embed 内时父层 pinch 不参与。
        // 一内一外时 child 无法独占两点，父 pinch 仍工作。
        enabled: !_pinchBelongsToChild
        onActiveChanged: {
            if (active) {
                _pinchStartZoom = zoomLevel
                _pinchStartPanX = panX
                _pinchStartPanY = panY
                _touchInputActive = true
            }
        }
        onActiveScaleChanged: {
            var rawZoom = _pinchStartZoom * activeScale
            // Issue #805 评论 5907045450 第 1 部分：缩到最小时只 clamp，
            // 不再触发 drillUp（递归渲染由 StarMapScene 处理）。
            zoomLevel = Math.max(0.35, Math.min(2.5, rawZoom))
            // 以手势中心缩放
            var cx = centroid.position.x
            var cy = centroid.position.y
            applyPan(cx - (cx - _pinchStartPanX) * (zoomLevel / _pinchStartZoom),
                     cy - (cy - _pinchStartPanY) * (zoomLevel / _pinchStartZoom))
        }
    }

    // Issue #814 评论 5935346839: press 边界观察器。
    // PointHandler 只取 passive grab，不参与 exclusive grab 竞争：Node/Embed
    // 自己的 TapHandler/DragHandler 照常拿到完整手势。四个 handler 分别绑定
    // 具体的 button，避免依赖未公开的 point.pressedButtons。
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
    // Issue #814 评论 5946795049 / 5947130795: touch press-time 所有权观察器（前两根手指）。
    // PointHandler 只取 passive grab，不参与 exclusive grab 竞争。Qt 对同 parent 的多个
    // PointHandler 会把不同 touchpoint 分配给不同实例：第一根手指进 touchOwnerA，第二根进
    // touchOwnerB。press 时同时记录 pointer_press 边界日志 + 该手指所属 child Embed，
    // release 时清空。父层 bgTouchDrag / canvasPinch 通过 enabled 绑定这些属性在 grab 之前让出。
    // Issue #814 评论 5947130795: 不再保留第三个独立 TouchScreen PointHandler 日志观察器——
    // 同 parent 下多个 PointHandler 组成分配组，一个触点被某个 sibling 取得 passive grab 后
    // 其他 sibling 不再选择该触点；独立的日志观察器会把第一根手指分走，导致 touchOwnerA/B
    // 凑不齐两根、press-time ownership 在最普通的一指/两指场景里失效。日志合并进 A/B 后
    // 两根触点都能记录 pointer_press，且不破坏 ownership。
    PointHandler {
        id: touchOwnerA
        acceptedDevices: PointerDevice.TouchScreen
        // Issue #814 评论 5947395841: 触屏无按钮。acceptedButtons 用 NoButton 而非 LeftButton，
        // 避免 synthetic mouse 的 LeftButton 反激活 owner、清掉 press-time ownership。
        acceptedButtons: Qt.NoButton
        onActiveChanged: {
            if (active) {
                canvasArea.logPointerPress("left", "touch", point)
                _touchOwnerA = childOwnerAtScreen(point.pressPosition.x, point.pressPosition.y)
            } else {
                _touchOwnerA = ""
            }
        }
    }
    PointHandler {
        id: touchOwnerB
        acceptedDevices: PointerDevice.TouchScreen
        // Issue #814 评论 5947395841: 同 touchOwnerA — 触屏无按钮，用 NoButton
        // 避免 synthetic mouse 的 LeftButton 反激活 owner 清掉 press-time ownership。
        acceptedButtons: Qt.NoButton
        onActiveChanged: {
            if (active) {
                canvasArea.logPointerPress("left", "touch", point)
                _touchOwnerB = childOwnerAtScreen(point.pressPosition.x, point.pressPosition.y)
            } else {
                _touchOwnerB = ""
            }
        }
    }

    // Issue #817 评论 5949494799: pan 拖动改为 press-time 手势归属 + 拖动阈值。
    // 按下时先用 hitPointerAtScreen 判命中：node/embedChrome/childContent 时
    // mouse.accepted = false 让事件穿透给对应对象/子 Scene；只有 empty/edge
    // 才记录 pressHitKind，左键等移动超过 dragThreshold 后才真正 beginPan。
    // 中键仍直接 beginPan。滚轮已移到独立 WheelHandler（sceneWheel）。
    MouseArea {
        id: bgDragArea
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton | Qt.MiddleButton
        hoverEnabled: true

        // Issue #817 评论 5949494799: press-time 手势归属。
        property string pressHitKind: ""
        property real pressX: 0
        property real pressY: 0
        property real lastX: 0
        property real lastY: 0
        property bool panStarted: false

        // Issue #817 评论 5953678540: 统一清理 pan 手势本地状态。
        // onReleased 之外（onCanceled / resetInteraction / 切图 / 窗口隐藏）
        // 也要清 pressHitKind/panStarted，否则脏状态会让普通鼠标移动继续拖动画布。
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
            var hit = hitPointerAtScreen(mouse.x, mouse.y)

            if (mouse.button === Qt.LeftButton) {
                // Issue #817 评论 5949494799: 命中 node/embedChrome/childContent 时
                // 不接受事件，让对应对象/子 Scene 处理；父 Scene 不进入 pan。
                if (hit.kind === "node"
                        || hit.kind === "embedChrome"
                        || hit.kind === "childContent") {
                    mouse.accepted = false
                    return
                }
                pressHitKind = hit.kind
                pressX = mouse.x
                pressY = mouse.y
                lastX = mouse.x
                lastY = mouse.y
                panStarted = false
                return
            }

            // 中键直接进入 pan（不依赖长按/阈值）
            // Issue #817 评论 5953678540: beginPan() 在非 idle 状态返回 false，
            // 此时不能设 panStarted=true，否则会制造 panStarted=true / pointerMode!=pan 的矛盾状态。
            if (mouse.button === Qt.MiddleButton) {
                if (!interaction.beginPan())
                    return

                pressHitKind = "empty"
                panStarted = true
                lastX = mouse.x
                lastY = mouse.y
                // Issue #814 评论 5935346839: pan_begin 边界日志（中键）。
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
                // Issue #814 评论 5935346839: pan_begin 边界日志。
                logInteraction("pan_begin", "empty", "", {
                    "startPanX": panX,
                    "startPanY": panY,
                    "device": "mouse"
                })
                return
            }

            // panStarted（中键直接 true，或左键已超阈值）：继续 pan
            // Issue #817 评论 5953678540: 同时确认 pointerMode 仍是 pan，
            // 避免 pan 被 cancel/reset 后本地 panStarted 拖留继续拖动画布。
            if (panStarted && interaction.pointerMode === "pan") {
                var dx = mouse.x - lastX
                var dy = mouse.y - lastY
                applyPan(panX + dx, panY + dy)
                lastX = mouse.x
                lastY = mouse.y
            }
        }

        onReleased: function(mouse) {
            // Issue #817 评论 5949494799: 只在 panStarted 时结束 pan。
            // 没有超过阈值就是普通点击，由 bgMouseLeftTap 处理 clearSelection。
            if (panStarted) {
                interaction.endPan()
                // Issue #814 评论 5935346839: pan_end 边界日志。
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

        // Issue #817 评论 5953678540: 系统取消抓取时也要结束 pan 并清本地状态。
        onCanceled: {
            if (interaction.pointerMode === "pan")
                interaction.endPan()
            resetMouseGesture()
        }
    }

    // Issue #817 评论 5953678540: 滚轮缩放只由根 Scene 唯一处理。
    // 缩放的是整张星图的相机/视角，不是鼠标落在哪个子星图就单独缩那个子 Scene。
    // 子 Scene 不再拥有自己的滚轮缩放入口；鼠标哪怕停在第三层子星图内部，
    // 最终变化的也只是根 Scene 的 zoomLevel/panX/panY，整棵递归树一起缩放。
    // Qt 的 WheelHandler 真正决定是否挡住后续 handler 的是 blocking；
    // 根层唯一处理，直接用默认阻塞语义。pixelDelta 可作为高分辨率触控板
    // 滚动的补充/回退（angleDelta 始终提供，pixelDelta 仅高分辨率设备额外给出）。
    WheelHandler {
        id: sceneWheel
        target: null
        enabled: pathKey === "root"
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
            var newZoom = Math.max(0.35, Math.min(2.5, oldZoom + delta * 0.1))
            if (newZoom === oldZoom)
                return

            var mx = point.position.x
            var my = point.position.y

            zoomLevel = newZoom
            applyPan(
                mx - (mx - panX) * (zoomLevel / oldZoom),
                my - (my - panY) * (zoomLevel / oldZoom)
            )

            logInteraction("zoom_wheel", "scene", starmapId, {
                "oldZoom": oldZoom,
                "newZoom": zoomLevel,
                "screenX": mx,
                "screenY": my
            })
        }
    }

    // Edges canvas (now fullscreen, translated/scaled dynamically to prevent panning drifts)
    Canvas {
        id: edgeCanvas
        anchors.fill: parent
        z: 1

        Connections {
            target: canvasArea
            function onPanXChanged() { edgeCanvas.requestPaint() }
            function onPanYChanged() { edgeCanvas.requestPaint() }
            function onZoomLevelChanged() { edgeCanvas.requestPaint() }
        }

        // Issue #814 评论 5946090360: 共享 selection 变化时 Edge Canvas 立即重绘。
        // onPaint 里读了 selectionController.matches(pathKey,"edge",edge.id) 派生选中色，
        // 但 Canvas 不会因 onPaint 内读取的属性自动重画；GraphController.selectEdge 现在
        // 只改共享 selection 不改 edgesModel、不触发 graphChanged。整棵递归树共用一个
        // selection 时，每层 Edge Canvas 都要在选中身份变化后立刻刷新。
        Connections {
            target: canvasArea.selectionController
            function onScenePathKeyChanged() { edgeCanvas.requestPaint() }
            function onKindChanged() { edgeCanvas.requestPaint() }
            function onItemIdChanged() { edgeCanvas.requestPaint() }
        }

        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, width, height)

            ctx.save()
            ctx.translate(panX, panY)
            ctx.scale(zoomLevel, zoomLevel)
            ctx.lineWidth = 2

            // Draw all edges using Linux platform render data
            var renders = graphController.edgeRenders
            for (var i = 0; i < renders.length; i++) {
                var r = renders[i]
                var edge = null
                for (var ei = 0; ei < graphController.edgesModel.length; ei++) {
                    if (graphController.edgesModel[ei].id === r.edgeId) { edge = graphController.edgesModel[ei]; break }
                }
                if (!edge) continue

                // Issue #814 评论 5935285879: edge.isSelected 不再由 GraphController 维护，
                // 从共享 selectionController 派生。
                var edgeSelected = selectionController ? selectionController.matches(pathKey, "edge", edge.id) : false
                var color = edgeSelected ? _accent : _border

                // Draw line
                ctx.beginPath()
                ctx.moveTo(r.startX, r.startY)
                ctx.lineTo(r.endX, r.endY)
                ctx.strokeStyle = color
                ctx.stroke()

                // Draw arrow head
                ctx.beginPath()
                ctx.moveTo(r.arrowTipX, r.arrowTipY)
                ctx.lineTo(r.arrowLeftX, r.arrowLeftY)
                ctx.lineTo(r.arrowRightX, r.arrowRightY)
                ctx.closePath()
                ctx.fillStyle = color
                ctx.fill()

                // Label
                if (edge.label) {
                    ctx.fillStyle = _surfaceContainer
                    var tw = ctx.measureText(edge.label).width
                    ctx.fillRect(r.labelX - tw/2 - 4, r.labelY - 10, tw + 8, 20)

                    ctx.fillStyle = _textPrimary
                    ctx.font = "12px sans-serif"
                    ctx.textAlign = "center"
                    ctx.textBaseline = "middle"
                    ctx.fillText(edge.label, r.labelX, r.labelY)
                }
            }

            // Draw connecting preview line while in connect mode
            // Issue #796 评论 5887280405: 起点根据 connectFromKind 查 Node 或 Embed；
            // 兼容旧 connectFromNodeId 路径（旧代码未设 connectFromKind 时回退）。
            if (interaction.pointerMode === "connect" && (interaction.connectFromId !== "" || interaction.connectFromNodeId !== "")) {
                var startNode = null
                var startKind = interaction.connectFromKind
                var startId = interaction.connectFromId
                if (startId === "" && interaction.connectFromNodeId !== "") {
                    // 兼容旧路径：仅 connectFromNodeId 被设
                    startKind = "node"
                    startId = interaction.connectFromNodeId
                }
                if (startKind === "node") {
                    startNode = getNode(startId)
                } else if (startKind === "embed") {
                    // Embed 起点用 graphController.getEmbed 拿坐标
                    startNode = graphController.getEmbed(startId)
                }
                if (startNode) {
                    ctx.beginPath()
                    ctx.moveTo(startNode.x + startNode.width/2, startNode.y + startNode.height/2)
                    ctx.lineTo(interaction.connectMouseX, interaction.connectMouseY)
                    ctx.strokeStyle = _accent
                    ctx.lineWidth = 2
                    ctx.stroke()
                }
            }

            ctx.restore()
        }
    }

    // Main transform layer
    //
    // Issue #814 评论 5935285879: 无限画布 — 不再用"viewport 大小的 container +
    // container 自身 x/y/scale"承载整个世界。改成一个始终和视口同尺寸的稳定
    // sceneLayer，节点和 Embed 直接把世界坐标映射到屏幕坐标。
    //   - sceneLayer 始终覆盖整个视口（anchors.fill: parent），Qt 命中链不会
    //     因为节点跑到负世界坐标就断掉。
    //   - delegate 的 x/y = worldToScreen(worldX/worldY)，scale = zoomLevel，
    //     transformOrigin = TopLeft。世界坐标可以是任意正负值。
    //   - edgeCanvas 自己 translate/scale，不受 sceneLayer 影响。
    Item {
        id: sceneLayer
        anchors.fill: parent
        z: 2

        Repeater {
            model: graphController.nodesModel
            delegate: StarMapNode {
                // Issue #798: Qt 6.11 Repeater 要求 delegate 用显式 required property
                // 接模型上下文，不能靠隐式 index。
                required property var modelData
                required property int index
                dt: canvasArea.dt
                property var nodeData: modelData

                // Issue #814 评论 5935285879: delegate 直接把世界坐标映射到屏幕坐标。
                // 当前节点处于 move 时读 interaction.moveX/moveY（世界坐标），
                // 否则读 canonical nodeData.x/y（世界坐标），再 worldToScreen。
                x: worldToScreenX(interaction.pointerMode === "move" && interaction.pressedNodeId === nodeData.id ? interaction.moveX : nodeData.x)
                y: worldToScreenY(interaction.pointerMode === "move" && interaction.pressedNodeId === nodeData.id ? interaction.moveY : nodeData.y)
                // scale 把世界尺寸（nodeData.width/height）缩放到屏幕尺寸
                scale: zoomLevel
                transformOrigin: Item.TopLeft
                width: nodeData.width
                height: nodeData.height
                title: nodeData.title
                // Issue #814 评论 5935285879: isSelected 从共享 selectionController 派生，
                // 不再读 nodeData.isSelected（GraphController 不再维护 isSelected）。
                isSelected: selectionController ? selectionController.matches(pathKey, "node", nodeData.id) : false
                // wobble 交给 StarMapNode 内部驱动，用 index 错开 phase
                wobbleIndex: index

                // Issue #798: 不再原地篡改 nodeData.x/y，拖动用 StarMapNode 自己的 x/y
                // 作为临时显示坐标（命令式赋值打破初始绑定），松手提交 Controller。
                onXChanged: edgeCanvas.requestPaint()
                onYChanged: edgeCanvas.requestPaint()

                // -------------------------------------------------------------------
                // 节点上抛信号 → Canvas 状态机决定行为
                // -------------------------------------------------------------------
                // Issue #801 评论 5894981235: 鼠标点击 Node 时切回鼠标模式，
                // 隐藏触屏 +/- 按钮。
                onMouseInteracted: _touchInputActive = false

                onSingleClicked: {
                    graphController.selectNode(nodeData.id)
                    // Issue #814 评论 5935346839: selection_changed 边界日志（node）。
                    // singleClicked 由鼠标/触屏两个 TapHandler 共用，这里不猜 device；
                    // 设备在 pointer_press / pan / move 边界日志里已经明确。
                    logInteraction("selection_changed", "node", nodeData.id, {})
                }

                onDoubleClicked: {
                    // Issue #801 评论 5895625744: 旧 portal Node 已在 Controller
                    // buildModels() 归一到 Embed，Node 双击统一走编辑。
                    var nd = nodeData
                    graphController.selectNode(nd.id)
                    editNodeRequested(nd)
                }

                // Issue #801 评论 5894035036: 鼠标长按直接进 connect
                // （#373 鼠标规则：长按后拖 = 拉线）
                onMouseLongPressed: {
                    var nd = nodeData
                    if (!interaction.beginConnect("node", nd.id, nodePath(nd.id), nd.x + nd.width / 2, nd.y + nd.height / 2)) {
                        return
                    }
                    // Issue #814 评论 5935346839: connect_begin 边界日志（node）。
                    logInteraction("connect_begin", "node", nd.id, {
                        "kind": "node",
                        "fromId": nd.id,
                        "fromX": nd.x + nd.width / 2,
                        "fromY": nd.y + nd.height / 2
                    })
                    isBeingDragged = true
                    edgeCanvas.requestPaint()
                }

                // Issue #801 评论 5894035036: 触屏长按进 contextPending
                // （不移动则松手弹菜单，移动超过阈值才转 connect）
                // Issue #801 评论 5895310100: 触屏长按当场显示菜单视觉层（#373：长按先出菜单反馈）。
                // 不等 onLeftReleased 才 popup；手指继续移动超过阈值时视觉层关闭转 connect，
                // 手指松开时视觉层关闭并弹出真正可点击的 nodeContextMenu。
                onTouchLongPressed: {
                    var nd = nodeData
                    if (!interaction.beginContextPending("node", nd.id, nodePath(nd.id), nd.x + nd.width / 2, nd.y + nd.height / 2)) {
                        return
                    }
                    isBeingDragged = true
                    // Issue #814 评论 5935285879: 节点中心世界坐标 → 屏幕坐标
                    var sceneX = worldToScreenX(nd.x + nd.width / 2)
                    var sceneY = worldToScreenY(nd.y + nd.height / 2)
                    touchContextPreview.show("node", sceneX, sceneY)
                    edgeCanvas.requestPaint()
                }

                onContextMenuRequested: function(sceneX, sceneY) {
                    var nd = nodeData
                    graphController.selectNode(nd.id)
                    selectedNodeForMenu = nd
                    // Issue #814 评论 5947740838: sceneX/sceneY 是 scene 坐标，
                    // 映射到本 Canvas local 弹菜单、映射到 world 记日志。
                    var cp = sceneToCanvas(sceneX, sceneY)
                    var wp = sceneToWorld(sceneX, sceneY)
                    logInteraction("context_menu_open", "node", nd.id, {
                        "menuKind": "node",
                        "worldX": wp.x,
                        "worldY": wp.y
                    })
                    nodeContextMenu.popup(cp.x, cp.y)
                }

                onMoveDelta: function(dx, dy) {
                    // Issue #814 评论 5947740838: Node 上抛的是 raw scene delta，
                    // 统一映射成 world delta，后面 moveX/moveY/connectMouse* 只存 world。
                    var wd = sceneDeltaToWorld(dx, dy)
                    dx = wd.x
                    dy = wd.y
                    // Issue #801: contextPending 状态下移动超过阈值则转 connect
                    if (interaction.pointerMode === "contextPending" && interaction.connectFromId === nodeData.id) {
                        interaction.connectMouseX += dx
                        interaction.connectMouseY += dy
                        var totalDx = interaction.connectMouseX - (nodeData.x + nodeData.width / 2)
                        var totalDy = interaction.connectMouseY - (nodeData.y + nodeData.height / 2)
                        if (Math.sqrt(totalDx * totalDx + totalDy * totalDy) > interaction._moveThreshold) {
                            interaction.contextPendingToConnect()
                        }
                        edgeCanvas.requestPaint()
                        return
                    }

                    if (interaction.pointerMode === "connect" && interaction.connectFromId === nodeData.id) {
                        interaction.updateConnect(interaction.connectMouseX + dx, interaction.connectMouseY + dy)
                        edgeCanvas.requestPaint()
                        return
                    }

                    if (interaction.pointerMode === "idle") {
                        // Issue #814 评论 5935285879: transient move 永远保存世界坐标，
                        // 不再拿 delegate 的屏幕 x/y 去 beginMove。
                        interaction.beginMove(nodeData.id, nodeData.x, nodeData.y)
                        // Issue #814 评论 5935346839: move_begin 边界日志（node）。
                        logInteraction("move_begin", "node", nodeData.id, {
                            "kind": "node",
                            "fromX": nodeData.x,
                            "fromY": nodeData.y
                        })
                    }

                    if (interaction.pointerMode === "move" && interaction.pressedNodeId === nodeData.id) {
                        // Issue #798: 不再命令式 x+=dx/y+=dy 打断 binding，
                        // 只更新 transient 坐标，delegate 的 x/y binding 自动跟随。
                        interaction.updateMove(interaction.moveX + dx, interaction.moveY + dy)
                        isBeingDragged = true
                        graphController.computeEdgeRenders(currentMoveOverride())
                        edgeCanvas.requestPaint()
                    }
                }

                onLeftReleased: {
                    // 统一结束当前节点交互：无论长按后拖动还是直接松手，
                    // 都由此出口闭环 connect/move/contextPending 状态（Issue #788 评论 5868205321）。
                    isBeingDragged = false
                    // Issue #801 评论 5895310100: contextPending 松手不移动，关闭视觉层并弹出可点击菜单
                    if (interaction.pointerMode === "contextPending" && interaction.connectFromId === nodeData.id) {
                        touchContextPreview.hide()
                        var pendingResult = interaction.endContextPending()
                        if (pendingResult && pendingResult.kind === "node") {
                            var nd = graphController.getNode(pendingResult.id)
                            if (nd) {
                                graphController.selectNode(nd.id)
                                selectedNodeForMenu = nd
                                // 用节点中心世界坐标 → 屏幕坐标弹出菜单
                                var sceneX = worldToScreenX(nd.x + nd.width / 2)
                                var sceneY = worldToScreenY(nd.y + nd.height / 2)
                                // Issue #814 评论 5935346839: context_menu_open 边界日志（node, touch 长按）。
                                logInteraction("context_menu_open", "node", nd.id, {
                                    "menuKind": "node",
                                    "worldX": nd.x + nd.width / 2,
                                    "worldY": nd.y + nd.height / 2
                                })
                                nodeContextMenu.popup(sceneX, sceneY)
                            }
                        }
                        edgeCanvas.requestPaint()
                        return
                    }
                    if (interaction.pointerMode === "connect" && interaction.connectFromId === nodeData.id) {
                        // Issue #796 评论 5887280405: 松手时 Node 和 Embed 都参与命中，
                        // 用 path 版建边支持 Embed 端点。
                        var targetNode = findNodeAt(interaction.connectMouseX, interaction.connectMouseY)
                        var _connectSuccess = false
                        var _connectCancel = false
                        var _toPath = null
                        if (targetNode && targetNode.id !== interaction.connectFromId) {
                            _toPath = nodePath(targetNode.id)
                            _connectSuccess = createEdgeWithPaths(interaction.connectFromPath, _toPath)
                        } else {
                            var targetEmbed = findEmbedChromeAt(interaction.connectMouseX, interaction.connectMouseY)
                            if (targetEmbed && targetEmbed.instanceId !== interaction.connectFromId) {
                                _toPath = embedPath(targetEmbed.instanceId)
                                _connectSuccess = createEdgeWithPaths(interaction.connectFromPath, _toPath)
                            } else {
                                _connectCancel = true
                            }
                        }
                        // Issue #814 评论 5935346839: connect_end 边界日志（node 端）。
                        logInteraction("connect_end", "node", nodeData.id, {
                            "fromPath": JSON.stringify(interaction.connectFromPath),
                            "toPath": _toPath ? JSON.stringify(_toPath) : "",
                            "success": _connectSuccess,
                            "cancel": _connectCancel
                        })
                        interaction.endConnect()
                        edgeCanvas.requestPaint()
                    } else if (interaction.pointerMode === "move" && interaction.pressedNodeId === nodeData.id) {
                        // Issue #801 评论 5895310100: 触屏 move 由 bgTouchDrag.onActiveChanged 独占 commit；
                        // 这里只处理鼠标 move（nodeDragHandler 驱动）。用 _wasTouchMove 区分，
                        // 无论 onLeftReleased 与 bgTouchDrag.onActiveChanged 的触发顺序如何都不会重复 commit。
                        if (!bgTouchDrag._wasTouchMove) {
                            var _nodeCommitOk = graphController.commitNodeMove(nodeData.id, interaction.moveX, interaction.moveY)
                            // Issue #814 评论 5935346839: move_end 边界日志（node, mouse）。
                            logInteraction("move_end", "node", nodeData.id, {
                                "toX": interaction.moveX,
                                "toY": interaction.moveY,
                                "commitSuccess": _nodeCommitOk,
                                "device": "mouse"
                            })
                            interaction.endMove()
                        }
                    }
                }
            }
        }

        // Issue #796 评论 5886483653: Embed Repeater，用 StarMapEmbed.qml 渲染。
        // Issue #814 评论 5935285879: Embed 和 Node 一样直接把世界坐标映射到屏幕坐标，
        // 不再依赖 container.x/y/scale。
        Repeater {
            model: graphController.embedsModel
            delegate: StarMapEmbed {
                // Issue #798: Qt 6.11 Repeater 显式 required property 模型契约。
                required property var modelData
                required property int index
                dt: canvasArea.dt
                property var embedData: modelData

                // Issue #814 评论 5935285879: delegate 直接把世界坐标映射到屏幕坐标。
                // 当前 Embed 处于 move 时读 interaction.moveX/moveY（世界坐标），
                // 否则读 canonical embedData.x/y（世界坐标），再 worldToScreen。
                x: worldToScreenX(interaction.pointerMode === "move" && interaction.pressedEmbedId === embedData.instanceId ? interaction.moveX : embedData.x)
                y: worldToScreenY(interaction.pointerMode === "move" && interaction.pressedEmbedId === embedData.instanceId ? interaction.moveY : embedData.y)
                // scale 把世界尺寸（embedData.width/height）缩放到屏幕尺寸
                scale: zoomLevel
                transformOrigin: Item.TopLeft
                width: embedData.width
                height: embedData.height
                instanceId: embedData.instanceId
                targetStarmapId: embedData.targetStarmapId
                label: embedData.label
                // Issue #814 评论 5935285879: isSelected 从共享 selectionController 派生，
                // 不再读 embedData.isSelected（GraphController 不再维护 isSelected）。
                isSelected: selectionController ? selectionController.matches(pathKey, "embed", embedData.instanceId) : false
                wobbleIndex: index

                // 递归子 Scene 只在这个 Embed 的投影矩形进入当前 Canvas 视口时激活。
                // 不能像旧实现那样只看 targetStarmapId 就递归展开所有子图，否则恢复
                // 星图页面时会在首帧同步构造整棵引用树。留 64px 预取边距，拖动/缩放
                // 接近视口时先开始异步创建，避免刚进入屏幕才闪一下。
                // Issue #814 评论 5935285879: delegate x/y 已是屏幕坐标，projected*
                // 直接用 x/y，不再 *zoomLevel+panX。width/height 是世界尺寸，要 *scale
                // （scale===zoomLevel）换算到屏幕尺寸。
                readonly property real projectedLeft: x
                readonly property real projectedTop: y
                readonly property real projectedRight: projectedLeft + width * scale
                readonly property real projectedBottom: projectedTop + height * scale
                childSceneInViewport: {
                    var margin = 64
                    return canvasArea.visible
                            && projectedRight >= -margin
                            && projectedBottom >= -margin
                            && projectedLeft <= canvasArea.width + margin
                            && projectedTop <= canvasArea.height + margin
                }

                // Issue #805 评论 5907045450 第 2 部分：递归渲染上下文。
                // 传 rootStarmapId / pathSegments / starmapBackendRef 给 Embed，
                // Embed 的 contentViewport 用这些构造子 Scene 的 pathSegments。
                rootStarmapId: canvasArea.rootStarmapId
                parentPathSegments: canvasArea.pathSegments
                // Issue #805 评论 5908703621 问题 1：传 pathKey 给 Embed，
                // Embed 用它构造 child Scene 的 pathKey（父路径 + "/embed_<instanceId>"）。
                parentPathKey: canvasArea.pathKey
                starmapBackendRef: canvasArea.starmapBackendRef
                // Issue #814 评论 5935285879: 共享选中控制器逐层下传，子 Scene 沿用同一个。
                selectionController: canvasArea.selectionController

                // Issue #798: 不再原地篡改 embedData.x/y，拖动用 StarMapEmbed 自己的 x/y
                // 作为临时显示坐标，松手提交 Controller。
                onXChanged: edgeCanvas.requestPaint()
                onYChanged: edgeCanvas.requestPaint()

                // 单击只选中
                // Issue #801 评论 5894981235: 鼠标点击 Embed 时切回鼠标模式，
                // 隐藏触屏 +/- 按钮。
                onMouseInteracted: _touchInputActive = false

                onClicked: function(instId) {
                    graphController.selectEmbed(instId)
                    // Issue #814 评论 5935346839: selection_changed 边界日志（embed）。
                    // clicked 由鼠标/触屏两个 TapHandler 共用，不猜 device。
                    logInteraction("selection_changed", "embed", instId, {})
                }

                // Issue #805 评论 5907045450 第 1/3 部分：双击不再 drillDown。
                // Embed 内部递归渲染子 StarMapScene，不需要"双击进入"语义。
                // onDoubleClicked 信号已从 StarMapEmbed 删除。

                // Issue #805 评论 5908703621 问题 5：child Scene 经 Embed 冒泡上来的
                // 节点编辑请求，转发到 Canvas 的 childEditNodeRequested，Scene 原样上抛。
                // Issue #805 评论 5912394108：ownerScene 一并原样转发，保持指向真正
                // 拥有该节点的子 Scene，不被本层 Canvas/Scene 替换。
                onEditNodeRequested: function(ownerScene, ownerStarmapId, ownerPathKey, node) {
                    canvasArea.childEditNodeRequested(ownerScene, ownerStarmapId, ownerPathKey, node)
                }

                // Issue #801 评论 5894035036: 鼠标长按直接进 connect
                onMouseLongPressed: function(instId) {
                    var ed = embedData
                    if (!interaction.beginConnect("embed", instId, embedPath(ed.instanceId), ed.x + ed.width / 2, ed.y + ed.height / 2)) {
                        return
                    }
                    // Issue #814 评论 5935346839: connect_begin 边界日志（embed）。
                    logInteraction("connect_begin", "embed", instId, {
                        "kind": "embed",
                        "fromId": instId,
                        "fromX": ed.x + ed.width / 2,
                        "fromY": ed.y + ed.height / 2
                    })
                    isBeingDragged = true
                    edgeCanvas.requestPaint()
                }

                // Issue #801 评论 5894035036: 触屏长按进 contextPending
                // Issue #801 评论 5895310100: 触屏长按当场显示菜单视觉层（与 Node 对称）。
                onTouchLongPressed: function(instId) {
                    var ed = embedData
                    if (!interaction.beginContextPending("embed", instId, embedPath(ed.instanceId), ed.x + ed.width / 2, ed.y + ed.height / 2)) {
                        return
                    }
                    isBeingDragged = true
                    // Issue #814 评论 5935285879: Embed 中心世界坐标 → 屏幕坐标
                    var sceneX = worldToScreenX(ed.x + ed.width / 2)
                    var sceneY = worldToScreenY(ed.y + ed.height / 2)
                    touchContextPreview.show("embed", sceneX, sceneY)
                    edgeCanvas.requestPaint()
                }

                // 右键上抛菜单
                onContextMenuRequested: function(instId, sceneX, sceneY) {
                    graphController.selectEmbed(instId)
                    selectedEmbedForMenu = graphController.getEmbed(instId)
                    // Issue #814 评论 5947740838: scene 坐标映射到 Canvas local 弹菜单、world 记日志。
                    var cp = sceneToCanvas(sceneX, sceneY)
                    var wp = sceneToWorld(sceneX, sceneY)
                    logInteraction("context_menu_open", "embed", instId, {
                        "menuKind": "embed",
                        "worldX": wp.x,
                        "worldY": wp.y
                    })
                    embedContextMenu.popup(cp.x, cp.y)
                }

                // Issue #796 评论 5888480054: Embed 拖动改上抛 moveDelta 增量，
                // 和 Node 的 onMoveDelta 对称。connect 模式更新预览线终点；
                // idle 转 move 移动 Embed position。
                onMoveDelta: function(dx, dy) {
                    // Issue #814 评论 5947740838: Embed 上抛的是 raw scene delta，
                    // 统一映射成 world delta，后面 moveX/moveY/connectMouse* 只存 world。
                    var wd = sceneDeltaToWorld(dx, dy)
                    dx = wd.x
                    dy = wd.y
                    // Issue #801: contextPending 状态下移动超过阈值则转 connect
                    if (interaction.pointerMode === "contextPending" && interaction.connectFromId === embedData.instanceId) {
                        interaction.connectMouseX += dx
                        interaction.connectMouseY += dy
                        var totalDx = interaction.connectMouseX - (embedData.x + embedData.width / 2)
                        var totalDy = interaction.connectMouseY - (embedData.y + embedData.height / 2)
                        if (Math.sqrt(totalDx * totalDx + totalDy * totalDy) > interaction._moveThreshold) {
                            interaction.contextPendingToConnect()
                        }
                        edgeCanvas.requestPaint()
                        return
                    }

                    if (interaction.pointerMode === "connect" && interaction.connectFromId === embedData.instanceId) {
                        interaction.updateConnect(interaction.connectMouseX + dx, interaction.connectMouseY + dy)
                        edgeCanvas.requestPaint()
                        return
                    }

                    if (interaction.pointerMode === "idle") {
                        // Issue #814 评论 5935285879: transient move 永远保存世界坐标，
                        // 不再拿 delegate 的屏幕 x/y 去 beginEmbedMove。
                        interaction.beginEmbedMove(embedData.instanceId, embedData.x, embedData.y)
                        // Issue #814 评论 5935346839: move_begin 边界日志（embed）。
                        logInteraction("move_begin", "embed", embedData.instanceId, {
                            "kind": "embed",
                            "fromX": embedData.x,
                            "fromY": embedData.y
                        })
                    }

                    if (interaction.pointerMode === "move" && interaction.pressedEmbedId === embedData.instanceId) {
                        // Issue #798: 不再命令式 x+=dx/y+=dy 打断 binding，
                        // 只更新 transient 坐标，delegate 的 x/y binding 自动跟随。
                        interaction.updateMove(interaction.moveX + dx, interaction.moveY + dy)
                        isBeingDragged = true
                        graphController.computeEdgeRenders(currentMoveOverride())
                        edgeCanvas.requestPaint()
                    }
                }

                onLeftReleased: {
                    isBeingDragged = false
                    // Issue #801 评论 5895310100: contextPending 松手不移动，关闭视觉层并弹出可点击菜单
                    if (interaction.pointerMode === "contextPending" && interaction.connectFromId === embedData.instanceId) {
                        touchContextPreview.hide()
                        var pendingResult = interaction.endContextPending()
                        if (pendingResult && pendingResult.kind === "embed") {
                            var ed = graphController.getEmbed(pendingResult.id)
                            if (ed) {
                                graphController.selectEmbed(ed.instanceId)
                                selectedEmbedForMenu = ed
                                // 用 Embed 中心世界坐标 → 屏幕坐标弹出菜单
                                var sceneX = worldToScreenX(ed.x + ed.width / 2)
                                var sceneY = worldToScreenY(ed.y + ed.height / 2)
                                // Issue #814 评论 5935346839: context_menu_open 边界日志（embed, touch 长按）。
                                logInteraction("context_menu_open", "embed", ed.instanceId, {
                                    "menuKind": "embed",
                                    "worldX": ed.x + ed.width / 2,
                                    "worldY": ed.y + ed.height / 2
                                })
                                embedContextMenu.popup(sceneX, sceneY)
                            }
                        }
                        edgeCanvas.requestPaint()
                        return
                    }
                    // Issue #796 评论 5887280405: connect 模式下松手，Node 和 Embed 都参与命中，
                    // 用 path 版建边；否则走原拖动结束保存位置逻辑。
                    if (interaction.pointerMode === "connect" && interaction.connectFromId === embedData.instanceId) {
                        var targetNode = findNodeAt(interaction.connectMouseX, interaction.connectMouseY)
                        var _eSuccess = false
                        var _eCancel = false
                        var _eToPath = null
                        if (targetNode && targetNode.id !== interaction.connectFromId) {
                            _eToPath = nodePath(targetNode.id)
                            _eSuccess = createEdgeWithPaths(interaction.connectFromPath, _eToPath)
                        } else {
                            var targetEmbed = findEmbedChromeAt(interaction.connectMouseX, interaction.connectMouseY)
                            if (targetEmbed && targetEmbed.instanceId !== interaction.connectFromId) {
                                _eToPath = embedPath(targetEmbed.instanceId)
                                _eSuccess = createEdgeWithPaths(interaction.connectFromPath, _eToPath)
                            } else {
                                _eCancel = true
                            }
                        }
                        // Issue #814 评论 5935346839: connect_end 边界日志（embed 端）。
                        logInteraction("connect_end", "embed", embedData.instanceId, {
                            "fromPath": JSON.stringify(interaction.connectFromPath),
                            "toPath": _eToPath ? JSON.stringify(_eToPath) : "",
                            "success": _eSuccess,
                            "cancel": _eCancel
                        })
                        interaction.endConnect()
                        edgeCanvas.requestPaint()
                    } else if (interaction.pointerMode === "move" && interaction.pressedEmbedId === embedData.instanceId) {
                        // Issue #801 评论 5895310100: 触屏 move 由 bgTouchDrag.onActiveChanged 独占 commit；
                        // 这里只处理鼠标 move。用 _wasTouchMove 区分避免重复 commit。
                        if (!bgTouchDrag._wasTouchMove) {
                            var _embedCommitOk = graphController.commitEmbedMove(embedData.instanceId, interaction.moveX, interaction.moveY)
                            // Issue #814 评论 5935346839: move_end 边界日志（embed, mouse）。
                            logInteraction("move_end", "embed", embedData.instanceId, {
                                "toX": interaction.moveX,
                                "toY": interaction.moveY,
                                "commitSuccess": _embedCommitOk,
                                "device": "mouse"
                            })
                            interaction.endMove()
                        }
                    }
                }
            }
        }
    }

    AppText {
        dt: canvasArea.dt
        anchors.centerIn: parent
        text: qsTr("右键空白处新建节点或子星图")
        color: _textSecondary
        font.pointSize: dt.fontLgPt
        visible: graphController.nodesModel.length === 0 && graphController.embedsModel.length === 0
    }

    // Issue #801 评论 5894639734: 触屏缩放 +/- 按钮（右下角浮层）。
    // 按需显示：第一次收到 TouchScreen 事件时显示，切回 Mouse 时隐藏。
    // 不做硬件探测，靠 _touchInputActive 跟踪最近一次输入设备。
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
            onClicked: {
                // 单击放大：直接调到上限，不触发 drillUp
                zoomLevel = Math.min(2.5, zoomLevel + 0.15)
            }
        }

        AppButton {
            dt: canvasArea.dt
            text: qsTr("−")
            onClicked: {
                var newZoom = zoomLevel - 0.15
                // Issue #805 评论 5907045450 第 1 部分：缩到最小时只 clamp，
                // 不再触发 drillUp（递归渲染由 StarMapScene 处理）。
                zoomLevel = Math.max(0.35, newZoom)
            }
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
            text: errorMessage
            color: _onError
            font.pointSize: dt.bodyPt
        }
        MouseArea {
            anchors.fill: parent
            onClicked: clearError()
        }
    }

    // Issue #801 评论 5895310100: 触屏长按菜单视觉层（不抢 pointer grab）。
    // #373 要求长按时菜单先出现作为视觉反馈；手指继续移动超过阈值则关闭转连线，
    // 手指松开则关闭视觉层并弹出真正可点击的 Menu。
    // 此组件纯视觉，无任何 TapHandler/MouseArea/Handler，不会抢走正在进行的触摸手势。
    Item {
        id: touchContextPreview
        visible: false
        z: 60

        property string previewKind: ""   // "node" / "embed"
        // 屏幕坐标锚点
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

    function currentMoveOverride() {
        if (interaction.pointerMode === "move") {
            if (interaction.pressedNodeId !== "") {
                return { kind: "node", id: interaction.pressedNodeId, x: interaction.moveX, y: interaction.moveY }
            } else if (interaction.pressedEmbedId !== "") {
                return { kind: "embed", id: interaction.pressedEmbedId, x: interaction.moveX, y: interaction.moveY }
            }
        }
        return null
    }

    function loadGraph() {
        graphController.loadGraph()
    }

    function buildModels() {
        graphController.buildModels()
    }

    function getNode(id) {
        if (sceneLayer) {
            for (var i = 0; i < sceneLayer.children.length; i++) {
                var child = sceneLayer.children[i];
                if (child && child.nodeData && child.nodeData.id === id) {
                    return child;
                }
            }
        }
        // Issue #798: 回退到 Controller 的 canonical 模型。
        return graphController.getNode(id);
    }

    function clearSelection() {
        graphController.clearSelection()
    }

    // Issue #796 评论 5886483653: 新建子星图改回正式 Embed 语义。
    // 流程：create_starmap → create_starmap_embed；失败时删除刚创建的目标 StarMap，
    // 成功后 reload graph 并选中新 Embed。
    // 旧 portal Node 仍按已有数据正常显示/进入，不再用它创建新的子星图。
    // title 由 createDialog 收集后传入，不再写死默认名。
    // Issue #798: 图操作移进 GraphController，Canvas 只转发。
    function createSubStarmapAt(title, wx, wy) {
        graphController.createSubStarmapAt(title, wx, wy)
    }

    function createEdge(fromId, toId) {
        graphController.createEdge(fromId, toId)
    }

    // Issue #796 评论 5887280405: Node 端点的 StarMapTargetPathDto JS 对象。
    // JSON 字段名遵循 DTO serde rename：starmapId（camelCase）、segments、target.type/nodeId。
    // Issue #805 评论 5908703621 问题 4：nodePath 由 Scene 上下文构造，
    // starmapId 用 rootStarmapId，segments 用 pathSegments.slice(0)，
    // 这样第 N 层连线端点不会退化成第一层。
    function nodePath(nodeId) {
        return {
            starmapId: rootStarmapId,
            segments: pathSegments.slice(0),
            target: { type: "node", nodeId: nodeId }
        }
    }

    // Issue #805 评论 5908703621 问题 4：Embed 端点的完整路径由 Canvas 拼接。
    // Controller 的 embedPathSegment 只返回当前 Embed 的一个 segment，
    // Canvas 用 pathSegments.concat 拼完整路径，starmapId 用 rootStarmapId。
    function embedPath(instanceId) {
        return {
            starmapId: rootStarmapId,
            segments: pathSegments.concat([
                graphController.embedPathSegment(instanceId)
            ]),
            target: { type: "starmap" }
        }
    }

    // Issue #796 评论 5887280405: 用 fromPath/toPath 建边，支持 Node 和 Embed 端点。
    // Issue #814 评论 5945557717 问题 3: 返回后端真实结果，connect_end.success 用它。
    function createEdgeWithPaths(fromPath, toPath) {
        return graphController.createEdgeWithPaths(fromPath, toPath)
    }

    function updateNodeFromInspector(nodeId, patch) {
        graphController.updateNode(nodeId, patch)
    }

    function deleteNodeFromInspector(nodeId) {
        graphController.deleteNode(nodeId)
    }

    function updateEdgeFromInspector(edgeId, patch) {
        graphController.updateEdge(edgeId, patch)
    }

    function deleteEdgeFromInspector(edgeId) {
        graphController.deleteEdge(edgeId)
    }

    // Helper function to find a node at world coordinates
    function findNodeAt(wx, wy) {
        return graphController.findNodeAt(wx, wy)
    }

    // Issue #805 评论 5908703621 问题 3：findEmbedChromeAt 只判断 chrome 命中区域
    // （标题条 + 四条 border），内部矩形返回 null。父 Canvas 不再把整个 Embed
    // 矩形判成命中，内部事件不会被父场景截走。
    function findEmbedChromeAt(wx, wy) {
        return graphController.findEmbedChromeAt(wx, wy)
    }

    // Issue #814 评论 5945557717 问题 1: findEmbedContentAt 判断整个 Embed 矩形内、
    // 但不在 chrome 的区域（子星图 contentViewport）。pointer_press 据此把合法的
    // 子场景内部点击记成 childContent，不再冒充 empty。
    function findEmbedContentAt(wx, wy) {
        return graphController.findEmbedContentAt(wx, wy)
    }

    // Issue #796 评论 5886483653: Embed 增删改转发给 graphController
    function updateEmbedFromInspector(instanceId, patch) {
        graphController.updateEmbed(instanceId, patch)
    }

    function deleteEmbedFromInspector(instanceId) {
        graphController.deleteEmbed(instanceId)
    }

    // Issue #793 评论 5884923277: 新节点落点选择，避免压在已有节点上。
    // 从 (wx,wy) 起按 ring 扩张枚举候选格点，第一个不与现有节点矩形相交的即返回。
    function findFreeSpawnPoint(wx, wy) {
        var step = 24
        var candidates = [{x: wx, y: wy}]

        for (var ring = 1; ring <= 8; ring++) {
            for (var dx = -ring; dx <= ring; dx++) {
                candidates.push({ x: wx + dx * step, y: wy - ring * step })
                candidates.push({ x: wx + dx * step, y: wy + ring * step })
            }
            for (var dy = -ring + 1; dy <= ring - 1; dy++) {
                candidates.push({ x: wx - ring * step, y: wy + dy * step })
                candidates.push({ x: wx + ring * step, y: wy + dy * step })
            }
        }

        for (var i = 0; i < candidates.length; i++) {
            if (!overlapsExistingNode(candidates[i].x, candidates[i].y, 150, 60, 12))
                return candidates[i]
        }

        return { x: wx, y: wy }
    }

    // 用 graphController.nodesModel 当前的 x/y/width/height 做矩形相交，四边多留 padding。
    function overlapsExistingNode(x, y, w, h, padding) {
        var nodes = graphController.nodesModel
        for (var i = 0; i < nodes.length; i++) {
            var n = nodes[i]
            var nx = n.x - padding
            var ny = n.y - padding
            var nw = n.width + padding * 2
            var nh = n.height + padding * 2
            if (x < nx + nw && x + w > nx && y < ny + nh && y + h > ny)
                return true
        }
        return false
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
            onTriggered: createDialog.open("node", contextMenuWorldX, contextMenuWorldY)
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
            onTriggered: createDialog.open("starmap", contextMenuWorldX, contextMenuWorldY)
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
                // Issue #791: 右键"编辑"走 editNodeRequested 打开按需浮层
                if (selectedNodeForMenu) {
                    graphController.selectNode(selectedNodeForMenu.id)
                    editNodeRequested(selectedNodeForMenu)
                }
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
                if (selectedNodeForMenu) {
                    interaction.beginMove(selectedNodeForMenu.id, selectedNodeForMenu.x, selectedNodeForMenu.y)
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
                if (selectedNodeForMenu) {
                    deleteNodeFromInspector(selectedNodeForMenu.id)
                }
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
                if (selectedEdgeForMenu) {
                    deleteEdgeFromInspector(selectedEdgeForMenu.id)
                }
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

        // Issue #796 评论 5887280405: Embed 移动菜单项，与 Node 移动对称。
        // 触发后进入 move 模式，pressedEmbedId 记录待移动 Embed。
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
                if (selectedEmbedForMenu) {
                    interaction.beginEmbedMove(selectedEmbedForMenu.instanceId, selectedEmbedForMenu.x, selectedEmbedForMenu.y)
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
                if (selectedEmbedForMenu) {
                    deleteEmbedFromInspector(selectedEmbedForMenu.instanceId)
                }
            }
        }
    }

    // Issue #796 评论 5886483653: 弹窗改 Qt Quick Controls Popup，不再手搓整屏 Rectangle。
    // modal + focus + closePolicy 交给 Popup，遮罩用 Overlay.modal 做半透明 dim。
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

        property string targetType: "" // "node" or "edge"
        property string targetId: ""
        property string initialText: ""

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 16

            AppText {
                dt: canvasArea.dt
                text: renameDialog.targetType === "node" ? qsTr("修改节点标题")
                     : renameDialog.targetType === "embed" ? qsTr("修改子星图名称")
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
            if (targetType === "node") {
                updateNodeFromInspector(targetId, { title: renameInput.text })
            } else if (targetType === "edge") {
                updateEdgeFromInspector(targetId, { label: renameInput.text })
            } else if (targetType === "embed") {
                updateEmbedFromInspector(targetId, { label: renameInput.text })
            }
            close()
        }
    }

    // Issue #793 评论 5884923277: 新建节点/子星图 Dialog
    // Issue #796 评论 5886483653: 改用 Qt Quick Controls Popup，不再手搓整屏 Rectangle。
    // 右键空白处不再直接创建，先收集名字再写 Core。
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

        function open(mode, wx, wy) {
            createMode = mode
            createWorldX = wx
            createWorldY = wy
            createInput.text = ""
            visible = true
            createInput.forceActiveFocus()
        }

        function close() {
            visible = false
        }

        function confirm() {
            var name = createInput.text.trim()
            if (name.length === 0) {
                close()
                return
            }
            // 先算 spawn 落点，避免新节点压在旧节点上
            var spawn = findFreeSpawnPoint(createWorldX, createWorldY)
            if (createMode === "node") {
                graphController.createNode(name, spawn.x, spawn.y)
            } else if (createMode === "starmap") {
                createSubStarmapAt(name, spawn.x, spawn.y)
            }
            close()
        }
    }


}
