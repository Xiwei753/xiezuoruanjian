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
    property var backendRef: null
    signal settingsChanged()

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
                        if ((root.backendRef ? root.backendRef.setting_theme_mode : 'system') === "light") return 1;
                        if ((root.backendRef ? root.backendRef.setting_theme_mode : 'system') === "dark") return 2;
                        return 0;
                    }
                    onActivated: {
                        if (!root.backendRef) return;
                        root.backendRef.setting_theme_mode = currentText;
                        if (root.backendRef.save_local_settings()) root.settingsChanged();
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
                    value: (root.backendRef ? root.backendRef.setting_font_size : 16)
                    from: 10
                    to: 40
                    onValueChanged: {
                        if (!root.backendRef) return;
                        root.backendRef.setting_font_size = value;
                        if (root.backendRef.save_local_settings()) root.settingsChanged();
                    }
                }
            }

            Row {
                spacing: 16
                width: parent.width
                Text { text: "自动保存"; color: theme ? theme.textMain : "#E0E0E0"; font.pixelSize: 14; anchors.verticalCenter: parent.verticalCenter; width: 100 }
                Switch {
                    id: autoSaveSwitch
                    checked: (root.backendRef ? root.backendRef.setting_auto_save_enabled : false)
                    onCheckedChanged: {
                        if (!root.backendRef) return;
                        root.backendRef.setting_auto_save_enabled = checked;
                        if (root.backendRef.save_local_settings()) root.settingsChanged();
                    }
                }
            }
        }
    }
}
