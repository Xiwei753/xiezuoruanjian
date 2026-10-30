// =============================================================================
// AiToolPanel.qml — 写作侧栏 / 星图右栏共用的 AI 面板
// =============================================================================
//
// Issue #1016：真正的会话主体是 AiConversationView；AgentCliManager 只作为
// 可展开的本机 Agent 管理区，与设置页共享同一个 AgentCliBackend。

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root

    required property var dt
    property var agentCliBackendRef: null
    property var runtimeBackendRef: null
    property string workspaceProjectId: ""
    property string workspaceProjectTitle: ""
    property string contextLabel: ""
    property bool managingAgents: false

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.smallSpacing
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Label {
                Layout.fillWidth: true
                text: qsTr("AI 助手")
                font.weight: Font.DemiBold
            }

            ToolButton {
                icon.name: "configure"
                display: AbstractButton.IconOnly
                checkable: true
                checked: root.managingAgents
                ToolTip.visible: hovered
                ToolTip.text: qsTr("管理本机 Agent")
                onClicked: root.managingAgents = checked
            }
        }

        // 本机 Agent 管理区：展开时才占用高度，默认让会话视图占满。
        ScrollView {
            Layout.fillWidth: true
            Layout.preferredHeight: root.managingAgents
                ? Math.min(Kirigami.Units.gridUnit * 14, root.height * 0.42) : 0
            visible: root.managingAgents
            clip: true
            contentWidth: availableWidth
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
            Layout.preferredHeight: 1
            visible: root.managingAgents
            color: Kirigami.Theme.disabledTextColor
            opacity: 0.35
        }

        AiConversationView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            dt: root.dt
            runtimeBackendRef: root.runtimeBackendRef
            agentCliBackendRef: root.agentCliBackendRef
            workspaceProjectId: root.workspaceProjectId
            workspaceProjectTitle: root.workspaceProjectTitle
            contextLabel: root.contextLabel
        }
    }
}
