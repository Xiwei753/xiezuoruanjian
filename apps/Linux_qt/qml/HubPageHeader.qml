// =============================================================================
// HubPageHeader.qml — Hub 页面头部
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
// 职责：Hub 页面的头部区域，包含标题、副标题和操作按钮
// 约束：
//   - 纯 UI 组件，操作通过 signal 传递
// Issue #900：迁移到 KDE/Breeze（Kirigami.Heading / Controls.Label /
// Controls.Button），颜色与间距取自 Kirigami.Theme / Kirigami.Units。
// 公共 API（dt / resolvedDt / title / subtitle / actionText / actionClicked）保持不变。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

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
        spacing: Kirigami.Units.largeSpacing

        ColumnLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Heading {
                Layout.fillWidth: true
                level: 1
                text: root.title
                elide: Text.ElideRight
            }

            Label {
                Layout.fillWidth: true
                text: root.subtitle
                color: Kirigami.Theme.disabledTextColor
                wrapMode: Text.Wrap
                visible: text.length > 0
            }
        }

        Button {
            visible: root.actionText.length > 0
            text: root.actionText
            highlighted: true
            onClicked: root.actionClicked()
        }
    }
}
