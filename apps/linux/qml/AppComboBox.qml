// =============================================================================
// AppComboBox.qml — 通用下拉选择框组件
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
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

    contentItem: Text {
        text: control.displayText
        color: control.enabled
            ? (control.theme ? control.theme.textPrimary : "#0f172a")
            : (control.theme ? control.theme.textDisabled : "#94a3b8")
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
            context.fillStyle = control.enabled
                ? (control.theme ? control.theme.textSecondary : "#475569")
                : (control.theme ? control.theme.textDisabled : "#94a3b8")
            context.fill()
        }
    }

    background: Rectangle {
        color: control.theme ? control.theme.inputBg : "#f1f5f9"
        border.color: {
            if (!control.theme) return "#e2e8f0"
            if (control.activeFocus) return control.theme.borderFocus
            if (control.hovered) return control.theme.outline
            return control.theme.border
        }
        border.width: 1
        radius: control.theme ? control.theme.radiusMd : 12
    }

    delegate: ItemDelegate {
        width: control.width
        contentItem: Text {
            text: modelData
            color: control.theme ? control.theme.textPrimary : "#0f172a"
            font.pixelSize: control.theme ? control.theme.fontMd : 13
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            color: control.highlightedIndex === index
                ? (control.theme ? control.theme.primaryContainer : "#dbeafe")
                : "transparent"
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
            color: control.theme ? control.theme.surface : "#ffffff"
            border.color: control.theme ? control.theme.border : "#e2e8f0"
            border.width: 1
            radius: control.theme ? control.theme.radiusLg : 16
        }
    }
}
