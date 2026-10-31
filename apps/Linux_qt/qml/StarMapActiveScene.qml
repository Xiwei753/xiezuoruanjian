// =============================================================================
// StarMapActiveScene.qml — 当前活动星图的实时矢量场景（独立无限画布）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：渲染「一张活动星图」的全部内容（节点 / 连线 / 子星图入口 / 文字），
//   提供该画布的唯一命中测试与 Router 需要的归属层 API。
//
// Issue #929：独立画布协议。整棵星图不再共用一台全局相机，也不再递归渲染
// 子画布内容：任意时刻只有 currentStarmapId 这一张活动画布。
//   - 本组件没有自己的相机状态：zoomLevel / panX / panY 由 StarMapCanvas
//     的当前镜头注入，本组件只做投影。Core 不存相机，平台层只做投影与渲染。
//   - 单一投影入口：world 几何 → 当前活动相机 → 视口几何（worldToScreenX/Y）。
//     渲染时以视口中心为相对原点（视口中心的世界坐标现算），避免巨大世界坐标
//     乘极大 zoom 后的浮点抖动；镜头尺度只有数值安全边界，边界不触发层级切换。
//   - 节点 / 子星图入口是 Qt Quick 矢量几何（矩形 / 正圆），连线由
//     QtQuick.Shapes 的 ShapePath 按当前投影实时绘制，文字由实时 Text/TextInput/
//     TextEdit 绘制。没有 layer FBO、没有缩略图纹理、没有把 1x 位图放大 N 倍。
//   - 线宽 / 文字字号是确定的屏幕口径：线宽 = world 线宽 × 镜头比例（夹在
//     1~6 屏幕像素）；字号 = 用户字号 token × 镜头比例（夹在 0.6~8 倍），
//     都在屏幕像素上现算，不依赖任何预栅格化纹理。
//   - 子星图入口在父图里只画 shell + fit-to-bounds 微缩概览
//     （StarMapEmbed → StarMapSceneContent），不递归创建可交互子场景。
//
// 坐标约定：
//   - 本组件局部坐标 = 当前活动画布的 authored world 坐标（场景本身不平移、
//     不缩放；相机只体现在投影函数里）。
//   - 输入路由 StarMapInputRouter 的 hit.owner 就是本组件：它按世界坐标查命中，
//     移动 / 连线 / 建边也全部落在本画布自己的 GraphController 上。
//
// 约束：
//   - 不出现 WheelHandler / PinchHandler / 背景 pan handler：相机与手势属于 Canvas
//   - 不写 Core 的相机 / 视口 / 缩放信息；业务状态仍由 Core 保存
// =============================================================================

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Shapes
import "StarMapPathPlanner.js" as StarMapPathPlanner
import "StarMapNodeMetrics.js" as StarMapNodeMetrics

