// =============================================================================
// StarMapSceneContent.qml — 子星图微缩概览（纯快照投影）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：把一份后台构建好的子星图预览快照，按 fit-to-bounds 投影成父图中
//   圆形入口内的微缩概览。它只回答一个问题："里面大概有什么"。
//
// Issue #1032：本组件不再在 Loader/Component.onCompleted 生命周期里同步读取
//   Core、实测文本或计算边几何。数据入口只有 `snapshot`。
//
// Issue #1035：快照节点 / 入口 / 连线的局部几何只在 snapshot 变化时生成一次，
//   全部保存在同一份 child world 坐标里；父图缩放只更新一个预览根变换和
//   原生对象层的相机矩阵，不重建 PathSvg，也不重新排版文字。
//
// Issue #1057：父画布的子星图圆形入口内部只表示"里面大概有什么"，不画完整
//   节点卡片和文字。节点 / 更深层入口统一由 StarMapObjectLayer 以固定屏幕
//   3px 半径圆点呈现（reveal: 0，顶点着色器 mix 到圆点半径），子图内部连线
//   用 1px cosmeticStroke，屏幕线宽不随 fitScale 变化。圆内不再实例化任何
//   文字卡片。
// =============================================================================

import QtQuick
import QtQuick.Shapes
import Sujian 1.0
import "StarMapNodeMetrics.js" as StarMapNodeMetrics

Item {
    id: overview

    required property var dt

    // 后台预览快照：{ starmapId, graphRevision, nodes, embeds, edges, bounds }。
    property var snapshot: null

    // Issue #949 保留该属性给视口过渡传值；#1009 起它不再切换任何形状。
    property real detailReveal: 1.0

    readonly property var nodes: snapshot && snapshot.nodes ? snapshot.nodes : []
    readonly property var embeds: snapshot && snapshot.embeds ? snapshot.embeds : []
    readonly property var edgeRenders: snapshot && snapshot.edges ? snapshot.edges : []
    readonly property var childBounds: snapshot ? snapshot.bounds : null
    readonly property string loadError: snapshot ? "" : "preview_not_ready"

    readonly property real _minWorldX: childBounds ? childBounds.minX : 0
    readonly property real _minWorldY: childBounds ? childBounds.minY : 0
    readonly property real _childWorldWidth: childBounds
            ? Math.max(0, childBounds.maxX - childBounds.minX) : 0
    readonly property real _childWorldHeight: childBounds
            ? Math.max(0, childBounds.maxY - childBounds.minY) : 0

    // 子图 world 单位 → 概览局部像素。没有内容时返回 0，不画任何投影。
    readonly property real fitScale:
        StarMapNodeMetrics.embedFitScale(childBounds, Math.min(width, height))
    readonly property bool hasContent: fitScale > 0

    // 概览唯一的缩放 / 居中入口：父图相机变化只改这里的 x/y 和根节点矩阵，
    // 不改任何子对象尺寸、描边或文字排版。
    readonly property real _contentX:
        (overview.width - overview._childWorldWidth * overview.fitScale) / 2
    readonly property real _contentY:
        (overview.height - overview._childWorldHeight * overview.fitScale) / 2

    function localX(wx) { return wx - _minWorldX }
    function localY(wy) { return wy - _minWorldY }

    // ── snapshot 局部几何：只依赖快照，不依赖父图相机 / 入口直径 ──
    readonly property string edgesPath: {
        var s = ""
        for (var i = 0; i < edgeRenders.length; i++) {
            var r = edgeRenders[i]
            s += "M " + localX(r.startX) + " " + localY(r.startY)
                    + " L " + localX(r.endX) + " " + localY(r.endY) + " "
        }
        return s
    }

    // 圆内只画圆点与连线，这里只保留定位 / 尺寸字段，不再为文字构造内容。
    readonly property var localNodes: {
        var out = []
        for (var i = 0; i < nodes.length; i++) {
            var n = nodes[i]
            out.push({
                id: n.id,
                x: localX(n.x), y: localY(n.y),
                width: n.width, height: n.height
            })
        }
        return out
    }

    readonly property var localEmbeds: {
        var out = []
        for (var i = 0; i < embeds.length; i++) {
            var e = embeds[i]
            out.push({
                id: e.instanceId,
                x: localX(e.x), y: localY(e.y),
                diameter: e.diameter
            })
        }
        return out
    }

    // 原生对象层只接受稳定的局部 world 矩形；fitScale 变化不会重新序列化。
    readonly property string objectNodesJson: {
        var out = []
        for (var i = 0; i < localNodes.length; i++) {
            var n = localNodes[i]
            out.push({
                id: n.id, x: n.x, y: n.y,
                width: n.width, height: n.height
            })
        }
        return JSON.stringify(out)
    }

    readonly property string objectEmbedsJson: {
        var out = []
        for (var i = 0; i < localEmbeds.length; i++) {
            var e = localEmbeds[i]
            out.push({
                instanceId: e.id, x: e.x, y: e.y,
                width: e.diameter, height: e.diameter
            })
        }
        return JSON.stringify(out)
    }

    Shape {
        x: overview._contentX
        y: overview._contentY
        width: overview._childWorldWidth
        height: overview._childWorldHeight
        visible: overview.hasContent
        antialiasing: true
        preferredRendererType: Shape.CurveRenderer
        z: 0

        transform: Scale {
            origin.x: 0
            origin.y: 0
            xScale: overview.fitScale
            yScale: overview.fitScale
        }

        ShapePath {
            strokeColor: overview.dt.border
            // 屏幕固定 1px 纤细连线；路径与描边参数都不依赖 fitScale。
            strokeWidth: 1
            cosmeticStroke: true
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            PathSvg { path: overview.edgesPath }
        }
    }

    StarMapObjectLayer {
        anchors.fill: parent
        visible: overview.hasContent
        nodesJson: overview.objectNodesJson
        embedsJson: overview.objectEmbedsJson
        selectedKind: ""
        selectedId: ""
        zoomLevel: overview.fitScale
        panX: overview._contentX
        panY: overview._contentY
        anchorX: 0
        anchorY: 0
        // Issue #1057：圆内只画 3px 半径（约 6px 直径）圆点，顶点着色器用
        // mix(dotRadius, originalHalfSize, reveal) 把完整卡片轮廓收成圆点，
        // 节点再多也不实例化文本卡片。
        reveal: 0
        revealDotRadiusPx: 3
        cornerRadius: 6
        nodeFillHex: String(overview.dt.surfaceContainer)
        nodeBorderHex: String(overview.dt.border)
        selectedBorderHex: String(overview.dt.border)
        embedFillHex: String(overview.dt.accentSoft)
        embedBorderHex: String(overview.dt.border)
        nodeBorderPx: 1
        nodeSelectedBorderPx: 1
        embedBorderPx: 1
        embedSelectedBorderPx: 1
        z: 1
    }
}
