// =============================================================================
// SidebarItem.qml — 侧栏导航项组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：侧栏的单个导航项，支持图标、文字和选中状态
// 约束：
//   - 纯 UI 组件，点击通过 onClicked 处理
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: control
    property var dt: null

    // Safe access: fallback to light-theme defaults when dt is null
    FallbackTokens { id: _fallback }
    readonly property var _dt: dt ?? _fallback

    property string text: ""
    property string icon: ""
    property bool active: false
    property bool compact: false
    signal clicked()

    height: 44
    implicitWidth: 160

    Rectangle {
        anchors.fill: parent
        anchors.leftMargin: _dt.sp8
        anchors.rightMargin: _dt.sp8
        radius: _dt.radiusPill
        color: {
            if (control.active) return _dt.primaryContainer
            return ma.containsMouse ? _dt.surfaceVariant : "transparent"
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: _dt.sp12
        anchors.rightMargin: _dt.sp8
        spacing: _dt.sp8

        AppText {
            text: control.icon
            font.pixelSize: _dt.fontMd
            Layout.preferredWidth: 20
            horizontalAlignment: Text.AlignHCenter
            visible: !control.compact
        }

        AppText {
            text: control.text
            color: {
                if (control.active) return _dt.onPrimaryContainer
                return _dt.onSurfaceVariant
            }
            font.pixelSize: _dt.label
            font.family: _dt.fontFamily
            font.weight: control.active ? Font.Medium : Font.Normal
            Layout.fillWidth: true
            elide: Text.ElideRight
            visible: !control.compact
        }
    }

    MouseArea {
        id: ma
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: control.clicked()
    }
}
