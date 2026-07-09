// =============================================================================
// CreativeHub.qml — 创作中心首页
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
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
    property var layoutPlan: null

    signal openProject(string projectId, string projectTitle)
    signal createProject()
    signal openSettings()
    signal requestSync()

    signal switchWorkspace()
    signal openStarmapWorkspace(string smId, string smTitle)
    signal renameProjectRequested(string projectId, string title)
    signal deleteProjectRequested(string projectId, string title)

    color: dt.bg

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Top navigation bar
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 64
            color: dt.surface
            border.color: dt.border
            border.width: 1

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: dt.sp32
                anchors.rightMargin: dt.sp32
                spacing: dt.sp32

                // Logo
                Row {
                    spacing: dt.sp10
                    Layout.alignment: Qt.AlignVCenter
                    AppText {
                        dt: root.dt
                        text: qsTr("素笺写作")
                        color: dt.primary
                        font.pixelSize: dt.fontXl
                        font.family: dt.fontFamily
                        font.weight: Font.Bold
                    }
                }

                // Navigation tabs
                Row {
                    spacing: dt.sp4
                    Layout.alignment: Qt.AlignVCenter

                    Repeater {
                        model: [
                            { label: qsTr("作品"), idx: 0 },
                            { label: qsTr("星图"), idx: 1 },
                            { label: qsTr("统计"), idx: 2 }
                        ]

                        Rectangle {
                            width: navLabel.implicitWidth + dt.sp20
                            height: 36
                            radius: dt.radiusPill
                            color: root.currentTab === modelData.idx ?
                                   dt.primaryContainer :
                                   navHover.containsMouse ? dt.surfaceVariant : "transparent"

                            AppText {
                                id: navLabel
                                dt: root.dt
                                anchors.centerIn: parent
                                text: modelData.label
                                color: root.currentTab === modelData.idx ?
                                       dt.onPrimaryContainer :
                                       dt.onSurfaceVariant
                                font.pixelSize: dt.label
                                font.family: dt.fontFamily
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
                    spacing: dt.sp8
                    Layout.alignment: Qt.AlignVCenter

                    // Sync status indicator
                    Rectangle {
                        width: syncRow.implicitWidth + dt.sp16
                        height: 40
                        radius: dt.radiusPill
                        color: syncHover.containsMouse ? dt.surfaceVariant : "transparent"
                        visible: root.appState && root.appState.sync && root.appState.sync.status !== "not_configured" && root.appState.sync.status !== "no_workspace"

                        Row {
                            id: syncRow
                            anchors.centerIn: parent
                            spacing: dt.sp6

                            Rectangle {
                                width: 8; height: 8; radius: 4
                                color: {
                                    var s = root.appState && root.appState.sync ? root.appState.sync.status : "none";
                                    if (s === "success") return dt.success;
                                    if (s === "syncing") return dt.warning;
                                    if (s === "error" || s === "conflict" || s === "partial_conflict") return dt.error;
                                    return dt.textMuted;
                                }
                                Layout.alignment: Qt.AlignVCenter
                            }

                             AppText {
                                 dt: root.dt
                                  text: {
                                      var s = root.appState && root.appState.sync ? root.appState.sync.status : "none";
                                      if (s === "success") return qsTr("已同步");
                                      if (s === "syncing") return qsTr("同步中");
                                      if (s === "error") return qsTr("同步失败");
                                      if (s === "conflict") return qsTr("同步冲突");
                                      if (s === "partial_conflict") return qsTr("同步冲突");
                                      // 已配置但无特定状态时显示"同步"
                                      return qsTr("同步");
                                  }
                                  color: dt.onSurfaceVariant
                                  font.pixelSize: dt.caption
                                  font.family: dt.fontFamily
                                  Layout.alignment: Qt.AlignVCenter
                                  visible: root.width > 700
                             }
                        }

                        MouseArea {
                            id: syncHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                root.requestSync();
                            }
                        }
                    }

                    // Settings button
                    Rectangle {
                        width: settingsText.implicitWidth + 24
                        height: 40
                        radius: dt.radiusPill
                        color: settingsHover.containsMouse ? dt.surfaceVariant : "transparent"

                        AppText {
                            id: settingsText
                            dt: root.dt
                            anchors.centerIn: parent
                            text: qsTr("设置")
                            color: dt.onSurfaceVariant
                            font.pixelSize: dt.caption
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
                        width: switchRow.implicitWidth + dt.sp16
                        height: 40
                        radius: dt.radiusPill
                        color: switchHover.containsMouse ? dt.surfaceVariant : "transparent"

                        Row {
                            id: switchRow
                            anchors.centerIn: parent
                            spacing: dt.sp6
                            AppText {
                                dt: root.dt
                                text: qsTr("切换工作区")
                                color: dt.onSurfaceVariant
                                font.pixelSize: dt.caption
                                font.family: dt.fontFamily
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
