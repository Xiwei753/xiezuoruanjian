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
        color: dt ? dt.surface : "#1A1D23"
        border.color: dt ? dt.border : "#2A2E36"
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
                    anchors.leftMargin: dt ? dt.sp8 : 8
                    anchors.rightMargin: dt ? dt.sp8 : 8
                    spacing: dt ? dt.sp4 : 4

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
                            width: tabLabel.implicitWidth + (dt ? dt.sp16 : 16)
                            height: 30
                            radius: dt ? dt.radiusSm : 8
                            color: root.currentTab === modelData.idx ?
                                   (dt ? dt.accentSoft : "rgba(123,140,222,0.12)") :
                                   hoverArea.containsMouse ? (dt ? dt.card : "#1E2128") : "transparent"

                            AppText {
                                id: tabLabel
                                anchors.centerIn: parent
                                text: modelData.label
                                color: root.currentTab === modelData.idx ?
                                       (dt ? dt.accentText : "#3D4D9E") :
                                       (dt ? dt.textSecondary : "#5C6070")
                                font.pixelSize: dt ? dt.fontSm : 12
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
                        color: closeHover.containsMouse ? (dt ? dt.card : "#1E2128") : "transparent"

                        AppText {
                            anchors.centerIn: parent
                            text: "\u2715"
                            color: dt ? dt.textMuted : "#606470"
                            font.pixelSize: dt ? dt.fontSm : 12
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
            Rectangle { Layout.fillWidth: true; height: 1; color: dt ? dt.border : "#2A2E36" }

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
                        spacing: dt ? dt.sp16 : 16

                        AppText {
                            text: "\uD83E\uDD16"
                            font.pixelSize: 32
                            Layout.alignment: Qt.AlignHCenter
                        }
                        AppText {
                            text: qsTr("AI 助手")
                            color: dt ? dt.textPrimary : "#E2E4E9"
                            font.pixelSize: dt ? dt.fontLg : 16
                            font.weight: Font.DemiBold
                            Layout.alignment: Qt.AlignHCenter
                        }
                        AppText {
                            text: qsTr("AI 功能将在后续版本实现")
                            color: dt ? dt.textMuted : "#606470"
                            font.pixelSize: dt ? dt.fontSm : 12
                            Layout.alignment: Qt.AlignHCenter
                        }
                    }
                }
            }
        }
    }
}
