import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

ScrollView {
    id: root
    clip: true

    property var backendRef: null
    property var theme: null
    property real fontSizeSpinValue: 16
    property bool autoSaveCheckChecked: false
    property real autoSaveDelaySpinValue: 1500
    property real lineSpacingSpinValue: 150
    property bool autoIndentCheckChecked: false
    property real autoIndentWidthSpinValue: 200
    property bool typingAnimCheckChecked: false
    property bool smoothCursorCheckChecked: false

    ColumnLayout {
        width: parent ? parent.width : 500
        spacing: theme ? theme.sp12 : 10

        // Header
        Label {
            text: "Action 调试"
            font.pixelSize: theme ? theme.fontXl : 18
            font.weight: Font.Bold
            color: theme ? theme.textPrimary : "#0f172a"
        }

        Label {
            text: "列出所有已注册的 Action，可执行 Query 类型或查看 Mutation 描述。"
            color: theme ? theme.textSecondary : "#475569"
            font.pixelSize: theme ? theme.fontSm : 12
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        // Action buttons
        RowLayout {
            Layout.fillWidth: true
            spacing: theme ? theme.sp8 : 8

            Button {
                text: "列出所有 Action"
                implicitHeight: 32; implicitWidth: 130
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
                contentItem: Text {
                    text: parent.text
                    color: theme ? theme.primaryText : "#ffffff"
                    font.pixelSize: theme ? theme.fontSm : 12
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
                background: Rectangle {
                    color: parent.hovered
                        ? (theme ? theme.primaryHover : "#60a5fa")
                        : (theme ? theme.primary : "#3b82f6")
                    radius: theme ? theme.radiusSm : 6
                }
            }

            Button {
                text: "清空结果"
                implicitHeight: 32
                flat: true
                onClicked: {
                    actionListRepeater.model = []
                    actionResultText.text = ""
                }
                contentItem: Text {
                    text: parent.text
                    color: theme ? theme.textSecondary : "#475569"
                    font.pixelSize: theme ? theme.fontSm : 12
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
                background: Rectangle {
                    color: parent.hovered ? (theme ? theme.hover : "#f1f5f9") : "transparent"
                    radius: theme ? theme.radiusSm : 6
                }
            }
        }

        // Action list
        Repeater {
            id: actionListRepeater
            model: []

            delegate: Rectangle {
                Layout.fillWidth: true
                implicitHeight: actionCardCol.implicitHeight + (theme ? theme.sp24 : 24)
                color: theme ? theme.surfaceAlt : "#f1f5f9"
                radius: theme ? theme.radiusMd : 8
                border.color: theme ? theme.border : "#e2e8f0"
                border.width: 1

                ColumnLayout {
                    id: actionCardCol
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.leftMargin: theme ? theme.sp12 : 12
                    anchors.rightMargin: theme ? theme.sp12 : 12
                    anchors.topMargin: theme ? theme.sp12 : 12
                    anchors.bottomMargin: theme ? theme.sp12 : 12
                    spacing: theme ? theme.sp6 : 4

                    RowLayout {
                        Layout.fillWidth: true
                        Label {
                            text: modelData.title || modelData.id || ""
                            font.pixelSize: theme ? theme.fontMd : 14
                            font.weight: Font.DemiBold
                            color: theme ? theme.textPrimary : "#0f172a"
                            Layout.fillWidth: true
                        }
                        Label {
                            text: {
                                var risk = modelData.riskLevel || ""
                                if (risk === "dangerous") return "危险"
                                if (risk === "contentWrite") return "内容写入"
                                if (risk === "safeWrite") return "写入"
                                return "只读"
                            }
                            font.pixelSize: theme ? theme.fontXs : 11
                            color: {
                                var risk = modelData.riskLevel || ""
                                if (risk === "dangerous") return theme ? theme.danger : "#ef4444"
                                if (risk === "contentWrite") return theme ? theme.warning : "#f59e0b"
                                if (risk === "safeWrite") return theme ? theme.success : "#22c55e"
                                return theme ? theme.textSecondary : "#475569"
                            }
                        }
                    }

                    Label {
                        text: modelData.id || ""
                        font.pixelSize: theme ? theme.fontXs : 11
                        color: theme ? theme.textSecondary : "#475569"
                        font.family: "monospace"
                    }

                    Label {
                        text: modelData.description || ""
                        font.pixelSize: theme ? theme.fontSm : 12
                        color: theme ? theme.textSecondary : "#475569"
                        wrapMode: Text.Wrap
                        Layout.fillWidth: true
                    }

                    RowLayout {
                        spacing: theme ? theme.sp8 : 8
                        Button {
                            text: "执行"
                            visible: modelData.kind === "query" || modelData.kind === "preview"
                            implicitHeight: 28; implicitWidth: 60
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
                            contentItem: Text {
                                text: parent.text
                                color: theme ? theme.primaryText : "#ffffff"
                                font.pixelSize: theme ? theme.fontSm : 12
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                            }
                            background: Rectangle {
                                color: parent.hovered
                                    ? (theme ? theme.primaryHover : "#60a5fa")
                                    : (theme ? theme.primary : "#3b82f6")
                                radius: theme ? theme.radiusSm : 6
                            }
                        }
                        Button {
                            text: "应用 (Mutation)"
                            visible: modelData.kind === "mutation"
                            enabled: { var risk = modelData.riskLevel || ""; return risk !== "dangerous" && risk !== "contentWrite" }
                            implicitHeight: 28; implicitWidth: 110
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
                            contentItem: Text {
                                text: parent.text
                                color: theme ? theme.primaryText : "#ffffff"
                                font.pixelSize: theme ? theme.fontSm : 12
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                            }
                            background: Rectangle {
                                color: parent.enabled
                                    ? (parent.hovered
                                        ? (theme ? theme.primaryHover : "#60a5fa")
                                        : (theme ? theme.primary : "#3b82f6"))
                                    : (theme ? theme.border : "#e2e8f0")
                                radius: theme ? theme.radiusSm : 6
                            }
                        }
                        Label {
                            text: "危险操作已阻断"
                            visible: modelData.kind === "mutation" && (modelData.riskLevel === "dangerous" || modelData.riskLevel === "contentWrite")
                            color: theme ? theme.danger : "#ef4444"
                            font.pixelSize: theme ? theme.fontSm : 12
                        }
                    }
                }
            }
        }

        // Result display
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: Math.min(actionResultText.implicitHeight + 20, 200)
            color: theme ? theme.surfaceAlt : "#f1f5f9"
            radius: theme ? theme.radiusSm : 6
            border.color: theme ? theme.border : "#e2e8f0"
            border.width: 1

            TextArea {
                id: actionResultText
                anchors.fill: parent
                anchors.margins: theme ? theme.sp8 : 8
                readOnly: true
                wrapMode: Text.Wrap
                color: theme ? theme.textPrimary : "#0f172a"
                font.pixelSize: theme ? theme.fontSm : 12
                font.family: "monospace"
                placeholderText: "执行结果将显示在此处..."
                background: Rectangle { color: "transparent" }
                placeholderTextColor: theme ? theme.textSecondary : "#475569"
            }
        }
    }
}
