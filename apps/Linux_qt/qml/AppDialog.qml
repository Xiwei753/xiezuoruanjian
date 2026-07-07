// =============================================================================
// AppDialog.qml — 通用 Material 3 对话框
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
// 职责：统一弹窗背景、圆角、边框、内边距与标题样式
// 约束：纯 UI 容器，业务动作由调用方信号/按钮处理
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: root
    property var dt: null

    // Elevation shadow support
    property int elevation: 3
    property var appShadow: null

    readonly property color _surfaceContainerHigh: dt.surfaceContainerHigh
    readonly property color _border: dt.border
    readonly property color _onSurface: dt.onSurface
    readonly property int _dialogRadius: dt.dialogRadius
    readonly property int _sp16: dt.sp16
    readonly property int _sp24: dt.sp24
    readonly property int _sp48: dt.sp48
    readonly property int _subtitle: dt.subtitle
    readonly property string _fontFamily: dt.fontFamily

    property string heading: title
    default property alias bodyData: bodyColumn.data

    modal: true
    padding: 0
    background: Item {
        // Shadow layer (behind the dialog background)
        Rectangle {
            anchors.fill: parent
            anchors.topMargin: root.elevation > 0 && root.appShadow ? root.appShadow.forElevation(root.elevation).verticalOffset : 0
            radius: _dialogRadius
            color: root.elevation > 0 && root.appShadow ? root.appShadow.forElevation(root.elevation).color : "transparent"
            opacity: 0.2
            visible: root.elevation > 0 && root.appShadow !== null
            z: -1
        }

        Rectangle {
            anchors.fill: parent
            color: _surfaceContainerHigh
            border.color: _border
            border.width: 1
            radius: _dialogRadius
        }
    }
    header: null

    contentItem: Item {
        implicitWidth: bodyColumn.implicitWidth + _sp48
        implicitHeight: bodyColumn.implicitHeight + _sp48

        ColumnLayout {
            id: bodyColumn
            anchors.fill: parent
            anchors.margins: _sp24
            spacing: _sp16

            AppText {
                dt: root.dt
                Layout.fillWidth: true
                text: root.heading
                visible: text.length > 0
                color: _onSurface
                font.pixelSize: _subtitle
                font.family: _fontFamily
                font.weight: Font.DemiBold
            }
        }
    }
}
