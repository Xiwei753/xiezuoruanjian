// =============================================================================
// WritingFormatGroup.qml — 工作台工具栏 Center 组（字号 / 行距 / 段落 / 一键排版）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：Core 七角色里的 ToolbarCenter 内容——字号 / 行距 / 首行缩进 / 一键排版 + 保存状态
// 约束（Issue #825）：
//   - 由 DesktopTopBar 挂在工具条带的中间组里，本组件自己不画工具条外框；
//   - 只发出信号，不直接修改 backend，所有设置变更交给 EditorController；
//   - 同步 / 搜索 / 设置 不在这里，归 ToolbarTrailing 的 GlobalTopActions；
//   - 使用 DesignTokens 统一样式，禁止硬编码颜色 / 间距。
//
// Issue #900：视觉迁移到 Breeze Controls（ToolButton / Slider / TextField / Switch /
//   Popup）+ Kirigami.Theme / Kirigami.Units。字号输入框字号提交逻辑、弹层互斥逻辑、
//   信号与全部属性签名保持不变。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

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
    // DesktopTopBar 统一画，这里保持透明。
    color: "transparent"

    implicitHeight: Kirigami.Units.gridUnit * 2.6
    implicitWidth: formatControlsRow.implicitWidth + Kirigami.Units.smallSpacing * 2

    RowLayout {
        id: formatControlsRow
        anchors.fill: parent
        spacing: Kirigami.Units.smallSpacing

        // Font button (triggers popover)
        ToolButton {
            id: fontButton
            Layout.preferredHeight: Math.max(40, Kirigami.Units.gridUnit * 2)
            Layout.minimumHeight: 40
            Layout.alignment: Qt.AlignVCenter
            text: "A " + Math.round(root.currentFontSize) + " px"
            icon.name: "draw-text"
            icon.width: Kirigami.Units.iconSizes.smallMedium
            icon.height: Kirigami.Units.iconSizes.smallMedium
            display: AbstractButton.TextBesideIcon
            highlighted: fontPopover.visible
            onClicked: {
                if (fontPopover.visible) fontPopover.close(); else fontPopover.open();
                lineSpacingPopover.close();
                layoutPopover.close();
            }
        }

        // Line Spacing button (triggers lineSpacingPopover)
        ToolButton {
            id: spacingButton
            Layout.preferredHeight: Math.max(40, Kirigami.Units.gridUnit * 2)
            Layout.minimumHeight: 40
            Layout.alignment: Qt.AlignVCenter
            text: Number(root.currentLineSpacing).toFixed(1) + "×"
            icon.name: "format-line-spacing-normal"
            icon.width: Kirigami.Units.iconSizes.smallMedium
            icon.height: Kirigami.Units.iconSizes.smallMedium
            display: AbstractButton.TextBesideIcon
            highlighted: lineSpacingPopover.visible
            onClicked: {
                if (lineSpacingPopover.visible) lineSpacingPopover.close(); else lineSpacingPopover.open();
                fontPopover.close();
                layoutPopover.close();
            }
        }

        // Paragraph Layout button (triggers layoutPopover)
        ToolButton {
            id: layoutButton
            Layout.preferredHeight: Math.max(40, Kirigami.Units.gridUnit * 2)
            Layout.minimumHeight: 40
            Layout.alignment: Qt.AlignVCenter
            text: qsTr("段落")
            icon.name: "format-justify-left"
            icon.width: Kirigami.Units.iconSizes.smallMedium
            icon.height: Kirigami.Units.iconSizes.smallMedium
            display: AbstractButton.TextBesideIcon
            highlighted: layoutPopover.visible
            onClicked: {
                if (layoutPopover.visible) layoutPopover.close(); else layoutPopover.open();
                fontPopover.close();
                lineSpacingPopover.close();
            }
        }

        // Format button
        ToolButton {
            Layout.preferredHeight: Math.max(40, Kirigami.Units.gridUnit * 2)
            Layout.minimumHeight: 40
            Layout.alignment: Qt.AlignVCenter
            text: qsTr("一键排版")
            icon.name: "format-text-bold"
            icon.width: Kirigami.Units.iconSizes.smallMedium
            icon.height: Kirigami.Units.iconSizes.smallMedium
            display: AbstractButton.TextBesideIcon
            onClicked: root.formatOneClick()
        }

        Item { Layout.fillWidth: true }

        // Save status
        Label {
            text: root.saveStatus || ""
            color: Kirigami.Theme.disabledTextColor
            visible: text !== ""
            elide: Text.ElideRight
            Layout.alignment: Qt.AlignVCenter
        }
    }

    // === Font Size Popover ===
    Popup {
        id: fontPopover
        y: root.height + Kirigami.Units.smallSpacing
        x: 60
        width: Kirigami.Units.gridUnit * 12
        padding: Kirigami.Units.largeSpacing
        closePolicy: Popup.CloseOnPressOutside | Popup.CloseOnEscape

        contentItem: ColumnLayout {
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Heading {
                level: 4
                text: qsTr("字号")
            }

            // Quick presets
            Flow {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Repeater {
                    model: [12, 14, 16, 18, 20, 24]

                    Button {
                        implicitWidth: Kirigami.Units.gridUnit * 2
                        text: modelData
                        flat: true
                        highlighted: Math.round(root.currentFontSize) === modelData
                        onClicked: {
                            root.fontSizeChanged(modelData)
                            fontPopover.close()
                        }
                    }
                }
            }

            // Slider
            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Label {
                    text: "10"
                    color: Kirigami.Theme.disabledTextColor
                }

                Slider {
                    id: fontSlider
                    Layout.fillWidth: true
                    from: root.minFontSize
                    to: root.maxFontSize
                    stepSize: 1
                    value: root.currentFontSize
                    onMoved: root.fontSizeChanged(value)
                }

                Label {
                    text: "72"
                    color: Kirigami.Theme.disabledTextColor
                }
            }

            RowLayout {
                Layout.alignment: Qt.AlignHCenter
                spacing: Kirigami.Units.smallSpacing

                TextField {
                    id: fontSizeInput
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 4
                    text: Math.round(root.currentFontSize).toString()
                    horizontalAlignment: TextInput.AlignHCenter
                    selectByMouse: true
                    inputMethodHints: Qt.ImhDigitsOnly
                    validator: IntValidator { bottom: root.minFontSize; top: root.maxFontSize }
                    onTextEdited: root.commitFontSizeInput(false)
                    onAccepted: root.commitFontSizeInput(true)
                    onEditingFinished: root.commitFontSizeInput(true)
                }

                Label {
                    text: "px"
                    color: Kirigami.Theme.disabledTextColor
                    Layout.alignment: Qt.AlignVCenter
                }
            }
        }
    }

    // === Line Spacing Popover ===
    Popup {
        id: lineSpacingPopover
        y: root.height + Kirigami.Units.smallSpacing
        x: 100
        width: Kirigami.Units.gridUnit * 12
        padding: Kirigami.Units.largeSpacing
        closePolicy: Popup.CloseOnPressOutside | Popup.CloseOnEscape

        contentItem: ColumnLayout {
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Heading {
                level: 4
                text: qsTr("行距倍数")
            }

            // Quick presets
            Flow {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Repeater {
                    model: [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0]

                    Button {
                        implicitWidth: Kirigami.Units.gridUnit * 2
                        text: Number(modelData).toFixed(2).replace(/\.00$/, "").replace(/(\.\d)0$/, "$1")
                        flat: true
                        highlighted: Math.abs(root.currentLineSpacing - modelData) < 0.01
                        onClicked: {
                            root.lineSpacingChanged(modelData)
                            lineSpacingPopover.close()
                        }
                    }
                }
            }

            // Slider
            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Label {
                    text: "1.0"
                    color: Kirigami.Theme.disabledTextColor
                }

                Slider {
                    id: lineSpacingSlider
                    Layout.fillWidth: true
                    from: 1.0
                    to: 3.0
                    stepSize: 0.1
                    value: root.currentLineSpacing
                    onMoved: root.lineSpacingChanged(value)
                }

                Label {
                    text: "3.0"
                    color: Kirigami.Theme.disabledTextColor
                }
            }

            Label {
                text: Number(root.currentLineSpacing).toFixed(1) + " x"
                color: Kirigami.Theme.disabledTextColor
                Layout.alignment: Qt.AlignHCenter
            }
        }
    }

    // === Layout Popover ===
    Popup {
        id: layoutPopover
        y: root.height + Kirigami.Units.smallSpacing
        x: 180
        width: Kirigami.Units.gridUnit * 14
        padding: Kirigami.Units.largeSpacing
        closePolicy: Popup.CloseOnPressOutside | Popup.CloseOnEscape

        contentItem: ColumnLayout {
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Heading {
                level: 4
                text: qsTr("段落设置")
            }

            // Issue #833：删除编辑器绝对宽度 widthSlider UI 及对
            // setting_desktop_editor_width 的写入。正文区宽度属于布局，
            // 不应再作为格式工具条里的用户设置去反向控制 Workbench 几何。

            // Divider
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: Kirigami.Theme.disabledTextColor
                opacity: 0.3
            }

            // First line indent
            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Column {
                    Layout.fillWidth: true
                    spacing: 2
                    Label {
                        text: qsTr("首行缩进")
                        color: Kirigami.Theme.textColor
                    }
                    Label {
                        text: qsTr("段落开头缩进两个字符")
                        color: Kirigami.Theme.disabledTextColor
                    }
                }

                Switch {
                    checked: root.firstLineIndent
                    onToggled: root.firstLineIndentToggled()
                }
            }
        }
    }
}