Item {
    id: scene

    required property var dt
    required property color canvasBackgroundColor
    // 当前活动画布 ID：由 Canvas 用 Core 导航路由解析后传入，QML 不自己猜目标 ID。
    required property string starmapId
    // 导航路由身份（日志 / 选中身份用；同一目标图的不同 Embed 入口靠完整 route 区分）。
    required property string rootStarmapId
    required property var pathSegments
    required property string scenePathKey
    required property var starmapBackendRef
    required property var selectionController
    required property var interactionController
    // 菜单宿主（根 Canvas）：内联编辑汇总键、长按菜单都回调它。
    property var menuHost: null
    // Issue #987：内联编辑器的视觉父级（Canvas 顶层宿主，位于输入路由之上）。
    // Scene 只声明唯一编辑器并持有草稿；宿主只提供层级与坐标。过渡旧层不注入，
    // 保持为场景内部子项且不参与编辑。
    property var editOverlayHost: null

    // ── 活动镜头（由 StarMapCanvas 注入）──
    // 屏幕坐标 = pan + world × zoom；pan 是世界原点的屏幕位置。
    property real zoomLevel: 1.0
    property real panX: 0
    property real panY: 0

    // ── Issue #949：过渡的显示层开关（不参与命中 / 业务状态）──
    // detailReveal：0 时节点画成概览同款小圆点，1 时是当前层完整节点
    //   （连线跟随同一几何、标题随后显现）。过渡期间只改显示，不改 authored
    //   坐标、图关系、选中对象或碰撞算法。
    property real detailReveal: 1.0
    // interactive=false 时本层只是过渡期间的旧画面：不参与 inline-edit 汇总
    //   （命中 / 菜单 / 编辑都不走它）。
    property bool interactive: true
    // 母图侧要"继续画到整圈离开可见视口"的入口圆壳（进场 / 出场那一层标它）。
    property string transitionEmbedId: ""
    // 过渡圆环（同一个圆壳的放大描边 + 圆内表面）：圆心 / 半径按同一时间线现算，
    //   只画在"圆内"的那一层（进场 = 活动子画布，出场 = 旧子画面层）。
    property bool ringVisible: false
    property real ringCenterX: 0
    property real ringCenterY: 0
    property real ringRadius: 0
    property real ringStrokeWidth: 2
    // 圆内表面在同一穿越进度上从入口壳颜色过渡到画布底色。
    property real ringBackgroundProgress: 0.0
    readonly property color transitionRingColor: {
        var from = dt.surfaceContainerLow
        var to = canvasBackgroundColor
        var p = Math.max(0, Math.min(1, ringBackgroundProgress))
        return Qt.rgba(from.r + (to.r - from.r) * p,
                       from.g + (to.g - from.g) * p,
                       from.b + (to.b - from.b) * p,
                       from.a + (to.a - from.a) * p)
    }

    // ── 单一投影入口 ──
    // 数学上 worldToScreenX(w) === panX + w * zoom，渲染时改写成
    // "视口中心 + (w - 视口中心世界坐标) × zoom"：中心偏移在 double 里现算，
    // 场景图只见到视口量级的相对坐标。
    readonly property real _safeZoom: zoomLevel > 0 && isFinite(zoomLevel) ? zoomLevel : 1.0
    function _finiteProduct(a, b) {
        var value = a * b
        if (isFinite(value)) return value
        return (a < 0) !== (b < 0) ? -Number.MAX_VALUE : Number.MAX_VALUE
    }
    function _finiteSum(a, b) {
        var value = a + b
        if (isFinite(value)) return value
        return value === Infinity ? Number.MAX_VALUE : -Number.MAX_VALUE
    }
    function _finiteDifference(a, b) {
        var value = a - b
        if (isFinite(value)) return value
        return value === Infinity ? Number.MAX_VALUE : -Number.MAX_VALUE
    }
    function _finiteQuotient(a, b) {
        if (!isFinite(b) || b === 0) return 0
        var value = a / b
        if (isFinite(value)) return value
        return value === Infinity ? Number.MAX_VALUE : -Number.MAX_VALUE
    }
    readonly property real _originWorldX: _finiteQuotient(_finiteDifference(width / 2, panX), _safeZoom)
    readonly property real _originWorldY: _finiteQuotient(_finiteDifference(height / 2, panY), _safeZoom)

    function worldToScreenX(wx) {
        return _finiteSum(width / 2, _finiteProduct(_finiteDifference(wx, _originWorldX), _safeZoom))
    }
    function worldToScreenY(wy) {
        return _finiteSum(height / 2, _finiteProduct(_finiteDifference(wy, _originWorldY), _safeZoom))
    }
    function screenToWorldX(sx) {
        return _finiteSum(_originWorldX, _finiteQuotient(_finiteDifference(sx, width / 2), _safeZoom))
    }
    function screenToWorldY(sy) {
        return _finiteSum(_originWorldY, _finiteQuotient(_finiteDifference(sy, height / 2), _safeZoom))
    }

    // Qt scene（窗口）坐标增量 → 屏幕增量 → world 增量。
    function qtSceneDeltaToWorld(dx, dy) {
        var origin = scene.mapFromItem(null, 0, 0)
        var point = scene.mapFromItem(null, dx, dy)
        return { x: (point.x - origin.x) / _safeZoom, y: (point.y - origin.y) / _safeZoom }
    }

    // ── 屏幕口径常量 ──
    // 边线宽：world 线宽 × 镜头比例，夹在 1~6 屏幕像素（缩放是缩放，但不做
    // 亚像素 / 巨线）。命中也吃同一份屏幕口径。
    readonly property real _edgeLineWorldWidth: 2
    readonly property real _edgeScreenStrokeWidth:
        Math.max(1.0, Math.min(_edgeLineWorldWidth * _safeZoom, 6.0))
    readonly property real _edgeHitScreenPx: 10
    function _edgeHitThresholdWorld() {
        return Math.max(_edgeHitScreenPx / _safeZoom, _edgeLineWorldWidth / 2)
    }
    // 文字口径：用户字号 token × 镜头比例，夹在 0.6~8 倍（字形缓存安全边界，
    // 不是层级边界）。投影太小的对象直接不画文字。
    readonly property real _textScale: Math.max(0.6, Math.min(8.0, _safeZoom))

    // Issue #949：概览圆点口径（与 StarMapSceneContent 的概览圆点同一公式），
    // 展开进度 0 时活动画布的节点先以这个屏幕半径的圆点出现，逐步长成完整节点。
    readonly property real _revealDotRadius:
        Math.max(2, Math.min(6, Math.min(width, height) * 0.018))
    // 展开进度（夹取，显示层安全）。
    readonly property real _reveal: Math.max(0, Math.min(1, detailReveal))

    readonly property color _accent: dt.accent
    readonly property color _border: dt.border
    readonly property color _error: dt.error
    readonly property color _surfaceContainer: dt.surfaceContainer
    readonly property color _textPrimary: dt.textPrimary
    readonly property color _textMuted: dt.textMuted

    // ── 本层图控制器：边渲染 / 命中 / 增删改仍然只有这一份真相 ──
    StarMapGraphController {
        id: graphController
        starmapId: scene.starmapId
        starmapBackendRef: scene.starmapBackendRef
        pathKey: scene.scenePathKey
        selectionController: scene.selectionController
        onGraphChanged: {
            scene.refreshEdges()
            scene.ensureEditingNodeExists()
        }
        onStructureChanged: scene.structureChanged()
        onSelectionCleared: scene.selectionCleared()
        onNodeSelected: function(node) { scene.nodeSelected(node) }
        onEdgeSelected: function(edge) { scene.edgeSelected(edge) }
    }

    // 隐藏排版器：不依赖节点是否显示在屏幕上。
    StarMapNodeMeasurer {
        id: nodeMeasurer
    }

    // 切换活动画布时显式落 id/pathKey 再加载：不能只依赖绑定求值顺序，
    // 否则 change handler 可能在绑定更新前先跑，读到上一张画布的图。
    function reloadActiveGraph() {
        graphController.starmapId = scene.starmapId
        graphController.pathKey = scene.scenePathKey
        graphController.loadGraph()
    }

    onStarmapIdChanged: {
        // 正常切图由 Canvas 先 flushPendingNodeEdit()；能走到这里说明旧画面
        // 已被替换，不能再把旧草稿写进新图。显式结束会话，不留悬挂输入层。
        finishActiveNodeEdit()
        reloadActiveGraph()
    }
    onScenePathKeyChanged: {
        graphController.pathKey = scene.scenePathKey
    }
    Component.onCompleted: {
        // 保证测量器先就位，首帧加载就按真实排版建图级快照。
        graphController.textMeasurer = nodeMeasurer
        reloadActiveGraph()
    }

    readonly property bool ready: starmapId !== "" && graphController.graphData !== null
    readonly property string errorMessage: graphController.errorMessage
    function clearError() { graphController.clearError() }

    signal selectionCleared()
    signal nodeSelected(var node)
    signal edgeSelected(var edge)
    // 节点/嵌入已成功落盘的结构或标题变化，供 Workspace 刷新导航树。
    signal structureChanged()

    // ── Issue #984：场景级唯一内联编辑会话 ──
    // 草稿只在这里保存一份；delegate 只负责显示和发出“编辑此节点”的意图。
    // 打开另一个节点前先提交当前节点，失败则继续留在原节点，避免多节点同时
    // 编辑、切画布只 flush 最后一个而丢掉其它草稿。
    property string editingNodeId: ""
    property string editingTitleDraft: ""
    property string editingBodyDraft: ""
    property string editingError: ""
    property bool editingSaving: false
    // Issue #989：打开编辑时的节点快照。稳定编辑器不能因为一次模型查询
    // 短暂返回 null 就静默隐藏；节点真正删除由 ensureEditingNodeExists() 收尾。
    property var editingNodeSnapshot: null
    // 打开该会话的左/右键手势序号，用于把 begin/ready/commit 串成同一条日志链。
    property int editingGestureId: 0
    property string editingGestureButton: ""
    readonly property bool hasActiveNodeEdit: editingNodeId !== ""
    readonly property string inlineEditingKey:
        editingNodeId !== "" ? scenePathKey + "/node_" + editingNodeId : ""

    onInlineEditingKeyChanged: {
        if (menuHost)
            menuHost.setInlineEditingKey(inlineEditingKey)
    }

    function finishActiveNodeEdit(preservePresentation) {
        if (!preservePresentation && editOverlay)
            editOverlay.abortTransition()
        editingNodeId = ""
        editingTitleDraft = ""
        editingBodyDraft = ""
        editingError = ""
        editingSaving = false
        editingNodeSnapshot = null
        editingGestureId = 0
        editingGestureButton = ""
    }

    // 兼容旧调用点：true = 进入编辑（先提交上一个），false = 结束该节点会话。
    function noteInlineEditing(nodeId, editing) {
        if (!interactive)
            return
        if (editing) {
            beginInlineEdit(nodeId)
        } else if (editingNodeId === nodeId) {
            editOverlay.closeEdit(false)
        }
    }

    // 正在编辑的节点从图里消失（删除 / 外部刷新）时，先结束会话，
    // 不能让稳定编辑器挂着已不存在的节点。
    function ensureEditingNodeExists() {
        if (editingNodeId !== "" && !graphController.getNode(editingNodeId))
            finishActiveNodeEdit()
    }

    // Issue #814 评论 5935346839: 单层交互边界日志入口，不进连续移动热路径。
    function logInteraction(event, itemKind, itemId, fields) {
        if (!starmapBackendRef)
            return
        var fj = fields ? JSON.stringify(fields) : ""
        starmapBackendRef.record_interaction(event, scenePathKey, starmapId, itemKind, itemId, fj)
    }

    // ── 路径构造（StarMapTargetPathDto JS 对象，camelCase）──
    // 从根星图出发的绝对路径：rootStarmapId + 本画布完整 segments + 目标。
    function nodePath(nodeId) {
        return {
            starmapId: rootStarmapId,
            segments: (pathSegments || []).slice(0),
            target: { type: "node", nodeId: nodeId }
        }
    }
    function embedPath(instanceId) {
        return {
            starmapId: rootStarmapId,
            segments: (pathSegments || []).concat([graphController.embedPathSegment(instanceId)]),
            target: { type: "starmap" }
        }
    }

    // ── 路径比较（Core DTO 的 null 字段不影响语义相等）──
    function _sameSegment(a, b) {
        if (!a || !b || a.type !== b.type)
            return false
        if (a.type === "enterEmbed")
            return (a.instanceId || "") === (b.instanceId || "")
        if (a.type === "enterPortal")
            return (a.nodeId || "") === (b.nodeId || "")
        return false
    }
    function _sameSegments(a, b) {
        if (!a || !b || a.length !== b.length)
            return false
        for (var i = 0; i < a.length; i++) {
            if (!_sameSegment(a[i], b[i]))
                return false
        }
        return true
    }

    // Link 没有独立位置：把 source path 解析成真正可画、可选的
    // 宿主对象（node 或 embed）。返回 null 表示该来源不是本画布可定位对象。
    function sourcePathOwner(sourcePath) {
        if (!sourcePath)
            return null
        var target = sourcePath.target || {}
        if (target.type === "node" && target.nodeId)
            return { kind: "node", id: target.nodeId }
        if (target.type === "anchor" && target.nodeId)
            return { kind: "node", id: target.nodeId }
        var segments = sourcePath.segments || []
        for (var i = segments.length - 1; i >= 0; i--) {
            if (segments[i].type === "enterEmbed" && segments[i].instanceId)
                return { kind: "embed", id: segments[i].instanceId }
        }
        return null
    }
    function linkOwner(linkId) {
        var items = graphController.listLinks()
        for (var i = 0; i < items.length; i++) {
            if (items[i].linkId === linkId)
                return sourcePathOwner(items[i].source)
        }
        return null
    }

    // ── 本画布可操作对象 ──
    function hitNode(nodeId) { return graphController.getNode(nodeId) }
    function hitEmbed(instanceId) { return graphController.getEmbed(instanceId) }
    function hitEdge(edgeId) {
        for (var i = 0; i < graphController.edgesModel.length; i++) {
            if (graphController.edgesModel[i].id === edgeId)
                return graphController.edgesModel[i]
        }
        return null
    }
    // 连线没有独立 item；用 Core 渲染后的两端世界坐标取中点。
    function edgeCenterScene(edgeId) {
        var renders = graphController.edgeRenders || []
        for (var i = 0; i < renders.length; i++) {
            if (renders[i].edgeId === edgeId) {
                return {
                    x: (renders[i].startX + renders[i].endX) / 2,
                    y: (renders[i].startY + renders[i].endY) / 2
                }
            }
        }
        return null
    }
    function itemOf(kind, id) {
        return kind === "node" ? graphController.getNode(id) : graphController.getEmbed(id)
    }
    // 命中对象在本画布 world 坐标里的中心 / 矩形（scene 坐标 == world 坐标）。
    function itemCenterScene(kind, id) {
        if (kind === "edge")
            return edgeCenterScene(id)
        var item = itemOf(kind, id)
        if (!item)
            return null
        return { x: item.x + item.width / 2, y: item.y + item.height / 2 }
    }
    function itemSceneRect(kind, id) {
        var item = itemOf(kind, id)
        if (!item)
            return null
        return { x: item.x, y: item.y, width: item.width, height: item.height }
    }
    function selectNode(nodeId) { graphController.selectNode(nodeId) }
    function selectEdge(edgeId) { graphController.selectEdge(edgeId) }
    function selectEmbed(instanceId) { graphController.selectEmbed(instanceId) }
    function clearLayerSelection() { graphController.clearSelection() }
    function updateNodeTitle(nodeId, title) { graphController.updateNode(nodeId, { title: title }) }
    function deleteNode(nodeId) {
        // 删除是明确丢弃该节点；先结束它的编辑会话，避免编辑器悬挂。
        if (editingNodeId === nodeId)
            finishActiveNodeEdit()
        graphController.deleteNode(nodeId)
    }
    function updateEdgeLabel(edgeId, label) { graphController.updateEdge(edgeId, { label: label }) }
    function deleteEdge(edgeId) { graphController.deleteEdge(edgeId) }
    function updateEmbedLabel(instanceId, label) { graphController.updateEmbed(instanceId, { label: label }) }
    function deleteEmbed(instanceId) { graphController.deleteEmbed(instanceId) }
    // Issue #984：普通节点名称 + 正文统一保存入口。
    // title === null 表示只改正文；body === null 表示只改名称或引用类型
    // （引用类型不写 content，绝不把 chapterRef/entityRef 改写成 inline）。
    // 返回 graphController 的 { success, message }，编辑层据此决定是否退出编辑。
    function commitNodeEdit(nodeId, title, body) {
        var patch = {}
        if (title !== null && title !== undefined)
            patch.title = title
        if (body !== null && body !== undefined) {
            var node = graphController.getNode(nodeId)
            var summary = node && node.content && node.content.summary !== undefined
                    ? node.content.summary : null
            patch.content = {
                type: "inline",
                summary: summary,
                // body 不 trim：空行、段落空白必须原样保存。
                body: body
            }
        }
        return graphController.updateNode(nodeId, patch)
    }

    // 切换画布前由 Canvas 调用：有未提交编辑时先尝试保存。
    // 返回 false 表示保存失败，调用方应中止切换，不能静默丢草稿。
    function flushPendingNodeEdit() {
        return commitActiveNodeEdit()
    }

    // true 只表示"没有会话"或"会话真的保存/结束"；编辑器不可见时不能假成功。
    function commitActiveNodeEdit() {
        if (editingNodeId === "")
            return true
        if (!editOverlay.visible || editOverlay.width <= 0 || editOverlay.height <= 0) {
            logInteraction("node_edit_commit_blocked", "node", editingNodeId, {
                "reason": "overlay_not_ready",
                "gesture": editingGestureId,
                "button": editingGestureButton
            })
            return false
        }
        return editOverlay.commitEdit() === true
    }

    // Issue #996：只让有效编辑会话认领当前动画帧矩形；关闭快照纯作视觉过渡。
    // 参数是 Scene 局部坐标（Canvas 先把指针点映射进来）。
    function editorContainsScenePoint(sceneX, sceneY) {
        if (editingNodeId === "")
            return false
        if (!editOverlay.visible || editOverlay.width <= 0 || editOverlay.height <= 0)
            return false
        var p = editOverlay.mapFromItem(scene, sceneX, sceneY)
        return p.x >= 0 && p.y >= 0
                && p.x <= editOverlay.width && p.y <= editOverlay.height
    }

    // Canvas 用这份 Scene 局部视觉矩形做一次性最小镜头平移；节点世界坐标不变。
    function nodeEditExpandedRect() {
        if (editingNodeId === "" || !editOverlay.visible)
            return null
        var r = editOverlay._expandedRect
        return { x: r.x, y: r.y, width: r.width, height: r.height }
    }

    // Repeater.itemAt() 的静态类型是 QQuickItem（qmllint 会把这些 delegate 属性
    // 报成 missing-property，属静态分析的已知误报），运行时取的是真实 delegate。
    function nodeItemOf(nodeId) {
        for (var i = 0; i < nodeRepeater.count; i++) {
            var it = nodeRepeater.itemAt(i)
            if (it && it.nodeId === nodeId)
                return it
        }
        return null
    }
    function embedItemOf(instanceId) {
        for (var i = 0; i < embedRepeater.count; i++) {
            var it = embedRepeater.itemAt(i)
            if (it && it.instanceId === instanceId)
                return it
        }
        return null
    }

    // 场景级唯一编辑入口（右键菜单"编辑" / Router 双击）。
    // 已有别的节点在编辑时，先提交它；失败则保持原节点，不切到新节点。
    function beginInlineEdit(nodeId, gestureId, button) {
        if (interactionController && interactionController.pointerMode === "pinch")
            return false
        if (editOverlay._closing && editingNodeId === "")
            editOverlay.abortTransition()
        if (editingNodeId === nodeId) {
            editOverlay.focusCurrent()
            return true
        }
        if (editingNodeId !== "" && !commitActiveNodeEdit())
            return false
        if (editOverlay._closing && editingNodeId === "")
            editOverlay.abortTransition()
        var node = graphController.getNode(nodeId)
        if (!node)
            return false
        editingGestureId = gestureId !== undefined ? gestureId : 0
        editingGestureButton = button !== undefined ? button : ""
        editingNodeId = nodeId
        editingNodeSnapshot = node
        editingTitleDraft = node.title || ""
        editingBodyDraft = StarMapNodeMetrics.bodyOf(node)
        editingError = ""
        editingSaving = false
        editOverlay.prepareExpand()
        scene.logInteraction("node_edit_begin", "node", nodeId, {
            "gesture": editingGestureId,
            "button": editingGestureButton
        })
        // 先建立正文真实焦点以接住紧随双击的输入，再只平移一次镜头露出展开卡片。
        editOverlay.activate()
        Qt.callLater(function() {
            if (scene.editingNodeId !== nodeId || !editOverlay.visible)
                return
            if (scene.menuHost && typeof scene.menuHost.ensureNodeEditVisible === "function")
                scene.menuHost.ensureNodeEditVisible()
            Qt.callLater(function() {
                if (scene.editingNodeId === nodeId)
                    editOverlay.startExpand()
            })
        })
        return true
    }

    // ── 子图入口的进入连续性信息（Canvas 穿越子画布时用）──
    // fitScale 是概览按当前父镜头把子图压进圆内接安全区的比例；
    // 它同时就是"进入该子图后的子画布 zoom"，切画布前后画面连续。
    function embedEntryInfo(instanceId) {
        var m = graphController.getEmbed(instanceId)
        if (!m)
            return null
        var d = embedItemOf(instanceId)
        return {
            instanceId: instanceId,
            targetStarmapId: m.targetStarmapId,
            width: m.width,
            height: m.height,
            centerX: m.x + m.width / 2,
            centerY: m.y + m.height / 2,
            bounds: d ? d.childBounds : null,
            fitScale: d ? d.childFitScale : 0
        }
    }

    function embedAtWorld(wx, wy) {
        for (var i = 0; i < graphController.embedsModel.length; i++) {
            var e = graphController.embedsModel[i]
            if (_insideEmbedCircle(e, wx, wy))
                return e
        }
        return null
    }

    // ── 命中测试（active canvas world 坐标）──
    function _embedCircle(e) {
        return { cx: e.x + e.width / 2, cy: e.y + e.height / 2,
                 radius: Math.min(e.width, e.height) / 2 }
    }
    function _insideEmbedCircle(e, wx, wy) {
        var c = _embedCircle(e)
        var dx = wx - c.cx
        var dy = wy - c.cy
        return dx * dx + dy * dy <= c.radius * c.radius
    }
    // chrome 的几何阈值按屏幕口径折算成 world（activity canvas 里圆可以很大）。
    function _embedTitleBandWorld(e) {
        return Math.min(24 / _safeZoom, Math.min(e.width, e.height) * 0.4)
    }
    function _embedBorderRingWorld(e) {
        return Math.min(6 / _safeZoom, Math.min(e.width, e.height) * 0.25)
    }
    function findEmbedChromeAt(wx, wy) {
        for (var i = 0; i < graphController.embedsModel.length; i++) {
            var e = graphController.embedsModel[i]
            if (!_insideEmbedCircle(e, wx, wy))
                continue
            if (wy <= e.y + _embedTitleBandWorld(e))
                return e
            var c = _embedCircle(e)
            var dx = wx - c.cx
            var dy = wy - c.cy
            var inner = c.radius - _embedBorderRingWorld(e)
            if (dx * dx + dy * dy >= inner * inner)
                return e
        }
        return null
    }
    function findEmbedContentAt(wx, wy) {
        for (var i = 0; i < graphController.embedsModel.length; i++) {
            var e = graphController.embedsModel[i]
            if (!_insideEmbedCircle(e, wx, wy))
                continue
            if (wy <= e.y + _embedTitleBandWorld(e))
                continue
            var c = _embedCircle(e)
            var dx = wx - c.cx
            var dy = wy - c.cy
            var inner = c.radius - _embedBorderRingWorld(e)
            if (dx * dx + dy * dy >= inner * inner)
                continue
            return e
        }
        return null
    }

    // 命中顺序：节点 → 子星图 chrome → 子星图内容 → 连线 → 空白。
    // 子星图内容区也是入口命中的一部分：整个圆就是"进入子星图"的入口。
    function hitTargetAtWorld(wx, wy) {
        if (starmapId === "")
            return null
        var node = graphController.findNodeAt(wx, wy)
        if (node) {
            return { owner: scene, scenePathKey: scenePathKey, starmapId: starmapId,
                     kind: "node", id: node.id, targetPath: nodePath(node.id),
                     localX: wx, localY: wy }
        }
        var chrome = findEmbedChromeAt(wx, wy)
        if (chrome) {
            return { owner: scene, scenePathKey: scenePathKey, starmapId: starmapId,
                     kind: "embed", id: chrome.instanceId,
                     targetPath: embedPath(chrome.instanceId),
                     localX: wx, localY: wy }
        }
        var inside = findEmbedContentAt(wx, wy)
        if (inside) {
            return { owner: scene, scenePathKey: scenePathKey, starmapId: starmapId,
                     kind: "embed", id: inside.instanceId,
                     targetPath: embedPath(inside.instanceId),
                     localX: wx, localY: wy }
        }
        var edge = graphController.hitTestEdge(wx, wy, _edgeHitThresholdWorld())
        if (edge) {
            return { owner: scene, scenePathKey: scenePathKey, starmapId: starmapId,
                     kind: "edge", id: edge.id, targetPath: null,
                     localX: wx, localY: wy }
        }
        return { owner: scene, scenePathKey: scenePathKey, starmapId: starmapId,
                 kind: "empty", id: "", targetPath: null,
                 localX: wx, localY: wy }
    }

    // ── 跨层关系规划（Core 唯一真相）──
    function planCrossLayerConnection(fromPath, toPath) {
        if (!starmapBackendRef || !fromPath || !toPath)
            return null
        var res = starmapBackendRef.plan_cross_layer_connection(
                    JSON.stringify(fromPath), JSON.stringify(toPath))
        if (!res || res.success !== true || !res.data)
            return null
        return res.data
    }
    // 独立画布模型下，能在 UI 里连线的两端都必须在当前活动画布内，
    // 因此宿主必须就是本画布（同一目标图的不同入口 route 不同，不算同一宿主）。
    function _planHostIsCurrent(plan) {
        if (!plan)
            return false
        if (plan.hostStarmapId !== starmapId)
            return false
        return _sameSegments(plan.hostSegments || [], pathSegments || [])
    }

    function commitEdgeWithPaths(fromPath, toPath) {
        return graphController.createEdgeWithPaths(fromPath, toPath)
    }
    function commitLinkWithPaths(fromPath, toPath) {
        return graphController.createLinkWithPaths(fromPath, toPath)
    }

    // 候选边预览：宿主就是本画布，直接把 Core 规划出的两端交给本层 edge renderer。
    function prospectiveEdgeRenderForPlan(plan) {
        if (!plan || !graphController)
            return null
        return graphController.computeProspectiveEdgeRender(plan.from, plan.to)
    }

    // connect 预览线与正式边共用同一套几何真相（Core edge render）。
    function refreshConnectPreview() {
        var ic = interactionController
        if (!ic || ic.pointerMode !== "connect" || ic.connectFromKind === "")
            return
        ic.connectPreviewEndX = ic.connectMouseX
        ic.connectPreviewEndY = ic.connectMouseY

        var hit = hitTargetAtWorld(ic.connectMouseX, ic.connectMouseY)
        if (hit && (hit.kind === "node" || hit.kind === "embed")) {
            var plan = planCrossLayerConnection(ic.connectFromPath, hit.targetPath)
            if (_planHostIsCurrent(plan)) {
                var preview = prospectiveEdgeRenderForPlan(plan)
                if (preview) {
                    ic.connectFromSceneX = preview.startX
                    ic.connectFromSceneY = preview.startY
                    ic.connectPreviewEndX = preview.endX
                    ic.connectPreviewEndY = preview.endY
                    return
                }
            }
        }

        // 退回：源对象边界朝当前鼠标方向（本层 world 坐标）。
        var item = ic.connectFromKind === "node"
                ? graphController.getNode(ic.connectFromId)
                : graphController.getEmbed(ic.connectFromId)
        if (!item)
            return
        var boundary = boundaryPointWorld(item.x, item.y, item.width, item.height,
                                          ic.connectFromKind === "embed",
                                          ic.connectMouseX, ic.connectMouseY)
        ic.connectFromSceneX = boundary.x
        ic.connectFromSceneY = boundary.y
    }

    // 源形状朝目标点的边界交点（world 坐标）。与 Rust edge_render 的
    // line_rect_entry / line_circle_entry 同一套语义。
    function boundaryPointWorld(x, y, width, height, isEmbed, tx, ty) {
        var cx = x + width / 2
        var cy = y + height / 2
        var dx = tx - cx
        var dy = ty - cy
        var len = Math.sqrt(dx * dx + dy * dy)
        if (len < 1e-6 || !(width > 0) || !(height > 0))
            return { x: cx, y: cy }
        var ux = dx / len
        var uy = dy / len
        if (isEmbed) {
            var radius = Math.min(width, height) / 2
            return { x: cx + ux * radius, y: cy + uy * radius }
        }
        var txMin = -Infinity
        var txMax = Infinity
        if (Math.abs(ux) > 1e-6) {
            var ax = (x - cx) / ux
            var bx = (x + width - cx) / ux
            txMin = Math.min(ax, bx)
            txMax = Math.max(ax, bx)
        }
        var tyMin = -Infinity
        var tyMax = Infinity
        if (Math.abs(uy) > 1e-6) {
            var ay = (y - cy) / uy
            var by = (y + height - cy) / uy
            tyMin = Math.min(ay, by)
            tyMax = Math.max(ay, by)
        }
        var tMin = Math.max(txMin, tyMin)
        var tMax = Math.min(txMax, tyMax)
        if (tMax < Math.max(tMin, 0))
            return { x: cx, y: cy }
        var t = tMin >= 0 ? tMin : tMax
        return { x: cx + t * ux, y: cy + t * uy }
    }

    // ── Router API：唯一输入主人按 hit.owner 直接调用这里 ──
    function applyMoveDelta(dxQtScene, dyQtScene) {
        var ic = interactionController
        if (!ic || ic.pointerMode !== "move" || ic.moveScenePathKey !== scenePathKey)
            return false
        var delta = qtSceneDeltaToWorld(dxQtScene, dyQtScene)
        ic.updateMove(ic.moveX + delta.x, ic.moveY + delta.y)
        return true
    }

    function applyConnectDelta(dxQtScene, dyQtScene) {
        var ic = interactionController
        if (!ic || ic.connectFromScenePathKey !== scenePathKey)
            return false
        if (ic.pointerMode !== "connect" && ic.pointerMode !== "contextPending")
            return false
        var delta = qtSceneDeltaToWorld(dxQtScene, dyQtScene)
        if (ic.pointerMode === "connect") {
            ic.updateConnect(ic.connectMouseX + delta.x, ic.connectMouseY + delta.y)
            refreshConnectPreview()
        } else {
            ic.connectMouseX += delta.x
            ic.connectMouseY += delta.y
        }
        return true
    }

    function finishMove() {
        var ic = interactionController
        var kind = ic.moveKind
        var id = ic.moveId
        var nx = ic.moveX
        var ny = ic.moveY
        var committed = false
        if (kind === "node" && id !== "") {
            committed = graphController.commitNodeMove(id, nx, ny)
            logInteraction("move_end", "node", id, {
                "toX": nx, "toY": ny, "commitSuccess": committed
            })
        } else if (kind === "embed" && id !== "") {
            committed = graphController.commitEmbedMove(id, nx, ny)
            logInteraction("move_end", "embed", id, {
                "toX": nx, "toY": ny, "commitSuccess": committed
            })
        }
        ic.endMove()
        refreshEdges()
    }

    function finishConnect() {
        var ic = interactionController
        var fromKind = ic.connectFromKind
        var fromId = ic.connectFromId
        var fromPath = ic.connectFromPath
        var hit = hitTargetAtWorld(ic.connectMouseX, ic.connectMouseY)
        var toPath = null
        var success = false
        var cancelled = true
        if (hit && (hit.kind === "node" || hit.kind === "embed")) {
            var sameTarget = hit.id === fromId && hit.kind === fromKind
            if (!sameTarget) {
                toPath = hit.targetPath
                var plan = planCrossLayerConnection(fromPath, toPath)
                if (_planHostIsCurrent(plan)) {
                    success = commitEdgeWithPaths(plan.from, plan.to)
                    cancelled = !success
                }
            }
        }
        logInteraction("connect_end", fromKind, fromId, {
            "fromPath": JSON.stringify(fromPath),
            "toPath": toPath ? JSON.stringify(toPath) : "",
            "hostPathKey": scenePathKey,
            "hostStarmapId": starmapId,
            "success": success,
            "cancel": cancelled
        })
        ic.endConnect()
        if (menuHost)
            menuHost.hideTouchPreview()
    }

    // linkArmed 下一次 tap 命中 node/embed → 建内部链接或重新指定。
    // 独立画布下两端都在当前画布，宿主就是本画布；"重新指定"只需更新本画布
    // 的 Link target（不存在跨层宿主迁移路径）。
    function finishLink(toHit) {
        var ic = interactionController
        var fromKind = ic.linkFromKind
        var fromId = ic.linkFromId
        var fromPath = ic.linkFromPath
        var toPath = null
        var success = false
        var cancelled = true
        var action = "create"
        if (toHit && (toHit.kind === "node" || toHit.kind === "embed")) {
            var sameTarget = toHit.id === fromId && toHit.kind === fromKind
            if (!sameTarget) {
                toPath = toHit.targetPath
                var plan = planCrossLayerConnection(fromPath, toPath)
                if (_planHostIsCurrent(plan)) {
                    if (ic.linkExistingId !== "") {
                        action = "update"
                        success = updateLink(ic.linkExistingId, {
                            target: plan.to,
                            clearLabel: false
                        })
                        cancelled = !success
                    } else {
                        success = commitLinkWithPaths(plan.from, plan.to)
                        cancelled = !success
                    }
                }
            }
        }
        logInteraction("link_end", fromKind, fromId, {
            "fromPath": JSON.stringify(fromPath),
            "toPath": toPath ? JSON.stringify(toPath) : "",
            "hostPathKey": scenePathKey,
            "hostStarmapId": starmapId,
            "action": action,
            "success": success,
            "cancel": cancelled
        })
        ic.cancelArmed()
        if (menuHost)
            menuHost.hideTouchPreview()
    }

    // 触屏长按后没移动就松手：关视觉层，弹出真正可点击的菜单。
    function finishContextPending() {
        var ic = interactionController
        var kind = ic.connectFromKind
        var id = ic.connectFromId
        ic.endContextPending()
        var center = itemCenterScene(kind, id)
        if (!center)
            return
        if (menuHost)
            menuHost.showLongPressMenu(kind, id, center.x, center.y, scene)
    }

    // ── 内部链接菜单（StarMapLink）──
    function listLinks() {
        return graphController.listLinks()
    }
    function deleteLink(linkId) {
        return graphController.deleteLink(linkId)
    }
    function updateLink(linkId, patch) {
        return graphController.updateLink(linkId, patch)
    }
    function addLinkReturningDto(sourcePath, targetPath, label) {
        return graphController.addLink(sourcePath, targetPath, label)
    }
    // 宿主感知的 Link 列表：本画布的 Link source 是 host-relative（segments 相对
    // starmapId），转成从根出发的绝对路径再与调用方的 sourcePath 比较。
    function listLinksForSource(sourcePath) {
        var out = []
        var items = graphController.listLinks()
        for (var i = 0; i < items.length; i++) {
            var absSource = {
                starmapId: rootStarmapId,
                segments: (pathSegments || []).concat(items[i].source.segments || []),
                target: items[i].source.target
            }
            if (StarMapPathPlanner.isSameTargetPath(absSource, sourcePath))
                out.push({ link: items[i], hostOwner: scene })
        }
        return out
    }

    // ── 空白右键新建（无限画布：不做安全区 clamp）──
    function findFreeSpawnPoint(wx, wy) {
        var step = 24
        var candidates = [{ x: wx, y: wy }]
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
            // Issue #984：新建节点也用统一卡片度量，不写死旧尺寸。
            if (!overlapsExistingNode(candidates[i].x, candidates[i].y,
                                      StarMapNodeMetrics.defaultNodeWidth(),
                                      StarMapNodeMetrics.defaultNodeHeight(), 12))
                return candidates[i]
        }
        return { x: wx, y: wy }
    }

    function overlapsExistingNode(x, y, w, h, padding) {
        var nodes = graphController.nodesModel
        for (var i = 0; i < nodes.count; i++) {
            var n = nodes.get(i)
            var nx = n.x - padding
            var ny = n.y - padding
            var nw = n.width + padding * 2
            var nh = n.height + padding * 2
            if (x < nx + nw && x + w > nx && y < ny + nh && y + h > ny)
                return true
        }
        return false
    }

    function createNodeWithName(name, sceneX, sceneY) {
        var spawn = findFreeSpawnPoint(sceneX, sceneY)
        graphController.createNode(name, spawn.x, spawn.y)
    }
    function createSubStarmapWithName(name, sceneX, sceneY) {
        var spawn = findFreeSpawnPoint(sceneX, sceneY)
        graphController.createSubStarmapAt(name, spawn.x, spawn.y)
    }

    // ── 边重绘：本画布 edgeRenders 只画本画布连线；瞬时 move 用共享状态机 override ──
    function currentMoveOverride() {
        if (!interactionController)
            return null
        var ic = interactionController
        if (ic.pointerMode !== "move" || ic.moveScenePathKey !== scenePathKey)
            return null
        if (ic.moveKind !== "" && ic.moveId !== "")
            return { kind: ic.moveKind, id: ic.moveId, x: ic.moveX, y: ic.moveY }
        return null
    }

    function refreshEdges() {
        graphController.computeEdgeRenders(currentMoveOverride())
    }

    function refreshAllEdges() {
        graphController.computeEdgeRenders(null)
    }

    Connections {
        target: scene.interactionController
        function onMoveXChanged() { if (scene.currentMoveOverride()) scene.refreshEdges() }
        function onMoveYChanged() { if (scene.currentMoveOverride()) scene.refreshEdges() }
        function onPointerModeChanged() { scene.refreshAllEdges() }
    }

    function isMovingNode(id) {
        return interactionController
                ? interactionController.isMovingTarget(scenePathKey, "node", id) : false
    }
    function isMovingEmbed(instanceId) {
        return interactionController
                ? interactionController.isMovingTarget(scenePathKey, "embed", instanceId) : false
    }
    function edgeLabelOf(edgeId) {
        for (var i = 0; i < graphController.edgesModel.length; i++) {
            if (graphController.edgesModel[i].id === edgeId)
                return graphController.edgesModel[i].label || ""
        }
        return ""
    }

    // =========================================================================
    // 视觉层：全部按当前投影实时绘制（无 layer / 无预栅格化纹理）
    // =========================================================================

    // ── 连线：QtQuick.Shapes，按世界几何的当前投影画路径 ──
    Repeater {
        id: edgeRepeater
        model: graphController.edgeRenders

        delegate: Item {
            id: edgeItem
            required property var modelData

            readonly property var render: modelData
            readonly property bool isSelected: scene.selectionController
                    ? scene.selectionController.matches(scene.scenePathKey, "edge", render.edgeId)
                    : false
            readonly property color strokeColor: isSelected ? scene._accent : scene._border
            readonly property string label: scene.edgeLabelOf(render.edgeId)

            anchors.fill: parent
            z: 0

            Shape {
                anchors.fill: parent
                antialiasing: true
                preferredRendererType: Shape.CurveRenderer
                // Issue #949：连线跟随同一个展开进度逐帧显现（几何不变）。
                opacity: scene._reveal

                // 线段
                ShapePath {
                    strokeColor: edgeItem.strokeColor
                    strokeWidth: scene._edgeScreenStrokeWidth
                    fillColor: "transparent"
                    capStyle: ShapePath.RoundCap
                    PathPolyline {
                        path: [
                            Qt.point(scene.worldToScreenX(edgeItem.render.startX),
                                     scene.worldToScreenY(edgeItem.render.startY)),
                            Qt.point(scene.worldToScreenX(edgeItem.render.endX),
                                     scene.worldToScreenY(edgeItem.render.endY))
                        ]
                    }
                }

                // 箭头
                ShapePath {
                    strokeColor: "transparent"
                    fillColor: edgeItem.strokeColor
                    PathPolyline {
                        path: [
                            Qt.point(scene.worldToScreenX(edgeItem.render.arrowTipX),
                                     scene.worldToScreenY(edgeItem.render.arrowTipY)),
                            Qt.point(scene.worldToScreenX(edgeItem.render.arrowLeftX),
                                     scene.worldToScreenY(edgeItem.render.arrowLeftY)),
                            Qt.point(scene.worldToScreenX(edgeItem.render.arrowRightX),
                                     scene.worldToScreenY(edgeItem.render.arrowRightY)),
                            Qt.point(scene.worldToScreenX(edgeItem.render.arrowTipX),
                                     scene.worldToScreenY(edgeItem.render.arrowTipY))
                        ]
                    }
                }
            }

            // 连线标签：实时 Text，字号同活动画布的屏幕口径。
            Rectangle {
                id: edgeLabelBg
                x: scene.worldToScreenX(edgeItem.render.labelX) - width / 2
                y: scene.worldToScreenY(edgeItem.render.labelY) - height / 2
                width: edgeLabelText.implicitWidth + 10
                height: edgeLabelText.implicitHeight + 4
                radius: 3 * scene._safeZoom
                color: scene._surfaceContainer
                visible: edgeItem.label.length > 0 && scene._safeZoom >= 0.4

                Text {
                    id: edgeLabelText
                    anchors.centerIn: parent
                    text: edgeItem.label
                    color: scene._textPrimary
                    font.pointSize: scene.dt.fontSmPt * scene._textScale
                }
            }
        }
    }

    // ── 节点：矢量圆角矩形卡片（编辑输入在场景级稳定浮层，见 editOverlay）──
    Repeater {
        id: nodeRepeater
        model: graphController.nodesModel
        // 模型行重建也可能触发 itemRemoved；只有节点确实从图里消失时才结束
        // 会话，不能让一次模型刷新清掉仍有效的草稿与编辑器。
        onItemRemoved: function(index, item) {
            scene.ensureEditingNodeExists()
        }

        delegate: Item {
            id: nodeItem
            required property var modelData
            required property int index

            readonly property var nodeData: modelData
            readonly property string nodeId: nodeData.id
            property real _wobbleAnimX: 0
            property real _wobbleAnimY: 0

            readonly property var closingSnapshot:
                editOverlay._closingSnapshot
                && editOverlay._closingSnapshot.nodeId === nodeData.id
                    ? editOverlay._closingSnapshot : null

            readonly property bool isSelected: scene.selectionController
                    ? scene.selectionController.matches(scene.scenePathKey, "node", nodeData.id)
                    : false
            readonly property bool isPressed: scene.interactionController
                    ? scene.interactionController.isPressedTarget(scene.scenePathKey, "node", nodeData.id)
                    : false
            readonly property bool isMoving: scene.isMovingNode(nodeData.id)
            readonly property bool isEditing: scene.editingNodeId === nodeData.id
                    || closingSnapshot !== null
            readonly property real worldX: closingSnapshot
                    ? closingSnapshot.worldX
                    : isMoving && scene.interactionController
                    ? scene.interactionController.moveX : nodeData.x
            readonly property real worldY: closingSnapshot
                    ? closingSnapshot.worldY
                    : isMoving && scene.interactionController
                    ? scene.interactionController.moveY : nodeData.y

            // Issue #984：宽高来自图级布局快照（由平台文本排版一次性测量），
            // delegate 不再实测 / 回写 / 维护编辑草稿。
            readonly property real cardWidth: closingSnapshot
                    ? closingSnapshot.worldWidth
                    : nodeData.width !== undefined
                        ? nodeData.width : StarMapNodeMetrics.defaultNodeWidth()
            readonly property real cardHeight: closingSnapshot
                    ? closingSnapshot.worldHeight
                    : nodeData.height !== undefined
                        ? nodeData.height : StarMapNodeMetrics.defaultNodeHeight()
            readonly property string bodyText: closingSnapshot
                    ? closingSnapshot.body : StarMapNodeMetrics.bodyOf(nodeData)
            readonly property string titleText: closingSnapshot
                    ? (closingSnapshot.title.length > 0
                       ? closingSnapshot.title : qsTr("未命名"))
                    : nodeData.title && nodeData.title.length > 0
                        ? nodeData.title : qsTr("未命名")

            x: scene.worldToScreenX(worldX)
               + (cardWidth * scene._safeZoom - width) / 2
            y: scene.worldToScreenY(worldY)
               + (cardHeight * scene._safeZoom - height) / 2
            width: cardWidth * scene._safeZoom * scene._reveal
                   + scene._revealDotRadius * 2 * (1 - scene._reveal)
            height: cardHeight * scene._safeZoom * scene._reveal
                    + scene._revealDotRadius * 2 * (1 - scene._reveal)
            z: 1

            Rectangle {
                id: visualNode
                anchors.fill: parent
                opacity: nodeItem.isEditing ? 1 - editOverlay.transitionProgress : 1
                radius: Math.min(scene.dt.radiusSm * scene._safeZoom,
                                 Math.min(width, height) / 2)
                color: scene._surfaceContainer
                border.color: nodeItem.isSelected || nodeItem.isEditing
                        ? scene._accent : scene._border
                border.width: (nodeItem.isSelected || nodeItem.isEditing) ? 2 : 1
                clip: true

                transform: Translate {
                    x: (nodeItem.isSelected || nodeItem.isPressed || nodeItem.isEditing)
                       ? 0 : nodeItem._wobbleAnimX
                    y: (nodeItem.isSelected || nodeItem.isPressed || nodeItem.isEditing)
                       ? 0 : nodeItem._wobbleAnimY
                }

                // 显示态：名称在上，正文预览在下。编辑输入层是场景级稳定容器，
                // 不挂在这里，模型行更新不会销毁正在输入的内容。
                Item {
                    id: displayLayer
                    anchors.fill: parent
                    anchors.margins: StarMapNodeMetrics.PADDING * scene._safeZoom
                    visible: nodeItem.height >= 18 && scene.detailReveal > 0.6

                    Column {
                        width: parent.width
                        spacing: nodeItem.bodyText.length > 0
                                 ? StarMapNodeMetrics.TITLE_BODY_GAP * scene._textScale : 0

                        Text {
                            id: titleLabel
                            width: parent.width
                            text: nodeItem.titleText
                            textFormat: Text.PlainText
                            color: scene._textPrimary
                            font.pixelSize: Math.max(1,
                                StarMapNodeMetrics.TITLE_FONT_SIZE * scene._textScale)
                            font.weight: Font.DemiBold
                            wrapMode: Text.Wrap
                            maximumLineCount: StarMapNodeMetrics.TITLE_MAX_LINES
                            elide: Text.ElideRight
                            lineHeightMode: Text.FixedHeight
                            lineHeight: StarMapNodeMetrics.TITLE_LINE_HEIGHT * scene._textScale
                        }

                        Text {
                            id: bodyPreview
                            width: parent.width
                            text: nodeItem.bodyText
                            textFormat: Text.PlainText
                            visible: text.length > 0
                            color: scene._textMuted
                            font.pixelSize: Math.max(1,
                                StarMapNodeMetrics.BODY_FONT_SIZE * scene._textScale)
                            wrapMode: Text.Wrap
                            maximumLineCount: StarMapNodeMetrics.PREVIEW_MAX_LINES
                            elide: Text.ElideRight
                            lineHeightMode: Text.FixedHeight
                            lineHeight: StarMapNodeMetrics.BODY_LINE_HEIGHT * scene._textScale
                        }
                    }
                }
            }

            SequentialAnimation on _wobbleAnimX {
                loops: Animation.Infinite
                running: !nodeItem.isSelected && !nodeItem.isPressed
                NumberAnimation { to: 0.6; duration: 7000 + (nodeItem.index % 7) * 400; easing.type: Easing.InOutSine }
                NumberAnimation { to: -0.6; duration: 7000 + (nodeItem.index % 7) * 400; easing.type: Easing.InOutSine }
            }
            SequentialAnimation on _wobbleAnimY {
                loops: Animation.Infinite
                running: !nodeItem.isSelected && !nodeItem.isPressed
                NumberAnimation { to: 0.4; duration: 8500 + (nodeItem.index % 5) * 300; easing.type: Easing.InOutSine }
                NumberAnimation { to: -0.4; duration: 8500 + (nodeItem.index % 5) * 300; easing.type: Easing.InOutSine }
            }
        }
    }

    // ── 子星图入口：矢量正圆 + 圆内微缩概览（不递归可交互子场景）──
    Repeater {
        id: embedRepeater
        model: graphController.embedsModel

        delegate: StarMapEmbed {
            id: embedItem
            required property var modelData
            required property int index

            readonly property var embedData: modelData
            readonly property bool isMoving: scene.isMovingEmbed(embedData.instanceId)
            readonly property real worldX: isMoving && scene.interactionController
                    ? scene.interactionController.moveX : embedData.x
            readonly property real worldY: isMoving && scene.interactionController
                    ? scene.interactionController.moveY : embedData.y

            dt: scene.dt
            parentScene: scene
            instanceId: embedData.instanceId
            targetStarmapId: embedData.targetStarmapId
            label: embedData.label
            wobbleIndex: index
            isSelected: scene.selectionController
                    ? scene.selectionController.matches(scene.scenePathKey, "embed", embedData.instanceId)
                    : false
            isPressed: scene.interactionController
                    ? scene.interactionController.isPressedTarget(scene.scenePathKey, "embed", embedData.instanceId)
                    : false
            starmapBackendRef: scene.starmapBackendRef

            x: scene.worldToScreenX(worldX) + (embedData.width * scene._safeZoom - width) / 2
            y: scene.worldToScreenY(worldY) + (embedData.height * scene._safeZoom - height) / 2
            width: embedData.width * scene._safeZoom * scene._reveal
                   + scene._revealDotRadius * 2 * (1 - scene._reveal)
            height: embedData.height * scene._safeZoom * scene._reveal
                    + scene._revealDotRadius * 2 * (1 - scene._reveal)
            z: 2
            // Issue #949：只有走过渡的那个入口按同一个展开进度继续画圆壳。
            revealProgress: embedData.instanceId === scene.transitionEmbedId
                    ? scene.detailReveal : -1
        }
    }

    // ── Issue #949：过渡圆环（同一圆壳的放大描边 + 圆内表面）──
    // 只影响显示：圆心 / 半径由同一个 progress 现算 —— 进场是入口圆壳向
    // 视口中心放大到整圈出视口，出场是当前帧入口圆投影收向目标父帧的 Embed
    // 圆壳投影；圆内这段画面就是本层真实矢量内容（不是把圆点预览截图放大）。
    // 画在本层内容之下：圆外留给另一层（进场 = 旧父图，出场 = 新父图）。
    Rectangle {
        id: transitionRing
        visible: scene.ringVisible
        x: scene.ringCenterX - scene.ringRadius
        y: scene.ringCenterY - scene.ringRadius
        width: Math.max(0, scene.ringRadius) * 2
        height: Math.max(0, scene.ringRadius) * 2
        radius: width / 2
        color: scene.transitionRingColor
        border.color: scene.dt.inverseSurface
        border.width: scene.ringStrokeWidth
        antialiasing: true
        z: -1
    }

    // ── 场景级唯一节点编辑器（不挂在 Repeater delegate 内）──
    // 模型行更新 / 文本重测 / 保存成功都不会重建输入层。编辑卡以原节点投影
    // 矩形为锚点、保持中心展开；镜头负责露出卡片，不改 authored position。
    Rectangle {
        id: editOverlay

        // 优先读当前模型；模型刚刷新时的瞬时 null 由打开会话时的快照兜底，
        // 保证有效会话不会因为一次查询失败就静默隐藏。
        readonly property var editingNode: {
            if (scene.editingNodeId === "")
                return null
            return scene.graphController.getNode(scene.editingNodeId)
                    || scene.editingNodeSnapshot
        }
        readonly property real _margin: 12
        readonly property var _nodeRect: {
            var closingNode = !editingNode ? _closingSnapshot : null
            if (!editingNode && !closingNode)
                return { x: 0, y: 0, width: 0, height: 0 }
            var nodeId = closingNode ? closingNode.nodeId : editingNode.id
            var item = scene.nodeItemOf(nodeId)
            if (item) {
                return { x: item.x, y: item.y, width: item.width, height: item.height }
            }
            var worldX = closingNode ? closingNode.worldX : editingNode.x
            var worldY = closingNode ? closingNode.worldY : editingNode.y
            var nodeWidth = closingNode ? closingNode.worldWidth
                    : editingNode.width !== undefined
                        ? editingNode.width : StarMapNodeMetrics.defaultNodeWidth()
            var nodeHeight = closingNode ? closingNode.worldHeight
                    : editingNode.height !== undefined
                        ? editingNode.height : StarMapNodeMetrics.defaultNodeHeight()
            var projectedWidth = nodeWidth * scene._safeZoom * scene._reveal
                    + scene._revealDotRadius * 2 * (1 - scene._reveal)
            var projectedHeight = nodeHeight * scene._safeZoom * scene._reveal
                    + scene._revealDotRadius * 2 * (1 - scene._reveal)
            return {
                x: scene.worldToScreenX(worldX)
                   + (nodeWidth * scene._safeZoom - projectedWidth) / 2,
                y: scene.worldToScreenY(worldY)
                   + (nodeHeight * scene._safeZoom - projectedHeight) / 2,
                width: projectedWidth,
                height: projectedHeight
            }
        }
        readonly property var _expandedRect: {
            var r = _nodeRect
            var vw = Math.max(0, scene.width)
            var vh = Math.max(0, scene.height)
            var m = _margin
            var availableW = Math.max(0, vw - m * 2)
            var availableH = Math.max(0, vh - m * 2)
            var w = Math.min(Math.max(0, r.width, 260), availableW)
            var h = Math.min(Math.max(r.height, 220),
                             Math.min(StarMapNodeMetrics.EDIT_MAX_HEIGHT, availableH))
            var x = r.x + (r.width - w) / 2
            var y = r.y + (r.height - h) / 2
            return { x: x, y: y, width: w, height: h }
        }
        property real transitionProgress: 0
        property bool _transitionRunning: false
        property bool _closing: false
        property int _transitionGeneration: 0
        property int _expandGeneration: 0
        property int _collapseGeneration: 0
        property var _closingSnapshot: null
        readonly property bool _inputReady: scene.hasActiveNodeEdit && !_closing
        readonly property bool _inputVisible: visible
                && scene.hasActiveNodeEdit && !_closing && !_transitionRunning
                && transitionProgress >= 1
        readonly property var _presentedRect: {
            var p = Math.max(0, Math.min(1, transitionProgress))
            return interpolateRect(_nodeRect, _expandedRect, p)
        }
        // 视觉父级在 Canvas 顶层时显式转换当前动画帧的 Scene 坐标。
        readonly property var _presentedHostPos: {
            var a = _presentedRect
            if (!parent || parent === scene)
                return Qt.point(a.x, a.y)
            var p = scene.mapToItem(parent, a.x, a.y)
            return Qt.point(p.x, p.y)
        }

        function interpolateRect(from, to, progress) {
            var p = Math.max(0, Math.min(1, progress))
            return {
                x: from.x + (to.x - from.x) * p,
                y: from.y + (to.y - from.y) * p,
                width: from.width + (to.width - from.width) * p,
                height: from.height + (to.height - from.height) * p
            }
        }

        function presentationSnapshot(node) {
            return {
                nodeId: node.id,
                worldX: node.x,
                worldY: node.y,
                worldWidth: node.width !== undefined
                        ? node.width : StarMapNodeMetrics.defaultNodeWidth(),
                worldHeight: node.height !== undefined
                        ? node.height : StarMapNodeMetrics.defaultNodeHeight(),
                title: node.title || "",
                body: StarMapNodeMetrics.bodyOf(node)
            }
        }

        function prepareExpand() {
            _transitionGeneration += 1
            expandAnimation.stop()
            collapseAnimation.stop()
            transitionProgress = 0
            _transitionRunning = true
            _closing = false
            _closingSnapshot = null
        }

        function startExpand() {
            if (scene.editingNodeId === "" || _closing)
                return false
            if (!_transitionRunning)
                prepareExpand()
            transitionProgress = 0
            _expandGeneration = _transitionGeneration
            expandAnimation.start()
            return true
        }

        function finishCollapse() {
            transitionProgress = 0
            _transitionRunning = false
            _closing = false
            _closingSnapshot = null
        }

        function abortTransition() {
            _transitionGeneration += 1
            expandAnimation.stop()
            collapseAnimation.stop()
            finishCollapse()
        }

        function closeEdit(committed) {
            if (scene.editingNodeId === "")
                return
            var nodeId = scene.editingNodeId
            var currentNode = scene.graphController.getNode(nodeId)
            if (!currentNode) {
                scene.finishActiveNodeEdit()
                return
            }
            var displayNode = committed ? currentNode
                                        : (scene.editingNodeSnapshot || currentNode)
            expandAnimation.stop()
            _transitionGeneration += 1
            _collapseGeneration = _transitionGeneration
            _closingSnapshot = presentationSnapshot(displayNode)
            _closing = true
            _transitionRunning = true
            titleInput.focus = false
            bodyEdit.focus = false
            scene.finishActiveNodeEdit(true)
            if (transitionProgress <= 0) {
                finishCollapse()
                return
            }
            collapseAnimation.duration = Math.max(70, Math.round(180 * transitionProgress))
            collapseAnimation.start()
        }

        NumberAnimation {
            id: expandAnimation
            target: editOverlay
            property: "transitionProgress"
            to: 1
            duration: 190
            easing.type: Easing.OutCubic
            onFinished: {
                if (scene.editingNodeId === "" || editOverlay._closing
                        || editOverlay._expandGeneration
                            !== editOverlay._transitionGeneration)
                    return
                editOverlay.transitionProgress = 1
                editOverlay._transitionRunning = false
            }
        }
        NumberAnimation {
            id: collapseAnimation
            target: editOverlay
            property: "transitionProgress"
            to: 0
            duration: 180
            easing.type: Easing.InCubic
            onFinished: {
                if (editOverlay._closing
                        && editOverlay._collapseGeneration
                            === editOverlay._transitionGeneration)
                    editOverlay.finishCollapse()
            }
        }

        // _presentedRect 是 Scene 局部坐标；视觉父级在 Canvas 顶层时显式换算。
        // 会话或关闭快照保持可见；文本控件从会话开始接收输入，辅助控件在展开后显示。

        // Issue #987：编辑器视觉父级在 Router 之上；对象所有权仍属于本 Scene，
        // 切图/销毁时随 Scene 一并回收，不保留透明点击层或第二份编辑器。
        parent: scene.editOverlayHost ? scene.editOverlayHost : scene
        // 节点快照保证有效会话与收起展示都有稳定几何；节点真正删除时结束会话。
        visible: scene.hasActiveNodeEdit || _closing
        x: _presentedHostPos.x
        y: _presentedHostPos.y
        width: _presentedRect.width
        height: _presentedRect.height
        opacity: transitionProgress
        enabled: _inputReady
        radius: 10
        color: scene._surfaceContainer
        border.color: scene._accent
        border.width: 2
        clip: true
        z: 30
        property bool _bodyInputReported: false
        property bool _titleInputReported: false

        function activationFailed(reason) {
            scene.logInteraction("node_edit_activation_failed", "node", scene.editingNodeId, {
                "reason": reason,
                "visible": visible,
                "width": width,
                "height": height,
                "gesture": scene.editingGestureId,
                "button": scene.editingGestureButton
            })
            return false
        }

        function activate() {
            if (scene.editingNodeId === "")
                return activationFailed("no_session")
            if (!_inputReady || width <= 0 || height <= 0)
                return activationFailed("overlay_not_ready")
            titleInput.text = scene.editingTitleDraft
            bodyEdit.text = scene.editingBodyDraft
            _titleInputReported = false
            _bodyInputReported = false
            bodyFlick.contentY = 0
            var bodyEditable = StarMapNodeMetrics.canEditBody(editingNode)
            if (bodyEditable) {
                bodyEdit.cursorPosition = bodyEdit.length
                bodyEdit.forceActiveFocus()
            } else {
                titleInput.forceActiveFocus()
                titleInput.selectAll()
            }
            var titleFocus = titleInput.activeFocus
            var bodyFocus = bodyEdit.activeFocus
            if (bodyEditable ? !bodyFocus : !titleFocus)
                return activationFailed("focus_not_granted")
            scene.logInteraction("node_edit_ready", "node", scene.editingNodeId, {
                "visible": visible,
                "titleFocus": titleFocus,
                "bodyFocus": bodyFocus,
                "focusTarget": bodyFocus ? "body" : "title",
                "overlayRect": {
                    "x": x, "y": y, "width": width, "height": height
                },
                "gesture": scene.editingGestureId,
                "button": scene.editingGestureButton
            })
            return true
        }

        function focusCurrent() {
            if (!_inputReady)
                return
            if (StarMapNodeMetrics.canEditBody(editingNode))
                bodyEdit.forceActiveFocus()
            else
                titleInput.forceActiveFocus()
        }

        function commitEdit() {
            if (scene.editingNodeId === "")
                return true
            if (_closing)
                return false
            // 有会话但编辑器不可见/无尺寸：不能当作保存成功，草稿必须保留。
            if (!visible || width <= 0 || height <= 0) {
                scene.logInteraction("node_edit_commit_blocked", "node", scene.editingNodeId, {
                    "reason": "overlay_not_ready",
                    "visible": visible,
                    "gesture": scene.editingGestureId,
                    "button": scene.editingGestureButton
                })
                return false
            }
            if (scene.editingSaving)
                return false
            // IME 预编辑期间不提交，把 Enter 留给候选确认。
            if (titleInput.inputMethodComposing || bodyEdit.inputMethodComposing)
                return false
            var nextTitle = titleInput.text.trim()
            if (nextTitle.length === 0) {
                scene.editingError = qsTr("名称不能为空")
                scene.logInteraction("node_edit_commit", "node", scene.editingNodeId, {
                    "success": false, "reason": "empty_title",
                    "gesture": scene.editingGestureId,
                    "button": scene.editingGestureButton
                })
                titleInput.forceActiveFocus()
                return false
            }
            var node = scene.graphController.getNode(scene.editingNodeId)
            if (!node) {
                scene.logInteraction("node_edit_commit", "node", scene.editingNodeId, {
                    "success": true, "reason": "node_removed",
                    "gesture": scene.editingGestureId,
                    "button": scene.editingGestureButton
                })
                scene.finishActiveNodeEdit()
                return true
            }
            var bodyEditable = StarMapNodeMetrics.canEditBody(node)
            var nextBody = bodyEditable ? bodyEdit.text : null
            var titleChanged = nextTitle !== (node.title || "")
            var bodyChanged = bodyEditable && nextBody !== StarMapNodeMetrics.bodyOf(node)
            // 字段分别保存：什么都没改就直接关闭，不发无意义的 title/content。
            if (!titleChanged && !bodyChanged) {
                scene.logInteraction("node_edit_commit", "node", scene.editingNodeId, {
                    "success": true, "reason": "unchanged",
                    "gesture": scene.editingGestureId,
                    "button": scene.editingGestureButton
                })
                closeEdit(true)
                return true
            }
            scene.editingSaving = true
            scene.editingError = ""
            var result = scene.commitNodeEdit(node.id,
                                              titleChanged ? nextTitle : null,
                                              bodyChanged ? nextBody : null)
            scene.editingSaving = false
            if (!result || result.success !== true) {
                scene.editingError = result && result.message
                        ? result.message : qsTr("保存失败，请重试")
                scene.logInteraction("node_edit_commit", "node", scene.editingNodeId, {
                    "success": false, "reason": "save_failed",
                    "titleChanged": titleChanged, "bodyChanged": bodyChanged,
                    "gesture": scene.editingGestureId,
                    "button": scene.editingGestureButton
                })
                return false
            }
            scene.logInteraction("node_edit_commit", "node", scene.editingNodeId, {
                "success": true, "reason": "saved",
                "titleChanged": titleChanged, "bodyChanged": bodyChanged,
                "gesture": scene.editingGestureId,
                "button": scene.editingGestureButton
            })
            closeEdit(true)
            return true
        }

        readonly property real _footerHeight: 30
        readonly property real _errorHeight: scene.editingError.length > 0 ? 18 : 0
        readonly property real _titleHeight: Math.max(24, titleInput.implicitHeight)
        readonly property real _bodyTop: _titleHeight + 6
        readonly property real _bodyHeight: Math.max(0,
            height - _bodyTop - _footerHeight - _errorHeight - 8)

        TextInput {
            id: titleInput
            x: 10
            y: 4
            visible: editOverlay._inputReady
            width: parent.width - 20
            height: editOverlay._titleHeight
            color: scene._textPrimary
            font.pixelSize: 16
            font.weight: Font.DemiBold
            activeFocusOnPress: true
            selectByMouse: true
            clip: true
            padding: 0

            onActiveFocusChanged: {
                if (editOverlay._inputReady) {
                    scene.logInteraction("node_edit_focus_changed", "node",
                                         scene.editingNodeId, {
                        "focusTarget": "title",
                        "activeFocus": activeFocus,
                        "length": text.length,
                        "imeComposing": inputMethodComposing
                    })
                }
            }
            onTextChanged: {
                if (editOverlay._inputReady) {
                    scene.editingTitleDraft = text
                    if (scene.editingError.length > 0)
                        scene.editingError = ""
                }
            }
            onTextEdited: {
                if (editOverlay._inputReady && !editOverlay._titleInputReported) {
                    editOverlay._titleInputReported = true
                    scene.logInteraction("node_edit_title_changed", "node",
                                         scene.editingNodeId, {
                        "length": text.length,
                        "focusTarget": activeFocus ? "title" : "other",
                        "imeComposing": inputMethodComposing
                    })
                }
            }
            Keys.onReturnPressed: function(event) {
                if (titleInput.inputMethodComposing)
                    return
                if (event.modifiers & Qt.ControlModifier)
                    editOverlay.commitEdit()
                else if (StarMapNodeMetrics.canEditBody(editOverlay.editingNode))
                    bodyEdit.forceActiveFocus()
                else
                    editOverlay.commitEdit()
                event.accepted = true
            }
            Keys.onEnterPressed: function(event) {
                if (titleInput.inputMethodComposing)
                    return
                if (event.modifiers & Qt.ControlModifier)
                    editOverlay.commitEdit()
                else if (StarMapNodeMetrics.canEditBody(editOverlay.editingNode))
                    bodyEdit.forceActiveFocus()
                else
                    editOverlay.commitEdit()
                event.accepted = true
            }
            Keys.onEscapePressed: function(event) {
                editOverlay.closeEdit(false)
                event.accepted = true
            }
        }

        Text {
            anchors.left: parent.left
            anchors.leftMargin: 10
            anchors.right: parent.right
            anchors.rightMargin: 10
            y: editOverlay._bodyTop
            visible: editOverlay._inputVisible && editOverlay.editingNode !== null
                     && !StarMapNodeMetrics.canEditBody(editOverlay.editingNode)
            text: qsTr("引用内容仅支持修改名称")
            color: scene._textMuted
            font.pixelSize: 12
            wrapMode: Text.Wrap
            enabled: false
        }

        Flickable {
            id: bodyFlick
            x: 10
            y: editOverlay._bodyTop
            width: parent.width - 20
            height: editOverlay._bodyHeight
            visible: editOverlay._inputReady && editOverlay.editingNode !== null
                     && StarMapNodeMetrics.canEditBody(editOverlay.editingNode)
            clip: true
            contentWidth: width
            contentHeight: bodyEdit.contentHeight + 2
            boundsBehavior: Flickable.StopAtBounds
            flickableDirection: Flickable.VerticalFlick
            // 让 TextEdit 独占文本点击、选区与拖动；正文滚动由滚轮和光标可见性驱动。
            interactive: false

            Text {
                x: 3
                y: 2
                visible: bodyEdit.text.length === 0
                text: qsTr("输入节点内容")
                color: scene._textMuted
                font.pixelSize: 13
                enabled: false
            }

            WheelHandler {
                onWheel: function(event) {
                    var maxY = Math.max(0, bodyFlick.contentHeight - bodyFlick.height)
                    if (maxY <= 0)
                        return
                    bodyFlick.contentY = Math.max(0, Math.min(maxY,
                        bodyFlick.contentY - event.angleDelta.y))
                    event.accepted = true
                }
            }

            TextEdit {
                id: bodyEdit
                width: bodyFlick.width
                height: Math.max(bodyFlick.height, contentHeight)
                color: scene._textPrimary
                font.pixelSize: 13
                wrapMode: TextEdit.Wrap
                textFormat: TextEdit.PlainText
                activeFocusOnPress: true
                selectByMouse: true
                padding: 0

                onActiveFocusChanged: {
                    if (editOverlay._inputReady) {
                        scene.logInteraction("node_edit_focus_changed", "node",
                                             scene.editingNodeId, {
                            "focusTarget": "body",
                            "activeFocus": activeFocus,
                            "length": text.length,
                            "imeComposing": inputMethodComposing
                        })
                    }
                }
                onTextChanged: {
                    if (editOverlay._inputReady) {
                        scene.editingBodyDraft = text
                        if (scene.editingError.length > 0)
                            scene.editingError = ""
                    }
                }
                onTextEdited: {
                    if (editOverlay._inputReady && !editOverlay._bodyInputReported) {
                        editOverlay._bodyInputReported = true
                        scene.logInteraction("node_edit_body_changed", "node",
                                             scene.editingNodeId, {
                            "length": text.length,
                            "focusTarget": activeFocus ? "body" : "other",
                            "imeComposing": inputMethodComposing
                        })
                    }
                }
                onCursorRectangleChanged: {
                    var maxY = Math.max(0, bodyFlick.contentHeight - bodyFlick.height)
                    var top = bodyFlick.contentY
                    var bottom = top + bodyFlick.height
                    var cursorTop = cursorRectangle.y
                    var cursorBottom = cursorTop + cursorRectangle.height
                    if (cursorTop < top)
                        bodyFlick.contentY = Math.max(0, Math.min(maxY, cursorTop))
                    else if (cursorBottom > bottom)
                        bodyFlick.contentY = Math.max(0, Math.min(maxY,
                            cursorBottom - bodyFlick.height))
                }
                Keys.onPressed: function(event) {
                    if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter)
                            && (event.modifiers & Qt.ControlModifier)) {
                        editOverlay.commitEdit()
                        event.accepted = true
                    } else if (event.key === Qt.Key_Escape) {
                        editOverlay.closeEdit(false)
                        event.accepted = true
                    }
                }
            }
        }

        Text {
            id: editErrorText
            anchors.left: parent.left
            anchors.leftMargin: 10
            anchors.right: parent.right
            anchors.rightMargin: 10
            anchors.bottom: editFooter.top
            height: editOverlay._errorHeight
            visible: editOverlay._inputVisible && scene.editingError.length > 0
            text: scene.editingError
            color: scene._error
            font.pixelSize: 11
            elide: Text.ElideRight
            verticalAlignment: Text.AlignVCenter
        }

        Item {
            id: editFooter
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.margins: 8
            height: editOverlay._footerHeight
            visible: editOverlay._inputVisible

            Controls.Button {
                id: cancelButton
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                height: parent.height
                width: Math.max(52, implicitWidth)
                text: qsTr("取消")
                font.pixelSize: 12
                padding: 0
                enabled: !scene.editingSaving
                onClicked: editOverlay.closeEdit(false)
            }
            Controls.Button {
                id: saveButton
                anchors.right: cancelButton.left
                anchors.rightMargin: 6
                anchors.verticalCenter: parent.verticalCenter
                height: parent.height
                width: Math.max(52, implicitWidth)
                text: scene.editingSaving ? qsTr("保存中…") : qsTr("保存")
                font.pixelSize: 12
                padding: 0
                enabled: !scene.editingSaving
                onClicked: editOverlay.commitEdit()
            }
        }
    }

    // ── 加载 / 错误提示 ──
    Text {
        anchors.centerIn: parent
        visible: scene.starmapId !== "" && !scene.ready && scene.errorMessage === ""
        text: qsTr("加载中…")
        color: scene._textMuted
        font.pointSize: dt.fontSmPt
        z: 20
    }
}
