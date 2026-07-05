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
    DesignTokens { id: fallbackDt }
    readonly property var resolvedDt: dt || fallbackDt

    property string title: ""
    property string subtitle: ""
    property string actionText: ""
    signal actionClicked()

    RowLayout {
        anchors.fill: parent
        spacing: root.resolvedDt.sp16

        ColumnLayout {
            Layout.fillWidth: true
            spacing: root.resolvedDt.sp6

            AppText {
                dt: root.resolvedDt
                text: root.title
                color: root.resolvedDt.onBackground
                font.pixelSize: root.resolvedDt.fontTitle
                font.family: root.resolvedDt.fontFamily
                font.weight: Font.Bold
            }

            AppText {
                dt: root.resolvedDt
                text: root.subtitle
                color: root.resolvedDt.textSecondary
                font.pixelSize: root.resolvedDt.body
                font.family: root.resolvedDt.fontFamily
                visible: text.length > 0
            }
        }

        AppButton {
            visible: root.actionText.length > 0
            text: root.actionText
            dt: root.resolvedDt
            variant: "primary"
            onClicked: root.actionClicked()
        }
    }
}
