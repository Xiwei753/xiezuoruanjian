// =============================================================================
// EditorContextMenu.qml - 编辑器右键/长按菜单
// =============================================================================
//
// 职责：Desktop UI 层（QML 组件）
// 边界：只负责菜单弹出和命令分发（不包含选区逻辑和业务判断）
// 约束：
//   - 菜单项文本固定，不依赖外部数据源
//   - 菜单项状态绑定 SujianEditorItem 属性
//   - 不包含业务逻辑，不直接调用 Core

import QtQuick
import QtQuick.Controls

Menu {
    id: editorContextMenu

    property var editorItem: null
    property var dt: null

    // ── SystemPalette 推断：dt 为空时从系统调色板推断深浅色 ──
    SystemPalette { id: _sysPalette; colorGroup: SystemPalette.Active }
    readonly property bool _inferDark: {
        var wL = _sysPalette.window.r * 0.2126 + _sysPalette.window.g * 0.7152 + _sysPalette.window.b * 0.0722;
        var tL = _sysPalette.windowText.r * 0.2126 + _sysPalette.windowText.g * 0.7152 + _sysPalette.windowText.b * 0.0722;
        return tL > wL;
    }

    background: Rectangle {
        color: dt ? dt.surface : (_inferDark ? "#1A1D23" : "#FCFCFF")
        border.color: dt ? dt.border : (_inferDark ? "#2A2E36" : "#CBD5E1")
        radius: dt ? dt.radiusMd : 12
        border.width: 1
    }

    MenuItem {
        text: qsTr("复制")
        enabled: editorItem && editorItem.has_selection
        contentItem: AppText {
            text: parent.text
            color: parent.enabled ? (dt ? dt.textPrimary : (_inferDark ? "#E2E2E5" : "#1A1C1E")) : (dt ? dt.textMuted : (_inferDark ? "#8C9198" : "#74777F"))
            font.pixelSize: dt ? dt.label : 13
            font.family: dt ? dt.fontFamily : "sans-serif"
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            color: parent.highlighted ? (dt ? dt.surfaceVariant : (_inferDark ? "#42474E" : "#DFE3EB")) : "transparent"
        }
        onTriggered: {
            if (editorItem) editorItem.clipboard_copy()
        }
    }

    MenuItem {
        text: qsTr("粘贴")
        enabled: editorItem && editorItem.editor_enabled
        contentItem: AppText {
            text: parent.text
            color: parent.enabled ? (dt ? dt.textPrimary : (_inferDark ? "#E2E2E5" : "#1A1C1E")) : (dt ? dt.textMuted : (_inferDark ? "#8C9198" : "#74777F"))
            font.pixelSize: dt ? dt.label : 13
            font.family: dt ? dt.fontFamily : "sans-serif"
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            color: parent.highlighted ? (dt ? dt.surfaceVariant : (_inferDark ? "#42474E" : "#DFE3EB")) : "transparent"
        }
        onTriggered: {
            if (editorItem) editorItem.clipboard_paste()
        }
    }

    MenuItem {
        text: qsTr("全选")
        enabled: editorItem && editorItem.editor_enabled
        contentItem: AppText {
            text: parent.text
            color: parent.enabled ? (dt ? dt.textPrimary : (_inferDark ? "#E2E2E5" : "#1A1C1E")) : (dt ? dt.textMuted : (_inferDark ? "#8C9198" : "#74777F"))
            font.pixelSize: dt ? dt.label : 13
            font.family: dt ? dt.fontFamily : "sans-serif"
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            color: parent.highlighted ? (dt ? dt.surfaceVariant : (_inferDark ? "#42474E" : "#DFE3EB")) : "transparent"
        }
        onTriggered: {
            if (editorItem) editorItem.select_all()
        }
    }

    MenuItem {
        text: qsTr("删除")
        enabled: editorItem && editorItem.has_selection
        contentItem: AppText {
            text: parent.text
            color: parent.enabled ? (dt ? dt.error : (_inferDark ? "#FFB4AB" : "#BA1A1A")) : (dt ? dt.textMuted : (_inferDark ? "#8C9198" : "#74777F"))
            font.pixelSize: dt ? dt.label : 13
            font.family: dt ? dt.fontFamily : "sans-serif"
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            color: parent.highlighted ? (dt ? dt.surfaceVariant : (_inferDark ? "#42474E" : "#DFE3EB")) : "transparent"
        }
        onTriggered: {
            if (editorItem) editorItem.delete_selection()
        }
    }
}
