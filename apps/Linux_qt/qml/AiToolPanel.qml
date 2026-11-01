// =============================================================================
// AiToolPanel.qml — 写作侧栏 / 星图右栏共用的 AI 面板
// =============================================================================
//
// Issue #1030：面板顶部不再占用 CLI、终端、MCP 等工具入口；整个面板只保留
// 会话正文与底部输入工作台，工具收进输入框下方的「更多操作」菜单。

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
    // Issue #1028：只有当前可见的 AI 面板才能更新共享 Runtime 的 scope。
    property bool scopeActive: false
    property string scopeOwner: ""
    property string contextLabel: ""

    AiConversationView {
        anchors.fill: parent
        dt: root.dt
        runtimeBackendRef: root.runtimeBackendRef
        agentCliBackendRef: root.agentCliBackendRef
        workspaceProjectId: root.workspaceProjectId
        workspaceProjectTitle: root.workspaceProjectTitle
        starmapRootId: root.starmapRootId
        starmapActiveId: root.starmapActiveId
        starmapPathJson: root.starmapPathJson
        starmapRouteKey: root.starmapRouteKey
        scopeActive: root.scopeActive
        scopeOwner: root.scopeOwner
        contextLabel: root.contextLabel
    }
}
