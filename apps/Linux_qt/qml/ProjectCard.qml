// =============================================================================
// ProjectCard.qml — 作品卡片组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
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

    // Elevation shadow support
    property int elevation: 1
    property var appShadow: null

    readonly property color _primary: dt.primary
    readonly property color _card: dt.card
    readonly property color _cardHover: dt.cardHover
    readonly property color _border: dt.border
    readonly property color _textPrimary: dt.textPrimary
    readonly property color _textMuted: dt.textMuted
    readonly property int _cardRadius: dt.cardRadius
    readonly property int _sp12: dt.sp12
    readonly property int _sp16: dt.sp16
    readonly property int _sp20: dt.sp20
    readonly property int _subtitle: dt.subtitle
    readonly property int _body: dt.body
    readonly property int _caption: dt.caption
    readonly property string _fontFamily: dt.fontFamily
    readonly property int _animFast: dt.animFast

    property string projectId: ""
    property string title: ""
    property int wordCount: 0
    property int todayInput: 0
    property string lastEdited: ""
    property string syncStatus: "none"
    property string accentColor: _primary.toString()

    signal clicked()
    signal rightClicked()

    width: 220
    height: 180
    radius: _cardRadius
    color: hovered ? _cardHover : _card
    border.color: hovered ? _primary : _border
    border.width: 1

    property bool hovered: false

    Behavior on color { ColorAnimation { duration: _animFast } }
    Behavior on border.color { ColorAnimation { duration: _animFast } }

    // Shadow layer (behind the card)
    Rectangle {
        anchors.fill: parent
        anchors.topMargin: root.elevation > 0 && root.appShadow ? root.appShadow.forElevation(root.elevation).verticalOffset : 0
        radius: _cardRadius
        color: root.elevation > 0 && root.appShadow ? root.appShadow.forElevation(root.elevation).color : "transparent"
        opacity: 0.2
        visible: root.elevation > 0 && root.appShadow !== null
        z: -1
    }

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
        anchors.margins: _sp20
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
            dt: root.dt
            Layout.fillWidth: true
            Layout.topMargin: _sp12
            text: root.title || qsTr("未命名作品")
            color: _textPrimary
            font.pixelSize: _subtitle
            font.family: _fontFamily
            font.weight: Font.DemiBold
            elide: Text.ElideRight
            maximumLineCount: 2
            wrapMode: Text.Wrap
        }

        Item { Layout.fillHeight: true }

        // Stats row
        RowLayout {
            Layout.fillWidth: true
            spacing: _sp16

            Column {
                spacing: 2
                AppText {
                    dt: root.dt
                    text: root.wordCount >= 10000 ? (root.wordCount / 10000).toFixed(1) + "w" : root.wordCount.toLocaleString()
                    color: _textPrimary
                    font.pixelSize: _body
                    font.family: _fontFamily
                    font.weight: Font.Medium
                }
                AppText {
                    dt: root.dt
                    text: qsTr("总字数")
                    color: _textMuted
                    font.pixelSize: _caption
                    font.family: _fontFamily
                }
            }

            Column {
                spacing: 2
                visible: root.todayInput > 0
                AppText {
                    dt: root.dt
                    text: "+" + (root.todayInput >= 1000 ? (root.todayInput / 1000).toFixed(1) + "k" : root.todayInput.toLocaleString())
                    color: _primary
                    font.pixelSize: _body
                    font.family: _fontFamily
                    font.weight: Font.Medium
                }
                AppText {
                    dt: root.dt
                    text: qsTr("今日")
                    color: _textMuted
                    font.pixelSize: _caption
                    font.family: _fontFamily
                }
            }

            Item { Layout.fillWidth: true }

            AppText {
                dt: root.dt
                text: root.lastEdited || ""
                color: _textMuted
                font.pixelSize: _caption
                font.family: _fontFamily
                visible: text !== ""
            }
        }
    }
}
