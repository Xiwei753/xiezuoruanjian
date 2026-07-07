// =============================================================================
// SettingCard.qml — 设置卡片组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
// 职责：设置页面的卡片容器，带标题和圆角背景
// 约束：
//   - 纯 UI 组件，内容通过 default property 传入
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root
    property var dt: null

    // Elevation shadow support
    property int elevation: 0
    property var appShadow: null

    readonly property color _surfaceContainerLow: dt.surfaceContainerLow
    readonly property color _border: dt.border
    readonly property color _accent: dt.accent
    readonly property int _cardRadius: dt.cardRadius
    readonly property int _sp16: dt.sp16
    readonly property int _sp20: dt.sp20
    readonly property int _sp32: dt.sp32
    readonly property int _fontMd: dt.fontMd

    property string title: ""
    default property alias contentData: contentColumn.data
    radius: _cardRadius
    color: _surfaceContainerLow
    border.color: _border
    border.width: 1

    implicitHeight: contentColumn.implicitHeight + _sp32

    ColumnLayout {
        id: contentColumn
        anchors.fill: parent
        anchors.margins: _sp20
        spacing: _sp16

        AppText {
            dt: root.dt
            text: root.title
            color: _accent
            font.pixelSize: _fontMd
            font.weight: Font.Bold
        }
    }
}
