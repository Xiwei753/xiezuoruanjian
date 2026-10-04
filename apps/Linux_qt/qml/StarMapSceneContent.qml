// =============================================================================
// StarMapSceneContent.qml — 星图单层内容容器（递归内容，不是递归视口）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：加载"一层"星图数据 + 渲染该层节点/连线/子星图，并提供递归命中测试
//
// Issue #822：整棵星图只有一个全局 viewport/camera。
//   panX / panY / zoomLevel 只存在于根 StarMapCanvas；WheelHandler / PinchHandler
//   也只存在于根 Canvas。本文件是纯内容容器，不含任何相机属性或相机手势。
//   子星图显示多少细节是 Deep Zoom 档位：ownerEffectiveScale = 全局相机 ×
//   祖先 local fit，coverage = 投影尺寸 / 根视口短边，档位只决定子内容渲染
//   完整交互还是轻量 preview；本文件不能反写全局相机，
//   也不能产生自己可独立修改的 zoomLevel。
//
// 坐标约定：
//   - 本层局部坐标 = 该层星图的 world 坐标，节点/连线都画在 worldLayer 里。
//     每层内容做一次 local fit（worldLayer 的 x/y/scale），只改显示，不改
//     authored position。
//   - scene 坐标 = 根 Content 的局部坐标（整棵递归树的顶层 world 坐标）。
//     sceneToLocal / localToScene 一律用 Qt 真实 Item 映射
//     （worldLayer.mapFromItem/mapToItem(rootContent)）换算：contentViewport 的
//     布局偏移、每层 local fit、祖先位置全部自动进入同一条坐标链，
//     不再手工维护 sceneOrigin/sceneScale 矩阵。
//   - Qt scene 坐标（QQuickWindow 坐标）只出现在 delegate 上抛的信号里，
//     进来立刻用 mapFromItem 换算掉，不往状态机里存。
//
// 渲染档位（Deep Zoom，见 docs/starmap_viewport.md）：
//   interactive — 完整节点/Embed delegate，可命中、可编辑、可继续递归
//   preview     — 只画一张静态投影 Canvas：有形状和颜色，没有交互组件，
//                 也不再往里递归；只回答"里面大概有什么"
//   档位只改渲染细节，不改 authored position / world bounds / Embed 外壳几何。
//
// 递归命中测试：
//   hitTargetAtScene(sceneX, sceneY) 先查本层节点 / Embed chrome；落在 Embed
//   内容区时递归进子 Content。返回真正命中的那一层
//   （owner / scenePathKey / starmapId / kind / id / targetPath），
//   连线松手、右键空白新建、选中、pointer_press 日志全部走这一个入口。
//
// 约束：
//   - 不出现 panX / panY / zoomLevel / WheelHandler / PinchHandler / 背景 pan handler
//   - 相机与全局手势状态由根 Canvas 唯一持有，通过 interactionController 共享
//   - 不得静态引用自身或 StarMapEmbed 类型（会形成编译期环），
//     Embed 侧必须用运行时 Qt.resolvedUrl + Loader.setSource 递归创建本组件
// =============================================================================

import QtQuick
import "StarMapPathPlanner.js" as StarMapPathPlanner

