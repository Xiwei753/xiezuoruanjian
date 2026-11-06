// =============================================================================
// StarMapProofReplayDialog.qml — 星图创作记录 / 手写证明回放（Issue #1036）
// =============================================================================
//
// 层级：Linux_qt UI 层
// 职责：按 Core 返回的历史图状态逐帧显示星图从空白（或真实基线）到成型的
//   操作过程；播放/暂停、时间轴拖动、速度、设备分支与来源提示都在这里。
// 约束：不重算业务状态，不修改真实星图；图数据只来自 Core 回放帧。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Dialog {
    id: root
    modal: true
    title: qsTr("星图创作记录")
    width: Math.min(920, parent ? parent.width - 40 : 920)
    height: Math.min(660, parent ? parent.height - 40 : 660)
    padding: 0

    property var dt
    property var starmapBackendRef: null
    property string starmapId: ""
    property var frame: ({})
    property string deviceId: ""
    property int position: 0
    property bool playing: false
    property real playbackSpeed: 1
    readonly property int totalEvents: root.frame && root.frame.totalEvents
                                         ? root.frame.totalEvents : 0
    readonly property var graph:
        root.frame && root.frame.state && root.frame.state.graph
        ? root.frame.state.graph : ({ nodes: [], edges: [], embeds: [], links: [] })
    readonly property var branches:
        root.frame && root.frame.branches ? root.frame.branches : []

    function openFor(id) {
        root.starmapId = String(id || "")
        root.deviceId = ""
        root.position = 0
        root.playing = false
        root.frame = ({})
        root.open()
    }

    function requestFrame(pos) {
        if (!root.starmapBackendRef || root.starmapId === "")
            return
        var request = JSON.stringify({
            starmapId: root.starmapId,
            deviceId: root.deviceId,
            targetPosition: Math.max(0, Math.floor(pos)),
            includeTrajectory: true
        })
        var res = root.starmapBackendRef.replay_starmap_proof_object(request)
        if (!res || res.error) {
            root.errorText = res && res.error ? String(res.error) : qsTr("无法读取星图创作记录")
            return
        }
        if (!root.deviceId && res.deviceId)
            root.deviceId = String(res.deviceId)
        root.frame = res
        root.position = Number(res.position || 0)
        root.errorText = ""
        graphCanvas.requestPaint()
    }

    property string errorText: ""

    onOpened: requestFrame(root.position)

    Timer {
        id: playTimer
        interval: Math.max(16, Math.round(200 / root.playbackSpeed))
        repeat: true
        running: root.playing && root.position < root.totalEvents
        onTriggered: {
            var next = Math.min(root.totalEvents, root.position + 1)
            root.requestFrame(next)
            if (next >= root.totalEvents)
                root.playing = false
        }
    }

    // 世界坐标 → 对话框坐标；只用于回放展示，不写回任何对象。
    function graphBounds() {
        var nodes = root.graph && root.graph.nodes ? root.graph.nodes : []
        var embeds = root.graph && root.graph.embeds ? root.graph.embeds : []
        var minX = 0, minY = 0, maxX = 1, maxY = 1, seen = false
        for (var i = 0; i < nodes.length; i++) {
            var n = nodes[i]
            if (!n.position) continue
            if (!seen) { minX = maxX = n.position.x; minY = maxY = n.position.y; seen = true }
            minX = Math.min(minX, n.position.x); maxX = Math.max(maxX, n.position.x)
            minY = Math.min(minY, n.position.y); maxY = Math.max(maxY, n.position.y)
        }
        for (var j = 0; j < embeds.length; j++) {
            var e = embeds[j]
            if (!e.position) continue
            if (!seen) { minX = maxX = e.position.x; minY = maxY = e.position.y; seen = true }
            minX = Math.min(minX, e.position.x); maxX = Math.max(maxX, e.position.x)
            minY = Math.min(minY, e.position.y); maxY = Math.max(maxY, e.position.y)
        }
        return { minX: minX, minY: minY, maxX: maxX, maxY: maxY }
    }

    function screenX(worldX, width, height) {
        var b = graphBounds()
        var spanX = Math.max(1, b.maxX - b.minX)
        var spanY = Math.max(1, b.maxY - b.minY)
        var scale = Math.min((width - 120) / spanX, (height - 120) / spanY)
        scale = Math.max(0.2, Math.min(scale, 2.2))
        return width / 2 + (worldX - (b.minX + b.maxX) / 2) * scale - 48
    }

    function screenY(worldY, width, height) {
        var b = graphBounds()
        var spanX = Math.max(1, b.maxX - b.minX)
        var spanY = Math.max(1, b.maxY - b.minY)
        var scale = Math.min((width - 120) / spanX, (height - 120) / spanY)
        scale = Math.max(0.2, Math.min(scale, 2.2))
        return height / 2 + (worldY - (b.minY + b.maxY) / 2) * scale - 22
    }

    background: Rectangle {
        color: root.dt ? root.dt.card : Kirigami.Theme.backgroundColor
        radius: root.dt ? root.dt.dialogRadius : 12
        border.color: root.dt ? root.dt.border : Kirigami.Theme.disabledTextColor
    }

    contentItem: ColumnLayout {
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 16
            spacing: 12

            Label {
                text: qsTr("设备分支")
                color: root.dt ? root.dt.textSecondary : Kirigami.Theme.textColor
            }
            ComboBox {
                id: branchBox
                Layout.preferredWidth: 220
                model: root.branches.map(function(branch) {
                    return branch.deviceId + " · " + branch.eventCount
                })
                onActivated: function(index) {
                    var branch = root.branches[index]
                    if (!branch) return
                    root.playing = false
                    root.deviceId = String(branch.deviceId || "")
                    root.position = 0
                    root.requestFrame(0)
                }
            }
            Item { Layout.fillWidth: true }
            Label {
                visible: !root.frame.historyComplete && root.frame.position > 0
                text: qsTr("此前历史不完整（从真实基线开始）")
                color: root.dt ? root.dt.textMuted : Kirigami.Theme.disabledTextColor
                font.pixelSize: 12
            }
            Button {
                text: qsTr("关闭")
                onClicked: root.close()
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: 16
            clip: true
            color: root.dt ? root.dt.surfaceContainer : Kirigami.Theme.alternateBackgroundColor
            radius: root.dt ? root.dt.radiusSm : 8

            Canvas {
                id: graphCanvas
                anchors.fill: parent
                onPaint: {
                    var ctx = getContext("2d")
                    ctx.reset()
                    var edges = root.graph && root.graph.edges ? root.graph.edges : []
                    var nodes = root.graph && root.graph.nodes ? root.graph.nodes : []
                    var nodeById = ({})
                    for (var i = 0; i < nodes.length; i++)
                        nodeById[nodes[i].id] = nodes[i]
                    ctx.strokeStyle = root.dt ? root.dt.border : "#888"
                    ctx.lineWidth = 1.5
                    for (var j = 0; j < edges.length; j++) {
                        var edge = edges[j]
                        var from = edge.from && edge.from.target && edge.from.target.nodeId
                                   ? nodeById[edge.from.target.nodeId] : null
                        var to = edge.to && edge.to.target && edge.to.target.nodeId
                                   ? nodeById[edge.to.target.nodeId] : null
                        if (!from || !to || !from.position || !to.position) continue
                        ctx.beginPath()
                        ctx.moveTo(root.screenX(from.position.x, width, height) + 48,
                                   root.screenY(from.position.y, width, height) + 22)
                        ctx.lineTo(root.screenX(to.position.x, width, height) + 48,
                                   root.screenY(to.position.y, width, height) + 22)
                        ctx.stroke()
                    }
                    var links = root.graph && root.graph.links ? root.graph.links : []
                    ctx.strokeStyle = root.dt ? root.dt.accent : "#6f7fd8"
                    ctx.setLineDash([4, 4])
                    for (var k = 0; k < links.length; k++) {
                        var link = links[k]
                        var linkFrom = link.source && link.source.target && link.source.target.nodeId
                                       ? nodeById[link.source.target.nodeId] : null
                        var linkTo = link.target && link.target.target && link.target.target.nodeId
                                     ? nodeById[link.target.target.nodeId] : null
                        if (!linkFrom || !linkTo || !linkFrom.position || !linkTo.position) continue
                        ctx.beginPath()
                        ctx.moveTo(root.screenX(linkFrom.position.x, width, height) + 48,
                                   root.screenY(linkFrom.position.y, width, height) + 22)
                        ctx.lineTo(root.screenX(linkTo.position.x, width, height) + 48,
                                   root.screenY(linkTo.position.y, width, height) + 22)
                        ctx.stroke()
                    }
                    ctx.setLineDash([])
                    var embeds = root.graph && root.graph.embeds ? root.graph.embeds : []
                    ctx.fillStyle = root.dt ? root.dt.accentSoft : "#eef1ff"
                    ctx.strokeStyle = root.dt ? root.dt.accent : "#6f7fd8"
                    ctx.lineWidth = 2
                    for (var e = 0; e < embeds.length; e++) {
                        var embed = embeds[e]
                        if (!embed.position) continue
                        ctx.beginPath()
                        ctx.arc(root.screenX(embed.position.x, width, height) + 48,
                                root.screenY(embed.position.y, width, height) + 22,
                                26, 0, Math.PI * 2)
                        ctx.fill()
                        ctx.stroke()
                    }
                }
            }

            Repeater {
                model: root.graph && root.graph.nodes ? root.graph.nodes : []
                delegate: Rectangle {
                    required property var modelData
                    property bool active: root.frame && root.frame.event
                                          && root.frame.event.objectId === modelData.id
                    x: root.screenX(modelData.position ? modelData.position.x : 0,
                                    graphCanvas.width, graphCanvas.height)
                    y: root.screenY(modelData.position ? modelData.position.y : 0,
                                    graphCanvas.width, graphCanvas.height)
                    width: 96
                    height: 44
                    radius: 8
                    color: active
                           ? (root.dt ? root.dt.accentSoft : "#dbe4ff")
                           : (root.dt ? root.dt.card : "#ffffff")
                    border.width: active ? 2 : 1
                    border.color: active
                                  ? (root.dt ? root.dt.accent : "#4f6df5")
                                  : (root.dt ? root.dt.border : "#cccccc")
                    Label {
                        anchors.fill: parent
                        anchors.margins: 6
                        text: modelData.title || modelData.id
                        elide: Text.ElideRight
                        wrapMode: Text.Wrap
                        maximumLineCount: 2
                        font.pixelSize: 11
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                        color: root.dt ? root.dt.textPrimary : Kirigami.Theme.textColor
                    }
                }
            }

            Repeater {
                model: root.graph && root.graph.embeds ? root.graph.embeds : []
                delegate: Rectangle {
                    required property var modelData
                    x: root.screenX(modelData.position ? modelData.position.x : 0,
                                    graphCanvas.width, graphCanvas.height) + 22
                    y: root.screenY(modelData.position ? modelData.position.y : 0,
                                    graphCanvas.width, graphCanvas.height) - 4
                    width: 52
                    height: 52
                    radius: 26
                    color: "transparent"
                    border.width: 2
                    border.color: root.dt ? root.dt.accent : "#6f7fd8"
                    Label {
                        anchors.centerIn: parent
                        width: parent.width - 8
                        text: modelData.label || modelData.instanceId
                        elide: Text.ElideRight
                        maximumLineCount: 1
                        font.pixelSize: 9
                        horizontalAlignment: Text.AlignHCenter
                        color: root.dt ? root.dt.textSecondary : Kirigami.Theme.textColor
                    }
                }
            }

            Label {
                anchors.centerIn: parent
                visible: (!root.graph || !root.graph.nodes || root.graph.nodes.length === 0)
                         && root.totalEvents === 0
                text: root.errorText.length > 0 ? root.errorText : qsTr("暂无星图操作记录")
                color: root.dt ? root.dt.textMuted : Kirigami.Theme.disabledTextColor
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 16
            Layout.rightMargin: 16
            Layout.bottomMargin: 16
            spacing: 8

            RowLayout {
                Layout.fillWidth: true
                spacing: 12

                Button {
                    text: root.playing ? qsTr("暂停") : qsTr("播放")
                    icon.name: root.playing ? "media-playback-pause" : "media-playback-start"
                    enabled: root.totalEvents > 0
                    onClicked: {
                        if (root.position >= root.totalEvents)
                            root.requestFrame(0)
                        root.playing = !root.playing
                    }
                }
                Label {
                    text: qsTr("速度")
                    color: root.dt ? root.dt.textSecondary : Kirigami.Theme.textColor
                }
                ComboBox {
                    Layout.preferredWidth: 90
                    model: [0.5, 1, 2, 4, 8]
                    currentIndex: 1
                    onActivated: function(index) {
                        root.playbackSpeed = Number(model[index])
                    }
                }
                Slider {
                    Layout.fillWidth: true
                    from: 0
                    to: Math.max(1, root.totalEvents)
                    value: root.position
                    onMoved: {
                        root.playing = false
                        root.requestFrame(Math.round(value))
                    }
                }
                Label {
                    text: root.totalEvents > 0
                          ? (root.position + " / " + root.totalEvents)
                          : "0 / 0"
                    color: root.dt ? root.dt.textMuted : Kirigami.Theme.disabledTextColor
                    font.pixelSize: 12
                }
            }

            Label {
                Layout.fillWidth: true
                text: {
                    var event = root.frame ? root.frame.event : null
                    if (!event)
                        return qsTr("起始状态")
                    return event.actor + " · " + event.entrypoint + " · "
                           + event.operation + " " + event.objectType + " · " + event.objectId
                }
                color: root.dt ? root.dt.textSecondary : Kirigami.Theme.textColor
                font.pixelSize: 12
                elide: Text.ElideRight
            }
        }
    }
}
