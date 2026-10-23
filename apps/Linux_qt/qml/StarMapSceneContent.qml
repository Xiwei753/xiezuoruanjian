// =============================================================================
// StarMapSceneContent.qml — 子星图微缩概览（fit-to-bounds 预览投影）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：把「一张星图」的真实节点 / 边 / 子入口按 fit-to-bounds 投影成父图中
//   圆形入口内的微缩概览。它只回答一个问题："里面大概有什么"。
//
// Issue #929：独立画布协议下，父图不再递归渲染子画布的可交互内容。
//   - 本组件不是视口、不是画布：没有 pan/zoom/相机属性，也没有任何手势、
//     没有节点拖拽、没有局部相机、不往里递归 Loader；
//   - fit 只属于这份「父图的微缩概览投影」：真实子图的 authored 坐标不变，
//     也没有 localFitScale × ancestor effectiveScale 的递归尺寸链；
//   - 概览整体留在圆的内接安全区（约直径 66%），天然不会溢出圆外，
//     不需要 layer.enabled / MultiEffect 圆形 FBO 遮罩 / ShaderEffectSource；
//   - 未进入的子图中，普通节点、旧 portal、Embed 都统一显示为小圆点且不画名字；
//     进入后由活动画布恢复完整几何和标题；
//   - 不保存任何固定分辨率快照：父图相机变化时按当前投影重画几何。
//     超过进入阈值后由 StarMapActiveScene 用真实矢量对象接管，本组件不会
//     被拉伸成活动画布。
//
// 约束：
//   - 纯 UI 渲染，不调 Core 写入接口，不创建子画布
//   - 不引用 StarMapEmbed / StarMapActiveScene，避免编译期类型环
// =============================================================================

import QtQuick
import QtQuick.Shapes
import "StarMapNodeMetrics.js" as StarMapNodeMetrics

