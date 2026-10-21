// =============================================================================
// CreativeHub.qml — 创作中心首页
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：作品列表展示、最近编辑入口、星图入口、统计入口
// 约束：
//   - 纯展示层，业务逻辑通过 signal 传递给 main.qml
//   - 不直接操作文件系统或 Core 层
//
// Issue #900：视觉迁移到 KDE/Breeze（Kirigami 侧栏 + Controls.ItemDelegate /
// Controls.Button / Controls.Label），颜色与间距统一取 Kirigami.Theme /
// Kirigami.Units，不再读 DesignTokens 的 M3 字段。
// 保留：宽屏左侧一级导航 + 内容区（无底部导航），以及全部公共 API。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

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
    // Linux Qt 始终使用桌面外壳：一级导航固定在左栏，窗口宽度只影响内容区。

    // Issue #827 评论 3 第 1 点：桌面顶栏高度。左栏「素笺」行与右侧动作条
    // 必须用同一个值，两条分隔线才能落在同一个 y 上。
    readonly property int topBarHeight: 64

    signal openProject(string projectId, string projectTitle)
    signal createProject()
    signal openSettings()
    signal requestSync()
    signal requestSearch()

    signal openStarmapWorkspace(string smId, string smTitle)
    signal renameProjectRequested(string projectId, string title)
    signal deleteProjectRequested(string projectId, string title)

    color: Kirigami.Theme.backgroundColor

    RowLayout {
        anchors.fill: parent
        spacing: 0

        // Issue #829：宽屏一级导航只放左侧。这里就是手稿里的“作品 / 星图 / 统计”纵栏，
        // 作品/星图/统计仍复用原来的 currentTab + 三个 Loader，不另做第二套路由。
        // 宽屏不再有任何底部导航；侧栏也不为了追求沉浸光感改回底栏。
        Rectangle {
            Layout.fillHeight: true
            Layout.preferredWidth: 160
            color: Kirigami.Theme.alternateBackgroundColor
            border.color: Kirigami.Theme.disabledTextColor
            border.width: 1

            // Issue #827 评论 3 第 1 点：品牌行固定 64 高，与右侧 Top bar 同高，
            // divider 紧随其后 —— 左上「素笺」和右上动作属于同一行高度，
            // 横线才能像草图那样从最左贯通到最右。
            // 外层不加 anchors.margins（左右），divider 才能铺满整个左栏宽度。
            ColumnLayout {
                anchors.fill: parent
                spacing: 0

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: root.topBarHeight
                    color: Kirigami.Theme.alternateBackgroundColor

                    Kirigami.Heading {
                        anchors.left: parent.left
                        anchors.leftMargin: Kirigami.Units.largeSpacing
                        anchors.verticalCenter: parent.verticalCenter
                        level: 3
                        text: qsTr("素笺")
                        color: Kirigami.Theme.highlightColor
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 1
                    color: Kirigami.Theme.disabledTextColor
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.leftMargin: Kirigami.Units.smallSpacing
                    Layout.rightMargin: Kirigami.Units.smallSpacing
                    Layout.topMargin: Kirigami.Units.smallSpacing
                    Layout.bottomMargin: Kirigami.Units.smallSpacing
                    spacing: Kirigami.Units.smallSpacing

                    Repeater {
                        model: [
                            { label: qsTr("作品"), icon: "folder-documents", idx: 0 },
                            { label: qsTr("星图"), icon: "draw-star", idx: 1 },
                            { label: qsTr("统计"), icon: "office-chart-bar", idx: 2 }
                        ]

                        ItemDelegate {
                            readonly property bool selected: root.currentTab === modelData.idx
                            Layout.fillWidth: true
                            text: modelData.label
                            icon.name: modelData.icon
                            highlighted: selected
                            onClicked: {
                                if (root.appControllerRef) root.appControllerRef.hubTab = modelData.idx
                                else root.currentTab = modelData.idx
                            }
                        }
                    }

                    Item { Layout.fillHeight: true }
                }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            // Top navigation bar
            // Issue #827 评论 3 第 1 点：高度与左栏「素笺」行共用 root.topBarHeight。
            // Issue #827 评论 4 第 2 点：divider 不能再画在 64 高 Rectangle 内部
            // （那样落在 y=63..64，而左栏是 64+1=64..65，两条线差 1px 错格）。
            // 改成和左栏完全同构的「64 高面 + 1px divider 兄弟项」，
            // 横线才真正从最左贯通到最右。
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: root.topBarHeight
                    color: Kirigami.Theme.alternateBackgroundColor

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: Kirigami.Units.largeSpacing
                        anchors.rightMargin: Kirigami.Units.largeSpacing
                        spacing: Kirigami.Units.largeSpacing

                        Item { Layout.fillWidth: true }

                        // Right actions — 收口到 GlobalTopActions（同步 / 搜索 / 设置）
                        GlobalTopActions {
                            dt: root.dt
                            appState: root.appState
                            syncSource: "hub"
                            compact: parent.width < 640
                            onRequestSync: root.requestSync()
                            onRequestSearch: root.requestSearch()
                            onOpenSettings: root.openSettings()
                        }
                    }
                }

                // 1px divider 作为 64 高顶栏的兄弟项，和左栏那条严格同一 y。
                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 1
                    color: Kirigami.Theme.disabledTextColor
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
