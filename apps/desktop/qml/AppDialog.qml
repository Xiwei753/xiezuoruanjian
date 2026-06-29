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
    FallbackTokens { id: _fallback }
    readonly property var _dt: dt ?? _fallback

    property string heading: title
    default property alias bodyData: bodyColumn.data

    modal: true
    padding: 0
    background: Rectangle {
        color: _dt.surfaceContainerHigh
        border.color: _dt.border
        border.width: 1
        radius: _dt.dialogRadius
    }
    header: null

    contentItem: Item {
        implicitWidth: bodyColumn.implicitWidth + _dt.sp48
        implicitHeight: bodyColumn.implicitHeight + _dt.sp48

        ColumnLayout {
            id: bodyColumn
            anchors.fill: parent
            anchors.margins: _dt.sp24
            spacing: _dt.sp16

            AppText {
                Layout.fillWidth: true
                text: root.heading
                visible: text.length > 0
                color: _dt.onSurface
                font.pixelSize: _dt.subtitle
                font.family: _dt.fontFamily
                font.weight: Font.DemiBold
            }
        }
    }
}