Item {
    id: overview

    required property var dt

    // 被概览的子星图 ID（由父 Embed 传入；空串 = 不画）。
    property string starmapId: ""
    property var starmapBackendRef: null

    // 概览包围盒最大边 = 短边 × 该比例。内接正方形是 1/√2 ≈ 0.707，
    // 取 0.66 留出边距，概览不贴到圆边。
    readonly property real _safeRatio: 0.66
    // Issue #984：普通节点宽高统一由 StarMapNodeMetrics 度量（读原始
    // graphData 节点的 title/content），这里不再保留第二套固定几何。
    readonly property int _embedDiameter: 200
    // 概览线宽的屏幕口径：概览是父图投影的一部分，线宽恒定在屏幕像素上。
    readonly property real _previewStrokeWidth: 1.5

    // Issue #949：展开进度（只影响显示层，不改 authored 坐标 / 图关系）。
    // 1 = 概览本来样子；过渡期间与活动画布同一个 progress，圆点在两层里同步
    // 长大 / 收回，圆点半径公式与活动画布 _revealDotRadius 同一份。
    property real detailReveal: 1.0
    readonly property real _overviewDotRadius:
        Math.max(2, Math.min(6, Math.min(width, height) * 0.018))
        * Math.max(0, Math.min(1, detailReveal))

    // ── 目标图数据（只读快照，不写 Core）──
    property var graphData: null
    property var edgeRenders: []
    // Issue #984：整张子图的节点布局快照（含未打开的子图，由真实文本排版
    // 测量），概览 bounds / 端点 / 圆点共用这一份，不再回退到固定高度。
    property var layoutSizes: ({})
    property string loadError: ""

    StarMapNodeMeasurer {
        id: nodeMeasurer
    }

    readonly property var nodes: graphData && graphData.nodes ? graphData.nodes : []
    readonly property var embeds: graphData && graphData.embeds ? graphData.embeds : []

    // ── fit-to-bounds：真实 authored 坐标 → 概览局部像素 ──
    readonly property var childBounds: {
        var minX = 0
        var minY = 0
        var maxX = 0
        var maxY = 0
        var has = false
        var i
        for (i = 0; i < nodes.length; i++) {
            var n = nodes[i]
            var np = n.position || { x: 0, y: 0 }
            var isPortal = n.portal && n.portal.destinationStarmapId
            // Issue #984：普通节点读原始 graphData 节点的 content 度量，
            // 旧 portal 仍按 Embed 直径处理。
            var size = StarMapNodeMetrics.sizeFor(layoutSizes, n)
            var nw = isPortal ? _embedDiameter : size.width
            var nh = isPortal ? _embedDiameter : size.height
            if (!has) {
                minX = np.x; minY = np.y
                maxX = np.x + nw; maxY = np.y + nh
                has = true
            } else {
                minX = Math.min(minX, np.x); minY = Math.min(minY, np.y)
                maxX = Math.max(maxX, np.x + nw); maxY = Math.max(maxY, np.y + nh)
            }
        }
        for (i = 0; i < embeds.length; i++) {
            var e = embeds[i]
            var ep = e.position || { x: 0, y: 0 }
            if (!has) {
                minX = ep.x; minY = ep.y
                maxX = ep.x + _embedDiameter; maxY = ep.y + _embedDiameter
                has = true
            } else {
                minX = Math.min(minX, ep.x); minY = Math.min(minY, ep.y)
                maxX = Math.max(maxX, ep.x + _embedDiameter)
                maxY = Math.max(maxY, ep.y + _embedDiameter)
            }
        }
        if (!has)
            return null
        return { minX: minX, minY: minY, maxX: maxX, maxY: maxY,
                 width: maxX - minX, height: maxY - minY }
    }

    readonly property real _centerWorldX: childBounds
            ? (childBounds.minX + childBounds.maxX) / 2 : 0
    readonly property real _centerWorldY: childBounds
            ? (childBounds.minY + childBounds.maxY) / 2 : 0

    // 子图 world 单位 → 概览局部像素（这就是"进入该子图"时的子画布比例）。
    // 没有内容时返回 0，不画任何投影。
    readonly property real fitScale: {
        var b = childBounds
        var side = Math.min(width, height) * _safeRatio
        if (!b || !(side > 0) || !(b.width > 0) || !(b.height > 0))
            return 0
        return side / Math.max(b.width, b.height)
    }

    function projectX(wx) { return width / 2 + (wx - _centerWorldX) * fitScale }
    function projectY(wy) { return height / 2 + (wy - _centerWorldY) * fitScale }

    // ── 数据加载：只读入口，不写 Core ──
    function _layoutNodes() {
        var out = []
        for (var i = 0; i < nodes.length; i++) {
            var n = nodes[i]
            var pos = n.position || { x: 0, y: 0 }
            // 旧 portal 归一到 Embed 几何，和活动画布同一口径。
            // Issue #984：普通节点与 StarMapGraphController.buildModels 共用度量。
            var isPortal = n.portal && n.portal.destinationStarmapId
            out.push({
                id: n.id,
                x: pos.x, y: pos.y,
                width: isPortal ? _embedDiameter
                                : StarMapNodeMetrics.sizeFor(layoutSizes, n).width,
                height: isPortal ? _embedDiameter
                                 : StarMapNodeMetrics.sizeFor(layoutSizes, n).height
            })
        }
        return out
    }

    function _layoutEmbeds() {
        var out = []
        for (var i = 0; i < embeds.length; i++) {
            var e = embeds[i]
            var pos = e.position || { x: 0, y: 0 }
            out.push({
                instanceId: e.instanceId,
                x: pos.x, y: pos.y,
                width: _embedDiameter, height: _embedDiameter
            })
        }
        return out
    }

    // 边渲染仍走 Core 的唯一几何真相（starmap_view::compute_edge_renders），
    // 概览不另写一套端点 / 箭头 / 偏移算法。
    function reload() {
        if (!starmapBackendRef || starmapId === "") {
            graphData = null
            edgeRenders = []
            loadError = ""
            return
        }
        var res = starmapBackendRef.get_starmap_graph(starmapId)
        if (!res || res.success !== true || !res.data || !res.data.graph) {
            graphData = null
            edgeRenders = []
            loadError = res && res.errorCode ? res.errorCode : "load_failed"
            return
        }
        graphData = res.data.graph
        layoutSizes = StarMapNodeMetrics.snapshotFor(starmapId, nodes, nodeMeasurer.measureNode)
        loadError = ""
        var renderRes = starmapBackendRef.compute_edge_renders(
                    JSON.stringify(graphData),
                    JSON.stringify(_layoutNodes()),
                    JSON.stringify(_layoutEmbeds()))
        edgeRenders = (renderRes && renderRes.success === true && renderRes.data)
                ? renderRes.data : []
    }

    onStarmapIdChanged: reload()
    onStarmapBackendRefChanged: reload()
    Component.onCompleted: reload()

    // ── 单一投影入口画出的矢量几何（QtQuick.Shapes）──
    // 全是实时的几何路径，没有 layer / FBO / 快照纹理：父图相机一变，
    // 下面的绑定就按新投影重算路径字符串。
    function _circlePath(cx, cy, r) {
        return "M " + (cx - r) + " " + cy
                + " A " + r + " " + r + " 0 1 0 " + (cx + r) + " " + cy
                + " A " + r + " " + r + " 0 1 0 " + (cx - r) + " " + cy + " Z "
    }

    readonly property string edgesPath: {
        var s = ""
        if (!(fitScale > 0))
            return s
        for (var i = 0; i < edgeRenders.length; i++) {
            var r = edgeRenders[i]
            s += "M " + projectX(r.startX) + " " + projectY(r.startY)
                    + " L " + projectX(r.endX) + " " + projectY(r.endY) + " "
        }
        return s
    }

    readonly property string nodesPath: {
        var s = ""
        if (!(fitScale > 0))
            return s
        for (var i = 0; i < nodes.length; i++) {
            var n = nodes[i]
            var pos = n.position || { x: 0, y: 0 }
            if (n.portal && n.portal.destinationStarmapId)
                continue
            var dotSize = StarMapNodeMetrics.sizeFor(layoutSizes, n)
            s += _circlePath(projectX(pos.x + dotSize.width / 2),
                             projectY(pos.y + dotSize.height / 2),
                             _overviewDotRadius)
        }
        return s
    }

    readonly property string embedsPath: {
        var s = ""
        if (!(fitScale > 0))
            return s
        for (var i = 0; i < nodes.length; i++) {
            var n = nodes[i]
            var npos = n.position || { x: 0, y: 0 }
            if (!(n.portal && n.portal.destinationStarmapId))
                continue
            s += _circlePath(projectX(npos.x + _embedDiameter / 2),
                             projectY(npos.y + _embedDiameter / 2), _overviewDotRadius)
        }
        for (var j = 0; j < embeds.length; j++) {
            var e = embeds[j]
            var pos = e.position || { x: 0, y: 0 }
            s += _circlePath(projectX(pos.x + _embedDiameter / 2),
                             projectY(pos.y + _embedDiameter / 2), _overviewDotRadius)
        }
        return s
    }

    z: 0

    Shape {
        anchors.fill: parent
        antialiasing: true
        preferredRendererType: Shape.CurveRenderer

        // 边：和活动画布同一份 Core edge render 几何（直线段）。
        ShapePath {
            strokeColor: overview.dt.border
            strokeWidth: overview._previewStrokeWidth
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            PathSvg { path: overview.edgesPath }
        }

        // 未进入的子入口：和普通节点一样只显示小圆点。
        ShapePath {
            strokeColor: overview.dt.border
            strokeWidth: overview._previewStrokeWidth
            fillColor: overview.dt.accentSoft
            PathSvg { path: overview.embedsPath }
        }

        // 普通节点：未进入的子图中只显示小圆点。
        ShapePath {
            strokeColor: overview.dt.border
            strokeWidth: overview._previewStrokeWidth
            fillColor: overview.dt.surfaceContainer
            PathSvg { path: overview.nodesPath }
        }
    }

}
