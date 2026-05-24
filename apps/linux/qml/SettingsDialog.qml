import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Dialog {
    id: root
    modal: true
    width: 560
    height: 560
    anchors.centerIn: Overlay.overlay
    property var theme: null
    property var backendRef: null
    property var dt: theme
    signal settingsChanged()

    background: Rectangle { color: dt ? dt.surface : "#1A1D23"; border.color: dt ? dt.border : "#2A2E36"; border.width: 1; radius: dt ? dt.radiusMd : 12 }

    function updateValues() {
        if (!backendRef) return;
        fontValue.text = String(Math.round(backendRef.setting_font_size || 16));
        lineSpacing.currentIndex = Math.abs((backendRef.setting_line_spacing || 1.5) - 1.25) < 0.01 ? 0 : Math.abs((backendRef.setting_line_spacing || 1.5) - 1.5) < 0.01 ? 1 : Math.abs((backendRef.setting_line_spacing || 1.5) - 1.75) < 0.01 ? 2 : 3;
        autoSave.checked = backendRef.setting_auto_save_enabled;
        anim.checked = backendRef.setting_typing_animation_enabled;
        cursor.checked = backendRef.setting_smooth_cursor_enabled;
        aiSwitch.checked = backendRef.ai_enabled;
        var mode = backendRef.setting_theme_mode;
        themeCombo.currentIndex = mode === "light" ? 1 : (mode === "dark" ? 2 : 0);
    }
    onOpened: updateValues()

    header: Rectangle {
        height: 54
        color: "transparent"
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: dt ? dt.sp24 : 24
            anchors.rightMargin: dt ? dt.sp16 : 16
            Text { text: "设置"; color: dt ? dt.textPrimary : "#E2E4E9"; font.pixelSize: dt ? dt.fontLg : 16; font.weight: Font.Bold; Layout.fillWidth: true }
            Rectangle { width: 40; height: 40; radius: dt ? dt.actionButtonRadius : 12; color: closeHover.containsMouse ? (dt ? dt.cardHover : "#22262E") : "transparent"
                Text { anchors.centerIn: parent; text: "X"; color: dt ? dt.textSecondary : "#9CA0AB" }
                MouseArea { id: closeHover; anchors.fill: parent; hoverEnabled: true; onClicked: root.close() }
            }
        }
    }

    ScrollView {
        anchors.fill: parent
        anchors.margins: dt ? dt.sp20 : 20
        clip: true
        contentWidth: availableWidth

        ColumnLayout {
            width: parent.width
            spacing: dt ? dt.cardGap : 16

            SettingCard {
                dt: root.dt
                title: "外观"
                Layout.fillWidth: true
                RowLayout {
                    Layout.fillWidth: true
                    Text { text: "主题模式"; color: dt ? dt.textPrimary : "#E2E4E9"; Layout.fillWidth: true }
                    ComboBox {
                        id: themeCombo
                        width: 150
                        model: ["跟随系统", "浅色", "深色"]
                        onActivated: {
                            if (!backendRef) return;
                            var modes = ["system", "light", "dark"];
                            backendRef.setting_theme_mode = modes[currentIndex];
                            backendRef.save_local_settings();
                            root.settingsChanged();
                        }
                        background: Rectangle { radius: dt ? dt.radiusSm : 8; color: dt ? dt.surfaceVariant : "#242933"; border.width: 1; border.color: dt ? dt.controlBorder : "#3A3F49" }
                    }
                }
            }

            SettingCard {
                dt: root.dt
                title: "写作体验"
                Layout.fillWidth: true
                RowLayout {
                    Layout.fillWidth: true
                    Text { text: "字号"; color: dt ? dt.textPrimary : "#E2E4E9"; Layout.fillWidth: true }
                    Rectangle { width: 32; height: 32; radius: 8; color: dt ? dt.surfaceVariant : "#242933"; border.width: 1; border.color: dt ? dt.controlBorder : "#3A3F49"
                        Text { anchors.centerIn: parent; text: "-"; color: dt ? dt.textPrimary : "#E2E4E9" }
                        MouseArea { anchors.fill: parent; onClicked: { var v = Math.max(10, Number(fontValue.text) - 1); fontValue.text = String(v); backendRef.setting_font_size = v; backendRef.save_local_settings(); root.settingsChanged(); } }
                    }
                    Text { id: fontValue; width: 34; horizontalAlignment: Text.AlignHCenter; color: dt ? dt.textPrimary : "#E2E4E9" }
                    Rectangle { width: 32; height: 32; radius: 8; color: dt ? dt.surfaceVariant : "#242933"; border.width: 1; border.color: dt ? dt.controlBorder : "#3A3F49"
                        Text { anchors.centerIn: parent; text: "+"; color: dt ? dt.textPrimary : "#E2E4E9" }
                        MouseArea { anchors.fill: parent; onClicked: { var v = Math.min(40, Number(fontValue.text) + 1); fontValue.text = String(v); backendRef.setting_font_size = v; backendRef.save_local_settings(); root.settingsChanged(); } }
                    }
                }
                RowLayout {
                    Layout.fillWidth: true
                    Text { text: "行距"; color: dt ? dt.textPrimary : "#E2E4E9"; Layout.fillWidth: true }
                    ComboBox {
                        id: lineSpacing
                        width: 140
                        model: ["1.25 倍", "1.5 倍", "1.75 倍", "2.0 倍"]
                        onActivated: { var vals = [1.25, 1.5, 1.75, 2.0]; backendRef.setting_line_spacing = vals[currentIndex]; backendRef.save_local_settings(); root.settingsChanged(); }
                        background: Rectangle { radius: dt ? dt.radiusSm : 8; color: dt ? dt.surfaceVariant : "#242933"; border.width: 1; border.color: dt ? dt.controlBorder : "#3A3F49" }
                    }
                }
                RowLayout { Layout.fillWidth: true; Text { text: "自动保存"; color: dt ? dt.textPrimary : "#E2E4E9"; Layout.fillWidth: true }
                    ModernSwitch { id: autoSave; dt: root.dt; onToggled: function(v) { backendRef.setting_auto_save_enabled = v; backendRef.save_local_settings(); root.settingsChanged(); } }
                }
                RowLayout { Layout.fillWidth: true; Text { text: "打字动画"; color: dt ? dt.textPrimary : "#E2E4E9"; Layout.fillWidth: true }
                    ModernSwitch { id: anim; dt: root.dt; onToggled: function(v) { backendRef.setting_typing_animation_enabled = v; backendRef.save_local_settings(); root.settingsChanged(); } }
                }
                RowLayout { Layout.fillWidth: true; Text { text: "平滑光标"; color: dt ? dt.textPrimary : "#E2E4E9"; Layout.fillWidth: true }
                    ModernSwitch { id: cursor; dt: root.dt; onToggled: function(v) { backendRef.setting_smooth_cursor_enabled = v; backendRef.save_local_settings(); root.settingsChanged(); } }
                }
            }

            SettingCard {
                dt: root.dt
                title: "AI"
                Layout.fillWidth: true
                visible: root.backendRef ? root.backendRef.ai_available : false
                RowLayout { Layout.fillWidth: true; Text { text: "启用 AI 功能"; color: dt ? dt.textPrimary : "#E2E4E9"; Layout.fillWidth: true }
                    ModernSwitch { id: aiSwitch; dt: root.dt; onToggled: function(v) { backendRef.ai_enabled = v; backendRef.save_local_settings(); root.settingsChanged(); } }
                }
            }
        }
    }
}
