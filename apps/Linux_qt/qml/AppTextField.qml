// =============================================================================
// AppTextField.qml — 通用文本输入框组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
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

    readonly property color normalTextColor: control.theme.onSurface
    readonly property color placeholderColor: control.theme.textMuted
    readonly property color backgroundColor: control.theme.surfaceContainerLow
    readonly property color labelColor: control.theme.onSurfaceVariant
    readonly property color activeBorderColor: control.theme.primary
    readonly property color inactiveBorderColor: control.theme.outline
    readonly property color highlightColor: control.theme.primary
    readonly property color highlightTextColor: control.theme.onPrimary

    signal accepted()
    signal editingFinished()

    implicitHeight: inputField.height + (control.label.length > 0 ? 24 : 0)
    implicitWidth: 200

    ColumnLayout {
        anchors.fill: parent
        spacing: control.theme.sp4

        Text {
            text: control.label
            font.pixelSize: control.theme.label
            color: control.labelColor
            font.weight: Font.Medium
            font.family: control.theme.fontFamily
            visible: control.label.length > 0
        }

        TextField {
            id: inputField
            Layout.fillWidth: true
            implicitHeight: control.theme.settingsControlHeight
            placeholderText: control.placeholder
            color: control.normalTextColor
            placeholderTextColor: control.placeholderColor
            selectionColor: control.highlightColor
            selectedTextColor: control.highlightTextColor
            activeFocusOnTab: control.fieldTabFocus
            onAccepted: control.accepted()
            onEditingFinished: control.editingFinished()
            background: Rectangle {
                color: control.backgroundColor
                border.color: inputField.activeFocus ? control.activeBorderColor : control.inactiveBorderColor
                border.width: inputField.activeFocus ? 2 : 1
                radius: control.theme.radiusMd
            }
            font.pixelSize: control.theme.body
            font.family: control.theme.fontFamily
            leftPadding: control.theme.sp12
            rightPadding: control.theme.sp12
            topPadding: control.theme.sp8
            bottomPadding: control.theme.sp8
        }
    }
}
