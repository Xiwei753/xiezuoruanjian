// =============================================================================
// AgentCliPicker.qml — AI 面板顶部的紧凑 CLI 选择器
// =============================================================================
//
// Issue #1018：只展示本机真实扫描到的 CLI。目录里的“可安装”条目、协议、
// 包名和安装说明都不进入日常界面；多个真实启动路径只在次要行里选择。

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: root

    property var backendRef: null
    property var dt
    property var installedAgents: []
    // 探测到但版本不满足要求的候选（例如 OpenCode 1.x），只用于明确提示，
    // 不进入可选安装列表。
    property var rejectedAgents: []
    property string lastError: ""

    spacing: Kirigami.Units.smallSpacing

    function parseInventory(text) {
        try {
            var inventory = JSON.parse(String(text || "{}"))
            return inventory && inventory.installed ? inventory : { installed: [] }
        } catch (e) {
            return { installed: [] }
        }
    }

    function normalizeInstallation(installation) {
        var descriptor = installation && installation.descriptor ? installation.descriptor : {}
        var candidates = []
        var rawCandidates = installation && installation.candidates ? installation.candidates : []
        for (var i = 0; i < rawCandidates.length; ++i) {
            var candidate = rawCandidates[i] || {}
            var path = String(candidate.launchPath || "")
            if (path === "")
                continue
            candidates.push({
                path: path,
                label: path
            })
        }
        return {
            agentId: String(descriptor.id || ""),
            name: String(descriptor.name || descriptor.id || ""),
            selectedLaunchPath: String(installation.selectedLaunchPath || ""),
            candidates: candidates
        }
    }

    function reload() {
        if (!backendRef) {
            installedAgents = []
            rejectedAgents = []
            return
        }
        var inventory = parseInventory(backendRef.inventory_json)
        var next = []
        for (var i = 0; i < inventory.installed.length; ++i) {
            var installation = normalizeInstallation(inventory.installed[i])
            if (installation.agentId !== "")
                next.push(installation)
        }
        installedAgents = next
        var rejected = []
        var rawRejected = inventory.rejected ? inventory.rejected : []
        for (var r = 0; r < rawRejected.length; ++r) {
            var probe = rawRejected[r] && rawRejected[r].probe ? rawRejected[r].probe : {}
            var detail = String(probe.error || qsTr("版本不兼容，未加入可选安装"))
            rejected.push(detail)
        }
        rejectedAgents = rejected
        lastError = String(backendRef.last_error || "")
    }

    function selectionIndex() {
        if (!backendRef || !backendRef.selected_agent_id)
            return -1
        for (var i = 0; i < installedAgents.length; ++i) {
            if (installedAgents[i].agentId === backendRef.selected_agent_id)
                return i
        }
        return -1
    }

    function currentInstallation() {
        var index = selectionIndex()
        if (index < 0 || index >= installedAgents.length)
            return null
        return installedAgents[index]
    }

    function candidateIndex(installation) {
        if (!installation)
            return -1
        for (var i = 0; i < installation.candidates.length; ++i) {
            if (installation.candidates[i].path === installation.selectedLaunchPath)
                return i
        }
        return installation.candidates.length > 0 ? 0 : -1
    }

    function chooseAgent(index) {
        if (!backendRef || index < 0 || index >= installedAgents.length)
            return
        var installation = installedAgents[index]
        backendRef.select_agent(installation.agentId, installation.selectedLaunchPath)
    }

    function chooseCandidate(index) {
        var installation = currentInstallation()
        if (!backendRef || !installation || index < 0
                || index >= installation.candidates.length)
            return
        backendRef.select_agent(installation.agentId, installation.candidates[index].path)
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing

        Label {
            text: qsTr("AI 命令行")
            font.weight: Font.DemiBold
        }

        ComboBox {
            id: cliCombo
            Layout.fillWidth: true
            visible: root.installedAgents.length > 0
            model: root.installedAgents
            textRole: "name"
            currentIndex: root.selectionIndex()
            displayText: currentIndex >= 0 ? currentText : qsTr("选择本机 CLI")
            onActivated: function(index) { root.chooseAgent(index) }
        }

        ToolButton {
            visible: root.installedAgents.length > 0
            icon.name: "view-refresh"
            display: AbstractButton.IconOnly
            ToolTip.visible: hovered
            ToolTip.text: qsTr("重新扫描本机 CLI")
            onClicked: if (root.backendRef) root.backendRef.refresh_inventory()
        }

        // OpenCode / Codex 未在 PATH 中时，允许用户直接指定真实可执行文件。
        ToolButton {
            icon.name: "document-open-folder"
            display: AbstractButton.IconOnly
            ToolTip.visible: hovered
            ToolTip.text: qsTr("选择 CLI 安装路径")
            onClicked: cliPathDialog.open()
        }
    }

    FileDialog {
        id: cliPathDialog
        title: qsTr("选择 OpenCode 或 Codex 可执行文件")
        fileMode: FileDialog.OpenFile
        onAccepted: {
            if (!root.backendRef)
                return
            var path = decodeURIComponent(String(selectedFile).replace(/^file:\/\//, ""))
            root.backendRef.add_custom_executable(path)
        }
    }

    RowLayout {
        Layout.fillWidth: true
        visible: root.installedAgents.length === 0
        spacing: Kirigami.Units.smallSpacing

        Label {
            Layout.fillWidth: true
            text: qsTr("本机未检测到 AI 命令行工具")
            color: Kirigami.Theme.disabledTextColor
            wrapMode: Text.WordWrap
        }

        ToolButton {
            text: qsTr("重新扫描")
            onClicked: if (root.backendRef) root.backendRef.refresh_inventory()
        }
    }

    // 同一个 CLI 可能同时存在系统包与 npm 全局安装；路径只在这里作为
    // 次要详情出现，不污染主选择器。
    RowLayout {
        Layout.fillWidth: true
        visible: {
            var installation = root.currentInstallation()
            return installation !== null && installation.candidates.length > 1
        }
        spacing: Kirigami.Units.smallSpacing

        Label {
            text: qsTr("启动路径")
            color: Kirigami.Theme.disabledTextColor
        }

        ComboBox {
            Layout.fillWidth: true
            model: {
                var installation = root.currentInstallation()
                return installation ? installation.candidates : []
            }
            textRole: "label"
            currentIndex: root.candidateIndex(root.currentInstallation())
            onActivated: function(index) { root.chooseCandidate(index) }
        }
    }

    Label {
        Layout.fillWidth: true
        visible: root.rejectedAgents.length > 0
        text: root.rejectedAgents.join("\n")
        color: Kirigami.Theme.negativeTextColor
        wrapMode: Text.WordWrap
        font.pixelSize: Kirigami.Theme.smallFont.pixelSize
    }

    Label {
        Layout.fillWidth: true
        visible: root.lastError !== ""
        text: root.lastError
        color: Kirigami.Theme.negativeTextColor
        wrapMode: Text.WordWrap
    }

    Connections {
        target: root.backendRef
        function onInventory_changed() { root.reload() }
        function onSelected_agent_changed() { root.reload() }
        function onLast_error_changed() { root.reload() }
    }

    Component.onCompleted: {
        reload()
        if (backendRef)
            backendRef.refresh_inventory()
    }
}
