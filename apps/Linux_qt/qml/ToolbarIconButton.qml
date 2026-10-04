// =============================================================================
// ToolbarIconButton.qml — 工作台工具条带里的圆形图标按钮
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：ToolbarLeading 组的返回 / 撤销 / 重做入口的统一外观
// 约束（Issue #825）：
//   - 只发 triggered 信号，不直接改 backend；
//   - 使用 DesignTokens 统一样式，禁止硬编码颜色。
// =============================================================================

import QtQuick

Rectangle {
    id: root

    required property var dt
    property string glyph: ""
    property color glyphColor: dt.textSecondary
    property int iconSize: 28

    signal triggered()

    width: iconSize
    height: iconSize
    radius: dt.radiusPill
    color: hoverArea.containsMouse ? dt.surfaceVariant : "transparent"

    AppText {
        dt: root.dt
        anchors.centerIn: parent
        text: root.glyph
        color: root.glyphColor
        font.pointSize: root.dt.fontLgPt
        font.family: root.dt.fontFamily
    }

    MouseArea {
        id: hoverArea
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.triggered()
    }
}