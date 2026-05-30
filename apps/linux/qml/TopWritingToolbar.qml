// =============================================================================
// TopWritingToolbar.qml — 写作工具栏
// =============================================================================
//
// 层级：Linux UI 层（QML UI 组件）
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
    property real currentFontSize: 16
    property real currentLineSpacing: 1.5
    property bool firstLineIndent: false
    property string saveStatus: ""
    property string currentProjectId: ""

    signal fontSizeChanged(real size)
    signal lineSpacingChanged(real spacing)
    signal firstLineIndentToggled()
    signal formatOneClick()
    signal linkToStarMap()
    signal openStats()

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
                Text {
                    text: "A"
                    color: dt ? dt.onSurfaceVariant : "#42474E"
                    font.pixelSize: dt ? dt.label : 13
                    font.family: dt ? dt.fontFamily : "sans-serif"
                    font.weight: Font.Bold
                }
                Text {
                    text: Math.round(root.currentFontSize) + "px"
                    color: dt ? dt.textPrimary : "#E2E2E5"
                    font.pixelSize: dt ? dt.label : 13
                    font.family: dt ? dt.fontFamily : "sans-serif"
                }
                Text {
                    text: "\u25BE"
                    color: dt ? dt.onSurfaceVariant : "#42474E"
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
                    layoutPopover.close();
                }
            }
        }

        // Layout button (triggers popover)
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
                Text {
                    text: "\u2630"
                    color: dt ? dt.onSurfaceVariant : "#42474E"
                    font.pixelSize: dt ? dt.label : 13
                    font.family: dt ? dt.fontFamily : "sans-serif"
                }
                Text {
                    text: qsTr("排版设置")
                    color: dt ? dt.textPrimary : "#E2E2E5"
                    font.pixelSize: dt ? dt.label : 13
                    font.family: dt ? dt.fontFamily : "sans-serif"
                }
                Text {
                    text: "\u25BE"
                    color: dt ? dt.onSurfaceVariant : "#42474E"
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
                }
            }
        }

        // Format button
        Rectangle {
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
                Text {
                    text: qsTr("一键排版")
                    color: dt ? dt.onSurfaceVariant : "#42474E"
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
                Text {
                    text: qsTr("星图")
                    color: dt ? dt.onSurfaceVariant : "#42474E"
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
                Text {
                    text: qsTr("统计")
                    color: dt ? dt.onSurfaceVariant : "#42474E"
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
        Text {
            text: root.saveStatus || ""
            color: dt ? dt.textMuted : "#606470"
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

            Text {
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

                        Text {
                            anchors.centerIn: parent
                            text: modelData
                            color: Math.round(root.currentFontSize) === modelData ?
                                   (dt ? dt.onPrimaryContainer : "#001E31") :
                                   (dt ? dt.onSurfaceVariant : "#42474E")
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

                Text {
                    text: "10"
                    color: dt ? dt.textMuted : "#606470"
                    font.pixelSize: dt ? dt.fontXs : 11
                }

                AppSlider {
                    id: fontSlider
                    Layout.fillWidth: true
                    theme: dt
                    from: 10
                    to: 40
                    stepSize: 1
                    value: root.currentFontSize
                    onMoved: root.fontSizeChanged(value)
                }

                Text {
                    text: "40"
                    color: dt ? dt.textMuted : "#606470"
                    font.pixelSize: dt ? dt.fontXs : 11
                }
            }

            Text {
                text: Math.round(root.currentFontSize) + " px"
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
        x: 110
        width: 220
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

            Text {
                text: qsTr("排版设置")
                color: dt ? dt.textPrimary : "#E2E4E9"
                font.pixelSize: dt ? dt.subtitle : 18
                font.family: dt ? dt.fontFamily : "sans-serif"
                font.weight: Font.DemiBold
            }

            // Line spacing
            ColumnLayout {
                spacing: dt ? dt.sp6 : 6
                Text {
                    text: qsTr("行距")
                    color: dt ? dt.textSecondary : "#9CA0AB"
                    font.pixelSize: dt ? dt.fontSm : 12
                }

                Flow {
                    Layout.fillWidth: true
                    spacing: dt ? dt.sp6 : 6

                    Repeater {
                        model: [
                            { label: "1.25", value: 1.25 },
                            { label: "1.5", value: 1.5 },
                            { label: "1.75", value: 1.75 },
                            { label: "2.0", value: 2.0 }
                        ]

                        Rectangle {
                            width: 44; height: 32
                            radius: dt ? dt.radiusPill : 999
                            color: Math.abs(root.currentLineSpacing - modelData.value) < 0.01 ?
                                   (dt ? dt.primaryContainer : "#CCE5FF") :
                                   lsHover.containsMouse ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"

                            Text {
                                anchors.centerIn: parent
                                text: modelData.label
                                color: Math.abs(root.currentLineSpacing - modelData.value) < 0.01 ?
                                       (dt ? dt.onPrimaryContainer : "#001E31") :
                                       (dt ? dt.onSurfaceVariant : "#42474E")
                                font.pixelSize: dt ? dt.label : 13
                                font.family: dt ? dt.fontFamily : "sans-serif"
                                font.weight: Math.abs(root.currentLineSpacing - modelData.value) < 0.01 ? Font.DemiBold : Font.Normal
                            }

                            MouseArea {
                                id: lsHover
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.lineSpacingChanged(modelData.value)
                            }
                        }
                    }
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
                    Text {
                        text: qsTr("首行缩进")
                        color: dt ? dt.textPrimary : "#E2E2E5"
                        font.pixelSize: dt ? dt.fontMd : 14
                    }
                    Text {
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
