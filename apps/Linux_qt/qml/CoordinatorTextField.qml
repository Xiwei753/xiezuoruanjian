import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: control

    property var dt: null
    DesignTokens { id: fallbackDt }
    readonly property var resolvedDt: dt || fallbackDt

    property string label: ""
    property string placeholder: ""
    property alias placeholderText: control.placeholder
    property alias text: inputField.text
    property alias echoMode: inputField.echoMode
    property alias validator: inputField.validator
    property bool fieldTabFocus: true

    property var coordinator: null
    property string targetId: ""
    property bool isPersistent: false
    property bool isSecret: false
    property bool isSearch: false
    property bool isUrl: false

    readonly property color normalTextColor: control.resolvedDt.onSurface
    readonly property color placeholderColor: control.resolvedDt.textMuted
    readonly property color backgroundColor: control.resolvedDt.surfaceContainerLow
    readonly property color labelColor: control.resolvedDt.onSurfaceVariant
    readonly property color activeBorderColor: control.resolvedDt.primary
    readonly property color inactiveBorderColor: control.resolvedDt.outline
    readonly property color highlightColor: control.resolvedDt.primary
    readonly property color highlightTextColor: control.resolvedDt.onPrimary

    signal accepted()
    signal editingFinished()

    implicitHeight: inputField.height + (control.label.length > 0 ? 24 : 0)
    implicitWidth: 200

    Component.onCompleted: {
        if (coordinator && targetId.length > 0) {
            if (isSecret) {
                coordinator.register_secret_target(targetId, isPersistent, text)
            } else if (isSearch) {
                coordinator.register_search_target(targetId, text)
            } else if (isUrl) {
                coordinator.register_url_target(targetId, text)
            } else {
                coordinator.register_target(targetId, isPersistent, text)
            }
        }
    }

    Component.onDestruction: {
        if (coordinator && targetId.length > 0) {
            coordinator.unregister_target(targetId)
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: control.resolvedDt.sp4

        Text {
            text: control.label
            font.pixelSize: control.resolvedDt.label
            color: control.labelColor
            font.weight: Font.Medium
            font.family: control.resolvedDt.fontFamily
            visible: control.label.length > 0
        }

        TextField {
            id: inputField
            Layout.fillWidth: true
            implicitHeight: control.resolvedDt.settingsControlHeight
            placeholderText: control.placeholder
            color: control.normalTextColor
            placeholderTextColor: control.placeholderColor
            selectionColor: control.highlightColor
            selectedTextColor: control.highlightTextColor
            activeFocusOnTab: control.fieldTabFocus
            onAccepted: control.accepted()
            onEditingFinished: {
                if (coordinator && targetId.length > 0) {
                    coordinator.update_text(targetId, text)
                    coordinator.commit_edit()
                }
                control.editingFinished()
            }
            onActiveFocusChanged: {
                if (coordinator && targetId.length > 0) {
                    if (activeFocus) {
                        coordinator.begin_edit(targetId)
                    } else {
                        coordinator.update_text(targetId, text)
                        coordinator.commit_edit()
                    }
                }
            }
            onTextChanged: {
                if (coordinator && targetId.length > 0 && activeFocus) {
                    coordinator.update_text(targetId, text)
                }
            }
            background: Rectangle {
                color: control.backgroundColor
                border.color: inputField.activeFocus ? control.activeBorderColor : control.inactiveBorderColor
                border.width: inputField.activeFocus ? 2 : 1
                radius: control.resolvedDt.radiusMd
            }
            font.pixelSize: control.resolvedDt.body
            font.family: control.resolvedDt.fontFamily
            leftPadding: control.resolvedDt.sp12
            rightPadding: control.resolvedDt.sp12
            topPadding: control.resolvedDt.sp8
            bottomPadding: control.resolvedDt.sp8
        }
    }
}
