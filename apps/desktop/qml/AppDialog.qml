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
    property string heading: title
    default property alias bodyData: bodyColumn.data

    modal: true
    padding: 0
    background: Rectangle {
        color: dt.surfaceContainerHigh
        border.color: dt.border
        border.width: 1
        radius: dt ? dt.dialogRadius : dt.radiusXl
    }
    header: null

    contentItem: Item {
        implicitWidth: bodyColumn.implicitWidth + (dt ? dt.sp48 : 48)
        implicitHeight: bodyColumn.implicitHeight + (dt ? dt.sp48 : 48)

        ColumnLayout {
            id: bodyColumn
            anchors.fill: parent
            anchors.margins: dt ? dt.sp24 : 24
            spacing: dt ? dt.sp16 : 16

            AppText {
                Layout.fillWidth: true
                text: root.heading
                visible: text.length > 0
                color: dt.onSurface
                font.pixelSize: dt ? dt.subtitle : 18
                font.family: dt ? dt.fontFamily : "sans-serif"
                font.weight: Font.DemiBold
            }
        }
    }
}
