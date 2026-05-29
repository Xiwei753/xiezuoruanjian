// =============================================================================
// CreateProjectDialog.qml — 新建作品对话框
// =============================================================================
//
// 层级：Linux UI 层（QML UI 组件）
// 职责：新建作品的输入对话框，收集作品标题
// 约束：
//   - 纯 UI 组件，创建操作通过 signal 传递给 main.qml
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: root
    title: "新建作品"
    modal: true
    x: (parent ? (parent.width - width) / 2 : 0)
    y: (parent ? (parent.height - height) / 2 : 0)
    standardButtons: Dialog.Ok | Dialog.Cancel

    property var theme: null
    signal submitProject(string title)

    width: 400
    height: 220

    background: Rectangle {
        color: theme ? theme.bgDark : "#1E1E1E"
        border.color: theme ? theme.border : "#333333"
        radius: 8
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 16
        spacing: 16

        Text {
            text: "请输入作品名称："
            color: theme ? theme.textMain : "#E0E0E0"
            font.pixelSize: 14
        }

        TextField {
            id: titleField
            Layout.fillWidth: true
            placeholderText: "作品名称"
            color: theme ? theme.textMain : "#E0E0E0"
            background: Rectangle {
                color: theme ? theme.inputBg : "#2A2A2A"
                border.color: titleField.activeFocus ? (theme ? theme.accent : "#82AAFF") : (theme ? theme.border : "#444444")
                radius: 4
            }
            onAccepted: {
                if (text.trim() !== "") {
                    root.accept();
                }
            }
        }
    }

    onOpened: {
        titleField.text = "";
        titleField.forceActiveFocus();
    }

    onAccepted: {
        root.submitProject(titleField.text.trim());
    }
}
