// =============================================================================
// AgentCliManager.qml — Agent CLI 动态清单与管理
// =============================================================================
//
// Issue #1014：列表完全来自 agentCliBackend.inventory_json。这里不写死
// Agent 数量、CLI 名称或安装目录；已安装与可安装分组、同 Agent 多安装切换、
// 手工路径 / manifest 导入、文档与登录入口都在这一处。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: root

    property var backendRef: null
    property var inventory: ({ installed: [], available: [], registrySource: "", registryError: "" })

    readonly property var installedAgents: root.inventory.installed || []
    readonly property var availableAgents: root.inventory.available || []
    readonly property bool scanning: root.backendRef ? root.backendRef.scanning : false

    spacing: Kirigami.Units.smallSpacing

    function reload() {
        if (!root.backendRef) {
            root.inventory = { installed: [], available: [], registrySource: "", registryError: "" }
            return
        }
        try {
            root.inventory = JSON.parse(root.backendRef.inventory_json || "{}")
        } catch (error) {
            root.inventory = { installed: [], available: [], registrySource: "", registryError: String(error) }
        }
    }

    function filePath(url) {
        var text = String(url)
        if (text.indexOf("file://") === 0)
            text = text.substring(7)
        return decodeURIComponent(text)
    }

    function candidateFor(agent) {
        if (!agent || !agent.candidates || agent.candidates.length === 0)
            return null
        for (var index = 0; index < agent.candidates.length; ++index) {
            if (agent.candidates[index].executable.launchPath === agent.selectedLaunchPath)
                return agent.candidates[index]
        }
        return agent.candidates[0]
    }

    function candidateIndex(agent) {
        if (!agent || !agent.candidates)
            return 0
        for (var index = 0; index < agent.candidates.length; ++index) {
            if (agent.candidates[index].executable.launchPath === agent.selectedLaunchPath)
                return index
        }
        return 0
    }

    function statusText(candidate) {
        if (!candidate || !candidate.probe)
            return qsTr("尚未探测")
        var probe = candidate.probe
        var version = probe.versionStatus === "detected"
            ? (probe.version || qsTr("已探测"))
            : (probe.versionStatus === "failed" ? qsTr("读取失败") : qsTr("未探测"))
        var launch = probe.launchability === "launchable" ? qsTr("可启动")
            : (probe.launchability === "unavailable" ? qsTr("不可用") : qsTr("未确认"))
        var config = probe.configStatus === "configured" ? qsTr("已配置")
            : (probe.configStatus === "not-configured" ? qsTr("未配置") : qsTr("未确认"))
        var auth = probe.authStatus === "authenticated" ? qsTr("已登录")
            : (probe.authStatus === "unauthenticated" ? qsTr("未登录") : qsTr("未确认"))
        return qsTr("版本：%1 · 启动：%2 · 配置：%3 · 登录：%4")
            .arg(version).arg(launch).arg(config).arg(auth)
    }

    function isSelected(agent) {
        return root.backendRef
            && root.backendRef.selected_agent_id === agent.descriptor.id
            && root.backendRef.selected_launch_path === agent.selectedLaunchPath
    }

    function selectCandidate(agent, index) {
        if (!root.backendRef || !agent.candidates || !agent.candidates[index])
            return
        root.backendRef.select_agent(agent.descriptor.id,
                                     agent.candidates[index].executable.launchPath)
    }

    // ── 顶部状态与刷新 ──
    RowLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing

        Label {
            text: qsTr("Agent CLI")
            font.weight: Font.DemiBold
            Layout.fillWidth: true
        }

        BusyIndicator {
            running: root.scanning
            visible: running
            implicitWidth: Kirigami.Units.gridUnit
            implicitHeight: Kirigami.Units.gridUnit
        }

        ToolButton {
            icon.name: "view-refresh"
            text: qsTr("重新扫描")
            enabled: !root.scanning && root.backendRef !== null
            onClicked: root.backendRef.refresh_inventory()
            Accessible.name: qsTr("重新扫描本机 Agent CLI")
        }
    }

    Label {
        Layout.fillWidth: true
        visible: root.backendRef && root.backendRef.last_error !== ""
        text: root.backendRef ? root.backendRef.last_error : ""
        color: Kirigami.Theme.negativeTextColor
        wrapMode: Text.Wrap
        font.pixelSize: Kirigami.Theme.smallFont.pixelSize
    }

    Label {
        Layout.fillWidth: true
        visible: root.inventory.registryError !== undefined && root.inventory.registryError !== ""
        text: root.inventory.registryError || ""
        color: Kirigami.Theme.neutralTextColor
        wrapMode: Text.Wrap
        font.pixelSize: Kirigami.Theme.smallFont.pixelSize
    }

    // ── 已安装 ──
    Label {
        Layout.fillWidth: true
        text: qsTr("本地已安装（%1）").arg(root.installedAgents.length)
        font.weight: Font.DemiBold
        topPadding: Kirigami.Units.smallSpacing
    }

    Label {
        Layout.fillWidth: true
        visible: root.installedAgents.length === 0 && !root.scanning
        text: qsTr("尚未检测到已安装的 Agent CLI。可从下方导入本机可执行文件或 manifest；不会自动执行安装命令。")
        wrapMode: Text.Wrap
        color: Kirigami.Theme.disabledTextColor
    }

    Repeater {
        model: root.installedAgents

        Rectangle {
            id: installedCard
            Layout.fillWidth: true
            implicitHeight: installedLayout.implicitHeight + 2 * Kirigami.Units.smallSpacing
            radius: Kirigami.Units.smallSpacing
            color: Kirigami.Theme.backgroundColor
            border.width: 1
            border.color: root.isSelected(modelData)
                ? Kirigami.Theme.highlightColor
                : Kirigami.Theme.disabledTextColor

            readonly property var candidate: root.candidateFor(modelData)
            readonly property var descriptor: modelData.descriptor

            ColumnLayout {
                id: installedLayout
                anchors.fill: parent
                anchors.margins: Kirigami.Units.smallSpacing
                spacing: Kirigami.Units.smallSpacing / 2

                RowLayout {
                    Layout.fillWidth: true

                    Label {
                        text: installedCard.descriptor.name
                        font.weight: Font.DemiBold
                        Layout.fillWidth: true
                    }

                    Label {
                        text: String(installedCard.descriptor.protocol || "").toUpperCase()
                        color: Kirigami.Theme.highlightColor
                        font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                    }

                    Label {
                        visible: root.isSelected(modelData)
                        text: qsTr("当前使用")
                        color: Kirigami.Theme.positiveTextColor
                        font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                    }
                }

                Label {
                    Layout.fillWidth: true
                    visible: text !== ""
                    text: installedCard.descriptor.description || ""
                    wrapMode: Text.Wrap
                    font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                    color: Kirigami.Theme.disabledTextColor
                }

                Label {
                    Layout.fillWidth: true
                    text: installedCard.candidate
                        ? qsTr("来源：%1 · 包：%2")
                            .arg(installedCard.candidate.executable.source || qsTr("未知"))
                            .arg(installedCard.candidate.executable.packageId || qsTr("未知"))
                        : ""
                    elide: Text.ElideMiddle
                    font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                    color: Kirigami.Theme.disabledTextColor
                }

                Label {
                    Layout.fillWidth: true
                    text: installedCard.candidate
                        ? installedCard.candidate.executable.launchPath : ""
                    elide: Text.ElideMiddle
                    font.family: "monospace"
                    font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                    color: Kirigami.Theme.disabledTextColor
                }

                Label {
                    Layout.fillWidth: true
                    text: root.statusText(installedCard.candidate)
                    wrapMode: Text.Wrap
                    font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                }

                RowLayout {
                    Layout.fillWidth: true
                    visible: modelData.candidates.length > 1

                    Label {
                        text: qsTr("安装：")
                    }

                    ComboBox {
                        Layout.fillWidth: true
                        model: modelData.candidates
                        currentIndex: root.candidateIndex(modelData)
                        delegate: ItemDelegate {
                            width: ListView.view ? ListView.view.width : implicitWidth
                            text: modelData && modelData.executable
                                  ? modelData.executable.launchPath : ""
                        }
                        onActivated: function(index) {
                            root.selectCandidate(modelData, index)
                        }
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.smallSpacing

                    Button {
                        text: qsTr("使用此安装")
                        enabled: !root.isSelected(modelData)
                        onClicked: root.selectCandidate(modelData, root.candidateIndex(modelData))
                    }

                    Button {
                        text: qsTr("文档")
                        visible: !!(installedCard.descriptor.docsUrl
                                   || installedCard.descriptor.website
                                   || installedCard.descriptor.repository)
                        onClicked: root.backendRef.open_agent_docs(installedCard.descriptor.id)
                    }

                    Button {
                        text: qsTr("登录")
                        enabled: installedCard.candidate
                            && installedCard.candidate.probe
                            && installedCard.candidate.probe.launchability === "launchable"
                        onClicked: root.backendRef.open_agent_login(installedCard.descriptor.id)
                    }

                    Item { Layout.fillWidth: true }
                }
            }
        }
    }

    // ── 可安装目录 ──
    Label {
        Layout.fillWidth: true
        visible: root.availableAgents.length > 0
        text: qsTr("可安装（目录，%1）").arg(root.availableAgents.length)
        font.weight: Font.DemiBold
        topPadding: Kirigami.Units.smallSpacing
    }

    Label {
        Layout.fillWidth: true
        visible: root.availableAgents.length > 0
        text: qsTr("目录条目只说明可以安装，不会自动执行安装命令。")
        wrapMode: Text.Wrap
        color: Kirigami.Theme.disabledTextColor
        font.pixelSize: Kirigami.Theme.smallFont.pixelSize
    }

    Repeater {
        model: root.availableAgents

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: availableLayout.implicitHeight + 2 * Kirigami.Units.smallSpacing
            radius: Kirigami.Units.smallSpacing
            color: Kirigami.Theme.backgroundColor
            border.width: 1
            border.color: Kirigami.Theme.disabledTextColor

            ColumnLayout {
                id: availableLayout
                anchors.fill: parent
                anchors.margins: Kirigami.Units.smallSpacing
                spacing: Kirigami.Units.smallSpacing / 2

                RowLayout {
                    Layout.fillWidth: true

                    Label {
                        text: modelData.name
                        font.weight: Font.DemiBold
                        Layout.fillWidth: true
                    }

                    Label {
                        text: String(modelData.protocol || "").toUpperCase()
                        color: Kirigami.Theme.highlightColor
                        font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                    }

                    Label {
                        text: qsTr("未安装")
                        color: Kirigami.Theme.disabledTextColor
                        font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                    }
                }

                Label {
                    Layout.fillWidth: true
                    visible: text !== ""
                    text: modelData.description || ""
                    wrapMode: Text.Wrap
                    font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                    color: Kirigami.Theme.disabledTextColor
                }

                Label {
                    Layout.fillWidth: true
                    visible: !!(modelData.launch && modelData.launch.package)
                    text: modelData.launch && modelData.launch.package
                        ? qsTr("发行包：%1").arg(modelData.launch.package) : ""
                    elide: Text.ElideMiddle
                    font.family: "monospace"
                    font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                    color: Kirigami.Theme.disabledTextColor
                }

                Button {
                    text: qsTr("文档")
                    visible: !!(modelData.docsUrl || modelData.website || modelData.repository)
                    onClicked: root.backendRef.open_agent_docs(modelData.id)
                }
            }
        }
    }

    // ── 手工接入 ──
    Label {
        Layout.fillWidth: true
        text: qsTr("手工接入")
        font.weight: Font.DemiBold
        topPadding: Kirigami.Units.smallSpacing
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing

        Button {
            text: qsTr("添加可执行文件…")
            onClicked: executableDialog.open()
        }

        Button {
            text: qsTr("导入 Agent manifest…")
            onClicked: manifestDialog.open()
        }

        Item { Layout.fillWidth: true }
    }

    Label {
        Layout.fillWidth: true
        text: qsTr("目录来源：%1 · 扫描时间：%2")
            .arg(root.inventory.registrySource || qsTr("无"))
            .arg(root.inventory.scannedAt || qsTr("尚未扫描"))
        wrapMode: Text.Wrap
        color: Kirigami.Theme.disabledTextColor
        font.pixelSize: Kirigami.Theme.smallFont.pixelSize
    }

    FileDialog {
        id: executableDialog
        title: qsTr("选择 Agent 可执行文件")
        fileMode: FileDialog.OpenFile
        nameFilters: [qsTr("所有文件 (*)")]
        onAccepted: {
            if (root.backendRef)
                root.backendRef.add_custom_executable(root.filePath(selectedFile))
        }
    }

    FileDialog {
        id: manifestDialog
        title: qsTr("导入 Agent manifest")
        fileMode: FileDialog.OpenFile
        nameFilters: [qsTr("Agent manifest (*.json)"), qsTr("所有文件 (*)")]
        onAccepted: {
            if (root.backendRef)
                root.backendRef.add_agent_manifest(root.filePath(selectedFile))
        }
    }

    Connections {
        target: root.backendRef

        function onInventory_changed() {
            root.reload()
        }
    }

    Component.onCompleted: {
        root.reload()
        if (root.backendRef)
            root.backendRef.refresh_inventory()
    }
}
