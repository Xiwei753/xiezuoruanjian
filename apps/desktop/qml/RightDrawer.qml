// =============================================================================
// RightDrawer.qml — 右侧抽屉面板
// =============================================================================
//
// 层级：Desktop UI 层（QML UI 组件）
// 职责：从右侧滑出的抽屉面板，用于展示星图预览、统计等辅助信息
// 约束：
//   - 纯 UI 组件，内容通过 tab 切换
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    property var dt: null

    // Elevation shadow support
    property int elevation: 3
    property var appShadow: null

    readonly property color _sidebar: dt.sidebar
    readonly property color _border: dt.border
    readonly property color _accentSoft: dt.accentSoft
    readonly property color _accentText: dt.accentText
    readonly property color _card: dt.card
    readonly property color _textPrimary: dt.textPrimary
    readonly property color _textSecondary: dt.textSecondary
    readonly property color _textMuted: dt.textMuted
    readonly property int _sp4: dt.sp4
    readonly property int _sp8: dt.sp8
    readonly property int _sp16: dt.sp16
    readonly property int _radiusSm: dt.radiusSm
    readonly property int _fontSm: dt.fontSm
    readonly property int _fontLg: dt.fontLg

    property var backendRef: null
    property var starMapController: null
    property bool isOpen: false
    property int currentTab: 0
    property bool aiCapable: false
    property bool aiEnabled: false

    signal closeRequested()
    signal openStarMap()
    signal openSettings()

    color: "transparent"
    clip: true
    visible: isOpen

    // Shadow layer (behind the drawer panel)
    Rectangle {
        anchors.fill: parent
        anchors.topMargin: root.elevation > 0 && root.appShadow ? root.appShadow.forElevation(root.elevation).verticalOffset : 0
        radius: 0
        color: root.elevation > 0 && root.appShadow ? root.appShadow.forElevation(root.elevation).color : "transparent"
        opacity: 0.2
        visible: root.elevation > 0 && root.appShadow !== null
        z: -1
    }

    Rectangle {
        id: drawerPanel
        anchors.fill: parent
        color: _sidebar
        border.color: _border
        border.width: 1

        ColumnLayout {
            anchors.fill: parent
            spacing: 0

            // Tab bar
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 44
                color: "transparent"

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: _sp8
                    anchors.rightMargin: _sp8
                    spacing: _sp4

                    Repeater {
                        model: {
                            var tabs = [
                                { label: qsTr("星图"), idx: 0 }
                            ];
                            if (root.aiCapable && root.aiEnabled) {
                                tabs.push({ label: qsTr("AI"), idx: 1 });
                            }
                            tabs.push({ label: qsTr("统计"), idx: 2 });
                            // Settings tab removed — main entry is now in TopWritingToolbar
                            return tabs;
                        }

                        Rectangle {
                            width: tabLabel.implicitWidth + _sp16
                            height: 30
                            radius: _radiusSm
                            color: root.currentTab === modelData.idx ?
                                   _accentSoft :
                                   hoverArea.containsMouse ? _card : "transparent"

                            AppText {
                                id: tabLabel
                                anchors.centerIn: parent
                                text: modelData.label
                                color: root.currentTab === modelData.idx ?
                                       _accentText :
                                       _textSecondary
                                font.pixelSize: _fontSm
                                font.weight: root.currentTab === modelData.idx ? Font.DemiBold : Font.Normal
                            }

                            MouseArea {
                                id: hoverArea
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.currentTab = modelData.idx
                            }
                        }
                    }

                    Item { Layout.fillWidth: true }

                    // Close button
                    Rectangle {
                        width: 24; height: 24
                        radius: 12
                        color: closeHover.containsMouse ? _card : "transparent"

                        AppText {
                            anchors.centerIn: parent
                            text: "\u2715"
                            color: _textMuted
                            font.pixelSize: _fontSm
                        }

                        MouseArea {
                            id: closeHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.closeRequested()
                        }
                    }
                }
            }

            // Divider
            Rectangle { Layout.fillWidth: true; height: 1; color: _border }

            // Content area
            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true

                // Star Map tab
                StarMapPage {
                    dt: root.dt
                    backendRef: root.backendRef
                    starMapController: root.starMapController
                    appState: ({})
                    visible: root.currentTab === 0
                    anchors.fill: parent
                }

                // Stats tab
                StatsPreviewPage {
                    dt: root.dt
                    backendRef: root.backendRef
                    visible: root.currentTab === 2
                    anchors.fill: parent
                }

                // AI tab (placeholder, only shown when aiCapable && aiEnabled)
                Rectangle {
                    visible: root.currentTab === 1 && root.aiCapable && root.aiEnabled
                    anchors.fill: parent
                    color: "transparent"

                    ColumnLayout {
                        anchors.centerIn: parent
                        spacing: _sp16

                        AppText {
                            text: "\uD83E\uDD16"
                            dt: root.dt
                            font.pixelSize: 32
                            Layout.alignment: Qt.AlignHCenter
                        }
                        AppText {
                            text: qsTr("AI 助手")
                            color: _textPrimary
                            font.pixelSize: _fontLg
                            font.weight: Font.DemiBold
                            Layout.alignment: Qt.AlignHCenter
                        }
                        AppText {
                            text: qsTr("AI 功能将在后续版本实现")
                            color: _textMuted
                            font.pixelSize: _fontSm
                            Layout.alignment: Qt.AlignHCenter
                        }
                    }
                }
            }
        }
    }
}
