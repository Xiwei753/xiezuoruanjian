// =============================================================================
// ActionRegistryPage.qml — Action 注册表调试页面
// =============================================================================
//
// 层级：Linux UI 层（QML 页面）
// 职责：列出所有注册的 Action，支持执行 Query/Mutation 类型操作
// 约束：
//   - 调试用途，不面向普通用户
//   - 通过 backendRef 调用 AppBackend (Rust QObject) 执行 Action
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ScrollView {
    id: root
    clip: true

    property var backendRef: null
    property var appTheme: null
    property real fontSizeSpinValue: 16
    property bool autoSaveCheckChecked: false
    property real autoSaveDelaySpinValue: 1500
    property real lineSpacingSpinValue: 150
    property bool autoIndentCheckChecked: false
    property real autoIndentWidthSpinValue: 200
    property bool typingAnimCheckChecked: false
    property bool smoothCursorCheckChecked: false

    ColumnLayout {
        width: Math.min(540, root.availableWidth - 16)
        spacing: root.appTheme ? root.appTheme.sp12 : 10

        // Header
        Text {
            text: qsTr("Action 调试")
            font.pixelSize: root.appTheme ? root.appTheme.fontXl : 18
            font.weight: Font.Bold
            color: root.appTheme ? root.appTheme.textPrimary : "#0f172a"
        }

        Text {
            text: qsTr("列出所有已注册的 Action，可执行 Query 类型或查看 Mutation 描述。")
            color: root.appTheme ? root.appTheme.textSecondary : "#475569"
            font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        // Action buttons
        RowLayout {
            Layout.fillWidth: true
            spacing: root.appTheme ? root.appTheme.sp8 : 8

            Button {
                text: qsTr("列出所有 Action")
                implicitHeight: 32; implicitWidth: 130
                onClicked: {
                    var result = root.backendRef.list_registered_actions()
                    try {
                        var actions = JSON.parse(result)
                        actionListRepeater.model = actions
                        actionResultText.text = qsTr("共加载 ") + actions.length + qsTr(" 个 Action")
                    } catch(e) {
                        actionResultText.text = qsTr("解析 Action 列表失败: ") + e
                    }
                }
                contentItem: AppText {
                    text: parent.text
                    color: root.appTheme ? root.appTheme.primaryText : "#ffffff"
                    font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
                background: Rectangle {
                    color: parent.hovered
                        ? (root.appTheme ? root.appTheme.primaryHover : "#60a5fa")
                        : (root.appTheme ? root.appTheme.primary : "#3b82f6")
                    radius: root.appTheme ? root.appTheme.radiusSm : 6
                }
            }

            Button {
                text: qsTr("清空结果")
                implicitHeight: 32
                flat: true
                onClicked: {
                    actionListRepeater.model = []
                    actionResultText.text = ""
                }
                contentItem: AppText {
                    text: parent.text
                    color: root.appTheme ? root.appTheme.textSecondary : "#475569"
                    font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
                background: Rectangle {
                    color: parent.hovered ? (root.appTheme ? root.appTheme.hover : "#f1f5f9") : "transparent"
                    radius: root.appTheme ? root.appTheme.radiusSm : 6
                }
            }
        }

        // Action list
        Repeater {
            id: actionListRepeater
            model: []

            delegate: Rectangle {
                Layout.fillWidth: true
                implicitHeight: 120
                color: root.appTheme ? root.appTheme.surfaceAlt : "#f1f5f9"
                radius: root.appTheme ? root.appTheme.radiusMd : 8
                border.color: root.appTheme ? root.appTheme.border : "#e2e8f0"
                border.width: 1

                ColumnLayout {
                    id: actionCardCol
                    anchors.fill: parent
                    anchors.margins: root.appTheme ? root.appTheme.sp12 : 12
                    spacing: root.appTheme ? root.appTheme.sp6 : 4

                    RowLayout {
                        Layout.fillWidth: true
                        Text {
                            text: modelData.title || modelData.id || ""
                            font.pixelSize: root.appTheme ? root.appTheme.fontMd : 14
                            font.weight: Font.DemiBold
                            color: root.appTheme ? root.appTheme.textPrimary : "#0f172a"
                            Layout.fillWidth: true
                        }
                        Text {
                            text: {
                                var risk = modelData.riskLevel || ""
                                if (risk === "dangerous") return qsTr("危险")
                                if (risk === "contentWrite") return qsTr("内容写入")
                                if (risk === "safeWrite") return qsTr("写入")
                                return qsTr("只读")
                            }
                            font.pixelSize: root.appTheme ? root.appTheme.fontXs : 11
                            color: {
                                var risk = modelData.riskLevel || ""
                                if (risk === "dangerous") return root.appTheme ? root.appTheme.danger : "#ef4444"
                                if (risk === "contentWrite") return root.appTheme ? root.appTheme.warning : "#f59e0b"
                                if (risk === "safeWrite") return root.appTheme ? root.appTheme.success : "#22c55e"
                                return root.appTheme ? root.appTheme.textSecondary : "#475569"
                            }
                        }
                    }

                    Text {
                        text: modelData.id || ""
                        font.pixelSize: root.appTheme ? root.appTheme.fontXs : 11
                        color: root.appTheme ? root.appTheme.textSecondary : "#475569"
                        font.family: "monospace"
                    }

                    Text {
                        text: modelData.description || ""
                        font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                        color: root.appTheme ? root.appTheme.textSecondary : "#475569"
                        wrapMode: Text.Wrap
                        Layout.fillWidth: true
                    }

                    RowLayout {
                        spacing: root.appTheme ? root.appTheme.sp8 : 8
                        Button {
                            text: qsTr("执行")
                            visible: modelData.kind === "query" || modelData.kind === "preview"
                            implicitHeight: 28; implicitWidth: 60
                            onClicked: {
                                var actionId = modelData.id
                                var result = root.backendRef.execute_action(actionId, "", "")
                                try {
                                    var r = JSON.parse(result)
                                    if (r.success) {
                                        actionResultText.text = qsTr("执行成功: ") + (r.message || "") + qsTr("\n数据: ") + JSON.stringify(r.data, null, 2)
                                    } else {
                                        actionResultText.text = qsTr("执行失败: ") + (r.message || qsTr("未知错误"))
                                    }
                                } catch(e) {
                                    actionResultText.text = qsTr("解析结果失败: ") + result
                                }
                            }
                            contentItem: AppText {
                                text: parent.text
                                color: root.appTheme ? root.appTheme.primaryText : "#ffffff"
                                font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                            }
                            background: Rectangle {
                                color: parent.hovered
                                    ? (root.appTheme ? root.appTheme.primaryHover : "#60a5fa")
                                    : (root.appTheme ? root.appTheme.primary : "#3b82f6")
                                radius: root.appTheme ? root.appTheme.radiusSm : 6
                            }
                        }
                        Button {
                            text: qsTr("应用 (Mutation)")
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
                                        actionResultText.text = qsTr("应用成功: ") + (r.message || "") + qsTr("\n数据: ") + JSON.stringify(r.data, null, 2)
                                    } else {
                                        actionResultText.text = qsTr("应用失败: ") + (r.message || qsTr("未知错误"))
                                    }
                                } catch(e) {
                                    actionResultText.text = qsTr("解析结果失败: ") + result
                                }
                            }
                            contentItem: AppText {
                                text: parent.text
                                color: root.appTheme ? root.appTheme.primaryText : "#ffffff"
                                font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                            }
                            background: Rectangle {
                                color: parent.enabled
                                    ? (parent.hovered
                                        ? (root.appTheme ? root.appTheme.primaryHover : "#60a5fa")
                                        : (root.appTheme ? root.appTheme.primary : "#3b82f6"))
                                    : (root.appTheme ? root.appTheme.border : "#e2e8f0")
                                radius: root.appTheme ? root.appTheme.radiusSm : 6
                            }
                        }
                        Text {
                            text: qsTr("危险操作已阻断")
                            visible: modelData.kind === "mutation" && (modelData.riskLevel === "dangerous" || modelData.riskLevel === "contentWrite")
                            color: root.appTheme ? root.appTheme.danger : "#ef4444"
                            font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                        }
                    }
                }
            }
        }

        // Result display
        Rectangle {
            Layout.fillWidth: true
            height: 120
            color: root.appTheme ? root.appTheme.surfaceAlt : "#f1f5f9"
            radius: root.appTheme ? root.appTheme.radiusSm : 6
            border.color: root.appTheme ? root.appTheme.border : "#e2e8f0"
            border.width: 1

            TextArea {
                id: actionResultText
                anchors.fill: parent
                anchors.margins: root.appTheme ? root.appTheme.sp8 : 8
                readOnly: true
                wrapMode: Text.Wrap
                color: root.appTheme ? root.appTheme.textPrimary : "#0f172a"
                font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                font.family: "monospace"
                placeholderText: qsTr("执行结果将显示在此处...")
                background: Rectangle { color: "transparent" }
                placeholderTextColor: root.appTheme ? root.appTheme.textSecondary : "#475569"
            }
        }

        // Workspace diagnostics
        SectionHeader { theme: root.appTheme; text: qsTr("工作区诊断") }
        Rectangle {
            Layout.fillWidth: true
            height: 320
            radius: root.appTheme ? root.appTheme.radiusMd : 8
            color: root.appTheme ? root.appTheme.surface : "#ffffff"
            border.color: root.appTheme ? root.appTheme.border : "#e2e8f0"
            border.width: 1
            ColumnLayout {
                anchors.fill: parent
                anchors.margins: root.appTheme ? root.appTheme.sp12 : 12
                spacing: root.appTheme ? root.appTheme.sp8 : 8
                Text {
                    text: qsTr("获取当前工作区的详细状态信息，可用于排查新建作品失败等问题。")
                    font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                    color: root.appTheme ? root.appTheme.textSecondary : "#475569"
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: root.appTheme ? root.appTheme.sp8 : 8
                    Button {
                        text: qsTr("获取工作区诊断")
                        implicitHeight: 32; implicitWidth: 130
                        onClicked: {
                            var diag = root.backendRef.get_workspace_diagnostics()
                            try {
                                var obj = JSON.parse(diag)
                                if (obj.success) {
                                    workspaceDiagText.text = JSON.stringify(obj.data, null, 2)
                                } else {
                                    workspaceDiagText.text = qsTr("诊断失败: ") + (obj.userMessage || obj.errorCode || qsTr("未知错误"))
                                }
                            } catch(e) {
                                workspaceDiagText.text = qsTr("解析诊断失败: ") + e
                            }
                        }
                        contentItem: AppText {
                            text: parent.text
                            color: root.appTheme ? root.appTheme.primaryText : "#ffffff"
                            font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }
                        background: Rectangle {
                            color: parent.hovered ? (root.appTheme ? root.appTheme.primaryHover : "#60a5fa") : (root.appTheme ? root.appTheme.primary : "#3b82f6")
                            radius: root.appTheme ? root.appTheme.radiusSm : 6
                        }
                    }
                    Button {
                        text: qsTr("复制诊断")
                        implicitHeight: 32; flat: true
                        onClicked: {
                            if (workspaceDiagText.text.length > 0 && root.backendRef) {
                                var result = JSON.parse(root.backendRef.copy_text_to_clipboard(workspaceDiagText.text))
                                workspaceDiagText.text = result.success ? qsTr("诊断已复制") : (qsTr("复制失败: ") + (result.userMessage || result.message || ""))
                            }
                        }
                        contentItem: AppText {
                            text: parent.text
                            color: root.appTheme ? root.appTheme.textSecondary : "#475569"
                            font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }
                        background: Rectangle {
                            color: parent.hovered ? (root.appTheme ? root.appTheme.hover : "#f1f5f9") : "transparent"
                            radius: root.appTheme ? root.appTheme.radiusSm : 6
                        }
                    }
                }
                ScrollView {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    TextArea {
                        id: workspaceDiagText
                        width: parent.width
                        readOnly: true
                        selectByMouse: true
                        wrapMode: TextArea.Wrap
                        font.family: "monospace"
                        font.pixelSize: root.appTheme ? root.appTheme.fontXs : 11
                        color: root.appTheme ? root.appTheme.textPrimary : "#0f172a"
                        background: Rectangle {
                            color: root.appTheme ? root.appTheme.surfaceAlt : "#f1f5f9"
                            radius: root.appTheme ? root.appTheme.radiusSm : 6
                            border.color: root.appTheme ? root.appTheme.border : "#e2e8f0"
                            border.width: 1
                        }
                        leftPadding: 8; topPadding: 8; rightPadding: 8; bottomPadding: 8
                        placeholderText: qsTr("点击「获取工作区诊断」查看详情...")
                        placeholderTextColor: root.appTheme ? root.appTheme.textSecondary : "#475569"
                    }
                }
            }
        }
        Item { height: root.appTheme ? root.appTheme.sp16 : 16 }
    }
}
