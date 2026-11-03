// =============================================================================
// StarMapSceneContent.qml — 子星图微缩概览（纯快照投影）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：把一份后台构建好的子星图预览快照，按 fit-to-bounds 投影成父图中
//   圆形入口内的微缩概览。它只回答一个问题："里面大概有什么"。
//
// Issue #1032：本组件不再在 Loader/Component.onCompleted 生命周期里同步读取
//   Core、实测文本或计算边几何。数据入口只有 `snapshot`：同一
//   (targetStarmapId, graphRevision) 的多个入口共享同一份不可变快照；相机
//   缩放 / 平移只改变下面的 fitScale 与投影，不会重新读图。
//
// 约束：
//   - 纯 UI 渲染，不调 Core / backend，不创建子画布
//   - 不引用 StarMapEmbed / StarMapActiveScene，避免编译期类型环
//   - 仍是真实矢量节点 / 边 / 入口投影，不做预栅格化
// =============================================================================

import QtQuick
import QtQuick.Shapes
import "StarMapNodeMetrics.js" as StarMapNodeMetrics

Item {
    id: overview

    required property var dt

    // 后台预览快照：{ starmapId, graphRevision, nodes, embeds, edges, bounds }。
    property var snapshot: null

    // Issue #949 保留该属性给视口过渡传值；#1009 起它不再切换任何形状。
    property real detailReveal: 1.0
    readonly property real _titleFontPx: Math.max(0.01, 14 * fitScale)
    readonly property real _bodyFontPx: Math.max(0.01, 12 * fitScale)
    readonly property real _cardRadiusPx: Math.max(0.1, 6 * fitScale)
    // 概览线宽的屏幕口径：概览是父图投影的一部分，线宽恒定在屏幕像素上。
    readonly property real _previewStrokeWidth: 1.5

    readonly property var nodes: snapshot && snapshot.nodes ? snapshot.nodes : []
    readonly property var embeds: snapshot && snapshot.embeds ? snapshot.embeds : []
    readonly property var edgeRenders: snapshot && snapshot.edges ? snapshot.edges : []
    readonly property var childBounds: snapshot ? snapshot.bounds : null
    readonly property string loadError: snapshot ? "" : "preview_not_ready"

    readonly property real _centerWorldX: childBounds
            ? (childBounds.minX + childBounds.maxX) / 2 : 0
    readonly property real _centerWorldY: childBounds
            ? (childBounds.minY + childBounds.maxY) / 2 : 0

    // 子图 world 单位 → 概览局部像素。没有内容时返回 0，不画任何投影。
    readonly property real fitScale:
        StarMapNodeMetrics.embedFitScale(childBounds, Math.min(width, height))

    function projectX(wx) { return width / 2 + (wx - _centerWorldX) * fitScale }
    function projectY(wy) { return height / 2 + (wy - _centerWorldY) * fitScale }

    // ── 单一投影入口画出的矢量几何（QtQuick.Shapes）──
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

    // 节点卡片与入口圆壳只做投影，不再读原始 graph / 重新测量。
    readonly property var projectedNodes: {
        var out = []
        if (!(fitScale > 0))
            return out
        for (var i = 0; i < nodes.length; i++) {
            var n = nodes[i]
            out.push({
                id: n.id,
                x: projectX(n.x), y: projectY(n.y),
                width: n.width * fitScale, height: n.height * fitScale,
                title: n.title !== undefined ? n.title : "",
                body: n.body !== undefined && n.body !== null ? String(n.body) : ""
            })
        }
        return out
    }

    readonly property var projectedEmbeds: {
        var out = []
        if (!(fitScale > 0))
            return out
        for (var i = 0; i < embeds.length; i++) {
            var e = embeds[i]
            out.push({
                id: e.instanceId,
                x: projectX(e.x), y: projectY(e.y),
                diameter: e.diameter * fitScale,
                label: e.label !== undefined && e.label !== null ? e.label : ""
            })
        }
        return out
    }

    z: 0

    Shape {
        anchors.fill: parent
        antialiasing: true
        preferredRendererType: Shape.CurveRenderer

        ShapePath {
            strokeColor: overview.dt.border
            strokeWidth: overview._previewStrokeWidth
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            PathSvg { path: overview.edgesPath }
        }
    }

    Repeater {
        model: overview.projectedNodes
        delegate: Rectangle {
            x: modelData.x
            y: modelData.y
            width: modelData.width
            height: modelData.height
            radius: overview._cardRadiusPx
            color: overview.dt.surfaceContainer
            border.color: overview.dt.border
            border.width: Math.max(0.05, 1 * overview.fitScale)
            clip: true
            visible: width > 0.3 && height > 0.3

            Column {
                anchors.fill: parent
                anchors.margins: Math.max(0.1, 6 * overview.fitScale)
                spacing: Math.max(0.05, 2 * overview.fitScale)
                clip: true

                Text {
                    width: parent.width
                    text: modelData.title
                    color: overview.dt.textPrimary
                    font.pixelSize: overview._titleFontPx
                    font.bold: true
                    elide: Text.ElideRight
                    maximumLineCount: 1
                }

                Text {
                    width: parent.width
                    text: modelData.body
                    color: overview.dt.textSecondary
                    font.pixelSize: overview._bodyFontPx
                    elide: Text.ElideRight
                    maximumLineCount: 2
                    wrapMode: Text.Wrap
                    visible: text.length > 0
                }
            }
        }
    }

    Repeater {
        model: overview.projectedEmbeds
        delegate: Rectangle {
            x: modelData.x
            y: modelData.y
            width: modelData.diameter
            height: modelData.diameter
            radius: modelData.diameter / 2
            color: overview.dt.accentSoft
            border.color: overview.dt.border
            border.width: Math.max(0.05, 1 * overview.fitScale)
            clip: true
            visible: width > 0.3 && height > 0.3

            Text {
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.topMargin: Math.max(0.1, 8 * overview.fitScale)
                text: modelData.label
                color: overview.dt.textPrimary
                font.pixelSize: overview._titleFontPx
                horizontalAlignment: Text.AlignHCenter
                elide: Text.ElideRight
                maximumLineCount: 1
            }
        }
    }
}
