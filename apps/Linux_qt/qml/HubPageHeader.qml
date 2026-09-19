// =============================================================================
// HubPageHeader.qml — Hub 页面头部
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
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
    // Issue #701 评论 5699565102: 删除内部 fallbackDt。组件必须消费调用方
    // 传入的根 dt；漏传就是调用错误，不偷偷生成独立主题。
    // Issue #715: 改为 required property，不再允许组件先以 null 创建、
    // 随后才补主题。
    readonly property var resolvedDt: dt

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
                font.pointSize: root.resolvedDt.fontTitlePt
                font.family: root.resolvedDt.fontFamily
                font.weight: Font.Bold
            }

            AppText {
                dt: root.resolvedDt
                text: root.subtitle
                color: root.resolvedDt.textSecondary
                font.pointSize: root.resolvedDt.bodyPt
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
