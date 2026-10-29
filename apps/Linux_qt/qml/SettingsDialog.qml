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

MotionDialog {
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

    // Issue #920：设置面板始终是浮层，悬浮在打开设置前的那一页之上，不改成路由页。
    // 宽度 / 形态由 Qt 自己按宿主窗口宽度断点决定，不再从 Core 读 layoutPlan：
    // - ≥840vp：宽屏两列大号面板（与一级导航 Side 的断点一致）；
    // - ≥600vp：Workbench 形态，保留顶部搜索；
    // - 更窄：窄对话框、单列。
    readonly property int wideBreakpoint: 840
    readonly property int overlayBreakpoint: 600
    // Issue #912：可用尺寸只取 Overlay 宿主（即主窗口）大小，去掉旧的最小宽度
    // 400，避免小窗口下弹窗比窗口还宽被推出屏幕。
    readonly property real hostWidth: Overlay.overlay ? Overlay.overlay.width : 0
    readonly property real hostHeight: Overlay.overlay ? Overlay.overlay.height : 0
    readonly property bool widePanel: root.hostWidth >= root.wideBreakpoint
    readonly property bool overlayPanel: root.hostWidth >= root.overlayBreakpoint && root.hostHeight >= 480
    width: Math.max(1, Math.min(root.widePanel ? 1120 : 640, hostWidth - 2 * margins))
    height: Math.max(1, Math.min(root.widePanel ? 880 : 720, hostHeight - 2 * margins))

    // 宽屏顶部搜索关键词。只做分组过滤，不改任何设置读取/保存路径。
    property string searchText: ""
    property var theme: null
    property var settingsBackendRef: null
    property var workspaceBackendRef: null
    property var syncBackendRef: null
    property var editorBackendRef: null
    property var themeControllerRef: null
    // Issue #1014：与写作栏 AI 面板共享同一份 Agent CLI 清单/选择。
    property var agentCliBackendRef: null
    property var beforeSyncHook: null

    signal settingsChanged()
    // Issue #762 评论 5826175490 第 4 点：转发 SyncPage 的 openConflict 信号给主窗口
    // Issue #902：openConflict 增加 targetKind 参数（"app"/"project"），区分 App 级与作品级冲突。
    signal openConflict(string targetKind, string projectId, string path)
    // Issue #790 评论 5875963057: 切换工作区入口收口到设置页
    signal switchWorkspaceRequested()

    function flushSave() { settingsPage.flushSave() }

    function revealSection(section, settingKey) { settingsPage.revealSection(section, settingKey) }

    KdeSettingsPage {
        id: settingsPage
        anchors.fill: parent
        // Issue #920：面板形态由 SettingsDialog 按宿主窗口宽度断点算好后注入。
        widePanel: root.widePanel
        overlayPanel: root.overlayPanel
        settingsBackendRef: root.settingsBackendRef
        workspaceBackendRef: root.workspaceBackendRef
        syncBackendRef: root.syncBackendRef
        editorBackendRef: root.editorBackendRef
        themeControllerRef: root.themeControllerRef
        agentCliBackendRef: root.agentCliBackendRef
        beforeSyncHook: root.beforeSyncHook
        onSettingsChanged: root.settingsChanged()
        // Issue #902：转发 targetKind 给主窗口，保持信号链一致。
        onOpenConflict: function(targetKind, projectId, path) { root.openConflict(targetKind, projectId, path) }
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
