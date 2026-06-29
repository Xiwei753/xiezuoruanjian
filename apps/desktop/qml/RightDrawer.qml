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

    // Safe access: fallback to light-theme defaults when dt is null
    readonly property color _sidebar: dt ? dt.sidebar : "#F3F7FC"
    readonly property color _border: dt ? dt.border : "#71788057"
    readonly property color _accentSoft: dt ? dt.accentSoft : "#CCE5FF"
    readonly property color _accentText: dt ? dt.accentText : "#001E31"
    readonly property color _card: dt ? dt.card : "#F6F8FC"
    readonly property color _textPrimary: dt ? dt.textPrimary : "#1A1C1E"
    readonly property color _textSecondary: dt ? dt.textSecondary : "#42474E"
    readonly property color _textMuted: dt ? dt.textMuted : "#747880"
    readonly property int _sp4: dt ? dt.sp4 : 4
    readonly property int _sp8: dt ? dt.sp8 : 8
    readonly property int _sp16: dt ? dt.sp16 : 16
    readonly property int _radiusSm: dt ? dt.radiusSm : 8
    readonly property int _fontSm: dt ? dt.fontSm : 12
    readonly property int _fontLg: dt ? dt.fontLg : 16

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
