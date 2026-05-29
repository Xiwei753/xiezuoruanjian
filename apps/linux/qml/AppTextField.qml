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

    signal editingFinished()

    implicitHeight: inputField.height + 22
    implicitWidth: 200

    ColumnLayout {
        anchors.fill: parent
        spacing: control.theme ? control.theme.sp4 : 4

        Label {
            text: control.label
            font.pixelSize: control.theme ? control.theme.fontMd : 13
            color: control.theme ? control.theme.text : "#e2e8f0"
            font.weight: Font.Medium
            visible: control.label.length > 0
        }

        TextField {
            id: inputField
            Layout.fillWidth: true
            implicitHeight: 32
            placeholderText: control.placeholder
            color: control.theme ? control.theme.text : "#e2e8f0"
            echoMode: control.echoMode
            validator: control.validator
            activeFocusOnTab: control.fieldTabFocus
            onEditingFinished: control.editingFinished()
            background: Rectangle {
                color: control.theme ? control.theme.surface : "#1a1a2e"
                border.color: inputField.activeFocus ? (control.theme ? control.theme.borderFocus : "#0ea5e9") : (control.theme ? control.theme.border : "#334155")
                border.width: 1
                radius: control.theme ? control.theme.radiusSm : 4
            }
            font.pixelSize: control.theme ? control.theme.fontMd : 13
            leftPadding: control.theme ? control.theme.sp8 : 8
            topPadding: control.theme ? control.theme.sp6 : 6
            bottomPadding: control.theme ? control.theme.sp6 : 6
        }
    }
}
