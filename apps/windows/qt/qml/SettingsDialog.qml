// Windows 设置浮层：Qt Quick Controls Dialog + WindowsSettingsPage。
// 保留与 Linux SettingsDialog 相同的 property/signal/function 面。

import QtQuick
import QtQuick.Controls

Dialog {
    id: root

    modal: true
    anchors.centerIn: Overlay.overlay
    margins: 16
    title: qsTr("设置")
    standardButtons: Dialog.Close
    padding: 0

    readonly property real hostWidth: Overlay.overlay ? Overlay.overlay.width : 0
    readonly property real hostHeight: Overlay.overlay ? Overlay.overlay.height : 0
    readonly property bool widePanel: hostWidth >= 840
    readonly property bool overlayPanel: hostWidth >= 600 && hostHeight >= 480
    width: Math.max(1, Math.min(widePanel ? 1100 : 680, hostWidth - 2 * margins))
    height: Math.max(1, Math.min(widePanel ? 860 : 720, hostHeight - 2 * margins))

    property string searchText: ""
    property var theme: null
    property var settingsBackendRef: null
    property var workspaceBackendRef: null
    property var syncBackendRef: null
    property var editorBackendRef: null
    property var themeControllerRef: null
    property var beforeSyncHook: null

    signal settingsChanged()
    signal openConflict(string targetKind, string projectId, string path)
    signal switchWorkspaceRequested()

    function flushSave() { settingsPage.flushSave() }
    function revealSection(section, settingKey) { settingsPage.revealSection(section, settingKey) }

    WindowsSettingsPage {
        id: settingsPage
        anchors.fill: parent
        widePanel: root.widePanel
        overlayPanel: root.overlayPanel
        settingsBackendRef: root.settingsBackendRef
        workspaceBackendRef: root.workspaceBackendRef
        syncBackendRef: root.syncBackendRef
        editorBackendRef: root.editorBackendRef
        themeControllerRef: root.themeControllerRef
        beforeSyncHook: root.beforeSyncHook
        onSettingsChanged: root.settingsChanged()
        onOpenConflict: function(targetKind, projectId, path) {
            root.openConflict(targetKind, projectId, path)
        }
        onSwitchWorkspaceRequested: root.switchWorkspaceRequested()
    }

    onOpened: {
        settingsPage.resetSearch()
        settingsPage.onDialogOpened()
    }
    onClosed: settingsPage.onDialogClosed()
}
