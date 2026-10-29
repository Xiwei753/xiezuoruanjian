// =============================================================================
// AiToolPanel.qml — 写作侧栏 AI 面板
// =============================================================================
//
// Issue #1014：AI 入口先提供真实的 Agent CLI 清单/选择；真正的会话 UI 由
// Issue #506 接入。这里不伪造聊天内容，只负责把选中的 Agent 和启动路径
// 展示给用户，并与设置页共享同一个 AgentCliBackend。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root

    required property var dt
    property var agentCliBackendRef: null

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.smallSpacing
        spacing: Kirigami.Units.smallSpacing

        Label {
            Layout.fillWidth: true
            text: qsTr("AI 助手")
            font.weight: Font.DemiBold
        }

        Label {
            Layout.fillWidth: true
            visible: root.agentCliBackendRef
                && root.agentCliBackendRef.selected_agent_id !== ""
            text: root.agentCliBackendRef
                ? qsTr("当前 Agent：%1").arg(root.agentCliBackendRef.selected_agent_id)
                : ""
            wrapMode: Text.Wrap
        }

        Label {
            Layout.fillWidth: true
            visible: root.agentCliBackendRef
                && root.agentCliBackendRef.selected_launch_path !== ""
            text: root.agentCliBackendRef ? root.agentCliBackendRef.selected_launch_path : ""
            elide: Text.ElideMiddle
            font.family: "monospace"
            font.pixelSize: Kirigami.Theme.smallFont.pixelSize
            color: Kirigami.Theme.disabledTextColor
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            contentWidth: availableWidth
            // Issue #782：桌面鼠标左键不拖页面，保留滚轮滚动。
            Component.onCompleted: {
                if (contentItem)
                    contentItem.acceptedButtons = Qt.NoButton
            }

            AgentCliManager {
                width: parent.width
                backendRef: root.agentCliBackendRef
            }
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: placeholderLayout.implicitHeight + 2 * Kirigami.Units.smallSpacing
            radius: Kirigami.Units.smallSpacing
            color: Kirigami.Theme.alternateBackgroundColor
            border.width: 1
            border.color: Kirigami.Theme.disabledTextColor

            ColumnLayout {
                id: placeholderLayout
                anchors.fill: parent
                anchors.margins: Kirigami.Units.smallSpacing
                spacing: Kirigami.Units.smallSpacing / 2

                Label {
                    Layout.fillWidth: true
                    text: qsTr("AI 会话尚未接入")
                    font.weight: Font.DemiBold
                }

                Label {
                    Layout.fillWidth: true
                    text: qsTr("这里先完成 Agent CLI 的发现与选择。真正的对话界面由 Issue #506 接入；登录沿用所选 CLI 自己的账户，不在素笺内保存凭据。")
                    wrapMode: Text.Wrap
                    color: Kirigami.Theme.disabledTextColor
                    font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                }
            }
        }
    }
}
