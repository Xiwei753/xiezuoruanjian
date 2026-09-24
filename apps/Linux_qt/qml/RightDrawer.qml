// =============================================================================
// RightDrawer.qml — 右侧抽屉面板
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
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
    required property var dt

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
    readonly property real _fontSm: dt.fontSmPt
    readonly property real _fontLg: dt.fontLgPt

    property var editorBackendRef: null
    property var starMapController: null
    property bool isOpen: false
    property int currentTab: 0
    property bool aiCapable: false
    property bool aiEnabled: false
    // Issue #757 评论 5818193510 第 5 点：冲突侧栏支持。
    // syncBackendRef + workspaceProjectId 透传给 SyncConflictPanel。
    // hasConflicts 控制冲突 tab 显隐；变 true 时自动切到冲突 tab。
    property var syncBackendRef: null
    property string workspaceProjectId: ""
    property bool hasConflicts: false
    // 冲突 tab 固定 idx=3，不偏移现有星图(0)/AI(1)/统计(2)，保持兼容。
    readonly property int conflictTabIdx: 3

    signal closeRequested()
    signal openStarMap()
    signal openSettings()
    // 冲突 tab 被请求时发出（hasConflicts 从 false 变 true），外部据此打开 drawer。
    signal conflictTabRequested()

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
                            // Issue #757 评论 5818193510 第 5 点：有未解决冲突时增加"冲突"tab。
                            // idx=3 固定，不偏移现有 tab，保持 WritingWorkspace drawerTab 绑定兼容。
                            if (root.hasConflicts) {
                                tabs.push({ label: qsTr("冲突"), idx: root.conflictTabIdx });
                            }
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
                                dt: root.dt
                                anchors.centerIn: parent
                                text: modelData.label
                                color: root.currentTab === modelData.idx ?
                                       _accentText :
                                       _textSecondary
                                font.pointSize: _fontSm
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
                            dt: root.dt
                            anchors.centerIn: parent
                            text: "\u2715"
                            color: _textMuted
                            font.pointSize: _fontSm
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

                // Star Map tab — 施工占位
                Rectangle {
                    visible: root.currentTab === 0
                    anchors.fill: parent
                    color: "transparent"

                    ColumnLayout {
                        anchors.centerIn: parent
                        spacing: _sp16

                        AppText {
                            text: "\uD83C\uDF0C"
                            dt: root.dt
                            font.pointSize: dt.fontEmojiSmPt
                            Layout.alignment: Qt.AlignHCenter
                        }
                        AppText {
                            text: qsTr("星图正在施工")
                            dt: root.dt
                            color: _textPrimary
                            font.pointSize: _fontLg
                            font.weight: Font.DemiBold
                            Layout.alignment: Qt.AlignHCenter
                        }
                        AppText {
                            text: qsTr("星图功能将在后续版本实现")
                            dt: root.dt
                            color: _textMuted
                            font.pointSize: _fontSm
                            Layout.alignment: Qt.AlignHCenter
                        }
                    }
                }

                // Stats tab
                StatsPreviewPage {
                    dt: root.dt
                    editorBackendRef: root.editorBackendRef
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
                            font.pointSize: dt.fontEmojiSmPt
                            Layout.alignment: Qt.AlignHCenter
                        }
                        AppText {
                            text: qsTr("AI 助手")
                            dt: root.dt
                            color: _textPrimary
                            font.pointSize: _fontLg
                            font.weight: Font.DemiBold
                            Layout.alignment: Qt.AlignHCenter
                        }
                        AppText {
                            text: qsTr("AI 功能将在后续版本实现")
                            dt: root.dt
                            color: _textMuted
                            font.pointSize: _fontSm
                            Layout.alignment: Qt.AlignHCenter
                        }
                    }
                }

                // Issue #757 评论 5818193510 第 5 点：冲突 tab — 直接复用现有 RightDrawer 做临时冲突侧栏。
                // SyncConflictPanel 负责调 SyncBackend 拿冲突列表/预览/解决动作，
                // 不在 QML 维护第二份可编辑正文，不自己拼磁盘路径读 conflicts.json。
                SyncConflictPanel {
                    visible: root.currentTab === root.conflictTabIdx && root.hasConflicts
                    anchors.fill: parent
                    dt: root.dt
                    syncBackendRef: root.syncBackendRef
                    projectId: root.workspaceProjectId
                    onCloseRequested: root.closeRequested()
                    onConflictsResolved: {
                        // 解决一个冲突后刷新列表；若全部解决，外部应把 hasConflicts 置 false。
                        root.conflictTabRequested();
                    }
                }
            }
        }
    }

    // Issue #757 评论 5818193510 第 5 点：冲突刚产生时自动切到冲突 tab。
    // 打开 drawer 由外部（WritingWorkspace）监听 conflictTabRequested 完成，
    // RightDrawer 不自己控制 isOpen（单向属性，由外部绑定）。
    onHasConflictsChanged: {
        if (root.hasConflicts) {
            root.currentTab = root.conflictTabIdx;
            root.conflictTabRequested();
        }
    }
}
