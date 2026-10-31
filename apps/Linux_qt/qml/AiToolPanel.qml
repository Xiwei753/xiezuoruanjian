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
