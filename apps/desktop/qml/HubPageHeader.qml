// =============================================================================
// HubPageHeader.qml — Hub 页面头部
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：Hub 页面的头部区域，包含标题、副标题和操作按钮
// 约束：
//   - 纯 UI 组件，操作通过 signal 传递
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Layouts

Item {
    id: root
    required property var dt
    property string title: ""
    property string subtitle: ""
    property string actionText: ""
    signal actionClicked()

    RowLayout {
        anchors.fill: parent
        spacing: dt.sp16

        ColumnLayout {
            Layout.fillWidth: true
            spacing: dt.sp6

            AppText {
                dt: root.dt
                text: root.title
                color: dt.onBackground
                font.pixelSize: dt.fontTitle
                font.family: dt.fontFamily
                font.weight: Font.Bold
            }

            AppText {
                dt: root.dt
                text: root.subtitle
                color: dt.textSecondary
                font.pixelSize: dt.body
                font.family: dt.fontFamily
                visible: text.length > 0
            }
        }

        AppButton {
            visible: root.actionText.length > 0
            text: root.actionText
            dt: root.dt
            variant: "primary"
            onClicked: root.actionClicked()
        }
    }
}
