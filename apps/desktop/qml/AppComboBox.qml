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

    // ── SystemPalette 推断：theme 为空时从系统调色板推断深浅色 ──
    SystemPalette { id: _sysPalette; colorGroup: SystemPalette.Active }
    readonly property bool _inferDark: {
        var wL = _sysPalette.window.r * 0.2126 + _sysPalette.window.g * 0.7152 + _sysPalette.window.b * 0.0722;
        var tL = _sysPalette.windowText.r * 0.2126 + _sysPalette.windowText.g * 0.7152 + _sysPalette.windowText.b * 0.0722;
        return tL > wL;
    }

    readonly property color normalTextColor: control.theme ? control.theme.onSurface : (_inferDark ? "#E2E2E5" : "#1A1C1E")
    readonly property color disabledTextColor: control.theme ? control.theme.textDisabled : (_inferDark ? "#5A5E66" : "#1A1C1E61")
    readonly property color highlightedTextColor: control.theme ? control.theme.onPrimaryContainer : (_inferDark ? "#CCE5FF" : "#001E31")
    readonly property color highlightedBackground: control.theme ? control.theme.primaryContainer : (_inferDark ? "#004A77" : "#CCE5FF")
    readonly property color defaultBackground: control.theme ? control.theme.surfaceContainerLow : (_inferDark ? "#1F2229" : "#F1F5F9")
    readonly property color indicatorColor: control.theme ? control.theme.onSurfaceVariant : (_inferDark ? "#C3C6CF" : "#42474E")

    implicitWidth: 180
    implicitHeight: Math.max(control.theme ? control.theme.settingsControlHeight : 36, 40)
    leftPadding: control.theme ? control.theme.sp12 : 12
    rightPadding: (control.theme ? control.theme.sp12 : 12) + 20
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
        font.pixelSize: control.theme ? control.theme.label : 13
        font.family: control.theme ? control.theme.fontFamily : "sans-serif"
        leftPadding: control.leftPadding
        rightPadding: control.rightPadding
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }

    indicator: Canvas {
        id: canvas
        x: control.width - width - (control.theme ? control.theme.sp8 : 8)
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
            if (!control.theme) return _inferDark ? "#8C9198" : "#e2e8f0"
            if (control.activeFocus) return control.theme.borderFocus
            if (control.hovered) return control.theme.outline
            return control.theme.border
        }
        border.width: 1
        radius: control.theme ? control.theme.radiusMd : 12
    }

    delegate: ItemDelegate {
        width: control.width
        contentItem: AppText {
            text: modelData
            color: control.highlightedIndex === index ? control.highlightedTextColor : control.normalTextColor
            font.pixelSize: control.theme ? control.theme.fontMd : 13
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
        padding: control.theme ? control.theme.sp4 : 4

        contentItem: ListView {
            clip: true
            implicitHeight: contentHeight
            model: control.delegateModel
            currentIndex: control.highlightedIndex
            ScrollIndicator.vertical: ScrollIndicator {}
        }

        background: Rectangle {
            color: control.theme ? control.theme.surface : (_inferDark ? "#1A1D23" : "#FFFFFF")
            border.color: control.theme ? control.theme.border : (_inferDark ? "#2A2E36" : "#e2e8f0")
            border.width: 1
            radius: control.theme ? control.theme.radiusLg : 16
        }
    }
}
