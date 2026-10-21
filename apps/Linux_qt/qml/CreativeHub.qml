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
//
// Issue #977：顶栏与品牌区上移到 main.qml 的 DesktopTopBar。本页只负责
// 「一级导航 + 当前 tab 内容」，不再自绘第二条 64px 顶栏或独立 divider；
// 导航宽度由 main.qml 注入，和顶栏品牌区保持同宽。
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
    // Issue #977：与 DesktopTopBar.navigationWidth 共用同一个窗口级宽度。
    property int navigationWidth: 200
    // Issue #796 评论 5886483653: Hub tab 由 appController.hubTab 驱动，
    // 不要 Loader 重建后永远回默认 0。
    property var appControllerRef: null
    property int currentTab: appControllerRef ? appControllerRef.hubTab : 0
    property bool aiCapable: false
    property bool aiEnabled: false
    // Linux Qt 始终使用桌面外壳：一级导航固定在左栏，窗口宽度只影响内容区。

    signal openProject(string projectId, string projectTitle)
    signal createProject()
    signal openSettings()
    signal requestSync()
    signal requestSearch()

    signal openStarmapWorkspace(string smId, string smTitle)
    signal renameProjectRequested(string projectId, string title)
    signal deleteProjectRequested(string projectId, string title)

    color: Kirigami.Theme.backgroundColor

    // 顶栏已在 main.qml 统一绘制，本页内容从顶栏底线之下开始。
    RowLayout {
        anchors.fill: parent
        spacing: 0

        // Issue #829：宽屏一级导航只放左侧。这里就是手稿里的“作品 / 星图 / 统计”纵栏，
        // 作品/星图/统计仍复用原来的 currentTab + 三个 Loader，不另做第二套路由。
        // 宽屏不再有任何底部导航；侧栏也不为了追求沉浸光感改回底栏。
        // Issue #977：导航加宽到 navigationWidth，条目使用 44–48px 行高与
        // 12–16px 左右内边距，不把文字挤在固定 160px 的窄栏里。
        Rectangle {
            Layout.fillHeight: true
            Layout.preferredWidth: root.navigationWidth
            Layout.minimumWidth: root.navigationWidth
            Layout.maximumWidth: root.navigationWidth
            color: Kirigami.Theme.alternateBackgroundColor

            ColumnLayout {
                anchors.fill: parent
                anchors.leftMargin: Kirigami.Units.largeSpacing
                anchors.rightMargin: Kirigami.Units.largeSpacing
                anchors.topMargin: Kirigami.Units.largeSpacing
                anchors.bottomMargin: Kirigami.Units.smallSpacing
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
                        implicitHeight: Math.max(44, Kirigami.Units.gridUnit * 2.6)
                        leftPadding: Kirigami.Units.largeSpacing
                        rightPadding: Kirigami.Units.largeSpacing
                        text: modelData.label
                        icon.name: modelData.icon
                        icon.width: Kirigami.Units.iconSizes.smallMedium
                        icon.height: Kirigami.Units.iconSizes.smallMedium
                        icon.color: selected ? Kirigami.Theme.highlightColor
                                             : Kirigami.Theme.textColor
                        font.weight: selected ? Font.DemiBold : Font.Normal
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

        // Content：三个 tab 仍复用原 Loader，不因顶栏上移重建页面状态。
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
