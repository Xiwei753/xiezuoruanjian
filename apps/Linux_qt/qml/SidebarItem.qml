// =============================================================================
// SidebarItem.qml — 侧栏导航项组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
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

    readonly property color _primaryContainer: dt.primaryContainer
    readonly property color _onPrimaryContainer: dt.onPrimaryContainer
    readonly property color _onSurfaceVariant: dt.onSurfaceVariant
    readonly property color _surfaceVariant: dt.surfaceVariant
    readonly property int _sp8: dt.sp8
    readonly property int _sp12: dt.sp12
    readonly property int _radiusPill: dt.radiusPill
    readonly property int _fontMd: dt.fontMd
    readonly property int _label: dt.label
    readonly property string _fontFamily: dt.fontFamily

    property string text: ""
    property string icon: ""
    property bool active: false
    property bool compact: false
    signal clicked()

    height: 44
    implicitWidth: 160

    Rectangle {
        anchors.fill: parent
        anchors.leftMargin: _sp8
        anchors.rightMargin: _sp8
        radius: _radiusPill
        color: {
            if (control.active) return _primaryContainer
            return ma.containsMouse ? _surfaceVariant : "transparent"
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: _sp12
        anchors.rightMargin: _sp8
        spacing: _sp8

        AppText {
            text: control.icon
            dt: root.dt
            font.pixelSize: _fontMd
            Layout.preferredWidth: 20
            horizontalAlignment: Text.AlignHCenter
            visible: !control.compact
        }

        AppText {
            dt: control.dt
            text: control.text
            color: {
                if (control.active) return _onPrimaryContainer
                return _onSurfaceVariant
            }
            font.pixelSize: _label
            font.family: _fontFamily
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
