// =============================================================================
// ProjectCard.qml — 作品卡片组件
// =============================================================================
//
// 层级：Linux UI 层（QML UI 组件）
// 职责：单个作品的卡片展示（标题、字数、今日输入、同步状态）
// 约束：
//   - 纯展示组件，数据通过 property 传入
//   - 点击和右键菜单通过 signal 传递给 ProjectHomePage
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

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
    radius: dt ? dt.radiusLg : 16
    color: hovered ? (dt ? dt.surfaceContainer : "#F0F3F7") : (dt ? dt.card : "#F6F8FB")
    border.color: hovered ? (dt ? dt.primary : "#006497") : (dt ? dt.border : "#CBD5E1")
    border.width: 1

    property bool hovered: false

    Behavior on color { ColorAnimation { duration: dt ? dt.animFast : 120 } }
    Behavior on border.color { ColorAnimation { duration: dt ? dt.animFast : 120 } }

    MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton
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

            StatusPill {
                theme: dt
                status: root.syncStatus === "success" ? "success" : (root.syncStatus === "syncing" ? "warning" : (root.syncStatus === "error" ? "error" : "info"))
                text: ""
                visible: root.syncStatus !== "none"
            }
        }

        // Title
        AppText {
            Layout.fillWidth: true
            Layout.topMargin: dt ? dt.sp12 : 12
            text: root.title || qsTr("未命名作品")
            color: dt ? dt.textPrimary : "#E2E4E9"
            font.pixelSize: dt ? dt.subtitle : 18
            font.family: dt ? dt.fontFamily : "sans-serif"
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
                AppText {
                    text: root.wordCount >= 10000 ? (root.wordCount / 10000).toFixed(1) + "w" : root.wordCount.toLocaleString()
                    color: dt ? dt.textPrimary : "#E2E4E9"
                    font.pixelSize: dt ? dt.body : 14
                    font.family: dt ? dt.fontFamily : "sans-serif"
                    font.weight: Font.Medium
                }
                AppText {
                    text: qsTr("总字数")
                    color: dt ? dt.textMuted : "#606470"
                    font.pixelSize: dt ? dt.caption : 12
                    font.family: dt ? dt.fontFamily : "sans-serif"
                }
            }

            Column {
                spacing: 2
                visible: root.todayInput > 0
                AppText {
                    text: "+" + (root.todayInput >= 1000 ? (root.todayInput / 1000).toFixed(1) + "k" : root.todayInput.toLocaleString())
                    color: dt ? dt.primary : "#006497"
                    font.pixelSize: dt ? dt.body : 14
                    font.family: dt ? dt.fontFamily : "sans-serif"
                    font.weight: Font.Medium
                }
                AppText {
                    text: qsTr("今日")
                    color: dt ? dt.textMuted : "#606470"
                    font.pixelSize: dt ? dt.caption : 12
                    font.family: dt ? dt.fontFamily : "sans-serif"
                }
            }

            Item { Layout.fillWidth: true }

            AppText {
                text: root.lastEdited || ""
                color: dt ? dt.textMuted : "#606470"
                font.pixelSize: dt ? dt.caption : 12
                font.family: dt ? dt.fontFamily : "sans-serif"
                visible: text !== ""
            }
        }
    }
}
