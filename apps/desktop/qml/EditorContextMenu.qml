// =============================================================================
// EditorContextMenu.qml - ?????/???????
// =============================================================================
//
// ??:Desktop UI ?(QML ??)
// ??:??????????(???????????)
// ??:
//   - ???????,???????
//   - ??????? SujianEditorItem ???
//   - ?????????? Core
//

import QtQuick
import QtQuick.Controls

Menu {
    id: editorContextMenu

    property var editorItem: null

    background: Rectangle {
        color: dt ? dt.surface : "#1A1D23"
        border.color: dt ? dt.border : "#2A2E36"
        radius: dt ? dt.radiusMd : 12
        border.width: 1
    }

    MenuItem {
        text: qsTr("??")
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
        text: qsTr("??")
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
        text: qsTr("??")
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
        text: qsTr("??")
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