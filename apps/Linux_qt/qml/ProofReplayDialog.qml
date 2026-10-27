// =============================================================================
// ProofReplayDialog.qml — 手写证明回放（Issue #1008）
// =============================================================================
// 只渲染 Core 返回的正文、选区、来源和进度；不自行推演编辑规则。
// 拖动时间轴时按设备内 seq 定位，逐次应用 Core replay 结果。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: root

    property var editorBackendRef: null
    property string chapterId: ""
    property string deviceId: ""
    property var dt: null
    property var frame: null
    property int currentSeq: 0
    property int lastSeq: 0
    property bool playing: false
    property string exportJson: ""

    title: qsTr("手写证明回放")
    modal: true
    width: 760
    height: 620
    anchors.centerIn: parent

    function loadFrame(seq) {
        if (!editorBackendRef || !editorBackendRef.replay_chapter_proof_object || !chapterId)
            return
        var request = {
            "chapterId": chapterId,
            "deviceId": deviceId,
            "targetSeq": seq
        }
        var next = editorBackendRef.replay_chapter_proof_object(JSON.stringify(request))
        if (!next || next.valid === undefined) {
            errorLabel.text = qsTr("无法读取回放数据")
            return
        }
        frame = next
        lastSeq = Number(next.lastSeq || 0)
        currentSeq = Number(next.seq || 0)
        errorLabel.text = (next.errors && next.errors.length > 0)
                ? next.errors.join("\n") : ""
    }

    function openForChapter(chapter, device) {
        chapterId = String(chapter || "")
        deviceId = String(device || "")
        playing = false
        exportJson = ""
        frame = null
        loadFrame(null)
        open()
    }

    function step(delta) {
        var next = Math.max(0, Math.min(lastSeq, currentSeq + delta))
        if (next !== currentSeq)
            loadFrame(next)
    }

    onOpened: loadFrame(null)

    Timer {
        interval: 260
        repeat: true
        running: root.playing
        onTriggered: {
            if (root.currentSeq >= root.lastSeq) {
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
                enabled: root.lastSeq > 0
                onClicked: root.playing = !root.playing
            }
            Button {
                text: qsTr("上一步")
                enabled: root.currentSeq > 0
                onClicked: { root.playing = false; root.step(-1) }
            }
            Button {
                text: qsTr("下一步")
                enabled: root.currentSeq < root.lastSeq
                onClicked: { root.playing = false; root.step(1) }
            }
            Label {
                Layout.fillWidth: true
                text: root.frame
                      ? qsTr("事件 %1 / %2 · 来源 %3")
                            .arg(root.currentSeq).arg(root.lastSeq)
                            .arg(String(root.frame.source || ""))
                      : qsTr("无事件")
                elide: Text.ElideRight
            }
        }

        Slider {
            Layout.fillWidth: true
            from: 0
            to: Math.max(1, root.lastSeq)
            stepSize: 1
            value: root.currentSeq
            enabled: root.lastSeq > 0
            onMoved: {
                root.playing = false
                root.loadFrame(Math.round(value))
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
                                root.chapterId, root.deviceId)
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
