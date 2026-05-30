// =============================================================================
// AppTextField.qml — 通用文本输入框组件
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
// 职责：统一风格的文本输入框，支持标签和占位符
// 约束：
//   - 纯 UI 组件，输入值通过 text property 绑定
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: control
    property var theme: null
    property string label: ""
    property string placeholder: ""
    property alias placeholderText: control.placeholder
    property alias text: inputField.text
    property alias echoMode: inputField.echoMode
    property alias validator: inputField.validator
    property bool fieldTabFocus: true

    signal accepted()
    signal editingFinished()

    implicitHeight: inputField.height + (control.label.length > 0 ? 24 : 0)
    implicitWidth: 200

    ColumnLayout {
        anchors.fill: parent
        spacing: control.theme ? control.theme.sp4 : 4

        Label {
            text: control.label
            font.pixelSize: control.theme ? control.theme.label : 13
            color: control.theme ? control.theme.textSecondary : control.palette.text
            font.weight: Font.Medium
            font.family: control.theme ? control.theme.fontFamily : "sans-serif"
            visible: control.label.length > 0
        }

        TextField {
            id: inputField
            Layout.fillWidth: true
            implicitHeight: control.theme ? control.theme.settingsControlHeight : 40
            placeholderText: control.placeholder
            color: control.theme ? control.theme.textPrimary : control.palette.text
            placeholderTextColor: control.theme ? control.theme.textMuted : control.palette.placeholderText
            selectionColor: control.theme ? control.theme.primary : control.palette.highlight
            selectedTextColor: control.theme ? control.theme.onPrimary : control.palette.highlightedText
            activeFocusOnTab: control.fieldTabFocus
            onAccepted: control.accepted()
            onEditingFinished: control.editingFinished()
            background: Rectangle {
                color: control.theme ? control.theme.inputBg : "#ffffff"
                border.color: inputField.activeFocus ? (control.theme ? control.theme.borderFocus : "#006497") : (control.theme ? control.theme.controlBorder : "#cbd5e1")
                border.width: inputField.activeFocus ? 2 : 1
                radius: control.theme ? control.theme.radiusMd : 12
            }
            font.pixelSize: control.theme ? control.theme.body : 14
            font.family: control.theme ? control.theme.fontFamily : "sans-serif"
            leftPadding: control.theme ? control.theme.sp12 : 12
            rightPadding: control.theme ? control.theme.sp12 : 12
            topPadding: control.theme ? control.theme.sp8 : 8
            bottomPadding: control.theme ? control.theme.sp8 : 8
        }
    }
}
