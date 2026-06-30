// =============================================================================
// CreateProjectDialog.qml — 新建作品对话框
// =============================================================================
//
// 层级：Desktop UI 层（QML UI 组件）
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
        color: theme.surface
        border.color: theme.border
        border.width: 1
        radius: theme.radiusXl
    }
    header: null

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: theme.sp24
        spacing: theme.sp16

        AppText {
            text: qsTr("新建作品")
            color: theme.textPrimary
            font.pixelSize: theme.subtitle
            font.family: theme.fontFamily
            font.weight: Font.DemiBold
        }

        AppText {
            text: qsTr("请输入作品名称：")
            color: theme.onSurfaceVariant
            font.pixelSize: theme.body
            font.family: theme.fontFamily
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
            AppButton { text: qsTr("取消"); dt: root.theme; variant: "text"; onClicked: root.reject() }
            AppButton { text: qsTr("创建"); dt: root.theme; variant: "primary"; onClicked: { if (titleField.text.trim() !== "") root.accept() } }
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
