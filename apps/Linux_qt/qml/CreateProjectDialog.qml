// =============================================================================
// CreateProjectDialog.qml — 新建作品对话框
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：新建作品的输入对话框，收集作品标题
// 约束：
//   - 纯 UI 组件，创建操作通过 signal 传递给 main.qml
// Issue #900：迁移到 Breeze Controls（Controls.Dialog / Controls.Label /
// Controls.TextField / Controls.Button），不再自绘背景与 M3 字号。
// 公共 API（theme 属性 / submitProject 信号 / accept / reject）保持不变。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Dialog {
    id: root
    title: qsTr("新建作品")
    modal: true
    x: (parent ? (parent.width - width) / 2 : 0)
    y: (parent ? (parent.height - height) / 2 : 0)
    standardButtons: Dialog.NoButton
    header: null

    property var theme: null
    signal submitProject(string title)

    width: 400
    height: 220

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.largeSpacing

        Kirigami.Heading {
            level: 2
            text: qsTr("新建作品")
        }

        Label {
            text: qsTr("请输入作品名称：")
            color: Kirigami.Theme.disabledTextColor
        }

        TextField {
            id: titleField
            Layout.fillWidth: true
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
            Button { text: qsTr("取消"); flat: true; onClicked: root.reject() }
            Button { text: qsTr("创建"); highlighted: true; onClicked: { if (titleField.text.trim() !== "") root.accept() } }
        }
    }

    onOpened: {
        titleField.text = "";
    }

    onAccepted: {
        root.submitProject(titleField.text.trim());
    }
}
