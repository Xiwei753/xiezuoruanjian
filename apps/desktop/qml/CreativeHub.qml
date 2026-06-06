// =============================================================================
// CreativeHub.qml — 创作中心首页
// =============================================================================
//
// 层级：Desktop UI 层（QML 页面）
// 职责：作品列表展示、最近编辑入口、星图入口、统计入口
// 约束：
//   - 纯展示层，业务逻辑通过 signal 传递给 main.qml
//   - 不直接操作文件系统或 Core 层
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    property var dt: null
    property var backendRef: null
    property var editorBackendRef: backendRef
    property var starmapBackendRef: backendRef
    property var starMapController: null
    property var appState: ({})
    property var tree: []
    property int currentTab: 0
    property bool aiCapable: false
    property bool aiEnabled: false

    signal openProject(string projectId, string projectTitle)
    signal createProject()
    signal openSettings()
    signal openSync()
    signal switchWorkspace()
    signal openStarmapWorkspace(string smId, string smTitle)
    signal renameProjectRequested(string projectId, string title)
    signal deleteProjectRequested(string projectId, string title)

    color: dt ? dt.bg : "#111318"

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Top navigation bar
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 64
            color: dt ? dt.surface : "#1A1D23"
            border.color: dt ? dt.border : "#2A2E36"
            border.width: 1

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: dt ? dt.sp32 : 32
                anchors.rightMargin: dt ? dt.sp32 : 32
                spacing: dt ? dt.sp32 : 32

                // Logo
                Row {
                    spacing: dt ? dt.sp10 : 10
                    Layout.alignment: Qt.AlignVCenter
                    AppText {
                        text: qsTr("素笺写作")
                        color: dt ? dt.primary : "#006497"
                        font.pixelSize: dt ? dt.fontXl : 18
                        font.family: dt ? dt.fontFamily : "sans-serif"
                        font.weight: Font.Bold
                    }
                }

                // Navigation tabs
                Row {
                    spacing: dt ? dt.sp4 : 4
                    Layout.alignment: Qt.AlignVCenter

                    Repeater {
                        model: [
                            { label: qsTr("作品"), idx: 0 },
                            { label: qsTr("星图"), idx: 1 },
                            { label: qsTr("统计"), idx: 2 }
                        ]

                        Rectangle {
                            width: navLabel.implicitWidth + (dt ? dt.sp20 : 20)
                            height: 36
                            radius: dt ? dt.radiusPill : 999
                            color: root.currentTab === modelData.idx ?
                                   (dt ? dt.primaryContainer : "#CCE5FF") :
                                   navHover.containsMouse ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"

                            AppText {
                                id: navLabel
                                anchors.centerIn: parent
                                text: modelData.label
                                color: root.currentTab === modelData.idx ?
                                       (dt ? dt.onPrimaryContainer : "#001E31") :
                                       (dt ? dt.onSurfaceVariant : "#E2E2E5")
                                font.pixelSize: dt ? dt.label : 13
                                font.family: dt ? dt.fontFamily : "sans-serif"
                                font.weight: root.currentTab === modelData.idx ? Font.DemiBold : Font.Normal
                            }

                            MouseArea {
                                id: navHover
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.currentTab = modelData.idx
                            }
                        }
                    }
                }

                Item { Layout.fillWidth: true }

                // Right actions — enlarged buttons
                Row {
                    spacing: dt ? dt.sp8 : 8
                    Layout.alignment: Qt.AlignVCenter

                    // Sync status indicator
                    Rectangle {
                        width: syncRow.implicitWidth + (dt ? dt.sp16 : 16)
                        height: 40
                        radius: dt ? dt.radiusPill : 999
                        color: syncHover.containsMouse ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"
                        visible: root.appState && root.appState.sync && root.appState.sync.status !== "not_configured"

                        Row {
                            id: syncRow
                            anchors.centerIn: parent
                            spacing: dt ? dt.sp6 : 6

                            Rectangle {
                                width: 8; height: 8; radius: 4
                                color: {
                                    var s = root.appState && root.appState.sync ? root.appState.sync.status : "none";
                                    if (s === "success") return dt ? dt.success : "#5CB880";
                                    if (s === "syncing") return dt ? dt.warning : "#E0A840";
                                    if (s === "error" || s === "conflict" || s === "partial_conflict") return dt ? dt.error : "#E06060";
                                    return dt ? dt.textMuted : "#606470";
                                }
                                Layout.alignment: Qt.AlignVCenter
                            }

                            AppText {
                                text: {
                                    var s = root.appState && root.appState.sync ? root.appState.sync.status : "none";
                                    if (s === "success") return qsTr("已同步");
                                    if (s === "syncing") return qsTr("同步中");
                                    if (s === "error") return qsTr("同步失败");
                                    if (s === "conflict") return qsTr("冲突");
                                    if (s === "partial_conflict") return qsTr("正文冲突");
                                    return qsTr("已配置");
                                }
                                color: dt ? dt.onSurfaceVariant : "#8C9198"
                                font.pixelSize: dt ? dt.caption : 12
                                font.family: dt ? dt.fontFamily : "sans-serif"
                                Layout.alignment: Qt.AlignVCenter
                                visible: root.width > 700
                            }
                        }

                        MouseArea {
                            id: syncHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.openSync()
                        }
                    }

                    // Settings button
                    Rectangle {
                        width: settingsText.implicitWidth + 24
                        height: 40
                        radius: dt ? dt.radiusPill : 999
                        color: settingsHover.containsMouse ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"

                        AppText {
                            id: settingsText
                            anchors.centerIn: parent
                            text: qsTr("设置")
                            color: dt ? dt.onSurfaceVariant : "#8C9198"
                            font.pixelSize: dt ? dt.caption : 12
                        }

                        MouseArea {
                            id: settingsHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.openSettings()
                        }
                    }

                    // Workspace switch
                    Rectangle {
                        width: switchRow.implicitWidth + (dt ? dt.sp16 : 16)
                        height: 40
                        radius: dt ? dt.radiusPill : 999
                        color: switchHover.containsMouse ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"

                        Row {
                            id: switchRow
                            anchors.centerIn: parent
                            spacing: dt ? dt.sp6 : 6
                            AppText {
                                text: qsTr("切换工作区")
                                color: dt ? dt.onSurfaceVariant : "#8C9198"
                                font.pixelSize: dt ? dt.caption : 12
                                font.family: dt ? dt.fontFamily : "sans-serif"
                                Layout.alignment: Qt.AlignVCenter
                                visible: true
                            }
                        }

                        MouseArea {
                            id: switchHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.switchWorkspace()
                        }
                    }
                }
            }
        }

        // Content
        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: root.currentTab

            Loader {
                Layout.fillWidth: true
                Layout.fillHeight: true
                active: root.currentTab === 0
                sourceComponent: ProjectHomePage {
                    dt: root.dt
                    backendRef: root.editorBackendRef
                    appState: root.appState
                    tree: root.tree
                    onOpenProject: function(projectId) {
                        var title = "";
                        for (var i = 0; i < root.tree.length; i++) {
                            if (root.tree[i].id === projectId) {
                                title = root.tree[i].title;
                                break;
                            }
                        }
                        root.openProject(projectId, title);
                    }
                    onCreateProject: root.createProject()
                    onRenameProjectRequested: function(projectId, title) { root.renameProjectRequested(projectId, title) }
                    onDeleteProjectRequested: function(projectId, title) { root.deleteProjectRequested(projectId, title) }
                }
            }

            Loader {
                Layout.fillWidth: true
                Layout.fillHeight: true
                active: root.currentTab === 1
                sourceComponent: StarMapPage {
                    dt: root.dt
                    starMapController: root.starMapController
                    appState: root.appState

                    onOpenStarmap: function(starmapId, title) {
                        root.openStarmapWorkspace(starmapId, title);
                    }
                }
            }

            Loader {
                Layout.fillWidth: true
                Layout.fillHeight: true
                active: root.currentTab === 2
                sourceComponent: StatsPreviewPage {
                    dt: root.dt
                    backendRef: root.editorBackendRef
                    appState: root.appState
                }
            }
        }
    }
}
