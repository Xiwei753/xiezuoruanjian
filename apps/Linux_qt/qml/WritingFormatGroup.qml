// =============================================================================
// WritingFormatGroup.qml — 工作台工具栏 Center 组（字号 / 行距 / 段落 / 一键排版）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：Core 七角色里的 ToolbarCenter 内容——字号 / 行距 / 首行缩进 / 一键排版 + 保存状态
// 约束（Issue #825）：
//   - 由 WritingWorkbenchToolbar 挂在工具条带的中间组里，本组件自己不画工具条外框；
//   - 只发出信号，不直接修改 backend，所有设置变更交给 EditorController；
//   - 同步 / 搜索 / 设置 不在这里，归 ToolbarTrailing 的 GlobalTopActions；
//   - 使用 DesignTokens 统一样式，禁止硬编码颜色 / 间距。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    required property var dt
    property real currentFontSize: 16
    property real currentLineSpacing: 1.5
    property bool firstLineIndent: false
    property string saveStatus: ""
    readonly property int minFontSize: 10
    readonly property int maxFontSize: 72

    signal fontSizeChanged(real size)
    signal lineSpacingChanged(real spacing)
    signal firstLineIndentToggled()
    signal formatOneClick()

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

    // Issue #825：本组件只提供 Center 组内容，工具条带背景/高度由
    // WritingWorkbenchToolbar 统一画，这里保持透明。
    color: "transparent"

    implicitHeight: 48

    RowLayout {
        anchors.fill: parent
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
                    dt: root.dt
                    text: "A"
                    color: dt.textSecondary
                    font.pointSize: dt.labelPt
                    font.family: dt.fontFamily
                    font.weight: Font.Bold
                }
                AppText {
                    dt: root.dt
                    text: Math.round(root.currentFontSize) + "px"
                    color: dt.textPrimary
                    font.pointSize: dt.labelPt
                    font.family: dt.fontFamily
                }
                AppText {
                    dt: root.dt
                    text: "\u25BE"
                    color: dt.textSecondary
                    font.pointSize: dt.fontXsPt
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
                    dt: root.dt
                    text: "\u2630"
                    color: dt.textSecondary
                    font.pointSize: dt.labelPt
                    font.family: dt.fontFamily
                }
                AppText {
                    dt: root.dt
                    text: Number(root.currentLineSpacing).toFixed(1) + "x"
                    color: dt.textPrimary
                    font.pointSize: dt.labelPt
                    font.family: dt.fontFamily
                }
                AppText {
                    dt: root.dt
                    text: "\u25BE"
                    color: dt.textSecondary
                    font.pointSize: dt.fontXsPt
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
                    dt: root.dt
                    text: "\u21E5"
                    color: dt.textSecondary
                    font.pointSize: dt.labelPt
                    font.family: dt.fontFamily
                    font.weight: Font.Bold
                }
                AppText {
                    dt: root.dt
                    text: qsTr("段落")
                    color: dt.textPrimary
                    font.pointSize: dt.labelPt
                    font.family: dt.fontFamily
                }
                AppText {
                    dt: root.dt
                    text: "\u25BE"
                    color: dt.textSecondary
                    font.pointSize: dt.fontXsPt
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
                    dt: root.dt
                    text: qsTr("一键排版")
                    color: dt.textSecondary
                    font.pointSize: dt.labelPt
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

        // Save status
        AppText {
            dt: root.dt
            text: root.saveStatus || ""
            color: dt.textSecondary
            font.pointSize: dt.captionPt
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
                dt: root.dt
                text: qsTr("字号")
                color: dt.textPrimary
                font.pointSize: dt.subtitlePt
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
                            dt: root.dt
                            anchors.centerIn: parent
                            text: modelData
                            color: Math.round(root.currentFontSize) === modelData ?
                                   dt.selectedText :
                                   dt.textSecondary
                            font.pointSize: dt.labelPt
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
                    dt: root.dt
                    text: "10"
                    color: dt.textMuted
                    font.pointSize: dt.fontXsPt
                }

                AppSlider {
                    id: fontSlider
                    Layout.fillWidth: true
                    dt: root.dt
                    from: root.minFontSize
                    to: root.maxFontSize
                    stepSize: 1
                    value: root.currentFontSize
                    onMoved: root.fontSizeChanged(value)
                }

                AppText {
                    dt: root.dt
                    text: "72"
                    color: dt.textMuted
                    font.pointSize: dt.fontXsPt
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
                    font.pointSize: dt.bodyPt
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
                    dt: root.dt
                    text: "px"
                    color: dt.textSecondary
                    font.pointSize: dt.fontSmPt
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
                dt: root.dt
                text: qsTr("行距倍数")
                color: dt.textPrimary
                font.pointSize: dt.subtitlePt
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
                               lineSpacingHover.containsMouse ? dt.surfaceVariant : "transparent"

                        AppText {
                            dt: root.dt
                            anchors.centerIn: parent
                            text: Number(modelData).toFixed(2).replace(/\.00$/, "").replace(/(\.\d)0$/, "$1")
                            color: Math.abs(root.currentLineSpacing - modelData) < 0.01 ?
                                   dt.selectedText :
                                   dt.textSecondary
                            font.pointSize: dt.labelPt
                            font.family: dt.fontFamily
                            font.weight: Math.abs(root.currentLineSpacing - modelData) < 0.01 ? Font.DemiBold : Font.Normal
                        }

                        MouseArea {
                            // Issue #830：字号预设用 presetHover，行距预设改用独立 id。
                            // 两个 preset Repeater 的 delegate 在同一文件里，同名 id 会触发
                            // qmllint 的 syntax.duplicate-ids 错误，挡住 required 属性门禁。
                            id: lineSpacingHover
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
                    dt: root.dt
                    text: "1.0"
                    color: dt.textMuted
                    font.pointSize: dt.fontXsPt
                }

                AppSlider {
                    id: lineSpacingSlider
                    Layout.fillWidth: true
                    dt: root.dt
                    from: 1.0
                    to: 3.0
                    stepSize: 0.1
                    value: root.currentLineSpacing
                    onMoved: root.lineSpacingChanged(value)
                }

                AppText {
                    dt: root.dt
                    text: "3.0"
                    color: dt.textMuted
                    font.pointSize: dt.fontXsPt
                }
            }

            AppText {
                dt: root.dt
                text: Number(root.currentLineSpacing).toFixed(1) + " x"
                color: dt.textSecondary
                font.pointSize: dt.fontSmPt
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
                dt: root.dt
                text: qsTr("段落设置")
                color: dt.textPrimary
                font.pointSize: dt.subtitlePt
                font.family: dt.fontFamily
                font.weight: Font.DemiBold
            }

            // Issue #833：删除编辑器绝对宽度 widthSlider UI 及对
            // setting_desktop_editor_width 的写入。正文区宽度属于布局，
            // 不应再作为格式工具条里的用户设置去反向控制 Workbench 几何。

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
                        dt: root.dt
                        text: qsTr("首行缩进")
                        color: dt.textPrimary
                        font.pointSize: dt.fontMdPt
                    }
                    AppText {
                        dt: root.dt
                        text: qsTr("段落开头缩进两个字符")
                        color: dt.textMuted
                        font.pointSize: dt.fontXsPt
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
