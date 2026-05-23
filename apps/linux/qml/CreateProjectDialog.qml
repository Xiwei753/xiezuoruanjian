import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Dialog {
    id: root
    title: "新建作品"
    modal: true
    anchors.centerIn: Overlay.overlay
    standardButtons: Dialog.Ok | Dialog.Cancel

    property var theme: null
    signal submitProject(string title)

    width: 400

    background: Rectangle {
        color: theme ? theme.bgDark : "#1E1E1E"
        border.color: theme ? theme.border : "#333333"
        radius: 8
    }

    ColumnLayout {
        width: parent.width
        spacing: 16

        Text {
            text: "请输入作品名称："
            color: theme ? theme.textMain : "#E0E0E0"
            font.pixelSize: 14
        }

        TextField {
            id: titleField
            Layout.fillWidth: true
            placeholderText: "作品名称"
            color: theme ? theme.textMain : "#E0E0E0"
            background: Rectangle {
                color: theme ? theme.inputBg : "#2A2A2A"
                border.color: titleField.activeFocus ? (theme ? theme.accent : "#82AAFF") : (theme ? theme.border : "#444444")
                radius: 4
            }
            onAccepted: {
                if (text.trim() !== "") {
                    root.accept();
                }
            }
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
