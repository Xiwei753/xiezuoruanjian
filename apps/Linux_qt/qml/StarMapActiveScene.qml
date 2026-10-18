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
//     QtQuick.Shapes 的 ShapePath 按当前投影实时绘制，文字由实时 Text/TextInput
//     绘制。没有 layer FBO、没有缩略图纹理、没有把 1x 位图放大 N 倍。
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
import QtQuick.Shapes
import "StarMapPathPlanner.js" as StarMapPathPlanner

Item {
    id: scene

    required property var dt
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

    // ── 单一投影入口 ──
    // 数学上 worldToScreenX(w) === panX + w * zoom，渲染时改写成
    // "视口中心 + (w - 视口中心世界坐标) × zoom"：中心偏移在 double 里现算，
    // 场景图只见到视口量级的相对坐标。
    readonly property real _safeZoom: zoomLevel > 0 ? zoomLevel : 1.0
    readonly property real _originWorldX: (width / 2 - panX) / _safeZoom
    readonly property real _originWorldY: (height / 2 - panY) / _safeZoom

    function worldToScreenX(wx) { return width / 2 + (wx - _originWorldX) * _safeZoom }
    function worldToScreenY(wy) { return height / 2 + (wy - _originWorldY) * _safeZoom }
    function screenToWorldX(sx) { return _originWorldX + (sx - width / 2) / _safeZoom }
    function screenToWorldY(sy) { return _originWorldY + (sy - height / 2) / _safeZoom }

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
        onGraphChanged: scene.refreshEdges()
        onSelectionCleared: scene.selectionCleared()
        onNodeSelected: function(node) { scene.nodeSelected(node) }
        onEdgeSelected: function(edge) { scene.edgeSelected(edge) }
    }

    // 切换活动画布时显式落 id/pathKey 再加载：不能只依赖绑定求值顺序，
    // 否则 change handler 可能在绑定更新前先跑，读到上一张画布的图。
    function reloadActiveGraph() {
        graphController.starmapId = scene.starmapId
        graphController.pathKey = scene.scenePathKey
        graphController.loadGraph()
    }

    onStarmapIdChanged: {
        inlineEditingKey = ""
        reloadActiveGraph()
    }
    onScenePathKeyChanged: {
        graphController.pathKey = scene.scenePathKey
    }
    Component.onCompleted: reloadActiveGraph()

    readonly property bool ready: starmapId !== "" && graphController.graphData !== null
    readonly property string errorMessage: graphController.errorMessage
    function clearError() { graphController.clearError() }

    signal selectionCleared()
    signal nodeSelected(var node)
    signal edgeSelected(var edge)

    // 内联编辑汇总键（经 menuHost 汇总到根 Canvas，Router 在编辑期间让位给 TextInput）
    property string inlineEditingKey: ""

    function noteInlineEditing(nodeId, editing) {
        // 过渡期间的旧画面层只绘制：不认领内联编辑汇总键。
        if (!interactive)
            return
        var key = scenePathKey + "/node_" + nodeId
        if (editing) {
            inlineEditingKey = key
            if (menuHost)
                menuHost.setInlineEditingKey(key)
        } else {
            if (inlineEditingKey === key)
                inlineEditingKey = ""
            // delegate 被销毁（切画布 / 删节点 / 掉档）时本层键可能已被换画布清空，
            // 但根 Canvas 的汇总键只要还是这个 key 就必须清掉，否则 Router 永久让位。
            if (menuHost && menuHost.inlineEditingKey === key)
                menuHost.setInlineEditingKey("")
        }
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

    // 超链接 source path 必须满足 Core 不变量 path.starmap_id == host graph.starmap_id：
    // 活动画布的 graph 就是 starmapId 自己，source 从本画布起步（segments 空）。
    function nodeSourcePath(nodeId) {
        return {
            starmapId: starmapId,
            segments: [],
            target: { type: "node", nodeId: nodeId }
        }
    }
    function embedSourcePath(instanceId) {
        return {
            starmapId: starmapId,
            segments: [graphController.embedPathSegment(instanceId)],
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
    function _sameTarget(a, b) {
        if (!a || !b || a.type !== b.type)
            return false
        switch (a.type) {
        case "node":
            return (a.nodeId || "") === (b.nodeId || "")
        case "starmap":
            return true
        case "anchor":
            return (a.nodeId || "") === (b.nodeId || "")
                    && (a.anchorId || "") === (b.anchorId || "")
        default:
            return (a.nodeId || "") === (b.nodeId || "")
                    && (a.anchorId || "") === (b.anchorId || "")
                    && (a.projectId || "") === (b.projectId || "")
                    && (a.volumeId || "") === (b.volumeId || "")
                    && (a.chapterId || "") === (b.chapterId || "")
                    && (a.entityType || "") === (b.entityType || "")
                    && (a.entityId || "") === (b.entityId || "")
                    && (a.uri || "") === (b.uri || "")
        }
    }
    function _sameSourcePath(a, b) {
        if (!a || !b || a.starmapId !== b.starmapId)
            return false
        if (!_sameSegments(a.segments || [], b.segments || []))
            return false
        return _sameTarget(a.target || {}, b.target || {})
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
    function itemOf(kind, id) {
        return kind === "node" ? graphController.getNode(id) : graphController.getEmbed(id)
    }
    // 命中对象在本画布 world 坐标里的中心 / 矩形（scene 坐标 == world 坐标）。
    function itemCenterScene(kind, id) {
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
    function deleteNode(nodeId) { graphController.deleteNode(nodeId) }
    function updateEdgeLabel(edgeId, label) { graphController.updateEdge(edgeId, { label: label }) }
    function deleteEdge(edgeId) { graphController.deleteEdge(edgeId) }
    function updateEmbedLabel(instanceId, label) { graphController.updateEmbed(instanceId, { label: label }) }
    function deleteEmbed(instanceId) { graphController.deleteEmbed(instanceId) }
    function commitNodeTitle(nodeId, title) { graphController.updateNode(nodeId, { title: title }) }

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

    // 内联编辑入口（右键菜单"编辑" / Router 双击）：放进节点框内的 TextInput。
    function beginInlineEdit(nodeId) {
        if (interactionController && interactionController.pointerMode === "pinch")
            return
        var it = nodeItemOf(nodeId)
        if (it)
            it.beginEdit()
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

    // 超链接（StarMapHyperlink）透传，保持组件 API 完整。
    function listHyperlinksForNode(nodeId) {
        return _filterHyperlinks(nodeSourcePath(nodeId))
    }
    function listHyperlinksForEmbed(instanceId) {
        return _filterHyperlinks(embedSourcePath(instanceId))
    }
    function _filterHyperlinks(want) {
        var items = graphController.listHyperlinks()
        var out = []
        for (var i = 0; i < items.length; i++) {
            if (_sameSourcePath(items[i].source, want))
                out.push(items[i])
        }
        return out
    }
    function addHyperlinkForNode(nodeId, uri, label) {
        return graphController.addHyperlink(nodeSourcePath(nodeId), uri, label)
    }
    function addHyperlinkForEmbed(instanceId, uri, label) {
        return graphController.addHyperlink(embedSourcePath(instanceId), uri, label)
    }
    function updateHyperlink(hlId, patch) {
        return graphController.updateHyperlink(hlId, patch)
    }
    function deleteHyperlink(hlId) {
        return graphController.deleteHyperlink(hlId)
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
            if (!overlapsExistingNode(candidates[i].x, candidates[i].y, 150, 60, 12))
                return candidates[i]
        }
        return { x: wx, y: wy }
    }

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

    // ── 节点：矢量圆角矩形 + 实时 TextInput（内联编辑）──
    Repeater {
        id: nodeRepeater
        model: graphController.nodesModel
        // 正在编辑的节点被销毁（切画布 / 删节点）时不会再发 editingChanged(false)，
        // 这里主动清掉内联编辑汇总键，否则 Router 会永久让位给文本输入。
        onItemRemoved: function(index, item) {
            if (item && item.editing)
                scene.noteInlineEditing(item.nodeId, false)
        }

        delegate: Item {
            id: nodeItem
            required property var modelData
            required property int index

            readonly property var nodeData: modelData
            readonly property string nodeId: nodeData.id
            property bool editing: false
            property real _wobbleAnimX: 0
            property real _wobbleAnimY: 0

            readonly property bool isSelected: scene.selectionController
                    ? scene.selectionController.matches(scene.scenePathKey, "node", nodeData.id)
                    : false
            readonly property bool isPressed: scene.interactionController
                    ? scene.interactionController.isPressedTarget(scene.scenePathKey, "node", nodeData.id)
                    : false
            readonly property bool isMoving: scene.isMovingNode(nodeData.id)
            readonly property real worldX: isMoving && scene.interactionController
                    ? scene.interactionController.moveX : nodeData.x
            readonly property real worldY: isMoving && scene.interactionController
                    ? scene.interactionController.moveY : nodeData.y

            x: scene.worldToScreenX(worldX) + (nodeData.width * scene._safeZoom - width) / 2
            y: scene.worldToScreenY(worldY) + (nodeData.height * scene._safeZoom - height) / 2
            width: nodeData.width * scene._safeZoom * scene._reveal
                   + scene._revealDotRadius * 2 * (1 - scene._reveal)
            height: nodeData.height * scene._safeZoom * scene._reveal
                    + scene._revealDotRadius * 2 * (1 - scene._reveal)
            z: 1

            function beginEdit() {
                if (editing)
                    return
                editing = true
                titleInput.text = nodeItem.nodeData.title
                titleInput.forceActiveFocus()
                titleInput.selectAll()
            }
            function cancelEdit() {
                if (!editing)
                    return
                editing = false
                titleInput.focus = false
                titleInput.text = nodeItem.nodeData.title
            }
            function commitEdit() {
                if (!editing)
                    return
                var next = titleInput.text.trim()
                var changed = next.length > 0 && next !== nodeItem.nodeData.title
                editing = false
                titleInput.focus = false
                titleInput.text = nodeItem.nodeData.title
                if (changed)
                    scene.commitNodeTitle(nodeItem.nodeData.id, next)
            }
            onEditingChanged: scene.noteInlineEditing(nodeData.id, editing)

            Rectangle {
                id: visualNode
                anchors.fill: parent
                radius: Math.min(scene.dt.radiusSm * scene._safeZoom,
                                 Math.min(width, height) / 2)
                color: scene._surfaceContainer
                border.color: nodeItem.isSelected || nodeItem.editing ? scene._accent : scene._border
                border.width: (nodeItem.isSelected || nodeItem.editing) ? 2 : 1

                transform: Translate {
                    x: (nodeItem.isSelected || nodeItem.isPressed || nodeItem.editing)
                       ? 0 : nodeItem._wobbleAnimX
                    y: (nodeItem.isSelected || nodeItem.isPressed || nodeItem.editing)
                       ? 0 : nodeItem._wobbleAnimY
                }

                TextInput {
                    id: titleInput
                    anchors.fill: parent
                    anchors.margins: 8 * scene._safeZoom
                    readOnly: !nodeItem.editing
                    enabled: nodeItem.editing
                    // 投影太小就不画文字（只省文字，不改形状）；展开进度到
                    // 一半以后标题才显出来。
                    visible: nodeItem.editing
                             || (nodeItem.height >= 18 && scene.detailReveal > 0.6)
                    color: scene._textPrimary
                    font.pointSize: scene.dt.fontSmPt * scene._textScale
                    selectByMouse: nodeItem.editing
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                    clip: true
                    padding: 0

                    Binding {
                        target: titleInput
                        property: "text"
                        value: nodeItem.nodeData.title
                        when: !nodeItem.editing
                        restoreMode: Binding.RestoreBindingOrValue
                    }

                    onAccepted: nodeItem.commitEdit()
                    onEditingFinished: nodeItem.commitEdit()
                    Keys.onEscapePressed: nodeItem.cancelEdit()
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
    // 只影响显示：圆心 / 半径由同一个 progress 现算，圆心向视口中心对齐、
    // 半径放大到整圈离开可见视口才停止绘制；圆内这段画面就是本层真实矢量
    // 内容（不是把圆点预览截图放大）。画在本层内容之下：圆外留给旧画面层。
    Rectangle {
        id: transitionRing
        visible: scene.ringVisible
        x: scene.ringCenterX - scene.ringRadius
        y: scene.ringCenterY - scene.ringRadius
        width: Math.max(0, scene.ringRadius) * 2
        height: Math.max(0, scene.ringRadius) * 2
        radius: width / 2
        color: scene.dt.surfaceContainerLow
        border.color: scene.dt.inverseSurface
        border.width: scene.ringStrokeWidth
        antialiasing: true
        z: -1
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
