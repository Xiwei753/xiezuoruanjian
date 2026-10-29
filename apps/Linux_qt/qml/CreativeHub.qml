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
// 「一级导航 + 当前 tab 内容」，不再自绘第二条 64px 顶栏或独立 divider。
// Issue #1014：导航改为 SplitView 左 pane，宽度从 DesktopLayoutState 按
// Hub tab 读取；拖动结束才保存覆盖值，切页 / 窗口缩放只更新显示宽度。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Rectangle {
    id: root
    required property var dt
    // Issue #1014：整窗唯一的页面骨架尺寸 / 折叠状态，由 main.qml 注入。
    property var layoutState: null
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
    // Issue #1014：三个 Hub tab 分别保存左栏拖动宽度。
    readonly property string currentPageKey:
        root.layoutState ? root.layoutState.hubPageKey(root.currentTab) : "hub.projects"
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

    // Issue #1014：切页 / 注入布局状态时只把 SplitView 显示宽度切到该页保存值，
    // 不调用 setDraggedWidth，避免把布局重算误记成用户拖动。
    function applyPageWidth() {
        if (!root.layoutState)
            return
        navigationPane.SplitView.preferredWidth =
            root.layoutState.paneWidth(root.currentPageKey, "left")
    }

    onCurrentTabChanged: applyPageWidth()
    onLayoutStateChanged: applyPageWidth()
    Component.onCompleted: applyPageWidth()

    // 顶栏已在 main.qml 统一绘制，本页内容从顶栏底线之下开始。
    // Issue #1014：导航与内容改为 Qt 原生 SplitView，左栏宽度按 Hub tab 分别保存。
    SplitView {
        id: hubSplitView
        anchors.fill: parent
        orientation: Qt.Horizontal

        // 原生 6vp 分隔线：只画视觉，不挂 MouseArea / DragHandler，拖动由
        // Qt SplitView 自己接管。命中区用 containmentMask 扩到 14vp，
        // 不改变 6vp 的视觉宽度。
        handle: Rectangle {
            id: splitHandle
            implicitWidth: 6
            containmentMask: Item {
                x: (splitHandle.width - width) / 2
                width: 14
                height: hubSplitView.height
            }
            color: SplitHandle.pressed
                   ? Kirigami.Theme.highlightColor
                   : (SplitHandle.hovered ? Kirigami.Theme.focusColor : "transparent")
            Rectangle {
                anchors.centerIn: parent
                width: 1
                height: parent.height
                color: Kirigami.Theme.disabledTextColor
                opacity: 0.3
            }
        }

        // 只有用户拖动结束才持久化一次；普通切页 / 窗口缩放不写覆盖值。
        onResizingChanged: {
            if (hubSplitView.resizing || !root.layoutState)
                return
            root.layoutState.setDraggedWidth(root.currentPageKey, "left",
                                             navigationPane.width)
        }

        // Issue #829：宽屏一级导航只放左侧。这里就是手稿里的“作品 / 星图 / 统计”纵栏，
        // 作品/星图/统计仍复用原来的 currentTab + 三个 Loader，不另做第二套路由。
        // 宽屏不再有任何底部导航；侧栏也不为了追求沉浸光感改回底栏。
        // Issue #977：条目使用 44–48px 行高与 12–16px 左右内边距，不把文字挤在
        // 固定 160px 的窄栏里。
        Rectangle {
            id: navigationPane
            SplitView.minimumWidth: root.layoutState ? root.layoutState.minLeftWidth : 200
            SplitView.preferredWidth: root.layoutState
                ? root.layoutState.paneWidth(root.currentPageKey, "left") : 240
            SplitView.maximumWidth: 480
            SplitView.fillHeight: true
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
            SplitView.fillWidth: true
            SplitView.fillHeight: true
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
