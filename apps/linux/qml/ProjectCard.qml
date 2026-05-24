import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    property var dt: null
    property string projectId: ""
    property string title: ""
    property int wordCount: 0
    property int todayInput: 0
    property string lastEdited: ""
    property string syncStatus: "none"
    property string accentColor: "#7B8CDE"

    signal clicked()
    signal rightClicked()

    width: 220
    height: 180
    radius: dt ? dt.radiusCard : 18
    color: hovered ? (dt ? dt.cardHover : "#22262E") : (dt ? dt.card : "#1E2128")
    border.color: hovered ? (dt ? dt.border : "#2A2E36") : "transparent"
    border.width: hovered ? 1 : 0

    property bool hovered: false

    Behavior on color { ColorAnimation { duration: dt ? dt.animFast : 120 } }
    Behavior on border.color { ColorAnimation { duration: dt ? dt.animFast : 120 } }

    MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        onContainsMouseChanged: root.hovered = containsMouse
        onClicked: function(mouse) {
            if (mouse.button === Qt.LeftButton) root.clicked();
            else if (mouse.button === Qt.RightButton) root.rightClicked();
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: dt ? dt.sp20 : 20
        spacing: 0

        // Accent dot + sync status
        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            Rectangle {
                width: 8; height: 8
                radius: 4
                color: root.accentColor
                opacity: 0.8
            }

            Item { Layout.fillWidth: true }

            Rectangle {
                width: 7; height: 7
                radius: 4
                color: root.syncStatus === "success" ? (dt ? dt.success : "#5CB880") :
                       root.syncStatus === "syncing" ? (dt ? dt.warning : "#E0A840") :
                       root.syncStatus === "error" ? (dt ? dt.danger : "#E06060") :
                       (dt ? dt.textMuted : "#606470")
                visible: root.syncStatus !== "none"
            }
        }

        // Title
        Text {
            Layout.fillWidth: true
            Layout.topMargin: dt ? dt.sp12 : 12
            text: root.title || "未命名作品"
            color: dt ? dt.textPrimary : "#E2E4E9"
            font.pixelSize: dt ? dt.fontLg : 16
            font.weight: Font.DemiBold
            elide: Text.ElideRight
            maximumLineCount: 2
            wrapMode: Text.Wrap
        }

        Item { Layout.fillHeight: true }

        // Stats row
        RowLayout {
            Layout.fillWidth: true
            spacing: dt ? dt.sp16 : 16

            Column {
                spacing: 2
                Text {
                    text: root.wordCount >= 10000 ? (root.wordCount / 10000).toFixed(1) + "w" : root.wordCount.toLocaleString()
                    color: dt ? dt.textPrimary : "#E2E4E9"
                    font.pixelSize: dt ? dt.fontMd : 14
                    font.weight: Font.Medium
                }
                Text {
                    text: "总字数"
                    color: dt ? dt.textMuted : "#606470"
                    font.pixelSize: dt ? dt.fontXs : 11
                }
            }

            Column {
                spacing: 2
                visible: root.todayInput > 0
                Text {
                    text: "+" + (root.todayInput >= 1000 ? (root.todayInput / 1000).toFixed(1) + "k" : root.todayInput.toLocaleString())
                    color: dt ? dt.accent : "#7B8CDE"
                    font.pixelSize: dt ? dt.fontMd : 14
                    font.weight: Font.Medium
                }
                Text {
                    text: "今日"
                    color: dt ? dt.textMuted : "#606470"
                    font.pixelSize: dt ? dt.fontXs : 11
                }
            }

            Item { Layout.fillWidth: true }

            Text {
                text: root.lastEdited || ""
                color: dt ? dt.textMuted : "#606470"
                font.pixelSize: dt ? dt.fontXs : 11
                visible: text !== ""
            }
        }
    }
}
