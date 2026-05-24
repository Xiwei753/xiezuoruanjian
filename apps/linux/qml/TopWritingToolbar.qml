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

    signal fontSizeChanged(real size)
    signal lineSpacingChanged(real spacing)
    signal firstLineIndentToggled()
    signal formatOneClick()
    signal linkToStarMap()

    color: dt ? dt.surface : "#1A1D23"
    height: 44

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: dt ? dt.sp16 : 16
        anchors.rightMargin: dt ? dt.sp16 : 16
        spacing: dt ? dt.sp4 : 4

        // Font size
        RowLayout {
            spacing: dt ? dt.sp4 : 4
            Text {
                text: "字号"
                color: dt ? dt.textMuted : "#606470"
                font.pixelSize: dt ? dt.fontXs : 11
            }
            Repeater {
                model: [12, 14, 16, 18, 20, 24]
                Rectangle {
                    width: 28; height: 24
                    radius: dt ? dt.radiusSm : 8
                    color: root.currentFontSize === modelData ?
                           (dt ? dt.accentSoft : "rgba(123,140,222,0.12)") :
                           "transparent"

                    Text {
                        anchors.centerIn: parent
                        text: modelData
                        color: root.currentFontSize === modelData ?
                               (dt ? dt.accentText : "#3D4D9E") :
                               (dt ? dt.textSecondary : "#5C6070")
                        font.pixelSize: dt ? dt.fontXs : 11
                        font.weight: root.currentFontSize === modelData ? Font.DemiBold : Font.Normal
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.fontSizeChanged(modelData)
                    }
                }
            }

            Item { Layout.preferredWidth: dt ? dt.sp12 : 12 }

            // Line spacing
            Text {
                text: "行距"
                color: dt ? dt.textMuted : "#606470"
                font.pixelSize: dt ? dt.fontXs : 11
            }
            Repeater {
                model: [1.25, 1.5, 1.75, 2.0]
                Rectangle {
                    width: 32; height: 24
                    radius: dt ? dt.radiusSm : 8
                    color: Math.abs(root.currentLineSpacing - modelData) < 0.01 ?
                           (dt ? dt.accentSoft : "rgba(123,140,222,0.12)") :
                           "transparent"

                    Text {
                        anchors.centerIn: parent
                        text: modelData.toFixed(2)
                        color: Math.abs(root.currentLineSpacing - modelData) < 0.01 ?
                               (dt ? dt.accentText : "#3D4D9E") :
                               (dt ? dt.textSecondary : "#5C6070")
                        font.pixelSize: dt ? dt.fontXs : 11
                        font.weight: Math.abs(root.currentLineSpacing - modelData) < 0.01 ? Font.DemiBold : Font.Normal
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.lineSpacingChanged(modelData)
                    }
                }
            }

            Item { Layout.preferredWidth: dt ? dt.sp12 : 12 }

            // First line indent toggle
            Rectangle {
                width: indentRow.implicitWidth + (dt ? dt.sp12 : 12)
                height: 24
                radius: dt ? dt.radiusSm : 8
                color: root.firstLineIndent ?
                       (dt ? dt.accentSoft : "rgba(123,140,222,0.12)") :
                       "transparent"
                border.color: root.firstLineIndent ? (dt ? dt.accent : "#7B8CDE") : (dt ? dt.border : "#2A2E36")
                border.width: 1

                Row {
                    id: indentRow
                    anchors.centerIn: parent
                    spacing: dt ? dt.sp4 : 4
                    Text {
                        text: "\u21A9"
                        color: root.firstLineIndent ? (dt ? dt.accentText : "#3D4D9E") : (dt ? dt.textSecondary : "#5C6070")
                        font.pixelSize: dt ? dt.fontSm : 12
                        anchors.verticalCenter: parent.verticalCenter
                    }
                    Text {
                        text: "首行缩进"
                        color: root.firstLineIndent ? (dt ? dt.accentText : "#3D4D9E") : (dt ? dt.textSecondary : "#5C6070")
                        font.pixelSize: dt ? dt.fontXs : 11
                        anchors.verticalCenter: parent.verticalCenter
                    }
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.firstLineIndentToggled()
                }
            }
        }

        Item { Layout.fillWidth: true }

        // Right side buttons
        RowLayout {
            spacing: dt ? dt.sp8 : 8

            // Format button (placeholder)
            Rectangle {
                width: formatRow.implicitWidth + (dt ? dt.sp12 : 12)
                height: 28
                radius: dt ? dt.radiusSm : 8
                color: "transparent"
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
                        font.pixelSize: dt ? dt.fontXs : 11
                        anchors.verticalCenter: parent.verticalCenter
                    }
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.formatOneClick()
                }
            }

            // Link to star map (placeholder)
            Rectangle {
                width: starRow.implicitWidth + (dt ? dt.sp12 : 12)
                height: 28
                radius: dt ? dt.radiusSm : 8
                color: "transparent"
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
                        font.pixelSize: dt ? dt.fontXs : 11
                        anchors.verticalCenter: parent.verticalCenter
                    }
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.linkToStarMap()
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
    }
}
