// =============================================================================
// AppDialog.qml — 通用 Material 3 对话框
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：统一弹窗背景、圆角、边框、内边距与标题样式
// 约束：纯 UI 容器，业务动作由调用方信号/按钮处理
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: root
    property var dt: null

    // Safe access: fallback to light-theme defaults when dt is null
    readonly property color _surfaceContainerHigh: dt ? dt.surfaceContainerHigh : "#EAEFF5"
    readonly property color _border: dt ? dt.border : "#71788057"
    readonly property color _onSurface: dt ? dt.onSurface : "#1A1C1E"
    readonly property int _dialogRadius: dt ? dt.dialogRadius : 24
    readonly property int _sp16: dt ? dt.sp16 : 16
    readonly property int _sp24: dt ? dt.sp24 : 24
    readonly property int _sp48: dt ? dt.sp48 : 48
    readonly property int _subtitle: dt ? dt.subtitle : 18
    readonly property string _fontFamily: dt ? dt.fontFamily : "sans-serif"

    property string heading: title
    default property alias bodyData: bodyColumn.data

    modal: true
    padding: 0
    background: Rectangle {
        color: _surfaceContainerHigh
        border.color: _border
        border.width: 1
        radius: _dialogRadius
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
