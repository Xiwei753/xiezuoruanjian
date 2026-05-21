import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: root
    title: "设置"
    modal: true
    width: 500
    height: 400
    anchors.centerIn: Overlay.overlay
    standardButtons: Dialog.Close

    property var theme: null

    background: Rectangle {
        color: theme ? theme.bgDark : "#1E1E1E"
        border.color: theme ? theme.border : "#333333"
        radius: 8
    }

    ScrollView {
        anchors.fill: parent
        anchors.margins: 16
        contentWidth: width

        Column {
            width: parent.width
            spacing: 16

            Text {
                text: "外观"
                color: theme ? theme.accent : "#82AAFF"
                font.pixelSize: 16
                font.bold: true
            }

            Row {
                spacing: 16
                width: parent.width
                Text { text: "主题模式"; color: theme ? theme.textMain : "#E0E0E0"; font.pixelSize: 14; anchors.verticalCenter: parent.verticalCenter; width: 100 }
                ComboBox {
                    id: themeCombo
                    width: 150
                    model: ["system", "light", "dark"]
                    currentIndex: {
                        if (backend.setting_theme_mode === "light") return 1;
                        if (backend.setting_theme_mode === "dark") return 2;
                        return 0;
                    }
                    onActivated: {
                        backend.setting_theme_mode = currentText;
                        backend.save_local_settings();
                    }
                }
            }

            Text {
                text: "编辑器"
                color: theme ? theme.accent : "#82AAFF"
                font.pixelSize: 16
                font.bold: true
            }

            Row {
                spacing: 16
                width: parent.width
                Text { text: "字号"; color: theme ? theme.textMain : "#E0E0E0"; font.pixelSize: 14; anchors.verticalCenter: parent.verticalCenter; width: 100 }
                SpinBox {
                    id: fontSpin
                    value: backend.setting_font_size
                    from: 10
                    to: 40
                    onValueChanged: {
                        backend.setting_font_size = value;
                        backend.save_local_settings();
                    }
                }
            }

            Row {
                spacing: 16
                width: parent.width
                Text { text: "自动保存"; color: theme ? theme.textMain : "#E0E0E0"; font.pixelSize: 14; anchors.verticalCenter: parent.verticalCenter; width: 100 }
                Switch {
                    id: autoSaveSwitch
                    checked: backend.setting_auto_save_enabled
                    onCheckedChanged: {
                        backend.setting_auto_save_enabled = checked;
                        backend.save_local_settings();
                    }
                }
            }
        }
    }
}
