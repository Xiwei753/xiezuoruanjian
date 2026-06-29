// =============================================================================
// ProjectCard.qml — 作品卡片组件
// =============================================================================
//
// 层级：Desktop UI 层（QML UI 组件）
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

    // Safe access: fallback to light-theme defaults when dt is null
    FallbackTokens { id: _fallback }
    readonly property var _dt: dt ?? _fallback

    property string projectId: ""
    property string title: ""
    property int wordCount: 0
    property int todayInput: 0
    property string lastEdited: ""
    property string syncStatus: "none"
    property string accentColor: _dt.primary.toString()

    signal clicked()
    signal rightClicked()

    width: 220
    height: 180
    radius: _dt.cardRadius
    color: hovered ? _dt.cardHover : _dt.card
    border.color: hovered ? _dt.primary : _dt.border
    border.width: 1

    property bool hovered: false

    Behavior on color { ColorAnimation { duration: _dt.animFast } }
    Behavior on border.color { ColorAnimation { duration: _dt.animFast } }

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
        anchors.margins: _dt.sp20
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
                dt: root.dt
                status: root.syncStatus === "success" ? "success" : (root.syncStatus === "syncing" ? "warning" : (root.syncStatus === "error" ? "error" : "info"))
                text: ""
                visible: root.syncStatus !== "none"
            }
        }

        // Title
        AppText {
            Layout.fillWidth: true
            Layout.topMargin: _dt.sp12
            text: root.title || qsTr("未命名作品")
            color: _dt.textPrimary
            font.pixelSize: _dt.subtitle
            font.family: _dt.fontFamily
            font.weight: Font.DemiBold
            elide: Text.ElideRight
            maximumLineCount: 2
            wrapMode: Text.Wrap
        }

        Item { Layout.fillHeight: true }

        // Stats row
        RowLayout {
            Layout.fillWidth: true
            spacing: _dt.sp16

            Column {
                spacing: 2
                AppText {
                    text: root.wordCount >= 10000 ? (root.wordCount / 10000).toFixed(1) + "w" : root.wordCount.toLocaleString()
                    color: _dt.textPrimary
                    font.pixelSize: _dt.body
                    font.family: _dt.fontFamily
                    font.weight: Font.Medium
                }
                AppText {
                    text: qsTr("总字数")
                    color: _dt.textMuted
                    font.pixelSize: _dt.caption
                    font.family: _dt.fontFamily
                }
            }

            Column {
                spacing: 2
                visible: root.todayInput > 0
                AppText {
                    text: "+" + (root.todayInput >= 1000 ? (root.todayInput / 1000).toFixed(1) + "k" : root.todayInput.toLocaleString())
                    color: _dt.primary
                    font.pixelSize: _dt.body
                    font.family: _dt.fontFamily
                    font.weight: Font.Medium
                }
                AppText {
                    text: qsTr("今日")
                    color: _dt.textMuted
                    font.pixelSize: _dt.caption
                    font.family: _dt.fontFamily
                }
            }

            Item { Layout.fillWidth: true }

            AppText {
                text: root.lastEdited || ""
                color: _dt.textMuted
                font.pixelSize: _dt.caption
                font.family: _dt.fontFamily
                visible: text !== ""
            }
        }
    }
}
