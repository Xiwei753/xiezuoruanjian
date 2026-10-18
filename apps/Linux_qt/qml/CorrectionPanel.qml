// =============================================================================
// CorrectionPanel.qml — 写作工具侧栏中的校对内容
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root

    // 校对状态只由 WritingWorkspace 的 EditorController 持有。
    property var editorControllerRef: null
    property bool resourcesExpanded: false

    onVisibleChanged: {
        if (visible && root.editorControllerRef) {
            Qt.callLater(function() {
                if (root.visible && root.editorControllerRef)
                    root.editorControllerRef.refreshCorrectionDictionaryStatus();
            });
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.smallSpacing
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Button {
                Layout.fillWidth: true
                text: root.editorControllerRef && root.editorControllerRef.correctionCheckPending
                      ? qsTr("正在检查") : qsTr("检查本章")
                enabled: !!(root.editorControllerRef
                            && root.editorControllerRef.chapterId
                            && !root.editorControllerRef.correctionCheckPending)
                onClicked: root.editorControllerRef.requestCorrectionChapter()
            }

            Button {
                Layout.fillWidth: true
                text: qsTr("清除标注")
                enabled: !!(root.editorControllerRef
                            && root.editorControllerRef.correctionSuggestions.length)
                onClicked: root.editorControllerRef.invalidateCorrectionResults()
            }
        }

        Label {
            Layout.fillWidth: true
            visible: !!(root.editorControllerRef
                        && (root.editorControllerRef.correctionError
                            || root.editorControllerRef.correctionStatus))
            text: root.editorControllerRef
                  ? (root.editorControllerRef.correctionError
                     || root.editorControllerRef.correctionStatus) : ""
            color: root.editorControllerRef && root.editorControllerRef.correctionError
                   ? "#C52222" : Kirigami.Theme.textColor
            wrapMode: Text.Wrap
        }

        ListView {
            id: correctionSuggestionList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: Kirigami.Units.smallSpacing
            model: root.editorControllerRef
                   ? root.editorControllerRef.correctionSuggestions : []

            delegate: Rectangle {
                required property var modelData
                width: correctionSuggestionList.width
                height: suggestionContent.implicitHeight + Kirigami.Units.largeSpacing
                radius: Kirigami.Units.cornerRadius
                color: Kirigami.Theme.alternateBackgroundColor

                ColumnLayout {
                    id: suggestionContent
                    anchors.fill: parent
                    anchors.margins: Kirigami.Units.smallSpacing
                    spacing: Kirigami.Units.smallSpacing / 2

                    Label {
                        Layout.fillWidth: true
                        text: root.editorControllerRef
                              ? root.editorControllerRef.correctionSeverityLabel(modelData) : ""
                        color: root.editorControllerRef
                               ? root.editorControllerRef.correctionSeverityColor(modelData)
                               : Kirigami.Theme.disabledTextColor
                        font.bold: true
                    }

                    Label {
                        Layout.fillWidth: true
                        text: qsTr("“%1” → “%2”")
                              .arg(modelData.original)
                              .arg(modelData.replacement)
                        wrapMode: Text.Wrap
                    }

                    Label {
                        Layout.fillWidth: true
                        text: qsTr("byte [%1, %2) · 字符 [%3, %4) · UTF-16 [%5, %6)")
                              .arg(modelData.startByte)
                              .arg(modelData.endByte)
                              .arg(modelData.startChar)
                              .arg(modelData.endChar)
                              .arg(modelData.startUtf16)
                              .arg(modelData.endUtf16)
                        font.pixelSize: 11
                        color: Kirigami.Theme.disabledTextColor
                        wrapMode: Text.Wrap
                    }

                    Label {
                        Layout.fillWidth: true
                        text: root.editorControllerRef
                              ? root.editorControllerRef.correctionReason(modelData) : ""
                        wrapMode: Text.Wrap
                        font.pixelSize: 12
                    }

                    Flow {
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.smallSpacing / 2

                        Button {
                            text: qsTr("替换")
                            onClicked: root.editorControllerRef.acceptCorrection(modelData)
                        }
                        Button {
                            text: qsTr("忽略一次")
                            onClicked: root.editorControllerRef.ignoreCorrection(modelData, "once")
                        }
                        Button {
                            text: qsTr("忽略本章")
                            onClicked: root.editorControllerRef.ignoreCorrection(modelData, "chapter")
                        }
                        Button {
                            text: qsTr("加入作品词典")
                            onClicked: root.editorControllerRef.addCorrectionWord(modelData)
                        }
                        Button {
                            text: qsTr("停用此规则")
                            onClicked: root.editorControllerRef.ignoreCorrection(modelData, "rule")
                        }
                    }
                }
            }
        }

        Button {
            Layout.fillWidth: true
            text: root.resourcesExpanded ? qsTr("收起校对资源") : qsTr("校对资源")
            icon.name: root.resourcesExpanded ? "go-up" : "go-down"
            onClicked: root.resourcesExpanded = !root.resourcesExpanded
        }

        ColumnLayout {
            Layout.fillWidth: true
            visible: root.resourcesExpanded
            spacing: Kirigami.Units.smallSpacing

            TextField {
                id: correctionModelPathField
                Layout.fillWidth: true
                placeholderText: qsTr("本地 .klm 模型文件路径")
                selectByMouse: true
                onAccepted: {
                    if (root.editorControllerRef)
                        root.editorControllerRef.setCorrectionModelPath(text);
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Button {
                    Layout.fillWidth: true
                    text: qsTr("选择模型")
                    onClicked: {
                        var selectedPath = root.editorControllerRef
                                ? root.editorControllerRef.chooseCorrectionModelPath() : "";
                        if (selectedPath) correctionModelPathField.text = selectedPath;
                    }
                }
                Button {
                    Layout.fillWidth: true
                    text: qsTr("设置路径")
                    onClicked: {
                        if (root.editorControllerRef)
                            root.editorControllerRef.setCorrectionModelPath(correctionModelPathField.text);
                    }
                }
            }

            Button {
                Layout.fillWidth: true
                text: qsTr("导入词库文件")
                onClicked: {
                    if (root.editorControllerRef)
                        root.editorControllerRef.importCorrectionDictionary();
                }
            }

            Label {
                Layout.fillWidth: true
                text: qsTr("固定命名的 .txt")
                color: Kirigami.Theme.disabledTextColor
                elide: Text.ElideRight
            }

            Flow {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Repeater {
                    model: root.editorControllerRef
                           ? root.editorControllerRef.correctionDictionaryResources : []
                    delegate: Label {
                        required property var modelData
                        text: qsTr("%1 · %2")
                              .arg(modelData.name)
                              .arg(modelData.ready ? qsTr("已准备") : qsTr("待准备"))
                        color: modelData.ready ? "#238636" : Kirigami.Theme.disabledTextColor
                        font.pixelSize: 11
                    }
                }
            }

            Label {
                Layout.fillWidth: true
                visible: !!(root.editorControllerRef
                            && root.editorControllerRef.correctionDictionaryError)
                text: root.editorControllerRef
                      ? root.editorControllerRef.correctionDictionaryError : ""
                color: "#C52222"
                wrapMode: Text.Wrap
            }
        }
    }
}
