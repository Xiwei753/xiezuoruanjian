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
//   全部保存在同一份 child world 坐标里；父图缩放只更新一个根 Item 的
//   scale / 居中位置，不重建 PathSvg、Repeater 模型，也不重新排版文字。
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
    // 概览连线在父图屏幕上的目标线宽；ShapePath 在 contentRoot 内，需要
    // 除以 fitScale 换算回局部世界单位。
    readonly property real _previewStrokeWidth: 1.5

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

    readonly property var localNodes: {
        var out = []
        for (var i = 0; i < nodes.length; i++) {
            var n = nodes[i]
            out.push({
                id: n.id,
                x: localX(n.x), y: localY(n.y),
                width: n.width, height: n.height,
                title: n.title !== undefined ? n.title : "",
                body: n.body !== undefined && n.body !== null ? String(n.body) : ""
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
                diameter: e.diameter,
                label: e.label !== undefined && e.label !== null ? e.label : ""
            })
        }
        return out
    }

    // 概览唯一的缩放 / 居中入口：父图相机变化只改这里的 scale 和 x/y，
    // 不改任何子对象尺寸或文字排版。
    Item {
        id: contentRoot
        x: (overview.width - overview._childWorldWidth * overview.fitScale) / 2
        y: (overview.height - overview._childWorldHeight * overview.fitScale) / 2
        width: overview._childWorldWidth
        height: overview._childWorldHeight
        visible: overview.hasContent
        transform: Scale {
            origin.x: 0
            origin.y: 0
            xScale: overview.fitScale
            yScale: overview.fitScale
        }

        Shape {
            anchors.fill: parent
            antialiasing: true
            preferredRendererType: Shape.CurveRenderer

            ShapePath {
                strokeColor: overview.dt.border
                // 世界线宽换算成父图屏幕上的 1.5px；路径本身不随 zoom 重建。
                strokeWidth: overview.hasContent
                        ? overview._previewStrokeWidth / overview.fitScale : 0
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                PathSvg { path: overview.edgesPath }
            }
        }

        Repeater {
            model: overview.localNodes
            delegate: Rectangle {
                x: modelData.x
                y: modelData.y
                width: modelData.width
                height: modelData.height
                radius: 6
                color: overview.dt.surfaceContainer
                border.color: overview.dt.border
                border.width: overview.hasContent
                        ? 1 / overview.fitScale : 0
                visible: width > 0.3 && height > 0.3

                Column {
                    anchors.fill: parent
                    anchors.margins: 6
                    spacing: 2

                    Text {
                        width: parent.width
                        text: modelData.title
                        color: overview.dt.textPrimary
                        font.pixelSize: 14
                        font.bold: true
                        elide: Text.ElideRight
                        maximumLineCount: 1
                    }

                    Text {
                        width: parent.width
                        text: modelData.body
                        color: overview.dt.textSecondary
                        font.pixelSize: 12
                        elide: Text.ElideRight
                        maximumLineCount: 2
                        wrapMode: Text.Wrap
                        visible: text.length > 0
                    }
                }
            }
        }

        Repeater {
            model: overview.localEmbeds
            delegate: Rectangle {
                x: modelData.x
                y: modelData.y
                width: modelData.diameter
                height: modelData.diameter
                radius: modelData.diameter / 2
                color: overview.dt.accentSoft
                border.color: overview.dt.border
                border.width: overview.hasContent
                        ? 1 / overview.fitScale : 0
                visible: width > 0.3 && height > 0.3

                Text {
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.topMargin: 8
                    text: modelData.label
                    color: overview.dt.textPrimary
                    font.pixelSize: 14
                    horizontalAlignment: Text.AlignHCenter
                    elide: Text.ElideRight
                    maximumLineCount: 1
                }
            }
        }
    }
}
