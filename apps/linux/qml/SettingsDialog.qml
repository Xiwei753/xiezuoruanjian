import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

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

    function updateValues() {
        if (!root.backendRef) return;
        fontSpin.value = root.backendRef.setting_font_size;
        autoSaveSwitch.checked = root.backendRef.setting_auto_save_enabled;
        
        var mode = root.backendRef.setting_theme_mode;
        if (mode === "light") themeCombo.currentIndex = 1;
        else if (mode === "dark") themeCombo.currentIndex = 2;
        else themeCombo.currentIndex = 0;
    }

    onOpened: {
        updateValues();
    }

    onBackendRefChanged: {
        updateValues();
    }

    Connections {
        target: root.backendRef
        function onSettings_changed() {
            root.updateValues();
        }
    }

    ScrollView {
        id: settingsScroll
        anchors.fill: parent
        anchors.margins: 16
        clip: true

        Column {
            width: settingsScroll.availableWidth
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
                    currentIndex: 0
                    onActivated: {
                        if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                            window.debugLog("settings", "theme_changed", "mode=" + currentText);
                        }
                        if (!root.backendRef) return;
                        root.backendRef.setting_theme_mode = currentText;
                        var success = root.backendRef.save_local_settings();
                        if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                            window.debugLog("settings", "save_theme_result", "success=" + success);
                        }
                        if (success) root.settingsChanged();
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
                    value: 16
                    from: 10
                    to: 40
                    onValueModified: {
                        if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                            window.debugLog("settings", "font_size_changed", "size=" + value);
                        }
                        if (!root.backendRef) return;
                        root.backendRef.setting_font_size = value;
                        var success = root.backendRef.save_local_settings();
                        if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                            window.debugLog("settings", "save_font_result", "success=" + success);
                        }
                        if (success) root.settingsChanged();
                    }
                }
            }

            Row {
                spacing: 16
                width: parent.width
                Text { text: "自动保存"; color: theme ? theme.textMain : "#E0E0E0"; font.pixelSize: 14; anchors.verticalCenter: parent.verticalCenter; width: 100 }
                Switch {
                    id: autoSaveSwitch
                    checked: false
                    onToggled: {
                        if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                            window.debugLog("settings", "autosave_changed", "enabled=" + checked);
                        }
                        if (!root.backendRef) return;
                        root.backendRef.setting_auto_save_enabled = checked;
                        var success = root.backendRef.save_local_settings();
                        if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                            window.debugLog("settings", "save_autosave_result", "success=" + success);
                        }
                        if (success) root.settingsChanged();
                    }
                }
            }
        }
    }
}
