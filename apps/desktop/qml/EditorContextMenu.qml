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

    background: Rectangle {
        color: dt ? dt.surface : "#1A1D23"
        border.color: dt ? dt.border : "#2A2E36"
        radius: dt ? dt.radiusMd : 12
        border.width: 1
    }

    MenuItem {
        text: qsTr("复制")
        enabled: editorItem && editorItem.has_selection
        contentItem: AppText {
            text: parent.text
            color: parent.enabled ? (dt ? dt.textPrimary : "#E2E2E5") : (dt ? dt.textMuted : "#5A5E66")
            font.pixelSize: dt ? dt.label : 13
            font.family: dt ? dt.fontFamily : "sans-serif"
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            color: parent.highlighted ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"
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
            color: parent.enabled ? (dt ? dt.textPrimary : "#E2E2E5") : (dt ? dt.textMuted : "#5A5E66")
            font.pixelSize: dt ? dt.label : 13
            font.family: dt ? dt.fontFamily : "sans-serif"
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            color: parent.highlighted ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"
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
            color: parent.enabled ? (dt ? dt.textPrimary : "#E2E2E5") : (dt ? dt.textMuted : "#5A5E66")
            font.pixelSize: dt ? dt.label : 13
            font.family: dt ? dt.fontFamily : "sans-serif"
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            color: parent.highlighted ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"
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
            color: parent.enabled ? (dt ? dt.error : "#FFB4AB") : (dt ? dt.textMuted : "#5A5E66")
            font.pixelSize: dt ? dt.label : 13
            font.family: dt ? dt.fontFamily : "sans-serif"
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            color: parent.highlighted ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"
        }
        onTriggered: {
            if (editorItem) editorItem.delete_selection()
        }
    }
}
