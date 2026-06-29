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

    // Safe access: fallback to light-theme defaults when dt is null
    FallbackTokens { id: _fallback }
    readonly property var _dt: dt ?? _fallback

    property string title: ""
    default property alias contentData: contentColumn.data
    radius: _dt.cardRadius
    color: _dt.surfaceContainerLow
    border.color: _dt.border
    border.width: 1

    implicitHeight: contentColumn.implicitHeight + _dt.sp32

    ColumnLayout {
        id: contentColumn
        anchors.fill: parent
        anchors.margins: _dt.sp20
        spacing: _dt.sp16

        AppText {
            text: root.title
            color: _dt.accent
            font.pixelSize: _dt.fontMd
            font.weight: Font.Bold
        }
    }
}
