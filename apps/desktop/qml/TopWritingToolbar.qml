// =============================================================================
// TopWritingToolbar.qml — 写作工具栏
// =============================================================================
//
// 层级：Desktop UI 层（QML UI 组件）
// 职责：字号/行距/首行缩进控制、一键排版、星图链接入口
// 约束：
//   - 只发出信号，不直接修改 backend
//   - 所有设置变更通过 signal 传递给 EditorController
//   - 使用 DesignTokens 统一样式，禁止硬编码颜色/间距
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    property var dt: null
    property var backendRef: null
    property string shellMode: "SinglePane"
    property real currentFontSize: 16
    property real currentLineSpacing: 1.5
    property bool firstLineIndent: false
    property string saveStatus: ""
    property string currentProjectId: ""
    readonly property int minFontSize: 10
    readonly property int maxFontSize: 120

    // ── ScreenPolicyAdapter：从 Core 获取按钮位置语义 ──
    ScreenPolicyAdapter {
        id: screenPolicy
        backendRef: root.backendRef
        screenRole: "Writing"
        shellMode: root.shellMode
    }

    signal fontSizeChanged(real size)
    signal lineSpacingChanged(real spacing)
    signal firstLineIndentToggled()
    signal formatOneClick()
    signal linkToStarMap()
    signal openStats()

    function syncFontSizeInput() {
        if (fontSizeInput) {
            fontSizeInput.text = Math.round(root.currentFontSize).toString()
        }
    }

    function commitFontSizeInput(finalize) {
        if (!fontSizeInput) return
        var rawText = fontSizeInput.text.trim()
        if (rawText.length === 0) {
            if (finalize) root.syncFontSizeInput()
            return
        }

        var nextSize = Number(rawText)
        if (!isFinite(nextSize)) {
            if (finalize) root.syncFontSizeInput()
            return
        }

        if (!finalize && (nextSize < root.minFontSize || nextSize > root.maxFontSize)) return

        nextSize = Math.max(root.minFontSize, Math.min(root.maxFontSize, Math.round(nextSize)))
        if (Math.round(root.currentFontSize) !== nextSize) {
            root.fontSizeChanged(nextSize)
        }
        if (finalize) fontSizeInput.text = nextSize.toString()
    }

    onCurrentFontSizeChanged: root.syncFontSizeInput()

    color: dt ? dt.surface : "#FCFCFF"
    height: 48

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: dt ? dt.sp16 : 16
        anchors.rightMargin: dt ? dt.sp16 : 16
        spacing: dt ? dt.sp4 : 4

        // Font button (triggers popover)
        Rectangle {
            width: fontRow.implicitWidth + (dt ? dt.sp12 : 12)
            height: 32
            radius: dt ? dt.radiusPill : 999
            color: fontPopover.visible || fontHover.containsMouse ?
                   (dt ? dt.primaryContainer : "#CCE5FF") : "transparent"

            Row {
                id: fontRow
                anchors.centerIn: parent
                spacing: dt ? dt.sp4 : 4
                AppText {
                    text: "A"
                    color: dt ? dt.textSecondary : "#8C9198"
                    font.pixelSize: dt ? dt.label : 13
                    font.family: dt ? dt.fontFamily : "sans-serif"
                    font.weight: Font.Bold
                }
                AppText {
                    text: Math.round(root.currentFontSize) + "px"
                    color: dt ? dt.textPrimary : "#E2E2E5"
                    font.pixelSize: dt ? dt.label : 13
                    font.family: dt ? dt.fontFamily : "sans-serif"
                }
                AppText {
                    text: "\u25BE"
                    color: dt ? dt.textSecondary : "#8C9198"
                    font.pixelSize: dt ? dt.fontXs : 11
                }
            }

            MouseArea {
                id: fontHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                    if (fontPopover.visible) fontPopover.close(); else fontPopover.open();
                    lineSpacingPopover.close();
                    layoutPopover.close();
                }
            }
        }

        // Line Spacing button (triggers lineSpacingPopover)
        Rectangle {
            width: spacingRow.implicitWidth + (dt ? dt.sp12 : 12)
            height: 32
            radius: dt ? dt.radiusPill : 999
            color: lineSpacingPopover.visible || spacingHover.containsMouse ?
                   (dt ? dt.primaryContainer : "#CCE5FF") : "transparent"

            Row {
                id: spacingRow
                anchors.centerIn: parent
                spacing: dt ? dt.sp4 : 4
                AppText {
                    text: "\u2630"
                    color: dt ? dt.textSecondary : "#8C9198"
                    font.pixelSize: dt ? dt.label : 13
                    font.family: dt ? dt.fontFamily : "sans-serif"
                }
                AppText {
                    text: Number(root.currentLineSpacing).toFixed(1) + "x"
                    color: dt ? dt.textPrimary : "#E2E2E5"
                    font.pixelSize: dt ? dt.label : 13
                    font.family: dt ? dt.fontFamily : "sans-serif"
                }
                AppText {
                    text: "\u25BE"
                    color: dt ? dt.textSecondary : "#8C9198"
                    font.pixelSize: dt ? dt.fontXs : 11
                }
            }

            MouseArea {
                id: spacingHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                    if (lineSpacingPopover.visible) lineSpacingPopover.close(); else lineSpacingPopover.open();
                    fontPopover.close();
                    layoutPopover.close();
                }
            }
        }

        // Paragraph Layout button (triggers layoutPopover)
        Rectangle {
            width: layoutRow.implicitWidth + (dt ? dt.sp12 : 12)
            height: 32
            radius: dt ? dt.radiusPill : 999
            color: layoutPopover.visible || layoutHover.containsMouse ?
                   (dt ? dt.primaryContainer : "#CCE5FF") : "transparent"

            Row {
                id: layoutRow
                anchors.centerIn: parent
                spacing: dt ? dt.sp4 : 4
                AppText {
                    text: "\u21E5"
                    color: dt ? dt.textSecondary : "#8C9198"
                    font.pixelSize: dt ? dt.label : 13
                    font.family: dt ? dt.fontFamily : "sans-serif"
                    font.weight: Font.Bold
                }
                AppText {
                    text: qsTr("段落")
                    color: dt ? dt.textPrimary : "#E2E2E5"
                    font.pixelSize: dt ? dt.label : 13
                    font.family: dt ? dt.fontFamily : "sans-serif"
                }
                AppText {
                    text: "\u25BE"
                    color: dt ? dt.textSecondary : "#8C9198"
                    font.pixelSize: dt ? dt.fontXs : 11
                }
            }

            MouseArea {
                id: layoutHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                    if (layoutPopover.visible) layoutPopover.close(); else layoutPopover.open();
                    fontPopover.close();
                    lineSpacingPopover.close();
                }
            }
        }

        // Format button
        Rectangle {
            visible: true
            width: formatRow.implicitWidth + (dt ? dt.sp12 : 12)
            height: 32
            radius: dt ? dt.radiusPill : 999
            color: formatHover.containsMouse ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"
            border.color: dt ? dt.border : "#2A2E36"
            border.width: 1

            Row {
                id: formatRow
                anchors.centerIn: parent
                spacing: dt ? dt.sp4 : 4
                AppText {
                    text: qsTr("一键排版")
                    color: dt ? dt.textSecondary : "#8C9198"
                    font.pixelSize: dt ? dt.label : 13
                    font.family: dt ? dt.fontFamily : "sans-serif"
                }
            }

            MouseArea {
                id: formatHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.formatOneClick()
            }
        }

        Item { Layout.fillWidth: true }

        // Star map button
        Rectangle {
            visible: true
            width: starRow.implicitWidth + (dt ? dt.sp12 : 12)
            height: 32
            radius: dt ? dt.radiusPill : 999
            color: starHover.containsMouse ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"
            border.color: dt ? dt.border : "#2A2E36"
            border.width: 1

            Row {
                id: starRow
                anchors.centerIn: parent
                spacing: dt ? dt.sp4 : 4
                AppText {
                    text: qsTr("星图")
                    color: dt ? dt.textSecondary : "#8C9198"
                    font.pixelSize: dt ? dt.label : 13
                    font.family: dt ? dt.fontFamily : "sans-serif"
                }
            }

            MouseArea {
                id: starHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.linkToStarMap()
            }
        }

        // Stats button
        Rectangle {
            visible: true
            width: statsRow.implicitWidth + (dt ? dt.sp12 : 12)
            height: 32
            radius: dt ? dt.radiusPill : 999
            color: statsHover.containsMouse ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"
            border.color: dt ? dt.border : "#2A2E36"
            border.width: 1

            Row {
                id: statsRow
                anchors.centerIn: parent
                spacing: dt ? dt.sp4 : 4
                AppText {
                    text: qsTr("统计")
                    color: dt ? dt.textSecondary : "#8C9198"
                    font.pixelSize: dt ? dt.label : 13
                    font.family: dt ? dt.fontFamily : "sans-serif"
                }
            }

            MouseArea {
                id: statsHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.openStats()
            }
        }

        Item { Layout.preferredWidth: dt ? dt.sp8 : 8 }

        // Save status
        AppText {
            text: root.saveStatus || ""
            color: dt ? dt.textSecondary : "#8C9198"
            font.pixelSize: dt ? dt.caption : 12
            font.family: dt ? dt.fontFamily : "sans-serif"
            visible: text !== ""
        }
    }

    // === Font Size Popover ===
    Popup {
        id: fontPopover
        y: root.height + (dt ? dt.sp8 : 8)
        x: 60
        width: 200
        padding: dt ? dt.sp12 : 12
        closePolicy: Popup.CloseOnPressOutside | Popup.CloseOnEscape
        background: Rectangle {
            radius: dt ? dt.radiusXl : 24
            color: dt ? dt.surface : "#1A1D23"
            border.color: dt ? dt.border : "#2A2E36"
            border.width: 1
        }

        contentItem: ColumnLayout {
            spacing: dt ? dt.sp12 : 12

            AppText {
                text: qsTr("字号")
                color: dt ? dt.textPrimary : "#E2E4E9"
                font.pixelSize: dt ? dt.subtitle : 18
                font.family: dt ? dt.fontFamily : "sans-serif"
                font.weight: Font.DemiBold
            }

            // Quick presets
            Flow {
                Layout.fillWidth: true
                spacing: dt ? dt.sp6 : 6

                Repeater {
                    model: [12, 14, 16, 18, 20, 24]

                    Rectangle {
                        width: 40; height: 32
                        radius: dt ? dt.radiusPill : 999
                        color: Math.round(root.currentFontSize) === modelData ?
                               (dt ? dt.primaryContainer : "#CCE5FF") :
                               presetHover.containsMouse ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"

                        AppText {
                            anchors.centerIn: parent
                            text: modelData
                            color: Math.round(root.currentFontSize) === modelData ?
                                   (dt ? dt.selectedText : "#CCE5FF") :
                                   (dt ? dt.textSecondary : "#8C9198")
                            font.pixelSize: dt ? dt.label : 13
                            font.family: dt ? dt.fontFamily : "sans-serif"
                            font.weight: Math.round(root.currentFontSize) === modelData ? Font.DemiBold : Font.Normal
                        }

                        MouseArea {
                            id: presetHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                root.fontSizeChanged(modelData)
                                fontPopover.close()
                            }
                        }
                    }
                }
            }

            // Slider
            RowLayout {
                Layout.fillWidth: true
                spacing: dt ? dt.sp8 : 8

                AppText {
                    text: "10"
                    color: dt ? dt.textMuted : "#8C9198"
                    font.pixelSize: dt ? dt.fontXs : 11
                }

                AppSlider {
                    id: fontSlider
                    Layout.fillWidth: true
                    theme: dt
                    from: root.minFontSize
                    to: root.maxFontSize
                    stepSize: 1
                    value: root.currentFontSize
                    onMoved: root.fontSizeChanged(value)
                }

                AppText {
                    text: "120"
                    color: dt ? dt.textMuted : "#8C9198"
                    font.pixelSize: dt ? dt.fontXs : 11
                }
            }

            RowLayout {
                Layout.alignment: Qt.AlignHCenter
                spacing: dt ? dt.sp6 : 6

                TextField {
                    id: fontSizeInput
                    Layout.preferredWidth: 68
                    Layout.preferredHeight: 34
                    text: Math.round(root.currentFontSize).toString()
                    horizontalAlignment: TextInput.AlignHCenter
                    selectByMouse: true
                    inputMethodHints: Qt.ImhDigitsOnly
                    validator: IntValidator { bottom: root.minFontSize; top: root.maxFontSize }
                    color: dt ? dt.textPrimary : "#E2E4E9"
                    selectionColor: dt ? dt.primary : "#006497"
                    selectedTextColor: dt ? dt.onPrimary : "#FFFFFF"
                    font.pixelSize: dt ? dt.body : 14
                    font.family: dt ? dt.fontFamily : "sans-serif"
                    leftPadding: dt ? dt.sp8 : 8
                    rightPadding: dt ? dt.sp8 : 8
                    topPadding: dt ? dt.sp4 : 4
                    bottomPadding: dt ? dt.sp4 : 4
                    onTextEdited: root.commitFontSizeInput(false)
                    onAccepted: root.commitFontSizeInput(true)
                    onEditingFinished: root.commitFontSizeInput(true)
                    background: Rectangle {
                        color: dt ? dt.surfaceContainerLow : "#ffffff"
                        border.color: fontSizeInput.activeFocus ? (dt ? dt.primary : "#006497") : (dt ? dt.border : "#2A2E36")
                        border.width: fontSizeInput.activeFocus ? 2 : 1
                        radius: dt ? dt.radiusMd : 12
                    }
                }

                AppText {
                    text: "px"
                    color: dt ? dt.textSecondary : "#9CA0AB"
                    font.pixelSize: dt ? dt.fontSm : 12
                    font.family: dt ? dt.fontFamily : "sans-serif"
                    Layout.alignment: Qt.AlignVCenter
                }
            }
        }
    }

    // === Line Spacing Popover ===
    Popup {
        id: lineSpacingPopover
        y: root.height + (dt ? dt.sp8 : 8)
        x: 100
        width: 200
        padding: dt ? dt.sp12 : 12
        closePolicy: Popup.CloseOnPressOutside | Popup.CloseOnEscape
        background: Rectangle {
            radius: dt ? dt.radiusXl : 24
            color: dt ? dt.surface : "#1A1D23"
            border.color: dt ? dt.border : "#2A2E36"
            border.width: 1
        }

        contentItem: ColumnLayout {
            spacing: dt ? dt.sp12 : 12

            AppText {
                text: qsTr("行距倍数")
                color: dt ? dt.textPrimary : "#E2E4E9"
                font.pixelSize: dt ? dt.subtitle : 18
                font.family: dt ? dt.fontFamily : "sans-serif"
                font.weight: Font.DemiBold
            }

            // Quick presets
            Flow {
                Layout.fillWidth: true
                spacing: dt ? dt.sp6 : 6

                Repeater {
                    model: [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0]

                    Rectangle {
                        width: 40; height: 32
                        radius: dt ? dt.radiusPill : 999
                        color: Math.abs(root.currentLineSpacing - modelData) < 0.01 ?
                               (dt ? dt.primaryContainer : "#CCE5FF") :
                               presetHover.containsMouse ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"

                        AppText {
                            anchors.centerIn: parent
                            text: Number(modelData).toFixed(2).replace(/\.00$/, "").replace(/(\.\d)0$/, "$1")
                            color: Math.abs(root.currentLineSpacing - modelData) < 0.01 ?
                                   (dt ? dt.selectedText : "#CCE5FF") :
                                   (dt ? dt.textSecondary : "#8C9198")
                            font.pixelSize: dt ? dt.label : 13
                            font.family: dt ? dt.fontFamily : "sans-serif"
                            font.weight: Math.abs(root.currentLineSpacing - modelData) < 0.01 ? Font.DemiBold : Font.Normal
                        }

                        MouseArea {
                            id: presetHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                root.lineSpacingChanged(modelData)
                                lineSpacingPopover.close()
                            }
                        }
                    }
                }
            }

            // Slider
            RowLayout {
                Layout.fillWidth: true
                spacing: dt ? dt.sp8 : 8

                AppText {
                    text: "1.0"
                    color: dt ? dt.textMuted : "#8C9198"
                    font.pixelSize: dt ? dt.fontXs : 11
                }

                AppSlider {
                    id: lineSpacingSlider
                    Layout.fillWidth: true
                    theme: dt
                    from: 1.0
                    to: 3.0
                    stepSize: 0.1
                    value: root.currentLineSpacing
                    onMoved: root.lineSpacingChanged(value)
                }

                AppText {
                    text: "3.0"
                    color: dt ? dt.textMuted : "#8C9198"
                    font.pixelSize: dt ? dt.fontXs : 11
                }
            }

            AppText {
                text: Number(root.currentLineSpacing).toFixed(1) + " x"
                color: dt ? dt.textSecondary : "#9CA0AB"
                font.pixelSize: dt ? dt.fontSm : 12
                Layout.alignment: Qt.AlignHCenter
            }
        }
    }

    // === Layout Popover ===
    Popup {
        id: layoutPopover
        y: root.height + (dt ? dt.sp8 : 8)
        x: 180
        width: 240
        padding: dt ? dt.sp12 : 12
        closePolicy: Popup.CloseOnPressOutside | Popup.CloseOnEscape
        background: Rectangle {
            radius: dt ? dt.radiusXl : 24
            color: dt ? dt.surface : "#1A1D23"
            border.color: dt ? dt.border : "#2A2E36"
            border.width: 1
        }

        contentItem: ColumnLayout {
            spacing: dt ? dt.sp12 : 12

            AppText {
                text: qsTr("段落设置")
                color: dt ? dt.textPrimary : "#E2E4E9"
                font.pixelSize: dt ? dt.subtitle : 18
                font.family: dt ? dt.fontFamily : "sans-serif"
                font.weight: Font.DemiBold
            }

            // Editor width slider
            ColumnLayout {
                Layout.fillWidth: true
                spacing: dt ? dt.sp4 : 4

                AppText {
                    text: qsTr("正文宽度")
                    color: dt ? dt.textSecondary : "#9CA0AB"
                    font.pixelSize: dt ? dt.fontSm : 12
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: dt ? dt.sp8 : 8

                    AppText {
                        text: "480"
                        color: dt ? dt.textMuted : "#8C9198"
                        font.pixelSize: dt ? dt.fontXs : 11
                    }

                    AppSlider {
                        id: widthSlider
                        Layout.fillWidth: true
                        theme: dt
                        from: 480
                        to: 3840
                        stepSize: 10
                        value: settingsBackend && settingsBackend.setting_desktop_editor_width > 0 ? settingsBackend.setting_desktop_editor_width : 820
                        onMoved: {
                            if (settingsBackend) {
                                settingsBackend.setting_desktop_editor_width = value;
                            }
                        }
                    }

                    AppText {
                        text: "3840"
                        color: dt ? dt.textMuted : "#8C9198"
                        font.pixelSize: dt ? dt.fontXs : 11
                    }
                }

                AppText {
                    text: Math.round(widthSlider.value) + " px"
                    color: dt ? dt.textSecondary : "#9CA0AB"
                    font.pixelSize: dt ? dt.fontSm : 12
                    Layout.alignment: Qt.AlignHCenter
                }
            }

            // Divider
            Rectangle { Layout.fillWidth: true; height: 1; color: dt ? dt.border : "#2A2E36" }

            // First line indent
            RowLayout {
                Layout.fillWidth: true
                spacing: dt ? dt.sp8 : 8

                Column {
                    Layout.fillWidth: true
                    spacing: 2
                    AppText {
                        text: qsTr("首行缩进")
                        color: dt ? dt.textPrimary : "#E2E2E5"
                        font.pixelSize: dt ? dt.fontMd : 14
                    }
                    AppText {
                        text: qsTr("段落开头缩进两个字符")
                        color: dt ? dt.textMuted : "#8C9198"
                        font.pixelSize: dt ? dt.fontXs : 11
                    }
                }

                ModernSwitch {
                    dt: root.dt
                    checked: root.firstLineIndent
                    onToggled: root.firstLineIndentToggled()
                }
            }
        }
    }
}
