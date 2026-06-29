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
    property string text: ""
    property string icon: ""
    property bool active: false
    property bool compact: false
    signal clicked()

    height: 44
    implicitWidth: 160

    Rectangle {
        anchors.fill: parent
        anchors.leftMargin: dt ? dt.sp8 : 8
        anchors.rightMargin: dt ? dt.sp8 : 8
        radius: dt ? dt.radiusPill : 999
        color: {
            if (control.active) return dt.primaryContainer
            return ma.containsMouse ? dt.surfaceVariant : "transparent"
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: dt ? dt.sp12 : 12
        anchors.rightMargin: dt ? dt.sp8 : 8
        spacing: dt ? dt.sp8 : 8

        AppText {
            text: control.icon
            font.pixelSize: dt ? dt.fontMd : 13
            Layout.preferredWidth: 20
            horizontalAlignment: Text.AlignHCenter
            visible: !control.compact
        }

        AppText {
            text: control.text
            color: {
                if (control.active) return dt.onPrimaryContainer
                return dt.onSurfaceVariant
            }
            font.pixelSize: dt ? dt.label : 13
            font.family: dt ? dt.fontFamily : "sans-serif"
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
