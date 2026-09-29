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
    required property var dt
    property var projectBackendRef: null
    property var editorBackendRef: null
    property var starmapBackendRef: null
    property var starMapController: null
    property var appState: ({})
    property var tree: []
    // Issue #796 评论 5886483653: Hub tab 由 appController.hubTab 驱动，
    // 不要 Loader 重建后永远回默认 0。
    property var appControllerRef: null
    property int currentTab: appControllerRef ? appControllerRef.hubTab : 0
    property bool aiCapable: false
    property bool aiEnabled: false
    property var layoutPlan: null

    signal openProject(string projectId, string projectTitle)
    signal createProject()
    signal openSettings()
    signal requestSync()
    signal requestSearch()

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
                        font.pointSize: dt.fontXlPt
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
                                font.pointSize: dt.labelPt
                                font.family: dt.fontFamily
                                font.weight: root.currentTab === modelData.idx ? Font.DemiBold : Font.Normal
                            }

                            MouseArea {
                                id: navHover
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                // Issue #796 评论 5886483653: 点击改 appControllerRef.hubTab，
                                // currentTab 由绑定跟随，Loader 重建后不丢。
                                onClicked: {
                                    if (root.appControllerRef) {
                                        root.appControllerRef.hubTab = modelData.idx
                                    } else {
                                        root.currentTab = modelData.idx
                                    }
                                }
                            }
                        }
                    }
                }

                Item { Layout.fillWidth: true }

                // Right actions — 收口到 GlobalTopActions（同步 / 搜索 / 设置）
                GlobalTopActions {
                    dt: root.dt
                    appState: root.appState
                    onRequestSync: root.requestSync()
                    onRequestSearch: root.requestSearch()
                    onOpenSettings: root.openSettings()
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
                    editorBackendRef: root.editorBackendRef
                    projectBackendRef: root.projectBackendRef
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
                    editorBackendRef: root.editorBackendRef
                    appState: root.appState
                }
            }
        }
    }
}
