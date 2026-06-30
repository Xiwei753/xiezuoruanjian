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

    // ── SystemPalette 推断：theme 为空时从系统调色板推断深浅色 ──
    SystemPalette { id: _sysPalette; colorGroup: SystemPalette.Active }
    readonly property bool _inferDark: {
        var wL = _sysPalette.window.r * 0.2126 + _sysPalette.window.g * 0.7152 + _sysPalette.window.b * 0.0722;
        var tL = _sysPalette.windowText.r * 0.2126 + _sysPalette.windowText.g * 0.7152 + _sysPalette.windowText.b * 0.0722;
        return tL > wL;
    }

    readonly property color normalTextColor: control.theme ? control.theme.onSurface : (_inferDark ? "#E2E2E5" : "#1A1C1E")
    readonly property color placeholderColor: control.theme ? control.theme.textMuted : (_inferDark ? "#8C9198" : "#74777F")
    readonly property color backgroundColor: control.theme ? control.theme.surfaceContainerLow : (_inferDark ? "#1F2229" : "#F6F8FC")
    readonly property color labelColor: control.theme ? control.theme.onSurfaceVariant : (_inferDark ? "#C3C6CF" : "#42474E")
    readonly property color activeBorderColor: control.theme ? control.theme.primary : (_inferDark ? "#92CCFF" : "#006497")
    readonly property color inactiveBorderColor: control.theme ? control.theme.outline : (_inferDark ? "#8C9198" : "#727880")
    readonly property color highlightColor: control.theme ? control.theme.primary : (_inferDark ? "#92CCFF" : "#006497")
    readonly property color highlightTextColor: control.theme ? control.theme.onPrimary : (_inferDark ? "#003351" : "#FFFFFF")

    signal accepted()
    signal editingFinished()

    implicitHeight: inputField.height + (control.label.length > 0 ? 24 : 0)
    implicitWidth: 200

    ColumnLayout {
        anchors.fill: parent
        spacing: control.theme ? control.theme.sp4 : 4

        Text {
            text: control.label
            font.pixelSize: control.theme ? control.theme.label : 13
            color: control.labelColor
            font.weight: Font.Medium
            font.family: control.theme ? control.theme.fontFamily : "sans-serif"
            visible: control.label.length > 0
        }

        TextField {
            id: inputField
            Layout.fillWidth: true
            implicitHeight: control.theme ? control.theme.settingsControlHeight : 40
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
