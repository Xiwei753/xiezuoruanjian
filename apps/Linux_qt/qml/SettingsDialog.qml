// =============================================================================
// SettingsDialog.qml — 设置浮层（KDE）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：浮在打开设置前那一页之上的设置弹层；内容由 KdeSettingsPage 承载。
// Issue #900：改为 KDE Controls Dialog + 滚动 KdeSettingsPage：
//   - 顶部标题/关闭由 Breeze Dialog 提供，不再手绘 topBar 与关闭按钮；
//   - 尺寸只由窗口可用高度与面板上限决定，展开内容在 ScrollView 内滚动；
//   - 保留原有 property/signal 面（settingsChanged/openConflict/
//     switchWorkspaceRequested/flushSave），主窗口接线不变。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Dialog {
    id: root
    modal: true
    // Issue #912：Dialog 是 Popup，位置坐标属于 Popup 父级（Overlay）而非普通
    // Item 父级。原来 anchors.centerIn: parent 会让弹窗相对 Loader 这个零尺寸
    // 宿主居中，结果被摆到窗口左上角甚至越界，「诊断与日志→导出日志」点不到。
    // 这里改成相对 Overlay.overlay 居中，并按 overlay 实际尺寸收口，弹窗始终
    // 落在当前主窗口内，窗口缩放后依然居中可用。
    // 参考 https://doc.qt.io/qt-6/qml-qtquick-controls-popup.html#popup-positioning
    anchors.centerIn: Overlay.overlay
    margins: Kirigami.Units.gridUnit
    title: qsTr("设置")
    // Issue #900 复核：Qt Quick Controls 的 Dialog.title 只画标题栏，不会自动给
    // 关闭按钮。用原生 standardButtons 让 Breeze DialogButtonBox 提供可见的
    // 「关闭」入口（点击即 reject/close），不再手绘顶部按钮。
    standardButtons: Dialog.Close

    // Issue #825：设置始终是浮层，悬浮在打开设置前的那一页之上，不改成路由页。
    // 宽度分两档：primaryNavigationPlacement === Side 用大号面板；其余用窄对话框。
    // Issue #836：layoutPlan 为 null 时默认宽屏 / Workbench。
    readonly property bool widePanel: layoutPlan === null || layoutPlan.primaryNavigationPlacement === "Side"
    // Workbench 形态保留顶部搜索。
    readonly property bool overlayPanel: layoutPlan === null || layoutPlan.workspaceLayoutMode === "Workbench"

    // Issue #912：可用尺寸只取 Overlay 宿主（即主窗口）大小，去掉旧的最小宽度
    // 400，避免小窗口下弹窗比窗口还宽被推出屏幕。
    readonly property real hostWidth: Overlay.overlay ? Overlay.overlay.width : 0
    readonly property real hostHeight: Overlay.overlay ? Overlay.overlay.height : 0
    width: Math.max(1, Math.min(root.widePanel ? 1120 : 640, hostWidth - 2 * margins))
    height: Math.max(1, Math.min(root.widePanel ? 880 : 720, hostHeight - 2 * margins))

    property var layoutPlan: null
    // 宽屏顶部搜索关键词。只做分组过滤，不改任何设置读取/保存路径。
    property string searchText: ""
    property var theme: null
    property var settingsBackendRef: null
    property var workspaceBackendRef: null
    property var syncBackendRef: null
    property var editorBackendRef: null
    property var themeControllerRef: null
    property var beforeSyncHook: null

    signal settingsChanged()
    // Issue #762 评论 5826175490 第 4 点：转发 SyncPage 的 openConflict 信号给主窗口
    signal openConflict(string projectId, string path)
    // Issue #790 评论 5875963057: 切换工作区入口收口到设置页
    signal switchWorkspaceRequested()

    function flushSave() { settingsPage.flushSave() }

    KdeSettingsPage {
        id: settingsPage
        anchors.fill: parent
        layoutPlan: root.layoutPlan
        settingsBackendRef: root.settingsBackendRef
        workspaceBackendRef: root.workspaceBackendRef
        syncBackendRef: root.syncBackendRef
        editorBackendRef: root.editorBackendRef
        themeControllerRef: root.themeControllerRef
        beforeSyncHook: root.beforeSyncHook
        onSettingsChanged: root.settingsChanged()
        onOpenConflict: function(projectId, path) { root.openConflict(projectId, path) }
        onSwitchWorkspaceRequested: root.switchWorkspaceRequested()
    }

    onOpened: {
        // Issue #825 / #900 复核：每次打开都重置宽屏搜索，避免上一次过滤结果留在浮层里；
        // 必须同时清空搜索文本框与 searchText，否则关闭再打开会出现
        // 「框内仍显示旧搜索词、但分组已全部显示」的失配。
        settingsPage.resetSearch()
        settingsPage.onDialogOpened()
    }
    onClosed: settingsPage.onDialogClosed()
}
