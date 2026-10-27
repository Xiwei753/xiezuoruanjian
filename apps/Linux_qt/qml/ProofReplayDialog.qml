// =============================================================================
// ProofReplayDialog.qml — 手写证明回放（Issue #1008）
// =============================================================================
// 只渲染 Core 返回的正文、选区、来源和进度；不自行推演编辑规则。
// 时间轴按章节内事件位置（0..N）定位，全局 seq 仅用于链校验展示。
// 播放间隔由相邻事件真实时间差和倍速计算。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: root

    property var editorBackendRef: null
    property string chapterId: ""
    property string deviceId: ""
    property string selectedDeviceId: ""
    property var dt: null
    property var frame: null
    property int currentPosition: 0
    property int totalEvents: 0
    property real currentTimestamp: 0
    property real nextTimestamp: 0
    property bool playing: false
    property real playbackSpeed: 1.0
    property int playbackInterval: 260
    property string exportJson: ""

    title: qsTr("手写证明回放")
    modal: true
    width: 760
    height: 620
    anchors.centerIn: parent

    function updatePlaybackInterval() {
        var delta = nextTimestamp > currentTimestamp
                ? nextTimestamp - currentTimestamp : 260
        playbackInterval = Math.max(40, Math.min(2000,
                Math.round(delta / Math.max(0.5, playbackSpeed))))
    }

    function loadFrame(position) {
        if (!editorBackendRef || !editorBackendRef.replay_chapter_proof_object || !chapterId)
            return
        var request = {
            "chapterId": chapterId,
            "deviceId": selectedDeviceId
        }
        if (position !== null && position !== undefined && position >= 0)
            request.targetIndex = position
        var next = editorBackendRef.replay_chapter_proof_object(JSON.stringify(request))
        if (!next || next.valid === undefined) {
            errorLabel.text = qsTr("无法读取回放数据")
            return
        }
        frame = next
        totalEvents = Number(next.totalEvents || 0)
        currentPosition = Number(next.position || 0)
        currentTimestamp = Number(next.timestampMs || 0)
        nextTimestamp = Number(next.nextTimestampMs || 0)
        updatePlaybackInterval()
        errorLabel.text = (next.errors && next.errors.length > 0)
                ? next.errors.join("\n") : ""
    }

    function openForChapter(chapter, device) {
        chapterId = String(chapter || "")
        deviceId = String(device || "")
        selectedDeviceId = deviceId
        playing = false
        exportJson = ""
        frame = null
        loadFrame(null)
        open()
    }

    function step(delta) {
        var next = Math.max(0, Math.min(totalEvents, currentPosition + delta))
        if (next !== currentPosition)
            loadFrame(next)
    }

    onOpened: loadFrame(null)
    onClosed: playing = false

    Timer {
        interval: root.playbackInterval
        repeat: true
        running: root.playing
        onTriggered: {
            if (root.currentPosition >= root.totalEvents) {
                root.playing = false
            } else {
                root.step(1)
            }
        }
    }

    header: Label {
        text: root.title
        font.pixelSize: 18
        padding: 12
    }

    contentItem: ColumnLayout {
        spacing: 10

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            Button {
                text: root.playing ? qsTr("暂停") : qsTr("播放")
                enabled: root.totalEvents > 0
                onClicked: root.playing = !root.playing
            }
            Button {
                text: qsTr("上一步")
                enabled: root.currentPosition > 0
                onClicked: { root.playing = false; root.step(-1) }
            }
            Button {
                text: qsTr("下一步")
                enabled: root.currentPosition < root.totalEvents
                onClicked: { root.playing = false; root.step(1) }
            }
            Label {
                Layout.fillWidth: true
                text: root.frame
                      ? qsTr("事件 %1 / %2 · 来源 %3")
                            .arg(root.currentPosition).arg(root.totalEvents)
                            .arg(String(root.frame.source || ""))
                      : qsTr("无事件")
                elide: Text.ElideRight
            }
            ComboBox {
                Layout.preferredWidth: 90
                model: [0.5, 1.0, 2.0, 4.0]
                currentIndex: 1
                textRole: "modelData"
                onActivated: {
                    root.playbackSpeed = Number(model[currentIndex])
                    root.updatePlaybackInterval()
                }
            }
        }

        Slider {
            Layout.fillWidth: true
            from: 0
            to: Math.max(1, root.totalEvents)
            stepSize: 1
            value: root.currentPosition
            enabled: root.totalEvents > 0
            onMoved: {
                root.playing = false
                root.loadFrame(Math.round(value))
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Label { text: qsTr("设备分支"); font.pixelSize: 12 }
            ComboBox {
                Layout.fillWidth: true
                model: root.frame && root.frame.branches
                       ? root.frame.branches.map(function(branch) {
                             return String(branch.deviceId || "")
                         })
                       : []
                onActivated: {
                    root.selectedDeviceId = String(model[currentIndex] || "")
                    root.playing = false
                    root.loadFrame(null)
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Label {
                Layout.fillWidth: true
                text: root.currentTimestamp > 0
                      ? qsTr("发生时间 %1").arg(Qt.formatDateTime(
                            new Date(root.currentTimestamp), "yyyy-MM-dd HH:mm:ss"))
                      : qsTr("事件时间未知")
                font.pixelSize: 12
            }
            Label {
                text: qsTr("链序号 %1").arg(root.frame ? Number(root.frame.seq || 0) : 0)
                font.pixelSize: 12
                opacity: 0.7
            }
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            TextArea {
                id: proofText
                readOnly: true
                wrapMode: TextArea.Wrap
                text: root.frame ? String(root.frame.text || "") : ""
                placeholderText: qsTr("该章节还没有可回放的编辑事件")
                font.pixelSize: 15
            }
        }

        Label {
            id: errorLabel
            Layout.fillWidth: true
            color: "#c0392b"
            wrapMode: Text.Wrap
            visible: text.length > 0
        }

        RowLayout {
            Layout.fillWidth: true
            Label {
                Layout.fillWidth: true
                text: root.frame && root.frame.valid
                      ? qsTr("哈希链校验通过")
                      : qsTr("校验未通过或记录不完整")
                color: root.frame && root.frame.valid ? "#2d7d46" : "#b9770e"
            }
            Button {
                text: qsTr("导出证明包")
                enabled: !!root.editorBackendRef && root.chapterId.length > 0
                onClicked: {
                    var exported = root.editorBackendRef.export_chapter_proof_object(
                                root.chapterId, root.selectedDeviceId)
                    if (!exported || exported.containsHistoricalText === undefined) {
                        errorLabel.text = qsTr("导出失败")
                        return
                    }
                    root.exportJson = JSON.stringify(exported, null, 2)
                    exportArea.text = root.exportJson
                    exportDialog.open()
                }
            }
            Button {
                text: qsTr("关闭")
                onClicked: root.close()
            }
        }
    }

    Dialog {
        id: exportDialog
        title: qsTr("证明包（包含历史正文，请确认后再分享）")
        modal: true
        width: 680
        height: 520
        anchors.centerIn: parent
        contentItem: ColumnLayout {
            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                TextArea {
                    id: exportArea
                    readOnly: true
                    wrapMode: TextArea.NoWrap
                    font.family: "monospace"
                    font.pixelSize: 12
                }
            }
            RowLayout {
                Layout.alignment: Qt.AlignRight
                Button {
                    text: qsTr("复制")
                    onClicked: {
                        exportArea.selectAll()
                        exportArea.copy()
                    }
                }
                Button {
                    text: qsTr("关闭")
                    onClicked: exportDialog.close()
                }
            }
        }
    }
}
