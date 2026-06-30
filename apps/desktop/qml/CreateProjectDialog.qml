// =============================================================================
// CreateProjectDialog.qml — 新建作品对话框
// =============================================================================
//
// 层级：Desktop UI 层（QML UI 组件）
// 职责：新建作品的输入对话框，收集作品标题
// 约束：
//   - 纯 UI 组件，创建操作通过 signal 传递给 main.qml
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: root
    title: qsTr("新建作品")
    modal: true
    x: (parent ? (parent.width - width) / 2 : 0)
    y: (parent ? (parent.height - height) / 2 : 0)
    standardButtons: Dialog.NoButton

    property var theme: null
    signal submitProject(string title)

    width: 400
    height: 220

    // ── SystemPalette 推断：theme 为空时从系统调色板推断深浅色 ──
    SystemPalette { id: _sysPalette; colorGroup: SystemPalette.Active }
    readonly property bool _inferDark: {
        var wL = _sysPalette.window.r * 0.2126 + _sysPalette.window.g * 0.7152 + _sysPalette.window.b * 0.0722;
        var tL = _sysPalette.windowText.r * 0.2126 + _sysPalette.windowText.g * 0.7152 + _sysPalette.windowText.b * 0.0722;
        return tL > wL;
    }

    background: Rectangle {
        color: theme ? theme.surface : (_inferDark ? "#1A1D23" : "#FCFCFF")
        border.color: theme ? theme.border : (_inferDark ? "#2A2E36" : "#CBD5E1")
        border.width: 1
        radius: theme ? theme.radiusXl : 28
    }
    header: null

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: theme ? theme.sp24 : 24
        spacing: theme ? theme.sp16 : 16

        AppText {
            text: qsTr("新建作品")
            color: theme ? theme.textPrimary : (_inferDark ? "#E2E2E5" : "#1A1C1E")
            font.pixelSize: theme ? theme.subtitle : 18
            font.family: theme ? theme.fontFamily : "sans-serif"
            font.weight: Font.DemiBold
        }

        AppText {
            text: qsTr("请输入作品名称：")
            color: theme ? theme.onSurfaceVariant : (_inferDark ? "#C3C6CF" : "#42474E")
            font.pixelSize: theme ? theme.body : 14
            font.family: theme ? theme.fontFamily : "sans-serif"
        }

        AppTextField {
            id: titleField
            Layout.fillWidth: true
            theme: root.theme
            placeholderText: qsTr("作品名称")
            onAccepted: {
                if (text.trim() !== "") {
                    root.accept();
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Item { Layout.fillWidth: true }
            AppButton { text: qsTr("取消"); dt: root.theme; variant: "text"; onClicked: root.reject() }
            AppButton { text: qsTr("创建"); dt: root.theme; variant: "primary"; onClicked: { if (titleField.text.trim() !== "") root.accept() } }
        }
    }

    onOpened: {
        titleField.text = "";
        titleField.forceActiveFocus();
    }

    onAccepted: {
        root.submitProject(titleField.text.trim());
    }
}
