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
    // 新会话的 conversation.model 为空，但运行时仍保存用户刚选的模型；
    // 先读显式 current_model，再回退到会话持久化值。
    readonly property string currentModel: runtimeBackendRef
        ? String(runtimeBackendRef.current_model || "")
        : ""
    readonly property string modelsState: runtimeBackendRef
        ? String(runtimeBackendRef.models_state || "none")
        : "none"
    readonly property string modelsError: runtimeBackendRef
        ? String(runtimeBackendRef.models_error || "")
        : ""
    property bool advancedModelInput: false
    property string modelSearch: ""

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

    // 打开历史会话后，把共用选择同步到该会话真实使用的安装。只有运行时
    // 确实已经绑定该会话和该 Agent 时才同步，避免迟到的 conversation_changed
    // 覆盖用户刚做出的显式 CLI 选择。
    function syncCliSelectionFromConversation() {
        if (!runtimeBackendRef || !agentCliBackendRef || !conversation)
            return
        var conversationId = String(runtimeBackendRef.current_conversation_id || "")
        var runtimeAgentId = String(runtimeBackendRef.current_agent_id || "")
        var id = String(conversation.agentId || "")
        var path = String(conversation.launchPath || "")
        if (conversationId === "" || runtimeAgentId !== id || id === "" || path === "")
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
            // 每轮显示实际执行者与模型，而不是当前下拉框的最新选择。
            var turnAgent = String(turn.agentName || turn.agentId || value.agentName || "")
            var turnModel = String(turn.model || "")
            if (turnAgent !== "" || turnModel !== "") {
                rows.push({
                    kind: "meta",
                    text: "",
                    name: turnAgent,
                    detail: root.modelLabel(turnModel)
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

    function splitModelId(id) {
        var value = String(id || "")
        var slash = value.indexOf("/")
        if (slash <= 0 || slash >= value.length - 1)
            return { providerId: "", modelId: value }
        return {
            providerId: value.slice(0, slash),
            modelId: value.slice(slash + 1)
        }
    }

    // 模型身份固定为完整 provider/model；显示名只用于辅助阅读。
    function modelChoices() {
        var models = parseJson(runtimeBackendRef ? runtimeBackendRef.models_json : "[]", [])
        var choices = []
        var seen = ({})
        for (var i = 0; i < models.length; ++i) {
            var model = models[i]
            var id = String(model.id || "")
            var key = "id:" + id
            if (id === "" || seen[key])
                continue
            seen[key] = true
            var split = splitModelId(id)
            var providerId = String(model.providerId || split.providerId || "")
            var modelId = String(model.modelId || split.modelId || id)
            var providerName = String(model.providerName || providerId || "")
            var displayName = String(model.displayName || modelId || id)
            var displayText = providerName !== ""
                ? providerName + " · " + displayName
                : displayName
            choices.push({
                id: id,
                providerId: providerId,
                modelId: modelId,
                providerName: providerName,
                displayName: displayName,
                displayText: displayText,
                searchText: (id + " " + modelId + " " + displayName + " "
                    + providerId + " " + providerName).toLowerCase()
            })
        }
        choices.sort(function(left, right) {
            var provider = left.providerId.localeCompare(right.providerId)
            if (provider !== 0)
                return provider
            var name = left.displayName.localeCompare(right.displayName)
            if (name !== 0)
                return name
            return left.id.localeCompare(right.id)
        })
        return choices
    }

    function modelChoiceById(id) {
        var choices = modelChoices()
        for (var i = 0; i < choices.length; ++i) {
            if (choices[i].id === id)
                return choices[i]
        }
        return null
    }

    function modelLabel(id) {
        var value = String(id || "")
        if (value === "")
            return ""
        var choice = modelChoiceById(value)
        if (choice)
            return choice.displayText
        // 目录尚未加载或该模型来自其它执行器时，仍保留完整来源，便于用户
        // 识别旧选择或手动输入的目标。
        var split = splitModelId(value)
        return split.providerId !== ""
            ? split.providerId + " · " + split.modelId
            : value
    }

    function filterModelChoices(query) {
        var needle = String(query || "").trim().toLowerCase()
        var choices = modelChoices()
        if (needle === "")
            return choices
        return choices.filter(function(choice) {
            return choice.searchText.indexOf(needle) >= 0
        })
    }

    function customModelId() {
        var value = String(modelSearch || "").trim()
        if (value === "" || value.indexOf("/") <= 0 || value.slice(-1) === "/")
            return ""
        return modelChoiceById(value) ? "" : value
    }

    function modelCandidates() {
        var choices = filterModelChoices(modelSearch)
        var custom = customModelId()
        if (custom !== "") {
            choices.push({
                id: custom,
                providerId: splitModelId(custom).providerId,
                modelId: splitModelId(custom).modelId,
                providerName: "",
                displayName: splitModelId(custom).modelId,
                displayText: modelLabel(custom) + qsTr("（未收录，仍将向该来源发送）"),
                searchText: custom.toLowerCase(),
                custom: true
            })
        }
        return choices
    }

    function chooseModelCandidate(id) {
        var value = String(id || "").trim()
        if (value === "")
            return
        runtimeBackendRef.choose_model(value)
        modelSearch = ""
    }

    function modelIndex() {
        var models = modelChoices()
        for (var i = 0; i < models.length; ++i) {
            if (models[i].id === currentModel)
                return i
        }
        return -1
    }

    function modelDisplayText() {
        if (currentModel !== "")
            return modelLabel(currentModel)
        if (modelsState === "loading")
            return qsTr("正在获取模型…")
        if (modelsState === "default")
            return qsTr("使用 CLI 默认模型")
        if (modelsState === "unsupported")
            return qsTr("该 CLI 不提供模型列表")
        if (modelsState === "error")
            return qsTr("模型列表获取失败")
        return qsTr("默认模型")
    }

    function modelsDetailText() {
        if (modelsError === "")
            return ""
        if (modelsState === "ready")
            return qsTr("已显示缓存模型；刷新失败：%1").arg(modelsError)
        return modelsError
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
        function onConversation_changed() { root.refresh() }
        function onConversation_opened() {
            root.syncCliSelectionFromConversation()
            root.refresh()
        }
        function onConversations_changed() { root.refresh() }
        function onModels_changed() { root.refresh() }
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

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            ComboBox {
                id: modelPicker
                Layout.fillWidth: true
                // 主选择器只列协议真实返回的模型；没有目录时显示明确状态，
                // 不留下一个空输入框。
                editable: false
                model: root.modelChoices()
                textRole: "displayText"
                valueRole: "id"
                currentIndex: root.modelIndex()
                displayText: root.modelDisplayText()
                enabled: root.hasAgent
                onActivated: function(index) {
                    var choices = root.modelChoices()
                    if (index >= 0 && index < choices.length)
                        root.runtimeBackendRef.choose_model(String(choices[index].id))
                }
            }

            ToolButton {
                icon.name: "view-refresh"
                display: AbstractButton.IconOnly
                enabled: root.hasAgent
                ToolTip.visible: hovered
                ToolTip.text: qsTr("刷新模型目录")
                onClicked: root.runtimeBackendRef.refresh_models()
            }

            ToolButton {
                icon.name: "insert-text"
                display: AbstractButton.IconOnly
                checkable: true
                checked: root.advancedModelInput
                ToolTip.visible: hovered
                ToolTip.text: qsTr("搜索或指定模型")
                onClicked: root.advancedModelInput = checked
            }
        }

        Label {
            Layout.fillWidth: true
            visible: text !== ""
            text: root.modelsDetailText()
            wrapMode: Text.Wrap
            color: root.modelsState === "error" || root.modelsState === "unsupported"
                ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.disabledTextColor
            font.pixelSize: Kirigami.Theme.smallFont.pixelSize
        }

        // 高级输入默认折叠；支持按 provider/模型名搜索，也允许使用完整
        // provider/model 指定目录中尚未出现的模型。
        ColumnLayout {
            Layout.fillWidth: true
            visible: root.advancedModelInput
            spacing: Kirigami.Units.smallSpacing / 2

            TextField {
                id: modelSearchField
                Layout.fillWidth: true
                text: root.modelSearch
                placeholderText: qsTr("搜索模型，或输入完整 provider/model")
                onTextChanged: {
                    if (text !== root.modelSearch)
                        root.modelSearch = text
                }
                onAccepted: {
                    var value = String(text || "").trim()
                    var exact = root.modelChoiceById(value)
                    if (exact)
                        root.chooseModelCandidate(exact.id)
                    else if (root.customModelId() !== "")
                        root.chooseModelCandidate(root.customModelId())
                }
            }

            Label {
                Layout.fillWidth: true
                visible: root.modelSearch.trim() !== ""
                    && root.modelCandidates().length === 0
                text: qsTr("没有匹配模型；请从候选中选择，或输入完整的 provider/model。")
                wrapMode: Text.Wrap
                color: Kirigami.Theme.disabledTextColor
                font.pixelSize: Kirigami.Theme.smallFont.pixelSize
            }

            ListView {
                id: modelCandidateList
                Layout.fillWidth: true
                Layout.preferredHeight: count > 0
                    ? Math.min(contentHeight, Kirigami.Units.gridUnit * 12) : 0
                visible: count > 0
                clip: true
                model: root.modelCandidates()
                spacing: 1

                delegate: ItemDelegate {
                    required property var modelData
                    width: ListView.view ? ListView.view.width : 0
                    text: String(modelData.displayText || modelData.id)
                    ToolTip.visible: hovered
                    ToolTip.text: String(modelData.id)
                    onClicked: root.chooseModelCandidate(String(modelData.id))
                }
            }
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
                        visible: modelData.kind === "tool"
                            || modelData.kind === "reasoning"
                            || modelData.kind === "meta"
                        text: modelData.kind === "tool"
                            ? qsTr("工具：%1（%2）").arg(modelData.name).arg(modelData.status)
                            : modelData.kind === "meta"
                                ? (modelData.detail !== ""
                                    ? qsTr("%1 · %2").arg(modelData.name).arg(modelData.detail)
                                    : String(modelData.name))
                                : qsTr("思考过程")
                        font.bold: modelData.kind !== "meta"
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
                    ? qsTr("向 %1 发送第一条消息").arg(root.agentId())
                    : qsTr("请先在上方选择一个本机已安装的 AI 命令行工具")
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
