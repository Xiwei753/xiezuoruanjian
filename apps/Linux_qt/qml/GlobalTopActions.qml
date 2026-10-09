// =============================================================================
// GlobalTopActions.qml — 全局顶栏公共入口
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：收口 Linux 非设置页面右上角的公共入口（同步 / 搜索 / 设置）
// 约束：
//   - 纯展示层，只发信号，不直接操作 backend
//   - 顺序固定右侧"同步 / 搜索 / 设置"
//   - 不插入"切换工作区"、星图操作、统计或排版操作
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

RowLayout {
    id: root
    required property var dt
    property var appState: ({})

    signal requestSync()
    signal requestSearch()
    signal openSettings()

    spacing: dt.sp8

    // 同步状态指示
    Rectangle {
        Layout.preferredWidth: syncRow.implicitWidth + dt.sp16
        Layout.preferredHeight: 40
        radius: dt.radiusPill
        color: syncHover.containsMouse ? dt.surfaceVariant : "transparent"

        Row {
            id: syncRow
            anchors.centerIn: parent
            spacing: dt.sp6

            Rectangle {
                width: 8; height: 8; radius: 4
                color: {
                    var s = root.appState && root.appState.sync ? root.appState.sync.status : "none";
                    if (s === "success") return dt.success;
                    if (s === "syncing") return dt.warning;
                    if (s === "error" || s === "conflict" || s === "partial_conflict") return dt.error;
                    return dt.textMuted;
                }
                Layout.alignment: Qt.AlignVCenter
            }

            AppText {
                dt: root.dt
                text: {
                    var s = root.appState && root.appState.sync ? root.appState.sync.status : "none";
                    if (s === "success") return qsTr("已同步");
                    if (s === "syncing") return qsTr("同步中");
                    if (s === "error") return qsTr("同步失败");
                    if (s === "conflict") return qsTr("同步冲突");
                    if (s === "partial_conflict") return qsTr("同步冲突");
                    // 已配置但无特定状态时显示"同步"
                    return qsTr("同步");
                }
                color: dt.onSurfaceVariant
                font.pointSize: dt.captionPt
                font.family: dt.fontFamily
                Layout.alignment: Qt.AlignVCenter
            }
        }

        MouseArea {
            id: syncHover
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.requestSync()
        }
    }

    // 搜索按钮（固定显示）
    Rectangle {
        Layout.preferredWidth: searchText.implicitWidth + 24
        Layout.preferredHeight: 40
        radius: dt.radiusPill
        color: searchHover.containsMouse ? dt.surfaceVariant : "transparent"

        AppText {
            id: searchText
            dt: root.dt
            anchors.centerIn: parent
            text: qsTr("搜索")
            color: dt.onSurfaceVariant
            font.pointSize: dt.captionPt
            font.family: dt.fontFamily
        }

        MouseArea {
            id: searchHover
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.requestSearch()
        }
    }

    // 设置按钮
    Rectangle {
        Layout.preferredWidth: settingsText.implicitWidth + 24
        Layout.preferredHeight: 40
        radius: dt.radiusPill
        color: settingsHover.containsMouse ? dt.surfaceVariant : "transparent"

        AppText {
            id: settingsText
            dt: root.dt
            anchors.centerIn: parent
            text: qsTr("设置")
            color: dt.onSurfaceVariant
            font.pointSize: dt.captionPt
            font.family: dt.fontFamily
        }

        MouseArea {
            id: settingsHover
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.openSettings()
        }
    }
}
