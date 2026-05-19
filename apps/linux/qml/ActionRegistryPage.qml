import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

ScrollView {
    id: root
    clip: true

    property var backendRef: null
    property real fontSizeSpinValue: 16
    property bool autoSaveCheckChecked: false
    property real autoSaveDelaySpinValue: 1500
    property real lineSpacingSpinValue: 150
    property bool autoIndentCheckChecked: false
    property real autoIndentWidthSpinValue: 200
    property bool typingAnimCheckChecked: false
    property bool smoothCursorCheckChecked: false

    ColumnLayout {
        width: parent ? parent.width - 20 : 600
        spacing: 10

        Label {
            text: "Action 调试入口"
            font.bold: true
            font.pixelSize: 16
        }
        Label {
            text: "列出所有已注册的 Action，可执行 Query 类型或查看 Mutation 描述。"
            color: "gray"
            font.pixelSize: 12
            wrapMode: Text.Wrap
        }

        RowLayout {
            Layout.fillWidth: true
            Button {
                text: "列出所有 Action"
                onClicked: {
                    var result = root.backendRef.list_registered_actions()
                    try {
                        var actions = JSON.parse(result)
                        actionListRepeater.model = actions
                        actionResultText.text = "共加载 " + actions.length + " 个 Action"
                    } catch(e) {
                        actionResultText.text = "解析 Action 列表失败: " + e
                    }
                }
            }
            Button {
                text: "清空结果"
                onClicked: {
                    actionListRepeater.model = []
                    actionResultText.text = ""
                }
            }
        }

        Repeater {
            id: actionListRepeater
            model: []

            delegate: Rectangle {
                width: parent.width
                height: actionCardCol.height + 10
                color: "#2a2a2a"
                radius: 4

                ColumnLayout {
                    id: actionCardCol
                    anchors.fill: parent
                    anchors.margins: 8
                    spacing: 4

                    RowLayout {
                        Layout.fillWidth: true
                        Label {
                            text: modelData.title || modelData.id || ""
                            font.bold: true
                            font.pixelSize: 14
                            color: "#e0e0e0"
                            Layout.fillWidth: true
                        }
                        Label {
                            text: {
                                var risk = modelData.riskLevel || ""
                                if (risk === "dangerous") return "\u26A0 危险"
                                if (risk === "contentWrite") return "\u26A0 内容写入"
                                if (risk === "safeWrite") return "写入"
                                return "只读"
                            }
                            font.pixelSize: 11
                            color: {
                                var risk = modelData.riskLevel || ""
                                if (risk === "dangerous") return "#f44336"
                                if (risk === "contentWrite") return "#ff9800"
                                if (risk === "safeWrite") return "#4caf50"
                                return "#90a4ae"
                            }
                        }
                    }

                    Label {
                        text: modelData.id || ""
                        font.pixelSize: 11
                        color: "#78909c"
                        font.family: "monospace"
                    }

                    Label {
                        text: modelData.description || ""
                        font.pixelSize: 12
                        color: "#b0bec5"
                        wrapMode: Text.Wrap
                        Layout.fillWidth: true
                    }

                    RowLayout {
                        spacing: 8
                        Button {
                            text: "执行"
                            visible: modelData.kind === "query" || modelData.kind === "preview"
                            onClicked: {
                                var actionId = modelData.id
                                var result = root.backendRef.execute_action(actionId, "", "")
                                try {
                                    var r = JSON.parse(result)
                                    if (r.success) {
                                        actionResultText.text = "执行成功: " + (r.message || "") + "\n数据: " + JSON.stringify(r.data, null, 2)
                                    } else {
                                        actionResultText.text = "执行失败: " + (r.message || "未知错误")
                                    }
                                } catch(e) {
                                    actionResultText.text = "解析结果失败: " + result
                                }
                            }
                        }
                        Button {
                            text: "应用 (Mutation)"
                            visible: modelData.kind === "mutation"
                            enabled: {
                                var risk = modelData.riskLevel || ""
                                return risk !== "dangerous" && risk !== "contentWrite"
                            }
                            onClicked: {
                                var actionId = modelData.id
                                var args = "{}"
                                if (actionId === "settings.editor.font_size.set") {
                                    args = JSON.stringify({fontSize: root.fontSizeSpinValue})
                                } else if (actionId === "settings.editor.auto_save.set") {
                                    args = JSON.stringify({enabled: root.autoSaveCheckChecked})
                                } else if (actionId === "settings.editor.auto_save_delay.set") {
                                    args = JSON.stringify({delayMs: root.autoSaveDelaySpinValue})
                                } else if (actionId === "settings.editor.line_spacing.set") {
                                    args = JSON.stringify({multiplier: root.lineSpacingSpinValue / 100.0})
                                } else if (actionId === "settings.editor.auto_indent.set") {
                                    args = JSON.stringify({enabled: root.autoIndentCheckChecked, widthChars: root.autoIndentWidthSpinValue / 100.0})
                                } else if (actionId === "settings.editor.typing_animation.set") {
                                    args = JSON.stringify({enabled: root.typingAnimCheckChecked})
                                } else if (actionId === "settings.editor.smooth_cursor.set") {
                                    args = JSON.stringify({enabled: root.smoothCursorCheckChecked})
                                }
                                var result = root.backendRef.execute_action(actionId, args, "")
                                try {
                                    var r = JSON.parse(result)
                                    if (r.success) {
                                        actionResultText.text = "应用成功: " + (r.message || "") + "\n数据: " + JSON.stringify(r.data, null, 2)
                                    } else {
                                        actionResultText.text = "应用失败: " + (r.message || "未知错误")
                                    }
                                } catch(e) {
                                    actionResultText.text = "解析结果失败: " + result
                                }
                            }
                        }
                        Label {
                            text: "危险操作已阻断"
                            visible: modelData.kind === "mutation" && (modelData.riskLevel === "dangerous" || modelData.riskLevel === "contentWrite")
                            color: "#f44336"
                            font.pixelSize: 12
                        }
                    }
                }
            }
        }

        TextArea {
            id: actionResultText
            Layout.fillWidth: true
            Layout.minimumHeight: 100
            readOnly: true
            wrapMode: Text.Wrap
            background: Rectangle { color: "#2a2a2a"; radius: 4 }
            color: "#e0e0e0"
            font.pixelSize: 12
            font.family: "monospace"
            placeholderText: "执行结果将显示在此处..."
        }
    }
}
