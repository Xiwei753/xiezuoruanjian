// =============================================================================
// AiConversationView.qml — 写作与星图共用的 AI 会话视图
// =============================================================================
//
// Issue #1016：界面只消费 AgentRuntimeBackend 的标准化会话/事件，不解析任何
// ACP / app-server / stream-json 原始字段。历史来自 Core，流式回答、工具过程、
// 权限请求和中断都由同一个运行时对象处理。

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root

    required property var dt
    property var runtimeBackendRef: null
    property var agentCliBackendRef: null
    property string workspaceProjectId: ""
    property string workspaceProjectTitle: ""
    // 星图工作区可传“当前星图对象”，写作区传当前作品。
    property string contextLabel: ""

    readonly property var conversation: parseJson(
        runtimeBackendRef ? runtimeBackendRef.conversation_json : "null", null)
    readonly property var conversationList: parseJson(
        runtimeBackendRef ? runtimeBackendRef.conversations_json : "[]", [])
    readonly property var messages: buildMessages(conversation)
    readonly property var permission: parseJson(
        runtimeBackendRef ? runtimeBackendRef.permission_json : "null", null)
    readonly property var mcpStatus: parseJson(
        runtimeBackendRef ? runtimeBackendRef.mcp_status_json : "{}", ({}))
    readonly property bool running: runtimeBackendRef ? runtimeBackendRef.running : false
    readonly property bool hasAgent: agentId() !== ""
    readonly property string currentModel: conversation && conversation.model
        ? String(conversation.model) : ""

    function parseJson(text, fallback) {
        if (!text)
            return fallback
        try {
            var value = JSON.parse(text)
            return value === null || value === undefined ? fallback : value
        } catch (error) {
            return fallback
        }
    }

    function selectedAgentDescriptor() {
        if (!agentCliBackendRef)
            return null
        var inventory = parseJson(agentCliBackendRef.inventory_json, ({}))
        var selected = String(agentCliBackendRef.selected_agent_id || "")
        if (!selected || !inventory.installed)
            return null
        for (var i = 0; i < inventory.installed.length; ++i) {
            var installation = inventory.installed[i]
            if (installation.descriptor && installation.descriptor.id === selected)
                return installation.descriptor
        }
        return null
    }

    function agentId() {
        return agentCliBackendRef ? String(agentCliBackendRef.selected_agent_id || "") : ""
    }

    function agentLaunchPath() {
        return agentCliBackendRef ? String(agentCliBackendRef.selected_launch_path || "") : ""
    }

    function syncAgentSelection() {
        if (!runtimeBackendRef || !agentCliBackendRef)
            return
        var descriptor = selectedAgentDescriptor()
        var id = agentId()
        if (!descriptor || !id) {
            runtimeBackendRef.set_agent("", "", "", "", "[]")
            return
        }
        var launchArgs = descriptor.launch && descriptor.launch.args
            ? descriptor.launch.args : []
        runtimeBackendRef.set_agent(
            id,
            String(descriptor.name || id),
            agentLaunchPath(),
            String(descriptor.protocol || "acp"),
            JSON.stringify(launchArgs))
    }

    function syncProjectContext() {
        if (!runtimeBackendRef)
            return
        runtimeBackendRef.set_project(workspaceProjectId, workspaceProjectTitle)
    }

    function syncAgentCatalog() {
        if (runtimeBackendRef && agentCliBackendRef)
            runtimeBackendRef.set_agent_catalog(agentCliBackendRef.inventory_json)
    }

    // 打开历史会话后，把设置页/右栏共用的选择同步到该会话真实使用的安装。
    function syncCliSelectionFromConversation() {
        if (!runtimeBackendRef || !agentCliBackendRef || !conversation)
            return
        var id = String(conversation.agentId || "")
        var path = String(conversation.launchPath || "")
        if (id === "" || path === "")
            return
        if (agentId() !== id || agentLaunchPath() !== path)
            agentCliBackendRef.select_agent(id, path)
    }

    function buildMessages(value) {
        if (!value || !value.turns)
            return []
        var rows = []
        for (var i = 0; i < value.turns.length; ++i) {
            var turn = value.turns[i]
            if (turn.userMessage)
                rows.push({ kind: "user", text: String(turn.userMessage), name: "" })
            var parts = turn.assistantParts || []
            for (var p = 0; p < parts.length; ++p) {
                var part = parts[p]
                rows.push({
                    kind: part.kind === "reasoning" ? "reasoning" : "assistant",
                    text: String(part.text || ""),
                    name: ""
                })
            }
            var tools = turn.toolEvents || []
            for (var t = 0; t < tools.length; ++t) {
                rows.push({
                    kind: "tool",
                    text: String(tools[t].outputJson || tools[t].inputJson || ""),
                    name: String(tools[t].name || "工具"),
                    status: String(tools[t].status || "")
                })
            }
            if (turn.error)
                rows.push({ kind: "error", text: String(turn.error), name: "" })
        }
        return rows
    }

    function conversationIndex() {
        var current = runtimeBackendRef
            ? String(runtimeBackendRef.current_conversation_id || "") : ""
        for (var i = 0; i < conversationList.length; ++i) {
            if (String(conversationList[i].id) === current)
                return i
        }
        return -1
    }

    function modelChoices() {
        var models = parseJson(runtimeBackendRef ? runtimeBackendRef.models_json : "[]", [])
        if (currentModel && models.indexOf(currentModel) < 0)
            models.push(currentModel)
        return models
    }

    onRuntimeBackendRefChanged: refresh()
    onAgentCliBackendRefChanged: {
        syncAgentCatalog()
        syncAgentSelection()
        refresh()
    }
    onWorkspaceProjectIdChanged: {
        syncProjectContext()
        refresh()
    }
    Component.onCompleted: {
        syncAgentCatalog()
        syncAgentSelection()
        syncProjectContext()
        refresh()
    }

    Connections {
        target: root.agentCliBackendRef
        function onSelected_agent_changed() { root.syncAgentSelection() }
        function onInventory_changed() {
            root.syncAgentCatalog()
            root.syncAgentSelection()
        }
    }

    Connections {
        target: root.runtimeBackendRef
        function onConversation_changed() {
            root.refresh()
            root.syncCliSelectionFromConversation()
        }
        function onAgent_changed() { root.syncCliSelectionFromConversation() }
        function onConversations_changed() { root.refresh() }
        function onRunning_changed() { root.refresh() }
        function onPermission_changed() { root.refresh() }
        function onLast_error_changed() { root.refresh() }
    }

    function refresh() {
        messageList.model = root.messages
        if (messageList.count > 0)
            Qt.callLater(function() { messageList.positionViewAtEnd() })
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: Kirigami.Units.smallSpacing

        // 会话切换 / 新建 / 模型
        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            ComboBox {
                id: conversationPicker
                Layout.fillWidth: true
                Layout.minimumWidth: 80
                model: root.conversationList
                textRole: "title"
                displayText: currentIndex >= 0 && currentText
                    ? currentText : qsTr("新会话")
                currentIndex: root.conversationIndex()
                onActivated: function(index) {
                    if (index >= 0 && root.conversationList[index])
                        root.runtimeBackendRef.open_conversation(
                            String(root.conversationList[index].id))
                }
            }

            ToolButton {
                icon.name: "list-add"
                display: AbstractButton.IconOnly
                ToolTip.visible: hovered
                ToolTip.text: qsTr("新建会话")
                onClicked: root.runtimeBackendRef.new_conversation()
            }
        }

        ComboBox {
            Layout.fillWidth: true
            editable: true
            model: root.modelChoices()
            currentIndex: model.indexOf(root.currentModel)
            displayText: root.currentModel
            onActivated: function(index) {
                root.runtimeBackendRef.choose_model(String(currentText))
            }
            onAccepted: root.runtimeBackendRef.choose_model(String(editText))
            ToolTip.visible: hovered
            ToolTip.text: qsTr("模型（可直接输入 provider/model）")
        }

        // 当前上下文 / Agent 状态
        Label {
            Layout.fillWidth: true
            visible: root.workspaceProjectId !== "" || root.contextLabel !== ""
            text: root.contextLabel !== ""
                ? qsTr("上下文：%1").arg(root.contextLabel)
                : qsTr("上下文：%1").arg(root.workspaceProjectTitle || root.workspaceProjectId)
            elide: Text.ElideRight
            color: Kirigami.Theme.disabledTextColor
            font.pixelSize: Kirigami.Theme.smallFont.pixelSize
        }

        // 会话消息
        ListView {
            id: messageList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: Kirigami.Units.smallSpacing
            model: root.messages
            // 桌面鼠标左键不拖页面，保留滚轮滚动（Issue #782）。
            Component.onCompleted: {
                if (contentItem)
                    contentItem.acceptedButtons = Qt.NoButton
            }

            delegate: Rectangle {
                required property var modelData
                width: ListView.view ? ListView.view.width : 0
                height: bubble.implicitHeight + Kirigami.Units.smallSpacing * 1.2
                radius: Kirigami.Units.smallSpacing
                color: modelData.kind === "user"
                    ? Qt.rgba(Kirigami.Theme.highlightColor.r,
                              Kirigami.Theme.highlightColor.g,
                              Kirigami.Theme.highlightColor.b, 0.16)
                    : modelData.kind === "tool"
                        ? Kirigami.Theme.alternateBackgroundColor
                        : modelData.kind === "error"
                            ? Qt.rgba(1, 0.3, 0.3, 0.12)
                            : "transparent"

                ColumnLayout {
                    id: bubble
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.margins: Kirigami.Units.smallSpacing
                    spacing: 2

                    Label {
                        Layout.fillWidth: true
                        visible: modelData.kind === "tool" || modelData.kind === "reasoning"
                        text: modelData.kind === "tool"
                            ? qsTr("工具：%1（%2）").arg(modelData.name).arg(modelData.status)
                            : qsTr("思考过程")
                        font.bold: true
                        font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                        color: Kirigami.Theme.disabledTextColor
                    }

                    Label {
                        Layout.fillWidth: true
                        text: modelData.text
                        wrapMode: Text.Wrap
                        textFormat: Text.PlainText
                        color: modelData.kind === "error"
                            ? Kirigami.Theme.negativeTextColor
                            : Kirigami.Theme.textColor
                        font.pixelSize: modelData.kind === "tool"
                            || modelData.kind === "reasoning"
                            ? Kirigami.Theme.smallFont.pixelSize
                            : Kirigami.Theme.defaultFont.pixelSize
                        visible: text !== ""
                        opacity: modelData.kind === "reasoning" ? 0.75 : 1.0
                    }
                }
            }

            Label {
                anchors.centerIn: parent
                visible: messageList.count === 0
                width: parent.width - Kirigami.Units.largeSpacing * 2
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                text: root.hasAgent
                    ? qsTr("向 %1 发送第一条消息").arg(String(root.agentCliBackendRef.selected_agent_id || ""))
                    : qsTr("请先在下方“管理本机 Agent”中选择一个已安装的 Agent")
                color: Kirigami.Theme.disabledTextColor
            }
        }

        // 权限请求
        Rectangle {
            Layout.fillWidth: true
            visible: root.permission !== null
            implicitHeight: permissionLayout.implicitHeight + Kirigami.Units.smallSpacing * 2
            radius: Kirigami.Units.smallSpacing
            color: Qt.rgba(1, 0.75, 0.2, 0.16)
            border.width: 1
            border.color: Kirigami.Theme.disabledTextColor

            ColumnLayout {
                id: permissionLayout
                anchors.fill: parent
                anchors.margins: Kirigami.Units.smallSpacing
                spacing: Kirigami.Units.smallSpacing / 2
                Label {
                    Layout.fillWidth: true
                    visible: root.permission !== null
                    text: root.permission ? String(root.permission.description || "") : ""
                    wrapMode: Text.Wrap
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.smallSpacing
                    Repeater {
                        model: root.permission && root.permission.options
                            ? root.permission.options : []
                        Button {
                            required property var modelData
                            text: String(modelData.name || modelData.optionId || "")
                            onClicked: root.runtimeBackendRef.respond_to_permission(
                                String(root.permission.requestId),
                                String(modelData.optionId || ""))
                        }
                    }
                }
            }
        }

        // 错误 / 状态
        Label {
            Layout.fillWidth: true
            visible: root.runtimeBackendRef
                && String(root.runtimeBackendRef.last_error || "") !== ""
            text: root.runtimeBackendRef ? String(root.runtimeBackendRef.last_error || "") : ""
            wrapMode: Text.Wrap
            color: Kirigami.Theme.negativeTextColor
            font.pixelSize: Kirigami.Theme.smallFont.pixelSize
        }

        Label {
            Layout.fillWidth: true
            visible: root.runtimeBackendRef
                && String(root.runtimeBackendRef.status_text || "") !== ""
            text: root.runtimeBackendRef ? String(root.runtimeBackendRef.status_text || "") : ""
            elide: Text.ElideRight
            color: Kirigami.Theme.disabledTextColor
            font.pixelSize: Kirigami.Theme.smallFont.pixelSize
        }

        Label {
            Layout.fillWidth: true
            visible: root.mcpStatus && root.mcpStatus.status
            text: root.mcpStatus && root.mcpStatus.status
                ? qsTr("MCP：%1").arg(String(root.mcpStatus.status)) : ""
            wrapMode: Text.Wrap
            color: root.mcpStatus && root.mcpStatus.available
                ? Kirigami.Theme.disabledTextColor : Kirigami.Theme.negativeTextColor
            font.pixelSize: Kirigami.Theme.smallFont.pixelSize
        }

        // 输入
        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            ScrollView {
                Layout.fillWidth: true
                Layout.preferredHeight: Kirigami.Units.gridUnit * 4
                TextArea {
                    id: input
                    placeholderText: root.running
                        ? qsTr("Agent 正在回答…")
                        : qsTr("输入消息，Ctrl+Enter 发送")
                    wrapMode: TextArea.Wrap
                    enabled: !root.running
                    Keys.onPressed: function(event) {
                        if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter)
                                && (event.modifiers & Qt.ControlModifier)) {
                            sendButton.send()
                            event.accepted = true
                        }
                    }
                }
            }

            ColumnLayout {
                spacing: Kirigami.Units.smallSpacing
                Button {
                    id: sendButton
                    text: qsTr("发送")
                    enabled: !root.running && input.text.trim() !== ""
                        && root.hasAgent && root.workspaceProjectId !== ""
                    function send() {
                        var message = input.text
                        if (!message.trim())
                            return
                        input.text = ""
                        root.runtimeBackendRef.send_message(message)
                    }
                    onClicked: send()
                }
                Button {
                    text: qsTr("停止")
                    visible: root.running
                    onClicked: root.runtimeBackendRef.cancel_turn()
                }
            }
        }
    }
}
