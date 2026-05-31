// =============================================================================
// SidebarItem.qml — 侧栏导航项组件
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
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
    property var theme: null
    property string text: ""
    property string icon: ""
    property bool active: false
    property bool compact: false
    signal clicked()

    height: 44
    implicitWidth: 160

    Rectangle {
        anchors.fill: parent
        anchors.leftMargin: control.theme ? control.theme.sp8 : 8
        anchors.rightMargin: control.theme ? control.theme.sp8 : 8
        radius: control.theme ? control.theme.radiusPill : 999
        color: {
            if (control.active) return control.theme ? control.theme.primaryContainer : "#CCE5FF"
            return ma.containsMouse ? (control.theme ? control.theme.surfaceVariant : "#DFE3EB") : "transparent"
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: control.theme ? control.theme.sp12 : 12
        anchors.rightMargin: control.theme ? control.theme.sp8 : 8
        spacing: control.theme ? control.theme.sp8 : 8

        AppText {
            text: control.icon
            font.pixelSize: control.theme ? control.theme.fontMd : 13
            Layout.preferredWidth: 20
            horizontalAlignment: Text.AlignHCenter
            visible: !control.compact
        }

        AppText {
            text: control.text
            color: {
                if (control.active) return control.theme ? control.theme.onPrimaryContainer : "#001E31"
                return control.theme ? control.theme.onSurfaceVariant : "#42474E"
            }
            font.pixelSize: control.theme ? control.theme.label : 13
            font.family: control.theme ? control.theme.fontFamily : "sans-serif"
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
