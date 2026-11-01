// =============================================================================
// AiToolPanel.qml — 写作侧栏 / 星图右栏共用的 AI 面板
// =============================================================================
//
// Issue #1018：顶部直接选择本机已安装 CLI，下面是会话和模型选择；不再内嵌
// “可安装目录”式的 Agent 管理器。写作侧栏与星图右栏共用同一个
// AgentCliBackend / AgentRuntimeBackend，因此选择状态天然一致。

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
    // Issue #1028：星图面板的真实画布身份（进入子图后 active 是子图 ID）。
    property string starmapRootId: ""
    property string starmapActiveId: ""
    property string starmapPathJson: "[]"
    property string starmapRouteKey: ""
    property string contextLabel: ""

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.smallSpacing
        spacing: Kirigami.Units.smallSpacing

        AgentCliPicker {
            Layout.fillWidth: true
            backendRef: root.agentCliBackendRef
            dt: root.dt
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Button {
                text: qsTr("打开终端")
                onClicked: if (root.agentCliBackendRef)
                    root.agentCliBackendRef.open_terminal()
            }

            Button {
                text: qsTr("复制 MCP 接入信息")
                enabled: root.runtimeBackendRef !== null
                onClicked: if (root.runtimeBackendRef)
                    root.runtimeBackendRef.copy_mcp_config()
            }

            Item { Layout.fillWidth: true }
        }

        AiConversationView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            dt: root.dt
            runtimeBackendRef: root.runtimeBackendRef
            agentCliBackendRef: root.agentCliBackendRef
            workspaceProjectId: root.workspaceProjectId
            workspaceProjectTitle: root.workspaceProjectTitle
            starmapRootId: root.starmapRootId
            starmapActiveId: root.starmapActiveId
            starmapPathJson: root.starmapPathJson
            starmapRouteKey: root.starmapRouteKey
            contextLabel: root.contextLabel
        }
    }
}
