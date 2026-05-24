import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Dialog {
    id: root
    title: ""
    modal: true
    width: 540
    height: 520
    anchors.centerIn: Overlay.overlay

    property var theme: null
    property var backendRef: null
    signal settingsChanged()

    background: Rectangle {
        color: theme ? theme.surface : "#1A1D23"
        border.color: theme ? theme.border : "#2A2E36"
        border.width: 1
        radius: theme ? theme.radiusMd : 12
    }

    header: Rectangle {
        width: parent.width
        height: 52
        color: "transparent"

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: dt ? dt.sp24 : 24
            anchors.rightMargin: dt ? dt.sp16 : 16

            Text {
                text: "设置"
                color: theme ? theme.textPrimary : "#E2E4E9"
                font.pixelSize: theme ? theme.fontLg : 16
                font.weight: Font.Bold
                Layout.fillWidth: true
            }

            Rectangle {
                width: 28; height: 28
                radius: 14
                color: closeBtnHover.containsMouse ? (theme ? theme.cardHover : "#22262E") : "transparent"

                Text {
                    anchors.centerIn: parent
                    text: "\u2715"
                    color: theme ? theme.textMuted : "#606470"
                    font.pixelSize: theme ? theme.fontSm : 12
                }

                MouseArea {
                    id: closeBtnHover
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.close()
                }
            }
        }
    }

    // Subtitle line
    footer: null

    function updateValues() {
        if (!root.backendRef) return;
        fontSpin.value = root.backendRef.setting_font_size;
        autoSaveSwitch.checked = root.backendRef.setting_auto_save_enabled;
        lineSpacingCombo.currentIndex = getLineSpacingIndex(root.backendRef.setting_line_spacing);

        var mode = root.backendRef.setting_theme_mode;
        if (mode === "light") themeCombo.currentIndex = 1;
        else if (mode === "dark") themeCombo.currentIndex = 2;
        else themeCombo.currentIndex = 0;

        if (root.backendRef.ai_available) {
            aiSwitch.checked = root.backendRef.ai_enabled;
        }
    }

    function getLineSpacingIndex(val) {
        if (Math.abs(val - 1.25) < 0.01) return 0;
        if (Math.abs(val - 1.5) < 0.01) return 1;
        if (Math.abs(val - 1.75) < 0.01) return 2;
        if (Math.abs(val - 2.0) < 0.01) return 3;
        return 1;
    }

    onOpened: updateValues()
    onBackendRefChanged: updateValues()

    Connections {
        target: root.backendRef
        function onSettings_changed() { root.updateValues(); }
    }

    property var dt: theme

    ScrollView {
        id: settingsScroll
        anchors.fill: parent
        anchors.margins: dt ? dt.sp20 : 20
        clip: true
        ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

        Column {
            width: settingsScroll.availableWidth
            spacing: dt ? dt.sp20 : 20

            // === 写作体验 ===
            Rectangle {
                width: parent.width
                radius: dt ? dt.radiusCard : 18
                color: dt ? dt.card : "#1E2128"
                height: writingCol.implicitHeight + (dt ? dt.sp32 : 32)

                ColumnLayout {
                    id: writingCol
                    anchors.fill: parent
                    anchors.margins: dt ? dt.sp20 : 20
                    spacing: dt ? dt.sp16 : 16

                    Text {
                        text: "写作体验"
                        color: dt ? dt.accent : "#7B8CDE"
                        font.pixelSize: dt ? dt.fontMd : 14
                        font.weight: Font.Bold
                    }

                    // Font size
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: dt ? dt.sp16 : 16

                        Column {
                            Layout.fillWidth: true
                            spacing: dt ? dt.sp4 : 4
                            Text {
                                text: "字号"
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontMd : 14
                            }
                            Text {
                                text: "编辑器文字大小"
                                color: dt ? dt.textMuted : "#606470"
                                font.pixelSize: dt ? dt.fontXs : 11
                            }
                        }

                        RowLayout {
                            spacing: dt ? dt.sp8 : 8
                            Rectangle {
                                width: 32; height: 32
                                radius: dt ? dt.radiusSm : 8
                                color: minusHover.containsMouse ? (theme ? theme.cardHover : "#22262E") : "transparent"
                                border.color: dt ? dt.border : "#2A2E36"
                                border.width: 1
                                Text {
                                    anchors.centerIn: parent
                                    text: "-"
                                    color: dt ? dt.textPrimary : "#E2E4E9"
                                    font.pixelSize: dt ? dt.fontLg : 16
                                    font.weight: Font.Bold
                                }
                                MouseArea {
                                    id: minusHover
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: {
                                        var v = Math.max(10, fontSpin.value - 1);
                                        fontSpin.value = v;
                                        root.backendRef.setting_font_size = v;
                                        root.backendRef.save_local_settings();
                                        root.settingsChanged();
                                    }
                                }
                            }
                            Text {
                                text: Math.round(fontSpin.value)
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontMd : 14
                                font.weight: Font.Bold
                                width: 30
                                horizontalAlignment: Text.AlignHCenter
                            }
                            Rectangle {
                                width: 32; height: 32
                                radius: dt ? dt.radiusSm : 8
                                color: plusHover.containsMouse ? (theme ? theme.cardHover : "#22262E") : "transparent"
                                border.color: dt ? dt.border : "#2A2E36"
                                border.width: 1
                                Text {
                                    anchors.centerIn: parent
                                    text: "+"
                                    color: dt ? dt.textPrimary : "#E2E4E9"
                                    font.pixelSize: dt ? dt.fontLg : 16
                                    font.weight: Font.Bold
                                }
                                MouseArea {
                                    id: plusHover
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: {
                                        var v = Math.min(40, fontSpin.value + 1);
                                        fontSpin.value = v;
                                        root.backendRef.setting_font_size = v;
                                        root.backendRef.save_local_settings();
                                        root.settingsChanged();
                                    }
                                }
                            }
                            SpinBox {
                                id: fontSpin
                                visible: false
                                value: 16
                                from: 10
                                to: 40
                            }
                        }
                    }

                    // Line spacing
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: dt ? dt.sp16 : 16

                        Column {
                            Layout.fillWidth: true
                            spacing: dt ? dt.sp4 : 4
                            Text {
                                text: "行距"
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontMd : 14
                            }
                            Text {
                                text: "段落之间的行间距"
                                color: dt ? dt.textMuted : "#606470"
                                font.pixelSize: dt ? dt.fontXs : 11
                            }
                        }

                        ComboBox {
                            id: lineSpacingCombo
                            width: 120
                            model: ["1.25 倍", "1.5 倍", "1.75 倍", "2.0 倍"]
                            currentIndex: 1
                            onActivated: {
                                var values = [1.25, 1.5, 1.75, 2.0];
                                root.backendRef.setting_line_spacing = values[currentIndex];
                                root.backendRef.save_local_settings();
                                root.settingsChanged();
                            }
                            background: Rectangle {
                                color: dt ? dt.paper : "#191C21"
                                border.color: dt ? dt.border : "#2A2E36"
                                border.width: 1
                                radius: dt ? dt.radiusSm : 8
                            }
                            contentItem: Text {
                                text: lineSpacingCombo.displayText
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontSm : 12
                                verticalAlignment: Text.AlignVCenter
                                leftPadding: dt ? dt.sp8 : 8
                            }
                        }
                    }

                    // Auto save
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: dt ? dt.sp16 : 16

                        Column {
                            Layout.fillWidth: true
                            spacing: dt ? dt.sp4 : 4
                            Text {
                                text: "自动保存"
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontMd : 14
                            }
                            Text {
                                text: "编辑时自动保存章节内容"
                                color: dt ? dt.textMuted : "#606470"
                                font.pixelSize: dt ? dt.fontXs : 11
                            }
                        }

                        Switch {
                            id: autoSaveSwitch
                            checked: false
                            onToggled: {
                                root.backendRef.setting_auto_save_enabled = checked;
                                root.backendRef.save_local_settings();
                                root.settingsChanged();
                            }
                        }
                    }
                }
            }

            // === 编辑器 ===
            Rectangle {
                width: parent.width
                radius: dt ? dt.radiusCard : 18
                color: dt ? dt.card : "#1E2128"
                height: editorCol.implicitHeight + (dt ? dt.sp32 : 32)

                ColumnLayout {
                    id: editorCol
                    anchors.fill: parent
                    anchors.margins: dt ? dt.sp20 : 20
                    spacing: dt ? dt.sp16 : 16

                    Text {
                        text: "编辑器"
                        color: dt ? dt.accent : "#7B8CDE"
                        font.pixelSize: dt ? dt.fontMd : 14
                        font.weight: Font.Bold
                    }

                    // Typing animation
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: dt ? dt.sp16 : 16

                        Column {
                            Layout.fillWidth: true
                            spacing: dt ? dt.sp4 : 4
                            Text {
                                text: "打字动画"
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontMd : 14
                            }
                            Text {
                                text: "输入时显示光标动画效果"
                                color: dt ? dt.textMuted : "#606470"
                                font.pixelSize: dt ? dt.fontXs : 11
                            }
                        }

                        Switch {
                            id: animSwitch
                            checked: root.backendRef ? root.backendRef.setting_typing_animation_enabled : true
                            onToggled: {
                                root.backendRef.setting_typing_animation_enabled = checked;
                                root.backendRef.save_local_settings();
                                root.settingsChanged();
                            }
                        }
                    }

                    // Smooth cursor
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: dt ? dt.sp16 : 16

                        Column {
                            Layout.fillWidth: true
                            spacing: dt ? dt.sp4 : 4
                            Text {
                                text: "平滑光标"
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontMd : 14
                            }
                            Text {
                                text: "光标移动使用平滑过渡"
                                color: dt ? dt.textMuted : "#606470"
                                font.pixelSize: dt ? dt.fontXs : 11
                            }
                        }

                        Switch {
                            id: cursorSwitch
                            checked: root.backendRef ? root.backendRef.setting_smooth_cursor_enabled : true
                            onToggled: {
                                root.backendRef.setting_smooth_cursor_enabled = checked;
                                root.backendRef.save_local_settings();
                                root.settingsChanged();
                            }
                        }
                    }
                }
            }

            // === 外观 ===
            Rectangle {
                width: parent.width
                radius: dt ? dt.radiusCard : 18
                color: dt ? dt.card : "#1E2128"
                height: appearanceCol.implicitHeight + (dt ? dt.sp32 : 32)

                ColumnLayout {
                    id: appearanceCol
                    anchors.fill: parent
                    anchors.margins: dt ? dt.sp20 : 20
                    spacing: dt ? dt.sp16 : 16

                    Text {
                        text: "外观"
                        color: dt ? dt.accent : "#7B8CDE"
                        font.pixelSize: dt ? dt.fontMd : 14
                        font.weight: Font.Bold
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: dt ? dt.sp16 : 16

                        Column {
                            Layout.fillWidth: true
                            spacing: dt ? dt.sp4 : 4
                            Text {
                                text: "主题模式"
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontMd : 14
                            }
                            Text {
                                text: "跟随系统 / 浅色 / 深色"
                                color: dt ? dt.textMuted : "#606470"
                                font.pixelSize: dt ? dt.fontXs : 11
                            }
                        }

                        ComboBox {
                            id: themeCombo
                            width: 120
                            model: ["跟随系统", "浅色", "深色"]
                            currentIndex: 0
                            onActivated: {
                                var modes = ["system", "light", "dark"];
                                root.backendRef.setting_theme_mode = modes[currentIndex];
                                root.backendRef.save_local_settings();
                                root.settingsChanged();
                            }
                            background: Rectangle {
                                color: dt ? dt.paper : "#191C21"
                                border.color: dt ? dt.border : "#2A2E36"
                                border.width: 1
                                radius: dt ? dt.radiusSm : 8
                            }
                            contentItem: Text {
                                text: themeCombo.displayText
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontSm : 12
                                verticalAlignment: Text.AlignVCenter
                                leftPadding: dt ? dt.sp8 : 8
                            }
                        }
                    }
                }
            }

            // === AI ===
            Rectangle {
                width: parent.width
                radius: dt ? dt.radiusCard : 18
                color: dt ? dt.card : "#1E2128"
                height: aiCol.implicitHeight + (dt ? dt.sp32 : 32)
                visible: root.backendRef ? root.backendRef.ai_available : false

                ColumnLayout {
                    id: aiCol
                    anchors.fill: parent
                    anchors.margins: dt ? dt.sp20 : 20
                    spacing: dt ? dt.sp16 : 16

                    Text {
                        text: "AI"
                        color: dt ? dt.accent : "#7B8CDE"
                        font.pixelSize: dt ? dt.fontMd : 14
                        font.weight: Font.Bold
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: dt ? dt.sp16 : 16

                        Column {
                            Layout.fillWidth: true
                            spacing: dt ? dt.sp4 : 4
                            Text {
                                text: "启用 AI 功能"
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontMd : 14
                            }
                            Text {
                                text: "需要配置 API 密钥后才能使用"
                                color: dt ? dt.textMuted : "#606470"
                                font.pixelSize: dt ? dt.fontXs : 11
                            }
                        }

                        Switch {
                            id: aiSwitch
                            checked: false
                            onToggled: {
                                root.backendRef.ai_enabled = checked;
                                root.backendRef.save_local_settings();
                                root.settingsChanged();
                            }
                        }
                    }
                }
            }

            // Bottom spacer
            Item { height: dt ? dt.sp16 : 16 }
        }
    }
}
