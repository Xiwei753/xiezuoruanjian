// =============================================================================
// ToolbarButton.qml — 工具栏按钮组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：工具栏的扁平按钮，支持主题适配
// 约束：
//   - 纯 UI 组件，点击通过 onClicked 处理
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls

Button {
    id: control
    flat: true
    property var dt: null

    readonly property color _primary: dt.primary
    readonly property color _onPrimaryContainer: dt.onPrimaryContainer
    readonly property color _onSurfaceVariant: dt.onSurfaceVariant
    readonly property color _primaryContainer: dt.primaryContainer
    readonly property color _surfaceVariant: dt.surfaceVariant
    readonly property int _radiusPill: dt.radiusPill
    readonly property int _fontSm: dt.fontSm
    readonly property int _label: dt.label
    readonly property string _fontFamily: dt.fontFamily

    property bool active: false

    implicitHeight: 36
    implicitWidth: Math.max(tm.width + 24, 52)

    TextMetrics { id: tm; text: control.text; font.pixelSize: _fontSm }

    contentItem: AppText {
        dt: control.dt
        text: control.text
        color: control.active ? _onPrimaryContainer : (control.hovered ? _primary : _onSurfaceVariant)
        font.pixelSize: _label
        font.family: _fontFamily
        font.weight: control.active ? Font.Medium : Font.Normal
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    background: Rectangle {
        color: control.active ? _primaryContainer : (control.hovered ? _surfaceVariant : "transparent")
        radius: _radiusPill
    }
}
