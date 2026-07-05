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
    property var dt: null

    Component.onCompleted: {
        if (!dt) console.warn("[DesignTokens] HubPageHeader created without dt — caller must pass dt property")
    }

    property string title: ""
    property string subtitle: ""
    property string actionText: ""
    signal actionClicked()

    RowLayout {
        anchors.fill: parent
        spacing: dt ? dt.sp16 : 16

        ColumnLayout {
            Layout.fillWidth: true
            spacing: dt ? dt.sp6 : 6

            AppText {
                dt: root.dt
                text: root.title
                color: dt ? dt.onBackground : "#E2E2E5"
                font.pixelSize: dt ? dt.fontTitle : 22
                font.family: dt ? dt.fontFamily : "sans-serif"
                font.weight: Font.Bold
            }

            AppText {
                dt: root.dt
                text: root.subtitle
                color: dt ? dt.textSecondary : "#B0B0B0"
                font.pixelSize: dt ? dt.body : 14
                font.family: dt ? dt.fontFamily : "sans-serif"
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
