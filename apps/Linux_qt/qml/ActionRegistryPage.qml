// =============================================================================
// ActionRegistryPage.qml — Action 注册表调试页面
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：列出所有注册的 Action，支持执行 Query/Mutation 类型操作
// Issue #900：视觉迁移到 Breeze/Kirigami —— 手绘卡片/按钮改为 Controls.Button +
// Controls.ListView + Kirigami Addons Delegates.RoundedItemDelegate，颜色/间距走
// Kirigami.Theme.* / Kirigami.Units.*，不再消费 DesignTokens M3 字段。
// 约束：
//   - 调试用途，不面向普通用户
//   - 通过 editorBackendRef 调用 AppBackend (Rust QObject) 执行 Action
//   - appTheme 等注入属性保留声明（公开 API 不变），仅内部不再读其 M3 字段
// =============================================================================

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.kirigamiaddons.delegates as Delegates

Controls.ScrollView {
    id: root
    clip: true
    // Issue #782 评论 5855709706: 桌面鼠标左键不能按住空白处拖页面。
    Component.onCompleted: {
        if (contentItem) contentItem.acceptedButtons = Qt.NoButton
    }

    property var editorBackendRef: null
    property var appTheme: null
    property real fontSizeSpinValue: 16
    property bool autoSaveCheckChecked: false
    property real autoSaveDelaySpinValue: 1500
    property real lineSpacingSpinValue: 150
    property bool autoIndentCheckChecked: false
    property real autoIndentWidthSpinValue: 200
    property bool typingAnimCheckChecked: false
    property bool smoothCursorCheckChecked: false

    // 已注册 Action 列表（原 Repeater.model，改用 ListView 后作为页面模型）。
    property var actionList: []

    /// 列出所有注册的 Action。
    function listActions() {
        if (!root.editorBackendRef)
            return
        var result = root.editorBackendRef.list_registered_actions()
        try {
            var actions = JSON.parse(result)
            root.actionList = actions
            actionResultText.text = qsTr("共加载 ") + actions.length + qsTr(" 个 Action")
        } catch (e) {
            actionResultText.text = qsTr("解析 Action 列表失败: ") + e
        }
    }

    /// 清空列表与结果。
    function clearResults() {
        root.actionList = []
        actionResultText.text = ""
    }

    /// 执行 Query/Preview 类型 Action。
    function runQuery(actionId) {
        if (!root.editorBackendRef)
            return
        var result = root.editorBackendRef.execute_action(actionId, "", "")
        try {
            var r = JSON.parse(result)
            if (r.success) {
                actionResultText.text = qsTr("执行成功: ") + (r.message || "") + qsTr("\n数据: ") + JSON.stringify(r.data, null, 2)
            } else {
                actionResultText.text = qsTr("执行失败: ") + (r.message || qsTr("未知错误"))
            }
        } catch (e) {
            actionResultText.text = qsTr("解析结果失败: ") + result
        }
    }

    /// 应用 Mutation 类型 Action（危险类由调用处禁用按钮，不在此重复判断）。
    function applyMutation(actionId) {
        if (!root.editorBackendRef)
            return
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
        var result = root.editorBackendRef.execute_action(actionId, args, "")
        try {
            var r = JSON.parse(result)
            if (r.success) {
                actionResultText.text = qsTr("应用成功: ") + (r.message || "") + qsTr("\n数据: ") + JSON.stringify(r.data, null, 2)
            } else {
                actionResultText.text = qsTr("应用失败: ") + (r.message || qsTr("未知错误"))
            }
        } catch (e) {
            actionResultText.text = qsTr("解析结果失败: ") + result
        }
    }

    ColumnLayout {
        width: Math.min(Kirigami.Units.gridUnit * 30, root.availableWidth - Kirigami.Units.smallSpacing)
        spacing: Kirigami.Units.largeSpacing

        // Header
        Kirigami.Heading {
            Layout.fillWidth: true
            level: 2
            text: qsTr("Action 调试")
        }

        Controls.Label {
            Layout.fillWidth: true
            text: qsTr("列出所有已注册的 Action，可执行 Query 类型或查看 Mutation 描述。")
            color: Kirigami.Theme.disabledTextColor
            wrapMode: Text.Wrap
        }

        // Action buttons
        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Controls.Button {
                text: qsTr("列出所有 Action")
                icon.name: "view-refresh"
                highlighted: true
                onClicked: root.listActions()
            }

            Controls.Button {
                text: qsTr("清空结果")
                icon.name: "edit-clear"
                onClicked: root.clearResults()
            }

            Item { Layout.fillWidth: true }
        }

        // Action list
        ListView {
            id: actionListView
            Layout.fillWidth: true
            Layout.preferredHeight: actionListView.contentHeight
            clip: true
            interactive: false
            spacing: 0
            model: root.actionList

            delegate: Delegates.RoundedItemDelegate {
                id: actionDelegate
                required property var modelData
                width: ListView.view.width

                contentItem: ColumnLayout {
                    spacing: Kirigami.Units.smallSpacing

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.smallSpacing

                        Controls.Label {
                            Layout.fillWidth: true
                            text: actionDelegate.modelData.title || actionDelegate.modelData.id || ""
                            color: Kirigami.Theme.textColor
                            font.bold: true
                            elide: Text.ElideRight
                        }

                        Controls.Label {
                            text: {
                                var risk = actionDelegate.modelData.riskLevel || ""
                                if (risk === "dangerous") return qsTr("危险")
                                if (risk === "contentWrite") return qsTr("内容写入")
                                if (risk === "safeWrite") return qsTr("写入")
                                return qsTr("只读")
                            }
                            color: {
                                var risk = actionDelegate.modelData.riskLevel || ""
                                if (risk === "dangerous") return Kirigami.Theme.negativeTextColor
                                if (risk === "contentWrite") return Kirigami.Theme.neutralTextColor
                                if (risk === "safeWrite") return Kirigami.Theme.positiveTextColor
                                return Kirigami.Theme.disabledTextColor
                            }
                        }
                    }

                    Controls.Label {
                        Layout.fillWidth: true
                        text: actionDelegate.modelData.id || ""
                        color: Kirigami.Theme.disabledTextColor
                        font.family: "monospace"
                        elide: Text.ElideRight
                    }

                    Controls.Label {
                        Layout.fillWidth: true
                        text: actionDelegate.modelData.description || ""
                        color: Kirigami.Theme.disabledTextColor
                        wrapMode: Text.Wrap
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.smallSpacing

                        Controls.Button {
                            text: qsTr("执行")
                            visible: actionDelegate.modelData.kind === "query" || actionDelegate.modelData.kind === "preview"
                            onClicked: root.runQuery(actionDelegate.modelData.id)
                        }

                        Controls.Button {
                            text: qsTr("应用 (Mutation)")
                            visible: actionDelegate.modelData.kind === "mutation"
                            enabled: {
                                var risk = actionDelegate.modelData.riskLevel || ""
                                return risk !== "dangerous" && risk !== "contentWrite"
                            }
                            onClicked: root.applyMutation(actionDelegate.modelData.id)
                        }

                        Controls.Label {
                            text: qsTr("危险操作已阻断")
                            visible: actionDelegate.modelData.kind === "mutation"
                                && (actionDelegate.modelData.riskLevel === "dangerous" || actionDelegate.modelData.riskLevel === "contentWrite")
                            color: Kirigami.Theme.negativeTextColor
                        }

                        Item { Layout.fillWidth: true }
                    }
                }
            }
        }

        // Result display
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: Kirigami.Units.gridUnit * 8
            color: Kirigami.Theme.alternateBackgroundColor
            border.color: Kirigami.Theme.disabledTextColor
            border.width: 1
            radius: Kirigami.Units.smallSpacing

            Controls.TextArea {
                id: actionResultText
                anchors.fill: parent
                anchors.margins: Kirigami.Units.smallSpacing
                readOnly: true
                wrapMode: TextEdit.Wrap
                color: Kirigami.Theme.textColor
                font.family: "monospace"
                placeholderText: qsTr("执行结果将显示在此处...")
                placeholderTextColor: Kirigami.Theme.disabledTextColor
                background: null
            }
        }

        Item { Layout.preferredHeight: Kirigami.Units.largeSpacing }
    }
}