Item {
    id: content

    required property var dt
    required property string rootStarmapId
    required property var pathSegments
    // Issue #822：required 且没有默认 "root"。
    // 根层由 Canvas 显式传 "root"，子层在创建时一次性传完整 "root/embed_xxx/..."，
    // 绝不允许子层临时冒充根层。
    required property string scenePathKey
    required property var starmapBackendRef
    required property var selectionController
    required property var interactionController
    // 全局相机比例：只对根层（depth 0）有意义，由根 Canvas 绑定注入。
    // 子层不复制这个标量，有效比例沿 ownerSceneContent 链现算。
    property real globalZoom: 1.0
    required property int depth

    // 根层由根 Canvas 指向自己，子层由父 Embed 原样传下来。
    property var rootContent: null
    // 菜单宿主（根 Canvas）。子层直接回调它，不需要把信号逐层冒泡。
    property var menuHost: null
    // 本层内容所属的父 SceneContent（根层为 null）。
    // 子层有效比例 = 本层 local fit × ownerSceneContent.effectiveScale，
    // 沿链现读，不复制标量：相机或祖先 fit 变化后不会拿到过期值。
    property var ownerSceneContent: null

    // 渲染档位（由父 Embed 的 Deep Zoom 判定传入）：interactive / preview。
    // 根层永远是 interactive。
    property string renderDetail: "interactive"

    // 根视口可见区域（scene 坐标）：根层由 Canvas 按全局相机绑定注入；
    // 子层沿 ownerSceneContent 链读同一份，不复制。
    property var rootViewportRect: ({ x: 0, y: 0, width: 0, height: 0 })
    readonly property var viewportRect: ownerSceneContent
            ? ownerSceneContent.viewportRect
            : rootViewportRect

    // 子内容可用边长（由父 Embed 传入）：父圆的内接正方形扣掉交互壳。
    // 本层再扣自己的留白得到安全区，local fit 与移动/新建 clamp 共用这一份，
    // 不再一边按内接正方形、一边按整个圆、一边再手写边框偏移。
    property real contentUsableSide: 0
    readonly property real _contentSafeSide:
        Math.max(0, contentUsableSide - _fitPadding * 2)

    // 根视口短边（屏幕像素）：coverage 的分母。根内容铺满全局视口，
    // 根 Content 自己的宽高就是视口尺寸。
    readonly property real viewportShortSide: {
        var w = rootContent ? rootContent.width : width
        var h = rootContent ? rootContent.height : height
        return (w > 0 && h > 0) ? Math.min(w, h) : 0
    }

    // 本 Scene 的累计有效比例（屏幕口径）：根层是全局相机，
    // 子层是 local fit × 祖先累计。Deep Zoom 的 coverage 只读这一份，
    // 命中/渲染仍走真实 Item transform（见 sceneToLocal/localToScene）。
    readonly property real effectiveScale: {
        if (depth === 0)
            return globalZoom
        var inherited = ownerSceneContent ? ownerSceneContent.effectiveScale : 1
        if (!(inherited > 0))
            inherited = 1
        return localFitScale * inherited
    }

    readonly property color _accent: dt.accent
    readonly property color _accentSoft: dt.accentSoft
    readonly property color _border: dt.border
    readonly property color _error: dt.error
    readonly property color _surfaceContainer: dt.surfaceContainer
    readonly property color _textPrimary: dt.textPrimary
    readonly property color _textMuted: dt.textMuted
    readonly property int _radiusSm: dt.radiusSm

    // ── 路径解析 ──
    // 解析出的本层星图 ID 只属于这块 Content，不能写回上层。
    property string finalStarmapId: ""
    property string resolveError: ""

    // ── 坐标换算：只认 Qt 真实 Item 映射 ──
    // scene 坐标 = 根 Content 局部坐标；本层局部坐标 = worldLayer 里的 authored
    // 坐标。contentViewport 的布局偏移（子内容区现在是圆的内接正方形）、每层
    // local fit、祖先 Embed 的实际变换都在 mapFromItem/mapToItem 里自动算完，
    // 不再手工维护矩阵，也不手抄任何偏移常量。
    function sceneToLocal(sceneX, sceneY) {
        return worldLayer.mapFromItem(rootContent, sceneX, sceneY)
    }
    function localToScene(localX, localY) {
        return worldLayer.mapToItem(rootContent, localX, localY)
    }

    // Qt scene（窗口）坐标增量 → 本层局部坐标增量。
    // mapFromItem 只能映射点，不能映射增量；映射两个点再相减。
    // 这是拖动 move 路径上唯一一次 Qt scene → 本层 local 的换算。
    function qtSceneDeltaToLocal(dx, dy) {
        var origin = worldLayer.mapFromItem(null, 0, 0)
        var point = worldLayer.mapFromItem(null, dx, dy)
        return { x: point.x - origin.x, y: point.y - origin.y }
    }

    // Qt scene（窗口）坐标增量 → root-world（scene 坐标）增量。
    // connect / contextPending 的端点存的是 scene 坐标，必须吃这一份换算：
    // 原始像素直接加到 root-world 上会随全局缩放漂移（zoom=2 时多走一倍）。
    // 拖动阈值仍吃原始像素，两个口径分开，不混用。
    function qtSceneDeltaToRootScene(dx, dy) {
        var origin = rootContent.mapFromItem(null, 0, 0)
        var point = rootContent.mapFromItem(null, dx, dy)
        return { x: point.x - origin.x, y: point.y - origin.y }
    }

    // ── 每层 local fit（只改显示，不改 authored position）──
    // fit 比例由本层内容包围盒 + 本层容器（根是视口，子是 Embed 内容区）算出；
    // 根层是相机本体，不做 fit（全局相机就是它的显示变换）。
    // 留白与 Harmony 的 STARMAP_EMBED_FIT_PADDING_VP 同值，跨平台同一个适配口径。
    readonly property real _fitPadding: 8
    readonly property real _minFitScale: 0.02
    readonly property real _maxFitScale: 4.0

    // 新建节点的显示尺寸（clamp 用；与 GraphController.buildModels 的显示尺寸一致）。
    readonly property int _newNodeWidth: 150
    readonly property int _newNodeHeight: 60

    // 边线宽的 world 单位常量：Canvas 绘制与命中阈值共用同一份。
    readonly property real _edgeLineWorldWidth: 2
    // 边命中阈值的屏幕口径：10 屏幕像素。
    // 相机允许 1e-4~1e5 之后，固定 world 阈值在屏幕上会差几个数量级，
    // 所以每次命中都用 effectiveScale 折算成本层 world 单位。
    readonly property real _edgeHitScreenPx: 10

    // 命中范围必须同时满足两条：
    // - 缩小时至少还有 10 屏幕像素（好点中）；
    // - 放大时至少覆盖真实可见的线宽本身（Canvas 的 lineWidth 是 world 单位，
    //   会跟着 effectiveScale 一起放大，不能在"肉眼很粗的线里"点不中）。
    function _edgeHitLocalThreshold() {
        var scale = Math.max(effectiveScale, 1e-6)
        return Math.max(_edgeHitScreenPx / scale, _edgeLineWorldWidth / 2)
    }

    // 源形状朝目标点的边界交点（本层 authored 坐标）。
    // 与 Rust edge_render 的 line_rect_entry / line_circle_entry 是同一套语义：
    // 普通节点取矩形边，Embed（含旧 portal 归一）取圆周。
    // 拉线预览必须和正式边用同一语义，否则松手瞬间端点会从圆心跳到圆周。
    function boundaryPointLocal(x, y, width, height, isEmbed, tx, ty) {
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
        // 线 × 矩形：slab 裁剪取进入点（与 Rust line_rect_entry 同一算法）。
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

    // 端点路径的 starmapId 必须绑定到宿主图（Core 按它区分本地/跨层）。
    // 预览与正式边提交共用这一份绑定，两边路径完全相同。
    function bindPlanToHost(plan, host) {
        if (!plan || !host || host.finalStarmapId === "")
            return null
        plan.from.starmapId = host.finalStarmapId
        plan.to.starmapId = host.finalStarmapId
        return plan
    }

    // 候选边预览：宿主 Content 把 prospective LCA 规划结果交给平台 edge renderer。
    // Content 只负责把规划路径交给宿主本层的 graphController；边界求交、
    // 双向偏移、旧 portal 归一、深路径投影全部由 Rust 正式边几何出一份真相。
    function prospectiveEdgeRenderForPlan(plan) {
        if (!plan || !graphController)
            return null
        return graphController.computeProspectiveEdgeRender(plan.from, plan.to)
    }

    // connect 预览线与正式边共用同一套几何真相：
    // - 鼠标悬停在合法 target 上时，先做 prospective LCA 规划，再把规划出的
    //   两条路径交给宿主的平台 edge renderer（候选边临时追加进宿主边表后只取
    //   它自己的 render）。Node 矩形 / Embed 圆周、深路径投影、已有反向边时的
    //   双向 12 world 偏移都与松手后的正式边完全一致；
    // - 没有合法 target 时退回"源对象边界 → 当前鼠标"。
    function refreshConnectPreview() {
        var ic = interactionController
        if (!ic || ic.pointerMode !== "connect" || ic.connectFromKind === "")
            return
        // 默认：预览终点就是原始鼠标位置（松手命中仍用 connectMouseX/Y）。
        ic.connectPreviewEndX = ic.connectMouseX
        ic.connectPreviewEndY = ic.connectMouseY

        var hit = rootContent
                ? rootContent.hitTargetAtScene(ic.connectMouseX, ic.connectMouseY)
                : null
        if (hit && (hit.kind === "node" || hit.kind === "embed")) {
            var plan = StarMapPathPlanner.planCrossLayerEdge(ic.connectFromPath, hit.targetPath)
            var host = plan && rootContent
                    ? rootContent.findContentByPathSegments(plan.hostSegments)
                    : null
            plan = bindPlanToHost(plan, host)
            if (plan) {
                var preview = host.prospectiveEdgeRenderForPlan(plan)
                if (preview) {
                    var startScene = host.localToScene(preview.startX, preview.startY)
                    var endScene = host.localToScene(preview.endX, preview.endY)
                    ic.connectFromSceneX = startScene.x
                    ic.connectFromSceneY = startScene.y
                    ic.connectPreviewEndX = endScene.x
                    ic.connectPreviewEndY = endScene.y
                    return
                }
            }
        }

        // 退回：源对象边界朝当前鼠标方向。
        var item = ic.connectFromKind === "node"
                ? graphController.getNode(ic.connectFromId)
                : graphController.getEmbed(ic.connectFromId)
        if (!item)
            return
        var target = sceneToLocal(ic.connectMouseX, ic.connectMouseY)
        var boundary = boundaryPointLocal(item.x, item.y, item.width, item.height,
                                          ic.connectFromKind === "embed",
                                          target.x, target.y)
        var scenePoint = localToScene(boundary.x, boundary.y)
        ic.connectFromSceneX = scenePoint.x
        ic.connectFromSceneY = scenePoint.y
    }

    readonly property var contentBounds: {
        var minX = 0
        var minY = 0
        var maxX = 0
        var maxY = 0
        var hasContent = false
        var i
        var nodes = graphController.nodesModel
        for (i = 0; i < nodes.length; i++) {
            var n = nodes[i]
            if (!hasContent) {
                minX = n.x; minY = n.y
                maxX = n.x + n.width; maxY = n.y + n.height
                hasContent = true
            } else {
                minX = Math.min(minX, n.x); minY = Math.min(minY, n.y)
                maxX = Math.max(maxX, n.x + n.width); maxY = Math.max(maxY, n.y + n.height)
            }
        }
        var embeds = graphController.embedsModel
        for (i = 0; i < embeds.length; i++) {
            var e = embeds[i]
            if (!hasContent) {
                minX = e.x; minY = e.y
                maxX = e.x + e.width; maxY = e.y + e.height
                hasContent = true
            } else {
                minX = Math.min(minX, e.x); minY = Math.min(minY, e.y)
                maxX = Math.max(maxX, e.x + e.width); maxY = Math.max(maxY, e.y + e.height)
            }
        }
        if (!hasContent)
            return null
        return { minX: minX, minY: minY, maxX: maxX, maxY: maxY,
                 width: maxX - minX, height: maxY - minY }
    }

    readonly property real localFitScale: {
        if (depth === 0)
            return 1.0
        var b = contentBounds
        if (!b || b.width <= 0 || b.height <= 0)
            return 1.0
        // 可用区 = 内容安全区（圆的内接正方形扣交互壳再扣留白），
        // 和移动/新建的 clamp 共用同一份，fit 完的内容天然在安全区内。
        var available = _contentSafeSide
        if (!(available > 0))
            return 1.0
        var raw = Math.min(available / b.width, available / b.height)
        if (!isFinite(raw) || raw <= 0)
            return _minFitScale
        return Math.max(_minFitScale, Math.min(_maxFitScale, raw))
    }
    readonly property real localFitOffsetX: {
        if (depth === 0)
            return 0
        var b = contentBounds
        if (!b)
            return 0
        return content.width / 2 - (b.minX + b.maxX) / 2 * localFitScale
    }
    readonly property real localFitOffsetY: {
        if (depth === 0)
            return 0
        var b = contentBounds
        if (!b)
            return 0
        return content.height / 2 - (b.minY + b.maxY) / 2 * localFitScale
    }

    // 把本层 authored 坐标的矩形夹回内容安全区（与 local fit 同一份安全区）。
    // 只对 depth > 0 的圆壳内容生效：根层是无限画布，不做约束。
    // 返回的就是最终坐标，拖动显示与写 Core 用同一份，不再"显示一份、存另一份"。
    function clampToContentSafeArea(x, y, width, height) {
        if (depth === 0 || !(_contentSafeSide > 0))
            return { x: x, y: y }
        var fit = localFitScale > 0 ? localFitScale : 1
        var boxSize = Math.min(content.width, content.height)
        var center = boxSize / 2
        var halfSafe = _contentSafeSide / 2
        var itemWidth = width * fit
        var itemHeight = height * fit
        var minLeft = center - halfSafe
        var maxLeft = center + halfSafe - itemWidth
        var minTop = center - halfSafe
        var maxTop = center + halfSafe - itemHeight
        // item 比整个安全区还大时没有合法区间，退回居中（至少不会贴着一侧溢出）。
        var left = maxLeft < minLeft
                ? center - itemWidth / 2
                : Math.max(minLeft, Math.min(maxLeft, x * fit + localFitOffsetX))
        var top = maxTop < minTop
                ? center - itemHeight / 2
                : Math.max(minTop, Math.min(maxTop, y * fit + localFitOffsetY))
        return { x: (left - localFitOffsetX) / fit, y: (top - localFitOffsetY) / fit }
    }

    // 当前 move 目标在模型里的显示尺寸（clamp 用）。
    function movingItemSize() {
        if (!interactionController)
            return { width: 0, height: 0 }
        var ic = interactionController
        if (ic.pressedNodeId !== "") {
            var node = graphController.getNode(ic.pressedNodeId)
            if (node)
                return { width: node.width, height: node.height }
        } else if (ic.pressedEmbedId !== "") {
            var embed = graphController.getEmbed(ic.pressedEmbedId)
            if (embed)
                return { width: embed.width, height: embed.height }
        }
        return { width: 0, height: 0 }
    }

    // 本层局部适配或尺寸变化：子 Embed 据此重算懒加载裁剪。
    // offset 也是真实显示变换：内容整体平移时 scale/宽高可能不变，只有 offset 变，
    // 漏掉它就等于子层投影位置变化不通知，懒加载可见性停在旧位置。
    signal transformChanged()
    onLocalFitScaleChanged: transformChanged()
    onLocalFitOffsetXChanged: transformChanged()
    onLocalFitOffsetYChanged: transformChanged()
    onWidthChanged: transformChanged()
    onHeightChanged: transformChanged()
    Connections {
        // 祖先链任何一层的 fit 变化都会平移本层在 scene 坐标里的位置。
        target: content.ownerSceneContent
        function onTransformChanged() { content.transformChanged() }
    }

    function resolvePath() {
        if (rootStarmapId === "") {
            resolveError = ""
            finalStarmapId = ""
            return
        }
        if (!starmapBackendRef) {
            // 后端未注入时不报错：创建顺序不保证，等后端到了再解析。
            resolveError = ""
            finalStarmapId = ""
            return
        }
        var res = starmapBackendRef.resolve_starmap_path(rootStarmapId, JSON.stringify(pathSegments))
        if (!res || res.success !== true) {
            resolveError = qsTr("解析星图层级路径失败")
                    + (res && res.errorCode ? " (" + res.errorCode + ")" : "")
            finalStarmapId = ""
            starmapBackendRef.record_interaction(
                "scene_resolve_failed", scenePathKey, rootStarmapId, "scene", "",
                JSON.stringify({
                    "pathKey": scenePathKey,
                    "errorCode": (res && res.errorCode) ? res.errorCode : "",
                    "rootStarmapId": rootStarmapId,
                    "depth": pathSegments.length
                }))
            return
        }
        var finalId = res.data && res.data.finalStarmapId ? res.data.finalStarmapId : ""
        if (finalId === "") {
            resolveError = qsTr("解析星图层级路径失败")
            finalStarmapId = ""
            starmapBackendRef.record_interaction(
                "scene_resolve_failed", scenePathKey, rootStarmapId, "scene", "",
                JSON.stringify({
                    "pathKey": scenePathKey,
                    "errorCode": "empty_final_id",
                    "rootStarmapId": rootStarmapId,
                    "depth": pathSegments.length
                }))
            return
        }
        resolveError = ""
        finalStarmapId = finalId
        starmapBackendRef.record_interaction(
            "scene_resolved", scenePathKey, rootStarmapId, "scene", finalId,
            JSON.stringify({
                "pathKey": scenePathKey,
                "rootStarmapId": rootStarmapId,
                "finalStarmapId": finalId,
                "depth": pathSegments.length
            }))
    }

    onRootStarmapIdChanged: resolvePath()
    onPathSegmentsChanged: resolvePath()
    onStarmapBackendRefChanged: {
        if (rootStarmapId !== "" && finalStarmapId === "")
            resolvePath()
    }
    onFinalStarmapIdChanged: {
        if (finalStarmapId !== "")
            graphController.loadGraph()
    }
    Component.onCompleted: resolvePath()

    signal selectionCleared()
    signal nodeSelected(var node)
    signal edgeSelected(var edge)

    readonly property string errorMessage: graphController.errorMessage

    function clearError() { graphController.clearError() }

    // ── 本层图控制器 ──
    // 每层 Content 拥有自己的 GraphController，节点与本层连线都按本层局部坐标渲染。
    StarMapGraphController {
        id: graphController
        starmapId: content.finalStarmapId
        starmapBackendRef: content.starmapBackendRef
        // Issue #814 评论 5935285879: 选择身份带本层 pathKey。
        pathKey: content.scenePathKey
        selectionController: content.selectionController
        onGraphChanged: content.refreshEdges()
        onSelectionCleared: content.selectionCleared()
        onNodeSelected: function(node) { content.nodeSelected(node) }
        onEdgeSelected: function(edge) { content.edgeSelected(edge) }
    }

    // 按 id 取本层模型条目，供菜单（移动/编辑/删除）使用。
    function hitNode(nodeId) { return graphController.getNode(nodeId) }
    function hitEmbed(instanceId) { return graphController.getEmbed(instanceId) }
    function hitEdge(edgeId) {
        for (var i = 0; i < graphController.edgesModel.length; i++) {
            if (graphController.edgesModel[i].id === edgeId)
                return graphController.edgesModel[i]
        }
        return null
    }

    // ── 路径构造（StarMapTargetPathDto JS 对象）──
    // JSON 字段名遵循 DTO serde rename：starmapId（camelCase）、segments、target.type/nodeId。
    // starmapId 用 rootStarmapId，segments 用本层 pathSegments，
    // 这样第 N 层连线端点不会退化成第一层。
    function nodePath(nodeId) {
        return {
            starmapId: rootStarmapId,
            segments: pathSegments.slice(0),
            target: { type: "node", nodeId: nodeId }
        }
    }
    // Embed 端点的完整路径由本层 pathSegments.concat 拼出来。
    function embedPath(instanceId) {
        return {
            starmapId: rootStarmapId,
            segments: pathSegments.concat([graphController.embedPathSegment(instanceId)]),
            target: { type: "starmap" }
        }
    }

    // ---------------------------------------------------------------------------
    // 递归命中测试：命中哪一层就返回哪一层的身份。
    // 顺序：本层节点 → 本层 Embed chrome → 子星图内容区（递归）→ 本层连线 → 本层空白。
    // ---------------------------------------------------------------------------
    function hitTargetAtScene(sceneX, sceneY) {
        if (finalStarmapId === "")
            return null
        // preview 档只回答"里面有什么"，不参与命中：返回 null，让父层把这个
        // 区域当作还没进入交互的子内容区（childContent），而不是假装命中节点。
        if (renderDetail !== "interactive")
            return null
        var p = sceneToLocal(sceneX, sceneY)

        var node = graphController.findNodeAt(p.x, p.y)
        if (node) {
            return {
                owner: content,
                scenePathKey: scenePathKey,
                starmapId: finalStarmapId,
                kind: "node",
                id: node.id,
                targetPath: nodePath(node.id),
                localX: p.x,
                localY: p.y
            }
        }

        var chrome = graphController.findEmbedChromeAt(p.x, p.y)
        if (chrome) {
            return {
                owner: content,
                scenePathKey: scenePathKey,
                starmapId: finalStarmapId,
                kind: "embed",
                id: chrome.instanceId,
                targetPath: embedPath(chrome.instanceId),
                localX: p.x,
                localY: p.y
            }
        }

        var inside = graphController.findEmbedContentAt(p.x, p.y)
        if (inside) {
            var child = childContentOf(inside.instanceId)
            if (child) {
                var deeper = child.hitTargetAtScene(sceneX, sceneY)
                if (deeper)
                    return deeper
            }
            // 子星图内容还没加载出来：命中点归本层的 childContent，
            // 不能冒充 empty（empty 专指"任何一层都没命中"）。
            return {
                owner: content,
                scenePathKey: scenePathKey,
                starmapId: finalStarmapId,
                kind: "childContent",
                id: inside.instanceId,
                targetPath: null,
                localX: p.x,
                localY: p.y
            }
        }

        var edge = graphController.hitTestEdge(p.x, p.y, _edgeHitLocalThreshold())
        if (edge) {
            return {
                owner: content,
                scenePathKey: scenePathKey,
                starmapId: finalStarmapId,
                kind: "edge",
                id: edge.id,
                targetPath: null,
                localX: p.x,
                localY: p.y
            }
        }

        return {
            owner: content,
            scenePathKey: scenePathKey,
            starmapId: finalStarmapId,
            kind: "empty",
            id: "",
            targetPath: null,
            localX: p.x,
            localY: p.y
        }
    }

    function embedItemOf(instanceId) {
        for (var i = 0; i < embedRepeater.count; i++) {
            var it = embedRepeater.itemAt(i)
            if (it && it.instanceId === instanceId)
                return it
        }
        return null
    }
    function nodeItemOf(nodeId) {
        for (var i = 0; i < nodeRepeater.count; i++) {
            var it = nodeRepeater.itemAt(i)
            if (it && it.nodeId === nodeId)
                return it
        }
        return null
    }
    function childContentOf(instanceId) {
        var it = embedItemOf(instanceId)
        return it ? it.childContent() : null
    }

    // 绝对路径段 → SceneContent：从整棵递归树的根开始逐段下钻。
    // 跨层连线的宿主就是按这条路找出来的（宿主 = 两端 Scene 的 LCA）。
    // 段语义（enterEmbed / enterPortal 的 UI 映射）统一在 StarMapPathPlanner。
    function findContentByPathSegments(segments) {
        var current = rootContent ? rootContent : content
        for (var i = 0; i < segments.length; i++) {
            if (!current)
                return null
            var instanceId = StarMapPathPlanner.uiInstanceIdOfSegment(segments[i])
            if (instanceId === "")
                return null
            current = current.childContentOf(instanceId)
        }
        return current
    }

    // 宿主图建边入口：由宿主 Content 把边交给自己的 GraphController。
    // 跨组件不能直接摸对方的 id，所以宿主必须提供这个声明入口。
    function commitEdgeWithPaths(fromPath, toPath) {
        return graphController.createEdgeWithPaths(fromPath, toPath)
    }

    // ---------------------------------------------------------------------------
    // 手势仲裁：全局状态机由根 Canvas 唯一创建并逐层共享。
    // 本层只负责"按下去的对象是不是我的"以及把位移换算成自己的局部坐标；
    // 状态只能被共享状态机提升一次，delegate 自己不决定 move/connect。
    // ---------------------------------------------------------------------------
    function ownsPress(kind, id) {
        if (!interactionController)
            return false
        var ic = interactionController
        if (ic.pointerMode === "connect" || ic.pointerMode === "contextPending")
            return ic.connectFromScenePathKey === scenePathKey
                && ic.connectFromKind === kind
                && ic.connectFromId === id
        if (ic.pointerMode === "move")
            return ic.moveScenePathKey === scenePathKey
                && (kind === "node" ? ic.pressedNodeId === id : ic.pressedEmbedId === id)
        return ic.pressScenePathKey === scenePathKey
            && ic.pressKind === kind
            && ic.pressId === id
    }

    // 节点/Embed chrome 上的鼠标按下：只登记归属，不立刻移动也不立刻连线。
    function onItemPressed(kind, id, targetPath, qtSceneX, qtSceneY) {
        if (!interactionController)
            return
        // Qt scene（窗口）坐标 → scene 坐标，一次 mapFromItem 到位。
        var sp = rootContent.mapFromItem(null, qtSceneX, qtSceneY)
        interactionController.beginPress(kind, id, targetPath, scenePathKey, sp.x, sp.y)
    }

    // delegate DragHandler 上抛的原始 Qt scene 位移。
    // 只有按下归属在本层的对象时才消费；仲裁逻辑和触屏共用同一条路径，
    // 保证"鼠标长按拉线"和"触屏长按拉线"不会各写一套状态提升。
    function onItemDragDelta(kind, id, dxQtScene, dyQtScene) {
        if (!interactionController)
            return
        if (!ownsPress(kind, id))
            return
        onSceneDragDelta(dxQtScene, dyQtScene)
    }

    function promoteToMove(kind, id) {
        if (!interactionController)
            return
        var item = kind === "node" ? graphController.getNode(id) : graphController.getEmbed(id)
        if (!item)
            return
        if (!interactionController.pressPendingToMove(kind, id, scenePathKey, item.x, item.y))
            return
        logInteraction("move_begin", kind, id, {
            "kind": kind, "fromX": item.x, "fromY": item.y
        })
        refreshEdges()
    }

    // 长按计时到：由共享 InteractionController 的 Timer 触发，只有归属层提升为 connect。
    function onPressTimeout() {
        if (!interactionController)
            return
        var ic = interactionController
        if (ic.pointerMode !== "pressPending")
            return
        var kind = ic.pressKind
        var id = ic.pressId
        if (!ownsPress(kind, id))
            return
        var item = kind === "node" ? graphController.getNode(id) : graphController.getEmbed(id)
        if (!item)
            return
        var center = localToScene(item.x + item.width / 2, item.y + item.height / 2)
        if (!ic.pressPendingToConnect(kind, id, ic.pressTargetPath, center.x, center.y))
            return
        logInteraction("connect_begin", kind, id, {
            "kind": kind, "fromId": id, "fromX": center.x, "fromY": center.y
        })
    }

    // 触屏长按：不移动弹菜单，移动超阈值转 connect（#373 触屏语义）。
    function onItemTouchLongPressed(kind, id, targetPath) {
        if (!interactionController)
            return
        var ic = interactionController
        // Issue #822 评论 5977278030：双指缩放优先。pinch 已经接管时拒绝晚到的
        // 长按回调，不再把状态改回 contextPending —— Qt 的 passive grab 在
        // PinchHandler 抢到 exclusive grab 后仍会继续收到事件。
        if (ic.pointerMode === "pinch")
            return
        var item = kind === "node" ? graphController.getNode(id) : graphController.getEmbed(id)
        if (!item)
            return
        var center = localToScene(item.x + item.width / 2, item.y + item.height / 2)
        if (!ic.beginContextPending(kind, id, targetPath, scenePathKey, center.x, center.y))
            return
        if (menuHost)
            menuHost.showTouchPreview(kind, center.x, center.y)
    }

    // 本层是不是当前手势的归属层（pressPending / connect / contextPending / move）。
    function isGestureOwner() {
        if (!interactionController)
            return false
        var ic = interactionController
        return ic.pressScenePathKey === scenePathKey
                || ic.connectFromScenePathKey === scenePathKey
                || ic.moveScenePathKey === scenePathKey
    }

    // 拖动位移：delegate 只上抛原始 activeTranslation 增量（Qt scene 坐标），
    // 所有 Qt scene → 本层 local 的换算只在这里做一次。
    // 手指起点可能在任意深层的节点上，归属层不一定是根层，
    // 所以从根开始往下找到真正的归属层，返回 true 表示已消费。
    function onSceneDragDelta(dxQtScene, dyQtScene) {
        if (!interactionController)
            return false
        var ic = interactionController
        if (ic.pointerMode !== "contextPending" && ic.pointerMode !== "connect"
                && ic.pointerMode !== "pressPending" && ic.pointerMode !== "move")
            return false
        if (!isGestureOwner()) {
            // 归属在更深的子层，转发下去。
            for (var i = 0; i < embedRepeater.count; i++) {
                var item = embedRepeater.itemAt(i)
                var child = item ? item.childContent() : null
                if (child && child.onSceneDragDelta(dxQtScene, dyQtScene))
                    return true
            }
            return false
        }

        // Issue #822: 按下仲裁 —— 先超拖动阈值转 move，先到长按时间转 connect。
        // noteDragDelta 吃原始 Qt scene 像素，阈值才是屏幕口径，不随全局缩放变形。
        if (ic.pointerMode === "pressPending") {
            ic.noteDragDelta(dxQtScene, dyQtScene)
            if (ic.pressDragDistance >= ic.dragThreshold)
                promoteToMove(ic.pressKind, ic.pressId)
            return true
        }
        if (ic.pointerMode === "move") {
            // 拖动候选位置先夹回内容安全区：显示的就是最终写 Core 的那一份，
            // 节点/子星图不会被拖到圆外或压住父圆的标题/边框交互壳。
            var localDelta = qtSceneDeltaToLocal(dxQtScene, dyQtScene)
            var candidateX = ic.moveX + localDelta.x
            var candidateY = ic.moveY + localDelta.y
            var movingSize = movingItemSize()
            var clamped = clampToContentSafeArea(candidateX, candidateY,
                                                 movingSize.width, movingSize.height)
            ic.updateMove(clamped.x, clamped.y)
            return true
        }
        if (ic.pointerMode === "contextPending") {
            // 长按后拖动：端点存 scene 坐标，必须累加 root-world 增量；
            // 转 connect 的阈值继续吃原始屏幕像素，两个口径分开。
            var contextDelta = qtSceneDeltaToRootScene(dxQtScene, dyQtScene)
            ic.connectMouseX += contextDelta.x
            ic.connectMouseY += contextDelta.y
            ic.noteDragDelta(dxQtScene, dyQtScene)
            if (ic.pressDragDistance > ic.moveThreshold)
                ic.contextPendingToConnect()
            // 转入 connect 后预览端点立刻按"宿主可见形状"重算。
            refreshConnectPreview()
            return true
        }
        // connect：预览线终点是 scene 坐标，只能累加 root-world 增量；
        // 直接把屏幕像素加进 scene 坐标会随全局缩放漂移。
        var connectDelta = qtSceneDeltaToRootScene(dxQtScene, dyQtScene)
        ic.updateConnect(ic.connectMouseX + connectDelta.x,
                         ic.connectMouseY + connectDelta.y)
        // 预览起止点与正式边共用同一套可见端点（悬停合法 target 时贴到目标边界）。
        refreshConnectPreview()
        return true
    }

    // 松手统一出口：click / move / connect 都从这里闭环。
    // Issue #822 评论 5977278030：pinch（双指缩放）不属于单指业务，
    // 这里对它什么都不做——缩放结束由 Canvas 的 endPinch() 统一复位。
    function releaseOwnerGesture() {
        if (!interactionController)
            return
        var mode = interactionController.pointerMode
        if (mode === "connect") {
            finishConnect()
        } else if (mode === "move") {
            finishMove()
        } else if (mode === "contextPending") {
            finishContextPending()
        } else if (mode === "pressPending") {
            interactionController.cancelPressPending()
        }
    }

    function finishConnect() {
        var ic = interactionController
        var fromKind = ic.connectFromKind
        var fromId = ic.connectFromId
        var fromPath = ic.connectFromPath
        var hit = rootContent ? rootContent.hitTargetAtScene(ic.connectMouseX, ic.connectMouseY) : null
        var toPath = null
        var success = false
        var cancelled = true
        var hostPathKey = ""
        var hostStarmapId = ""
        if (hit && (hit.kind === "node" || hit.kind === "embed")) {
            var sameTarget = hit.scenePathKey === ic.connectFromScenePathKey && hit.id === fromId
            if (!sameTarget) {
                toPath = hit.targetPath
                // 宿主 = 两端所在 Scene 的最近公共祖先（照 Harmony 的规划规则）。
                // 边的 starmapId 必须等于宿主的 finalStarmapId，segments 只保留
                // "从宿主往下"的部分，Core 才能从宿主图自己走完。
                var plan = StarMapPathPlanner.planCrossLayerEdge(fromPath, toPath)
                var host = plan && rootContent
                        ? rootContent.findContentByPathSegments(plan.hostSegments)
                        : null
                plan = bindPlanToHost(plan, host)
                if (plan) {
                    hostPathKey = host.scenePathKey
                    hostStarmapId = host.finalStarmapId
                    success = host.commitEdgeWithPaths(plan.from, plan.to)
                    cancelled = !success
                }
            }
        }
        logInteraction("connect_end", fromKind, fromId, {
            "fromPath": JSON.stringify(fromPath),
            "toPath": toPath ? JSON.stringify(toPath) : "",
            "hostPathKey": hostPathKey,
            "hostStarmapId": hostStarmapId,
            "success": success,
            "cancel": cancelled
        })
        ic.endConnect()
        if (menuHost)
            menuHost.hideTouchPreview()
    }

    function finishMove() {
        var ic = interactionController
        var nodeId = ic.pressedNodeId
        var embedId = ic.pressedEmbedId
        var nx = ic.moveX
        var ny = ic.moveY
        var committed = false
        if (nodeId !== "") {
            committed = graphController.commitNodeMove(nodeId, nx, ny)
            logInteraction("move_end", "node", nodeId, {
                "toX": nx, "toY": ny, "commitSuccess": committed, "device": "mouse"
            })
        } else if (embedId !== "") {
            committed = graphController.commitEmbedMove(embedId, nx, ny)
            logInteraction("move_end", "embed", embedId, {
                "toX": nx, "toY": ny, "commitSuccess": committed, "device": "mouse"
            })
        }
        ic.endMove()
        refreshEdges()
    }

    // 触屏长按后没移动就松手：关闭视觉层，弹出真正可点击的菜单。
    function finishContextPending() {
        var ic = interactionController
        var kind = ic.connectFromKind
        var id = ic.connectFromId
        ic.endContextPending()
        var item = kind === "node" ? graphController.getNode(id) : graphController.getEmbed(id)
        if (!item)
            return
        var center = localToScene(item.x + item.width / 2, item.y + item.height / 2)
        if (menuHost)
            menuHost.showLongPressMenu(kind, id, center.x, center.y, content)
    }

    // ---------------------------------------------------------------------------
    // 节点内联编辑：双击/右键"编辑"直接进入节点自身的 TextInput，
    // 提交后回写本层 GraphController，不再经过外部 Inspector Popup。
    // ---------------------------------------------------------------------------
    function beginInlineEdit(nodeId) {
        // Issue #822 评论 5977325046：pinch 接管期间拒绝迟到的双击进入编辑。
        if (interactionController && interactionController.pointerMode === "pinch")
            return
        var it = nodeItemOf(nodeId)
        if (it)
            it.beginEdit()
    }
    function commitNodeTitle(nodeId, title) {
        graphController.updateNode(nodeId, { title: title })
    }

    // ---------------------------------------------------------------------------
    // 右键空白新建：菜单归属层就是被点中的那一层，坐标在这里换算成局部坐标。
    // ---------------------------------------------------------------------------
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

    // 菜单归属层用：命名 → 换算到本层局部坐标 → 找不压旧节点的落点 →
    // 夹回内容安全区（同一个 clamp）→ 写 Core。
    function createNodeWithName(name, sceneX, sceneY) {
        var p = sceneToLocal(sceneX, sceneY)
        var spawn = findFreeSpawnPoint(p.x, p.y)
        var safe = clampToContentSafeArea(spawn.x, spawn.y, _newNodeWidth, _newNodeHeight)
        graphController.createNode(name, safe.x, safe.y)
    }
    function createSubStarmapWithName(name, sceneX, sceneY) {
        var p = sceneToLocal(sceneX, sceneY)
        var spawn = findFreeSpawnPoint(p.x, p.y)
        var diameter = graphController._embedDiameter
        var safe = clampToContentSafeArea(spawn.x, spawn.y, diameter, diameter)
        graphController.createSubStarmapAt(name, safe.x, safe.y)
    }

    function updateNodeTitle(nodeId, title) { graphController.updateNode(nodeId, { title: title }) }
    function deleteNode(nodeId) { graphController.deleteNode(nodeId) }
    function updateEdgeLabel(edgeId, label) { graphController.updateEdge(edgeId, { label: label }) }
    function deleteEdge(edgeId) { graphController.deleteEdge(edgeId) }
    function updateEmbedLabel(instanceId, label) { graphController.updateEmbed(instanceId, { label: label }) }
    function deleteEmbed(instanceId) { graphController.deleteEmbed(instanceId) }
    function selectNode(nodeId) { graphController.selectNode(nodeId) }
    function selectEdge(edgeId) { graphController.selectEdge(edgeId) }
    function selectEmbed(instanceId) { graphController.selectEmbed(instanceId) }
    function clearLayerSelection() { graphController.clearSelection() }

    function isMovingNode(id) {
        if (!interactionController)
            return false
        return interactionController.pointerMode === "move"
                && interactionController.moveScenePathKey === scenePathKey
                && interactionController.pressedNodeId === id
    }
    function isMovingEmbed(instanceId) {
        if (!interactionController)
            return false
        return interactionController.pointerMode === "move"
                && interactionController.moveScenePathKey === scenePathKey
                && interactionController.pressedEmbedId === instanceId
    }

    // ---------------------------------------------------------------------------
    // 连线重绘：本层 edgeRenders 只画本层连线；瞬时 move 期间用共享状态机做 override。
    // ---------------------------------------------------------------------------
    function currentMoveOverride() {
        if (!interactionController)
            return null
        var ic = interactionController
        if (ic.pointerMode !== "move" || ic.moveScenePathKey !== scenePathKey)
            return null
        if (ic.pressedNodeId !== "") {
            return { kind: "node", id: ic.pressedNodeId, x: ic.moveX, y: ic.moveY }
        }
        if (ic.pressedEmbedId !== "") {
            return { kind: "embed", id: ic.pressedEmbedId, x: ic.moveX, y: ic.moveY }
        }
        return null
    }

    function refreshEdges() {
        graphController.computeEdgeRenders(currentMoveOverride())
        edgeCanvas.requestPaint()
        previewCanvas.requestPaint()
    }

    // 整棵递归树回到 canonical 连线位置（切图/失焦/reset 时用）。
    function refreshAllEdges() {
        graphController.computeEdgeRenders(null)
        edgeCanvas.requestPaint()
        for (var i = 0; i < embedRepeater.count; i++) {
            var child = embedRepeater.itemAt(i)
            var inner = child ? child.childContent() : null
            if (inner)
                inner.refreshAllEdges()
        }
    }

    Connections {
        target: content.interactionController
        function onMoveXChanged() { if (content.currentMoveOverride()) content.refreshEdges() }
        function onMoveYChanged() { if (content.currentMoveOverride()) content.refreshEdges() }
        function onPointerModeChanged() { content.refreshAllEdges() }
        // Issue #822: 长按计时到只由共享状态机发信号，
        // 状态提升由"归属层"完成，避免每层都去改全局状态。
        function onPressTimeout() { content.onPressTimeout() }
    }

    // Issue #814 评论 5935346839: 单层交互边界日志入口，不进连续移动热路径。
    function logInteraction(event, itemKind, itemId, fields) {
        if (!starmapBackendRef)
            return
        var fj = fields ? JSON.stringify(fields) : ""
        starmapBackendRef.record_interaction(event, scenePathKey, finalStarmapId, itemKind, itemId, fj)
    }

    // 本层局部适配变化时，preview 静态投影也要重画。
    onRenderDetailChanged: previewCanvas.requestPaint()

    // ── 本层内容：worldLayer 承载每层 local fit ──
    // 显示变换（x/y/scale）只在这一层；节点/连线仍然用 authored 坐标摆放。
    // 根层是相机本体，fit 恒等；子层 fit 由内容包围盒 + 本层容器算出。
    Item {
        id: worldLayer
        width: content.width
        height: content.height
        x: content.localFitOffsetX
        y: content.localFitOffsetY
        scale: content.localFitScale
        transformOrigin: Item.TopLeft

        // ── 本层连线 ──
        Canvas {
            id: edgeCanvas
            anchors.fill: parent
            visible: content.renderDetail !== "preview"
            z: 0

            Connections {
                target: content.selectionController
                function onScenePathKeyChanged() { edgeCanvas.requestPaint() }
                function onKindChanged() { edgeCanvas.requestPaint() }
                function onItemIdChanged() { edgeCanvas.requestPaint() }
            }

            onPaint: {
                var ctx = getContext("2d")
                ctx.clearRect(0, 0, width, height)
                // 线宽常量与命中阈值同源：命中至少覆盖真实可见的线本体。
                ctx.lineWidth = content._edgeLineWorldWidth
                var renders = graphController.edgeRenders
                for (var i = 0; i < renders.length; i++) {
                    var r = renders[i]
                    var edge = null
                    for (var ei = 0; ei < graphController.edgesModel.length; ei++) {
                        if (graphController.edgesModel[ei].id === r.edgeId) {
                            edge = graphController.edgesModel[ei]
                            break
                        }
                    }
                    if (!edge)
                        continue
                    var edgeSelected = selectionController
                            ? selectionController.matches(scenePathKey, "edge", edge.id)
                            : false
                    var color = edgeSelected ? _accent : _border

                    ctx.beginPath()
                    ctx.moveTo(r.startX, r.startY)
                    ctx.lineTo(r.endX, r.endY)
                    ctx.strokeStyle = color
                    ctx.stroke()

                    ctx.beginPath()
                    ctx.moveTo(r.arrowTipX, r.arrowTipY)
                    ctx.lineTo(r.arrowLeftX, r.arrowLeftY)
                    ctx.lineTo(r.arrowRightX, r.arrowRightY)
                    ctx.closePath()
                    ctx.fillStyle = color
                    ctx.fill()

                    if (edge.label) {
                        ctx.fillStyle = _surfaceContainer
                        var tw = ctx.measureText(edge.label).width
                        ctx.fillRect(r.labelX - tw / 2 - 4, r.labelY - 10, tw + 8, 20)
                        ctx.fillStyle = _textPrimary
                        ctx.font = "12px sans-serif"
                        ctx.textAlign = "center"
                        ctx.textBaseline = "middle"
                        ctx.fillText(edge.label, r.labelX, r.labelY)
                    }
                }
            }
        }

        // ── preview 档的静态投影 ──
        // 和 interactive 档是同一张图：节点/子星图还是本层的矩形和同一套颜色，
        // 只省掉交互组件和文字。投影太小时不挂任何手势，也不往里递归。
        Canvas {
            id: previewCanvas
            anchors.fill: parent
            visible: content.renderDetail === "preview"
            z: 2

            onPaint: {
                var ctx = getContext("2d")
                ctx.clearRect(0, 0, width, height)

                // 和 interactive 档同一种形状：圆角矩形（半径取同一个 token）。
                function roundedRectPath(x, y, w, h, radius) {
                    var rr = Math.max(0, Math.min(radius, w / 2, h / 2))
                    ctx.beginPath()
                    ctx.moveTo(x + rr, y)
                    ctx.lineTo(x + w - rr, y)
                    ctx.arcTo(x + w, y, x + w, y + rr, rr)
                    ctx.lineTo(x + w, y + h - rr)
                    ctx.arcTo(x + w, y + h, x + w - rr, y + h, rr)
                    ctx.lineTo(x + rr, y + h)
                    ctx.arcTo(x, y + h, x, y + h - rr, rr)
                    ctx.lineTo(x, y + rr)
                    ctx.arcTo(x, y, x + rr, y, rr)
                    ctx.closePath()
                }

                var renders = graphController.edgeRenders
                ctx.lineWidth = content._edgeLineWorldWidth
                ctx.strokeStyle = content._border
                for (var i = 0; i < renders.length; i++) {
                    var r = renders[i]
                    ctx.beginPath()
                    ctx.moveTo(r.startX, r.startY)
                    ctx.lineTo(r.endX, r.endY)
                    ctx.stroke()
                }

                var embeds = graphController.embedsModel
                for (var j = 0; j < embeds.length; j++) {
                    var e = embeds[j]
                    // 子星图身份是正圆：和 interactive 档同一形状、同一颜色，
                    // 不因为掉档就从圆变成长方形卡片。
                    ctx.beginPath()
                    ctx.arc(e.x + e.width / 2, e.y + e.height / 2,
                            Math.min(e.width, e.height) / 2, 0, 2 * Math.PI)
                    ctx.fillStyle = content._accentSoft
                    ctx.strokeStyle = content._border
                    ctx.fill()
                    ctx.stroke()
                }

                var nodes = graphController.nodesModel
                for (var k = 0; k < nodes.length; k++) {
                    var n = nodes[k]
                    roundedRectPath(n.x, n.y, n.width, n.height, content._radiusSm)
                    ctx.fillStyle = content._surfaceContainer
                    ctx.strokeStyle = content._border
                    ctx.fill()
                    ctx.stroke()
                }
            }
        }

        // ── 本层节点与子星图（只属于 interactive 档）──
        Item {
            id: nodeLayer
            anchors.fill: parent
            z: 1

            Repeater {
                id: nodeRepeater
                // preview 档只由 previewCanvas 画静态投影，不实例化交互 delegate。
                model: content.renderDetail === "interactive" ? graphController.nodesModel : []
                delegate: StarMapNode {
                    required property var modelData
                    required property int index
                    dt: content.dt
                    property var nodeData: modelData
                    readonly property string nodeId: nodeData.id

                    // 本层局部坐标，不再经过 worldToScreen：相机在祖先 Content 上。
                    x: content.isMovingNode(nodeData.id) ? content.interactionController.moveX : nodeData.x
                    y: content.isMovingNode(nodeData.id) ? content.interactionController.moveY : nodeData.y
                    width: nodeData.width
                    height: nodeData.height
                    title: nodeData.title
                    isSelected: content.selectionController
                            ? content.selectionController.matches(content.scenePathKey, "node", nodeData.id)
                            : false
                    wobbleIndex: index
                    // Issue #822 评论 5977714294：Pinch 接管期间由归属层直接禁用
                    // delegate 的触屏 TapHandler（passive grab 的 tap 识别当场取消），
                    // 不再只靠回调时判断 —— Pinch 结束后的迟到 singleTapped 挡不住。
                    touchGestureBlocked: content.menuHost
                            ? content.menuHost.pinchOwnsTouchGesture()
                            : (content.interactionController
                               && content.interactionController.pointerMode === "pinch")

                    onMouseInteracted: {
                        if (content.menuHost) content.menuHost.noteMouseInteracted()
                    }

                    // 鼠标按下只登记归属（pressPending），不决定 move 还是 connect。
                    onItemPressed: function(qx, qy) {
                        content.onItemPressed("node", nodeData.id, content.nodePath(nodeData.id), qx, qy)
                    }

                    // 单击选中：TapHandler 的点击语义只上抛信号，归属层负责选中和边界日志
                    // （鼠标 / 触屏两个 TapHandler 共用，device 由 pointer_press 边界日志给出）。
                    onSingleClicked: {
                        content.selectNode(nodeData.id)
                        content.logInteraction("selection_changed", "node", nodeData.id, {})
                    }

                    onTouchLongPressed: {
                        content.onItemTouchLongPressed("node", nodeData.id, content.nodePath(nodeData.id))
                    }

                    onMoveDelta: function(dx, dy) {
                        content.onItemDragDelta("node", nodeData.id, dx, dy)
                    }

                    onLeftReleased: content.releaseOwnerGesture()

                    // Issue #822：节点自身就是编辑器，双击直接把光标放进节点框。
                    onDoubleClicked: content.beginInlineEdit(nodeData.id)

                    onTitleCommitted: function(newTitle) {
                        content.commitNodeTitle(nodeData.id, newTitle)
                    }
                }
            }

            Repeater {
                id: embedRepeater
                // preview 档不实例化 Embed delegate，子星图也不再往里递归。
                model: content.renderDetail === "interactive" ? graphController.embedsModel : []
                delegate: StarMapEmbed {
                    required property var modelData
                    required property int index
                    dt: content.dt
                    property var embedData: modelData

                    x: content.isMovingEmbed(embedData.instanceId) ? content.interactionController.moveX : embedData.x
                    y: content.isMovingEmbed(embedData.instanceId) ? content.interactionController.moveY : embedData.y
                    width: embedData.width
                    height: embedData.height
                    instanceId: embedData.instanceId
                    targetStarmapId: embedData.targetStarmapId
                    label: embedData.label
                    isSelected: content.selectionController
                            ? content.selectionController.matches(content.scenePathKey, "embed", embedData.instanceId)
                            : false
                    wobbleIndex: index
                    // Issue #822 评论 5977714294：与 Node 同一套让位规则 ——
                    // Pinch 激活期间由归属层直接禁用 chrome 的触屏 TapHandler。
                    touchGestureBlocked: content.menuHost
                            ? content.menuHost.pinchOwnsTouchGesture()
                            : (content.interactionController
                               && content.interactionController.pointerMode === "pinch")

                    // Issue #822：子星图显示档位只由 ownerEffectiveScale（全局相机 ×
                    // 祖先 local fit）+ 根视口短边算出；Embed 外壳的 world 几何恒定，
                    // 不再用 scale 改整颗 Embed，档位只决定子内容渲染多少细节。
                    ownerSceneContent: content
                    ownerEffectiveScale: content.effectiveScale
                    viewportShortSide: content.viewportShortSide

                    rootStarmapId: content.rootStarmapId
                    parentPathSegments: content.pathSegments
                    starmapBackendRef: content.starmapBackendRef
                    parentPathKey: content.scenePathKey
                    contentDepth: content.depth + 1
                    // 递归加载路径段由 Controller 统一分流：正式 Embed → enterEmbed，
                    // 旧 portal 归一 → enterPortal{nodeId}。Embed 不再自己猜路径。
                    pathSegment: graphController.embedPathSegment(embedData.instanceId)
                    selectionController: content.selectionController
                    interactionController: content.interactionController
                    rootContent: content.rootContent
                    menuHost: content.menuHost

                    onMouseInteracted: {
                        if (content.menuHost) content.menuHost.noteMouseInteracted()
                    }

                    onItemPressed: function(qx, qy) {
                        content.onItemPressed("embed", embedData.instanceId,
                                              content.embedPath(embedData.instanceId), qx, qy)
                    }

                    // 单击选中：Embed chrome 的 TapHandler 只上抛 clicked，
                    // 归属层负责选中和边界日志（鼠标 / 触屏共用）。
                    onClicked: function(instId) {
                        content.selectEmbed(instId)
                        content.logInteraction("selection_changed", "embed", instId, {})
                    }

                    onTouchLongPressed: {
                        content.onItemTouchLongPressed("embed", embedData.instanceId,
                                                       content.embedPath(embedData.instanceId))
                    }

                    onMoveDelta: function(dx, dy) {
                        content.onItemDragDelta("embed", embedData.instanceId, dx, dy)
                    }

                    onLeftReleased: content.releaseOwnerGesture()
                }
            }
        }
    }

    // ── 解析状态提示 ──
    AppText {
        dt: content.dt
        anchors.centerIn: parent
        visible: content.resolveError.length > 0
        text: content.resolveError
        color: content._error
        font.pointSize: content.dt.fontSmPt
        wrapMode: Text.Wrap
        horizontalAlignment: Text.AlignHCenter
    }

    AppText {
        dt: content.dt
        anchors.centerIn: parent
        visible: content.rootStarmapId !== "" && content.finalStarmapId === "" && content.resolveError === ""
        text: qsTr("加载中…")
        color: content._textMuted
        font.pointSize: content.dt.fontSmPt
    }
}
