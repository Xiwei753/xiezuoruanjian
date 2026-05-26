import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

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

    color: dt ? dt.surface : "#1A1D23"
    height: 44

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: dt ? dt.sp16 : 16
        anchors.rightMargin: dt ? dt.sp16 : 16
        spacing: dt ? dt.sp4 : 4

        // Font button (triggers popover)
        Rectangle {
            width: fontRow.implicitWidth + (dt ? dt.sp12 : 12)
            height: 32
            radius: dt ? dt.radiusSm : 8
            color: fontPopover.visible || fontHover.containsMouse ?
                   (dt ? dt.accentSoft : "rgba(123,140,222,0.12)") : "transparent"

            Row {
                id: fontRow
                anchors.centerIn: parent
                spacing: dt ? dt.sp4 : 4
                Text {
                    text: "A"
                    color: dt ? dt.textSecondary : "#5C6070"
                    font.pixelSize: dt ? dt.fontSm : 12
                    font.weight: Font.Bold
                    anchors.verticalCenter: parent.verticalCenter
                }
                Text {
                    text: Math.round(root.currentFontSize) + "px"
                    color: dt ? dt.textPrimary : "#E2E4E9"
                    font.pixelSize: dt ? dt.fontSm : 12
                    anchors.verticalCenter: parent.verticalCenter
                }
                Text {
                    text: "\u25BE"
                    color: dt ? dt.textMuted : "#606470"
                    font.pixelSize: dt ? dt.fontXs : 11
                    anchors.verticalCenter: parent.verticalCenter
                }
            }

            MouseArea {
                id: fontHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                    fontPopover.visible = !fontPopover.visible;
                    layoutPopover.visible = false;
                }
            }
        }

        // Layout button (triggers popover)
        Rectangle {
            width: layoutRow.implicitWidth + (dt ? dt.sp12 : 12)
            height: 32
            radius: dt ? dt.radiusSm : 8
            color: layoutPopover.visible || layoutHover.containsMouse ?
                   (dt ? dt.accentSoft : "rgba(123,140,222,0.12)") : "transparent"

            Row {
                id: layoutRow
                anchors.centerIn: parent
                spacing: dt ? dt.sp4 : 4
                Text {
                    text: "\u2630"
                    color: dt ? dt.textSecondary : "#5C6070"
                    font.pixelSize: dt ? dt.fontSm : 12
                    anchors.verticalCenter: parent.verticalCenter
                }
                Text {
                    text: "排版设置"
                    color: dt ? dt.textPrimary : "#E2E4E9"
                    font.pixelSize: dt ? dt.fontSm : 12
                    anchors.verticalCenter: parent.verticalCenter
                }
                Text {
                    text: "\u25BE"
                    color: dt ? dt.textMuted : "#606470"
                    font.pixelSize: dt ? dt.fontXs : 11
                    anchors.verticalCenter: parent.verticalCenter
                }
            }

            MouseArea {
                id: layoutHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                    layoutPopover.visible = !layoutPopover.visible;
                    fontPopover.visible = false;
                }
            }
        }

        // Format button
        Rectangle {
            width: formatRow.implicitWidth + (dt ? dt.sp12 : 12)
            height: 32
            radius: dt ? dt.radiusSm : 8
            color: formatHover.containsMouse ? (dt ? dt.cardHover : "#22262E") : "transparent"
            border.color: dt ? dt.border : "#2A2E36"
            border.width: 1

            Row {
                id: formatRow
                anchors.centerIn: parent
                spacing: dt ? dt.sp4 : 4
                Text {
                    text: "\u2728"
                    font.pixelSize: dt ? dt.fontSm : 12
                    anchors.verticalCenter: parent.verticalCenter
                }
                Text {
                    text: "一键排版"
                    color: dt ? dt.textSecondary : "#5C6070"
                    font.pixelSize: dt ? dt.fontSm : 12
                    anchors.verticalCenter: parent.verticalCenter
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
            radius: dt ? dt.radiusSm : 8
            color: starHover.containsMouse ? (dt ? dt.cardHover : "#22262E") : "transparent"
            border.color: dt ? dt.border : "#2A2E36"
            border.width: 1

            Row {
                id: starRow
                anchors.centerIn: parent
                spacing: dt ? dt.sp4 : 4
                Text {
                    text: "\u2B50"
                    font.pixelSize: dt ? dt.fontSm : 12
                    anchors.verticalCenter: parent.verticalCenter
                }
                Text {
                    text: "星图"
                    color: dt ? dt.textSecondary : "#5C6070"
                    font.pixelSize: dt ? dt.fontSm : 12
                    anchors.verticalCenter: parent.verticalCenter
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
            radius: dt ? dt.radiusSm : 8
            color: statsHover.containsMouse ? (dt ? dt.cardHover : "#22262E") : "transparent"
            border.color: dt ? dt.border : "#2A2E36"
            border.width: 1

            Row {
                id: statsRow
                anchors.centerIn: parent
                spacing: dt ? dt.sp4 : 4
                Text {
                    text: "\uD83D\uDCC8"
                    font.pixelSize: dt ? dt.fontSm : 12
                    anchors.verticalCenter: parent.verticalCenter
                }
                Text {
                    text: "统计"
                    color: dt ? dt.textSecondary : "#5C6070"
                    font.pixelSize: dt ? dt.fontSm : 12
                    anchors.verticalCenter: parent.verticalCenter
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
            font.pixelSize: dt ? dt.fontXs : 11
            visible: text !== ""
        }
    }

    // === Font Size Popover ===
    Rectangle {
        id: fontPopover
        visible: false
        width: 200
        height: fontPopoverCol.implicitHeight + (dt ? dt.sp24 : 24)
        radius: dt ? dt.radiusPanel : 22
        color: dt ? dt.surface : "#1A1D23"
        border.color: dt ? dt.border : "#2A2E36"
        border.width: 1
        anchors.top: parent.bottom
        anchors.topMargin: dt ? dt.sp8 : 8
        anchors.left: parent.left
        anchors.leftMargin: 60
        z: 100

        Behavior on visible { NumberAnimation { duration: dt ? dt.animFast : 120 } }

        ColumnLayout {
            id: fontPopoverCol
            anchors.fill: parent
            anchors.margins: dt ? dt.sp12 : 12
            spacing: dt ? dt.sp12 : 12

            Text {
                text: "字号"
                color: dt ? dt.textPrimary : "#E2E4E9"
                font.pixelSize: dt ? dt.fontMd : 14
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
                        radius: dt ? dt.radiusSm : 8
                        color: Math.round(root.currentFontSize) === modelData ?
                               (dt ? dt.accentSoft : "rgba(123,140,222,0.12)") :
                               presetHover.containsMouse ? (dt ? dt.card : "#1E2128") : "transparent"

                        Text {
                            anchors.centerIn: parent
                            text: modelData
                            color: Math.round(root.currentFontSize) === modelData ?
                                   (dt ? dt.accentText : "#3D4D9E") :
                                   (dt ? dt.textSecondary : "#5C6070")
                            font.pixelSize: dt ? dt.fontSm : 12
                            font.weight: Math.round(root.currentFontSize) === modelData ? Font.DemiBold : Font.Normal
                        }

                        MouseArea {
                            id: presetHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.fontSizeChanged(modelData)
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

                Slider {
                    id: fontSlider
                    Layout.fillWidth: true
                    from: 10
                    to: 40
                    stepSize: 1
                    value: root.currentFontSize
                    onMoved: root.fontSizeChanged(value)

                    background: Rectangle {
                        x: fontSlider.leftPadding
                        y: fontSlider.topPadding + fontSlider.availableHeight / 2 - height / 2
                        width: fontSlider.availableWidth
                        height: 4
                        radius: 2
                        color: dt ? dt.border : "#2A2E36"

                        Rectangle {
                            width: fontSlider.visualPosition * parent.width
                            height: parent.height
                            radius: 2
                            color: dt ? dt.accent : "#7B8CDE"
                        }
                    }

                    handle: Rectangle {
                        x: fontSlider.leftPadding + fontSlider.visualPosition * (fontSlider.availableWidth - width)
                        y: fontSlider.topPadding + fontSlider.availableHeight / 2 - height / 2
                        width: 16; height: 16
                        radius: 8
                        color: dt ? dt.accent : "#7B8CDE"
                        border.color: dt ? dt.surface : "#1A1D23"
                        border.width: 2
                    }
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

        // Close on outside click
        MouseArea {
            anchors.fill: parent
            propagateComposedEvents: true
            onPressed: mouse.accepted = false
        }
    }

    // === Layout Popover ===
    Rectangle {
        id: layoutPopover
        visible: false
        width: 220
        height: layoutPopoverCol.implicitHeight + (dt ? dt.sp24 : 24)
        radius: dt ? dt.radiusPanel : 22
        color: dt ? dt.surface : "#1A1D23"
        border.color: dt ? dt.border : "#2A2E36"
        border.width: 1
        anchors.top: parent.bottom
        anchors.topMargin: dt ? dt.sp8 : 8
        anchors.left: parent.left
        anchors.leftMargin: 110
        z: 100

        Behavior on visible { NumberAnimation { duration: dt ? dt.animFast : 120 } }

        ColumnLayout {
            id: layoutPopoverCol
            anchors.fill: parent
            anchors.margins: dt ? dt.sp12 : 12
            spacing: dt ? dt.sp12 : 12

            Text {
                text: "排版设置"
                color: dt ? dt.textPrimary : "#E2E4E9"
                font.pixelSize: dt ? dt.fontMd : 14
                font.weight: Font.DemiBold
            }

            // Line spacing
            ColumnLayout {
                spacing: dt ? dt.sp6 : 6
                Text {
                    text: "行距"
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
                            radius: dt ? dt.radiusSm : 8
                            color: Math.abs(root.currentLineSpacing - modelData.value) < 0.01 ?
                                   (dt ? dt.accentSoft : "rgba(123,140,222,0.12)") :
                                   lsHover.containsMouse ? (dt ? dt.card : "#1E2128") : "transparent"

                            Text {
                                anchors.centerIn: parent
                                text: modelData.label
                                color: Math.abs(root.currentLineSpacing - modelData.value) < 0.01 ?
                                       (dt ? dt.accentText : "#3D4D9E") :
                                       (dt ? dt.textSecondary : "#5C6070")
                                font.pixelSize: dt ? dt.fontSm : 12
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
                        text: "首行缩进"
                        color: dt ? dt.textPrimary : "#E2E4E9"
                        font.pixelSize: dt ? dt.fontMd : 14
                    }
                    Text {
                        text: "段落开头缩进两个字符"
                        color: dt ? dt.textMuted : "#606470"
                        font.pixelSize: dt ? dt.fontXs : 11
                    }
                }

                Switch {
                    checked: root.firstLineIndent
                    onToggled: root.firstLineIndentToggled()
                }
            }
        }

        MouseArea {
            anchors.fill: parent
            propagateComposedEvents: true
            onPressed: mouse.accepted = false
        }
    }

    // Close popovers on outside click
    Connections {
        target: window
        function onActiveChanged() {
            if (!window.active) {
                fontPopover.visible = false;
                layoutPopover.visible = false;
            }
        }
    }
}
