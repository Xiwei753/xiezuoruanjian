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
    property alias text: staticDisplay.displayText
    property alias echoMode: staticDisplay.echoMode
    property bool fieldTabFocus: true

    property var coordinator: null
    property string targetId: ""
    property bool isPersistent: false
    property bool isSecret: false
    property bool isSearch: false
    property bool isUrl: false
    property bool isEditingActive: false

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

    implicitHeight: staticDisplay.height + (control.label.length > 0 ? 24 : 0)
    implicitWidth: 200

    function _isSujianEditorItem(obj) {
        return obj && typeof obj.register_text_target_qml === "function"
    }

    Component.onCompleted: {
        if (coordinator && targetId.length > 0) {
            if (_isSujianEditorItem(coordinator)) {
                if (isSecret) {
                    coordinator.register_secret_target_qml(targetId, isPersistent, text)
                } else if (isSearch) {
                    coordinator.register_search_target_qml(targetId, text)
                } else if (isUrl) {
                    coordinator.register_url_target_qml(targetId, text)
                } else {
                    coordinator.register_text_target_qml(targetId, isPersistent, text)
                }
            } else {
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
    }

    Component.onDestruction: {
        if (coordinator && targetId.length > 0) {
            if (_isSujianEditorItem(coordinator)) {
                coordinator.unregister_text_target_qml(targetId)
            } else {
                coordinator.unregister_target(targetId)
            }
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

        Rectangle {
            id: staticDisplay
            Layout.fillWidth: true
            implicitHeight: control.resolvedDt.settingsControlHeight
            color: control.backgroundColor
            border.color: isEditingActive ? control.activeBorderColor : control.inactiveBorderColor
            border.width: isEditingActive ? 2 : 1
            radius: control.resolvedDt.radiusMd

            property string displayText: ""
            property int echoMode: TextInput.Normal

            Text {
                id: displayLabel
                anchors.fill: parent
                anchors.leftMargin: control.resolvedDt.sp12
                anchors.rightMargin: control.resolvedDt.sp12
                anchors.topMargin: control.resolvedDt.sp8
                anchors.bottomMargin: control.resolvedDt.sp8
                text: staticDisplay.echoMode === TextInput.Password
                      ? "\u2022".repeat(staticDisplay.displayText.length)
                      : (staticDisplay.displayText.length > 0 ? staticDisplay.displayText : control.placeholder)
                color: staticDisplay.displayText.length > 0 ? control.normalTextColor : control.placeholderColor
                font.pixelSize: control.resolvedDt.body
                font.family: control.resolvedDt.fontFamily
                verticalAlignment: Text.AlignVCenter
                elide: Text.ElideRight
            }

            MouseArea {
                anchors.fill: parent
                onClicked: {
                    if (coordinator && targetId.length > 0) {
                        isEditingActive = true
                        if (_isSujianEditorItem(coordinator)) {
                            coordinator.begin_text_edit_qml(targetId)
                        } else {
                            coordinator.begin_edit(targetId)
                        }
                    }
                }
            }
        }
    }

    function updateText(newText) {
        staticDisplay.displayText = newText
        if (coordinator && targetId.length > 0) {
            if (_isSujianEditorItem(coordinator)) {
                coordinator.update_target_text_qml(targetId, newText)
            } else {
                coordinator.update_text(targetId, newText)
            }
        }
    }
}
