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
    readonly property int maxFontSize: 72

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
    signal openSettings()

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

    color: dt.surface
    height: 48

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: dt.sp16
        anchors.rightMargin: dt.sp16
        spacing: dt.sp4

        // Font button (triggers popover)
        Rectangle {
            width: fontRow.implicitWidth + dt.sp12
            height: 32
            radius: dt.radiusPill
            color: fontPopover.visible || fontHover.containsMouse ?
                   dt.primaryContainer : "transparent"

            Row {
                id: fontRow
                anchors.centerIn: parent
                spacing: dt.sp4
                AppText {
                    text: "A"
                    color: dt.textSecondary
                    font.pixelSize: dt.label
                    font.family: dt.fontFamily
                    font.weight: Font.Bold
                }
                AppText {
                    text: Math.round(root.currentFontSize) + "px"
                    color: dt.textPrimary
                    font.pixelSize: dt.label
                    font.family: dt.fontFamily
                }
                AppText {
                    text: "\u25BE"
                    color: dt.textSecondary
                    font.pixelSize: dt.fontXs
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
            width: spacingRow.implicitWidth + dt.sp12
            height: 32
            radius: dt.radiusPill
            color: lineSpacingPopover.visible || spacingHover.containsMouse ?
                   dt.primaryContainer : "transparent"

            Row {
                id: spacingRow
                anchors.centerIn: parent
                spacing: dt.sp4
                AppText {
                    text: "\u2630"
                    color: dt.textSecondary
                    font.pixelSize: dt.label
                    font.family: dt.fontFamily
                }
                AppText {
                    text: Number(root.currentLineSpacing).toFixed(1) + "x"
                    color: dt.textPrimary
                    font.pixelSize: dt.label
                    font.family: dt.fontFamily
                }
                AppText {
                    text: "\u25BE"
                    color: dt.textSecondary
                    font.pixelSize: dt.fontXs
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
            width: layoutRow.implicitWidth + dt.sp12
            height: 32
            radius: dt.radiusPill
            color: layoutPopover.visible || layoutHover.containsMouse ?
                   dt.primaryContainer : "transparent"

            Row {
                id: layoutRow
                anchors.centerIn: parent
                spacing: dt.sp4
                AppText {
                    text: "\u21E5"
                    color: dt.textSecondary
                    font.pixelSize: dt.label
                    font.family: dt.fontFamily
                    font.weight: Font.Bold
                }
                AppText {
                    text: qsTr("段落")
                    color: dt.textPrimary
                    font.pixelSize: dt.label
                    font.family: dt.fontFamily
                }
                AppText {
                    text: "\u25BE"
                    color: dt.textSecondary
                    font.pixelSize: dt.fontXs
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
            width: formatRow.implicitWidth + dt.sp12
            height: 32
            radius: dt.radiusPill
            color: formatHover.containsMouse ? dt.surfaceVariant : "transparent"
            border.color: dt.border
            border.width: 1

            Row {
                id: formatRow
                anchors.centerIn: parent
                spacing: dt.sp4
                AppText {
                    text: qsTr("一键排版")
                    color: dt.textSecondary
                    font.pixelSize: dt.label
                    font.family: dt.fontFamily
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
            width: starRow.implicitWidth + dt.sp12
            height: 32
            radius: dt.radiusPill
            color: starHover.containsMouse ? dt.surfaceVariant : "transparent"
            border.color: dt.border
            border.width: 1

            Row {
                id: starRow
                anchors.centerIn: parent
                spacing: dt.sp4
                AppText {
                    text: qsTr("星图")
                    color: dt.textSecondary
                    font.pixelSize: dt.label
                    font.family: dt.fontFamily
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
            width: statsRow.implicitWidth + dt.sp12
            height: 32
            radius: dt.radiusPill
            color: statsHover.containsMouse ? dt.surfaceVariant : "transparent"
            border.color: dt.border
            border.width: 1

            Row {
                id: statsRow
                anchors.centerIn: parent
                spacing: dt.sp4
                AppText {
                    text: qsTr("统计")
                    color: dt.textSecondary
                    font.pixelSize: dt.label
                    font.family: dt.fontFamily
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

        // Settings button
        Rectangle {
            visible: true
            width: settingsRow.implicitWidth + dt.sp12
            height: 32
            radius: dt.radiusPill
            color: settingsHover.containsMouse ? dt.surfaceVariant : "transparent"
            border.color: dt.border
            border.width: 1

            Row {
                id: settingsRow
                anchors.centerIn: parent
                spacing: dt.sp4
                AppText {
                    text: qsTr("设置")
                    color: dt.textSecondary
                    font.pixelSize: dt.label
                    font.family: dt.fontFamily
                }
            }

            MouseArea {
                id: settingsHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.openSettings()
            }
        }

        Item { Layout.preferredWidth: dt.sp8 }

        // Save status
        AppText {
            text: root.saveStatus || ""
            color: dt.textSecondary
            font.pixelSize: dt.caption
            font.family: dt.fontFamily
            visible: text !== ""
        }
    }

    // === Font Size Popover ===
    Popup {
        id: fontPopover
        y: root.height + dt.sp8
        x: 60
        width: 200
        padding: dt.sp12
        closePolicy: Popup.CloseOnPressOutside | Popup.CloseOnEscape
        background: Rectangle {
            radius: dt.radiusXl
            color: dt.surface
            border.color: dt.border
            border.width: 1
        }

        contentItem: ColumnLayout {
            spacing: dt.sp12

            AppText {
                text: qsTr("字号")
                color: dt.textPrimary
                font.pixelSize: dt.subtitle
                font.family: dt.fontFamily
                font.weight: Font.DemiBold
            }

            // Quick presets
            Flow {
                Layout.fillWidth: true
                spacing: dt.sp6

                Repeater {
                    model: [12, 14, 16, 18, 20, 24]

                    Rectangle {
                        width: 40; height: 32
                        radius: dt.radiusPill
                        color: Math.round(root.currentFontSize) === modelData ?
                               dt.primaryContainer :
                               presetHover.containsMouse ? dt.surfaceVariant : "transparent"

                        AppText {
                            anchors.centerIn: parent
                            text: modelData
                            color: Math.round(root.currentFontSize) === modelData ?
                                   dt.selectedText :
                                   dt.textSecondary
                            font.pixelSize: dt.label
                            font.family: dt.fontFamily
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
                spacing: dt.sp8

                AppText {
                    text: "10"
                    color: dt.textMuted
                    font.pixelSize: dt.fontXs
                }

                AppSlider {
                    id: fontSlider
                    Layout.fillWidth: true
                    dt: dt
                    from: root.minFontSize
                    to: root.maxFontSize
                    stepSize: 1
                    value: root.currentFontSize
                    onMoved: root.fontSizeChanged(value)
                }

                AppText {
                    text: "72"
                    color: dt.textMuted
                    font.pixelSize: dt.fontXs
                }
            }

            RowLayout {
                Layout.alignment: Qt.AlignHCenter
                spacing: dt.sp6

                TextField {
                    id: fontSizeInput
                    Layout.preferredWidth: 68
                    Layout.preferredHeight: 34
                    text: Math.round(root.currentFontSize).toString()
                    horizontalAlignment: TextInput.AlignHCenter
                    selectByMouse: true
                    inputMethodHints: Qt.ImhDigitsOnly
                    validator: IntValidator { bottom: root.minFontSize; top: root.maxFontSize }
                    color: dt.textPrimary
                    selectionColor: dt.primary
                    selectedTextColor: dt.onPrimary
                    font.pixelSize: dt.body
                    font.family: dt.fontFamily
                    leftPadding: dt.sp8
                    rightPadding: dt.sp8
                    topPadding: dt.sp4
                    bottomPadding: dt.sp4
                    onTextEdited: root.commitFontSizeInput(false)
                    onAccepted: root.commitFontSizeInput(true)
                    onEditingFinished: root.commitFontSizeInput(true)
                    background: Rectangle {
                        color: dt.surfaceContainerLow
                        border.color: fontSizeInput.activeFocus ? dt.primary : dt.border
                        border.width: fontSizeInput.activeFocus ? 2 : 1
                        radius: dt.radiusMd
                    }
                }

                AppText {
                    text: "px"
                    color: dt.textSecondary
                    font.pixelSize: dt.fontSm
                    font.family: dt.fontFamily
                    Layout.alignment: Qt.AlignVCenter
                }
            }
        }
    }

    // === Line Spacing Popover ===
    Popup {
        id: lineSpacingPopover
        y: root.height + dt.sp8
        x: 100
        width: 200
        padding: dt.sp12
        closePolicy: Popup.CloseOnPressOutside | Popup.CloseOnEscape
        background: Rectangle {
            radius: dt.radiusXl
            color: dt.surface
            border.color: dt.border
            border.width: 1
        }

        contentItem: ColumnLayout {
            spacing: dt.sp12

            AppText {
                text: qsTr("行距倍数")
                color: dt.textPrimary
                font.pixelSize: dt.subtitle
                font.family: dt.fontFamily
                font.weight: Font.DemiBold
            }

            // Quick presets
            Flow {
                Layout.fillWidth: true
                spacing: dt.sp6

                Repeater {
                    model: [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0]

                    Rectangle {
                        width: 40; height: 32
                        radius: dt.radiusPill
                        color: Math.abs(root.currentLineSpacing - modelData) < 0.01 ?
                               dt.primaryContainer :
                               presetHover.containsMouse ? dt.surfaceVariant : "transparent"

                        AppText {
                            anchors.centerIn: parent
                            text: Number(modelData).toFixed(2).replace(/\.00$/, "").replace(/(\.\d)0$/, "$1")
                            color: Math.abs(root.currentLineSpacing - modelData) < 0.01 ?
                                   dt.selectedText :
                                   dt.textSecondary
                            font.pixelSize: dt.label
                            font.family: dt.fontFamily
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
                spacing: dt.sp8

                AppText {
                    text: "1.0"
                    color: dt.textMuted
                    font.pixelSize: dt.fontXs
                }

                AppSlider {
                    id: lineSpacingSlider
                    Layout.fillWidth: true
                    dt: dt
                    from: 1.0
                    to: 3.0
                    stepSize: 0.1
                    value: root.currentLineSpacing
                    onMoved: root.lineSpacingChanged(value)
                }

                AppText {
                    text: "3.0"
                    color: dt.textMuted
                    font.pixelSize: dt.fontXs
                }
            }

            AppText {
                text: Number(root.currentLineSpacing).toFixed(1) + " x"
                color: dt.textSecondary
                font.pixelSize: dt.fontSm
                Layout.alignment: Qt.AlignHCenter
            }
        }
    }

    // === Layout Popover ===
    Popup {
        id: layoutPopover
        y: root.height + dt.sp8
        x: 180
        width: 240
        padding: dt.sp12
        closePolicy: Popup.CloseOnPressOutside | Popup.CloseOnEscape
        background: Rectangle {
            radius: dt.radiusXl
            color: dt.surface
            border.color: dt.border
            border.width: 1
        }

        contentItem: ColumnLayout {
            spacing: dt.sp12

            AppText {
                text: qsTr("段落设置")
                color: dt.textPrimary
                font.pixelSize: dt.subtitle
                font.family: dt.fontFamily
                font.weight: Font.DemiBold
            }

            // Editor width slider
            ColumnLayout {
                Layout.fillWidth: true
                spacing: dt.sp4

                AppText {
                    text: qsTr("正文宽度")
                    color: dt.textSecondary
                    font.pixelSize: dt.fontSm
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: dt.sp8

                    AppText {
                        text: "480"
                        color: dt.textMuted
                        font.pixelSize: dt.fontXs
                    }

                    AppSlider {
                        id: widthSlider
                        Layout.fillWidth: true
                        dt: dt
                        from: 480
                        to: 3840
                        stepSize: 10
                        value: settingsBackend && settingsBackend.setting_desktop_editor_width > 0 ? settingsBackend.setting_desktop_editor_width : 820
                        onMoved: {
                            if (settingsBackend) {
                                settingsBackend.setting_desktop_editor_width = value;
                                settingsBackend.debounced_save_local_settings();
                            }
                        }
                    }

                    AppText {
                        text: "3840"
                        color: dt.textMuted
                        font.pixelSize: dt.fontXs
                    }
                }

                AppText {
                    text: Math.round(widthSlider.value) + " px"
                    color: dt.textSecondary
                    font.pixelSize: dt.fontSm
                    Layout.alignment: Qt.AlignHCenter
                }
            }

            // Divider
            Rectangle { Layout.fillWidth: true; height: 1; color: dt.border }

            // First line indent
            RowLayout {
                Layout.fillWidth: true
                spacing: dt.sp8

                Column {
                    Layout.fillWidth: true
                    spacing: 2
                    AppText {
                        text: qsTr("首行缩进")
                        color: dt.textPrimary
                        font.pixelSize: dt.fontMd
                    }
                    AppText {
                        text: qsTr("段落开头缩进两个字符")
                        color: dt.textMuted
                        font.pixelSize: dt.fontXs
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
