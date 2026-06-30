// =============================================================================
// AppComboBox.qml — 通用下拉选择框组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：统一风格的下拉选择框，支持主题适配
// 约束：
//   - 纯 UI 组件，选择值通过 currentIndex 绑定
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls

ComboBox {
    id: control
    property var theme: null

    readonly property color normalTextColor: control.theme.onSurface
    readonly property color disabledTextColor: control.theme.textDisabled
    readonly property color highlightedTextColor: control.theme.onPrimaryContainer
    readonly property color highlightedBackground: control.theme.primaryContainer
    readonly property color defaultBackground: control.theme.surfaceContainerLow
    readonly property color indicatorColor: control.theme.onSurfaceVariant

    implicitWidth: 180
    implicitHeight: Math.max(control.theme.settingsControlHeight, 40)
    leftPadding: control.theme.sp12
    rightPadding: control.theme.sp12 + 20
    topPadding: 0
    bottomPadding: 0

    function indexOfText(value) {
        if (control.model === null || control.model === undefined) return -1
        if (typeof control.model === "string") return control.model === value ? 0 : -1
        if (Array.isArray(control.model)) {
            for (var i = 0; i < control.model.length; i++) {
                var item = control.model[i]
                if (typeof item === "object" && item !== null) {
                    if (item.text === value || item.toString() === value) return i
                } else if (item === value) {
                    return i
                }
            }
            return -1
        }
        if (control.model.count !== undefined) {
            for (var j = 0; j < control.model.count; j++) {
                var entry = control.model.get(j)
                if (entry.text === value || entry.toString() === value) return j
            }
            return -1
        }
        return -1
    }

    contentItem: AppText {
        text: control.displayText
        color: control.enabled ? control.normalTextColor : control.disabledTextColor
        font.pixelSize: control.theme.label
        font.family: control.theme.fontFamily
        leftPadding: control.leftPadding
        rightPadding: control.rightPadding
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }

    indicator: Canvas {
        id: canvas
        x: control.width - width - control.theme.sp8
        y: control.height / 2 - height / 2
        width: 12
        height: 8
        contextType: "2d"
        onPaint: {
            context.reset()
            context.moveTo(0, 0)
            context.lineTo(width, 0)
            context.lineTo(width / 2, height)
            context.closePath()
            context.fillStyle = control.enabled ? control.indicatorColor : control.disabledTextColor
            context.fill()
        }
    }

    background: Rectangle {
        color: control.defaultBackground
        border.color: {
            if (control.activeFocus) return control.theme.borderFocus
            if (control.hovered) return control.theme.outline
            return control.theme.border
        }
        border.width: 1
        radius: control.theme.radiusMd
    }

    delegate: ItemDelegate {
        width: control.width
        contentItem: AppText {
            text: modelData
            color: control.highlightedIndex === index ? control.highlightedTextColor : control.normalTextColor
            font.pixelSize: control.theme.fontMd
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            color: control.highlightedIndex === index ? control.highlightedBackground : "transparent"
        }
    }

    popup: Popup {
        y: control.height
        width: control.width
        implicitHeight: contentItem.implicitHeight
        padding: control.theme.sp4

        contentItem: ListView {
            clip: true
            implicitHeight: contentHeight
            model: control.delegateModel
            currentIndex: control.highlightedIndex
            ScrollIndicator.vertical: ScrollIndicator {}
        }

        background: Rectangle {
            color: control.theme.surface
            border.color: control.theme.border
            border.width: 1
            radius: control.theme.radiusLg
        }
    }
}
