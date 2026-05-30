// =============================================================================
// AppDialog.qml — 通用 Material 3 对话框
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
// 职责：统一弹窗背景、圆角、边框、内边距与标题样式
// 约束：纯 UI 容器，业务动作由调用方信号/按钮处理
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: root
    property var theme: null
    property string heading: title
    default property alias bodyData: bodyColumn.data

    modal: true
    padding: 0
    background: Rectangle {
        color: root.theme ? root.theme.surface : "#FCFCFF"
        border.color: root.theme ? root.theme.border : "#CBD5E1"
        border.width: 1
        radius: root.theme ? root.theme.radiusXl : 24
    }
    header: null

    contentItem: Item {
        implicitWidth: bodyColumn.implicitWidth + (root.theme ? root.theme.sp48 : 48)
        implicitHeight: bodyColumn.implicitHeight + (root.theme ? root.theme.sp48 : 48)

        ColumnLayout {
            id: bodyColumn
            anchors.fill: parent
            anchors.margins: root.theme ? root.theme.sp24 : 24
            spacing: root.theme ? root.theme.sp16 : 16

            Text {
                Layout.fillWidth: true
                text: root.heading
                visible: text.length > 0
                color: root.theme ? root.theme.onSurface : "#E2E2E5"
                font.pixelSize: root.theme ? root.theme.subtitle : 18
                font.family: root.theme ? root.theme.fontFamily : "sans-serif"
                font.weight: Font.DemiBold
            }
        }
    }
}
