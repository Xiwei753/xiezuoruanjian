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
    // 宽屏骨架：Core 只给 Workbench 语义，具体左侧一级导航由 Qt UI 层呈现。
    readonly property bool wideShell: layoutPlan && layoutPlan.workspaceLayoutMode === "Workbench"

    signal openProject(string projectId, string projectTitle)
    signal createProject()
    signal openSettings()
    signal requestSync()
    signal requestSearch()

    signal openStarmapWorkspace(string smId, string smTitle)
    signal renameProjectRequested(string projectId, string title)
    signal deleteProjectRequested(string projectId, string title)

    color: dt.bg

    RowLayout {
        anchors.fill: parent
        spacing: 0

        // 宽屏一级导航（Workbench）。这里只负责壳层摆放和选中态呈现，
        // 作品/星图/统计仍复用原来的 currentTab + 三个 Loader，不另做第二套路由。
        Rectangle {
            visible: root.wideShell
            Layout.fillHeight: true
            Layout.preferredWidth: 200
            color: dt.surface
            border.color: dt.border
            border.width: 1

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: dt.sp12
                spacing: dt.sp8

                AppText {
                    dt: root.dt
                    text: qsTr("素笺写作")
                    color: dt.primary
                    font.pointSize: dt.fontLgPt
                    font.family: dt.fontFamily
                    font.weight: Font.Bold
                    Layout.fillWidth: true
                    Layout.leftMargin: dt.sp8
                    Layout.topMargin: dt.sp8
                    Layout.bottomMargin: dt.sp8
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 1
                    color: dt.border
                    Layout.bottomMargin: dt.sp8
                }

                Repeater {
                    model: [
                        { label: qsTr("作品"), idx: 0 },
                        { label: qsTr("星图"), idx: 1 },
                        { label: qsTr("统计"), idx: 2 }
                    ]

                    Rectangle {
                        id: wideNavItem
                        readonly property bool selected: root.currentTab === modelData.idx
                        Layout.fillWidth: true
                        Layout.preferredHeight: 44
                        radius: dt.radiusMd
                        color: selected
                               ? dt.primaryContainer
                               : wideNavHover.containsMouse ? dt.surfaceVariant : "transparent"

                        Behavior on color {
                            ColorAnimation { duration: dt.animFast }
                        }

                        // 选中态左侧竖条：和窄屏顶栏的 pill 选中态互补，
                        // 全部取自 DesignTokens，不写字面色值。
                        Rectangle {
                            visible: wideNavItem.selected
                            width: 3
                            height: 20
                            radius: dt.radiusPill
                            color: dt.primary
                            anchors.left: parent.left
                            anchors.leftMargin: dt.sp4
                            anchors.verticalCenter: parent.verticalCenter
                        }

                        AppText {
                            dt: root.dt
                            anchors.left: parent.left
                            anchors.leftMargin: dt.sp20
                            anchors.right: parent.right
                            anchors.rightMargin: dt.sp12
                            anchors.verticalCenter: parent.verticalCenter
                            text: modelData.label
                            color: wideNavItem.selected ? dt.onPrimaryContainer : dt.textSecondary
                            font.pointSize: dt.labelPt
                            font.family: dt.fontFamily
                            font.weight: wideNavItem.selected ? Font.DemiBold : Font.Normal
                            elide: Text.ElideRight
                        }

                        MouseArea {
                            id: wideNavHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                if (root.appControllerRef) root.appControllerRef.hubTab = modelData.idx
                                else root.currentTab = modelData.idx
                            }
                        }
                    }
                }

                Item { Layout.fillHeight: true }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
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

                    // Logo（窄屏顶栏；宽屏标题已移到左侧导航）
                    Row {
                        visible: !root.wideShell
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

                    // Navigation tabs（窄屏顶栏；宽屏使用左侧一级导航）
                    Row {
                        visible: !root.wideShell
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
}
