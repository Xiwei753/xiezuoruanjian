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
    title: qsTr("新建作品")
    modal: true
    x: (parent ? (parent.width - width) / 2 : 0)
    y: (parent ? (parent.height - height) / 2 : 0)
    standardButtons: Dialog.NoButton

    property var theme: null
    signal submitProject(string title)

    width: 400
    height: 220

    background: Rectangle {
        color: theme ? theme.surface : "#FCFCFF"
        border.color: theme ? theme.border : "#333333"
        border.width: 1
        radius: theme ? theme.radiusXl : 24
    }
    header: null

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: theme ? theme.sp24 : 24
        spacing: theme ? theme.sp16 : 16

        Text {
            text: qsTr("新建作品")
            color: theme ? theme.textPrimary : root.palette.text
            font.pixelSize: theme ? theme.subtitle : 18
            font.family: theme ? theme.fontFamily : "sans-serif"
            font.weight: Font.DemiBold
        }

        Text {
            text: qsTr("请输入作品名称：")
            color: theme ? theme.onSurfaceVariant : "#42474E"
            font.pixelSize: theme ? theme.body : 14
            font.family: theme ? theme.fontFamily : "sans-serif"
        }

        AppTextField {
            id: titleField
            Layout.fillWidth: true
            theme: root.theme
            placeholderText: qsTr("作品名称")
            onAccepted: {
                if (text.trim() !== "") {
                    root.accept();
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Item { Layout.fillWidth: true }
            AppButton { text: qsTr("取消"); theme: root.theme; variant: "text"; onClicked: root.reject() }
            AppButton { text: qsTr("创建"); theme: root.theme; variant: "primary"; onClicked: { if (titleField.text.trim() !== "") root.accept() } }
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
