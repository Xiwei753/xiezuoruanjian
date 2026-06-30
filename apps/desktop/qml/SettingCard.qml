// =============================================================================
// SettingCard.qml — 设置卡片组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
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

    // Safe access: fallback to light-theme defaults when dt is null
    readonly property color _surfaceContainerLow: dt ? dt.surfaceContainerLow : "#F6F8FC"
    readonly property color _border: dt ? dt.border : "#71788057"
    readonly property color _accent: dt ? dt.accent : "#006497"
    readonly property int _cardRadius: dt ? dt.cardRadius : 16
    readonly property int _sp16: dt ? dt.sp16 : 16
    readonly property int _sp20: dt ? dt.sp20 : 20
    readonly property int _sp32: dt ? dt.sp32 : 32
    readonly property int _fontMd: dt ? dt.fontMd : 14

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
            text: root.title
            color: _accent
            font.pixelSize: _fontMd
            font.weight: Font.Bold
        }
    }
}
