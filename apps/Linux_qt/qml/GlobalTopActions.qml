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
// Issue #900：视觉迁移到 Breeze Controls（Controls.ToolButton + Breeze 图标），
// 颜色统一取 Kirigami.Theme，不再读 DesignTokens 的 M3 字段。
// 公共 API（信号 requestSync/requestSearch/openSettings）保持不变。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

RowLayout {
    id: root
    required property var dt
    property var appState: ({})
    // 当前页面来源，用于把真实按钮点击与后续 request_id/source 关联起来。
    property string syncSource: "unknown"
    property bool compact: false

    signal requestSync()
    signal requestSearch()
    signal openSettings()

    spacing: Kirigami.Units.smallSpacing

    // 同步状态入口：Breeze refresh 图标 + 状态圆点 + 状态文案
    MotionToolButton {
        id: syncButton
        onClicked: {
            // Issue #970 诊断配套：在真正接收点击的按钮上留下用户发起记录。
            // 这里还没有 request_id（由 main.qml 的统一入口创建），因此记录
            // source + phase；main.qml 的 manual_sync_request_received 会带上
            // 同一 source 和新生成的 request_id，形成完整链路。
            if (typeof appBackend !== "undefined" && appBackend
                    && typeof appBackend.log_qml === "function") {
                appBackend.log_qml("info", "sync", "manual_sync_button_clicked",
                    "source=" + root.syncSource + " phase=button_clicked")
            }
            root.requestSync()
        }
        ToolTip.visible: hovered
        ToolTip.text: qsTr("立即同步")

        contentItem: RowLayout {
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Icon {
                Layout.alignment: Qt.AlignVCenter
                source: "view-refresh"
                implicitWidth: 20
                implicitHeight: 20
            }

            Rectangle {
                Layout.alignment: Qt.AlignVCenter
                implicitWidth: Kirigami.Units.smallSpacing
                implicitHeight: Kirigami.Units.smallSpacing
                radius: width / 2
                color: {
                    var s = root.appState && root.appState.sync ? root.appState.sync.status : "none";
                    if (s === "success") return Kirigami.Theme.positiveTextColor;
                    if (s === "syncing") return Kirigami.Theme.neutralTextColor;
                    if (s === "error" || s === "conflict" || s === "partial_conflict") return Kirigami.Theme.negativeTextColor;
                    return Kirigami.Theme.disabledTextColor;
                }
            }

            Label {
                Layout.alignment: Qt.AlignVCenter
                visible: !root.compact
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
                color: Kirigami.Theme.textColor
            }
        }
    }

    // 搜索按钮（固定显示）
    MotionToolButton {
        icon.name: "search"
        icon.width: 20
        icon.height: 20
        text: qsTr("搜索")
        display: root.compact ? AbstractButton.IconOnly : AbstractButton.TextBesideIcon
        ToolTip.visible: hovered
        ToolTip.text: qsTr("搜索")
        onClicked: root.requestSearch()
    }

    // 设置按钮
    MotionToolButton {
        icon.name: "configure"
        icon.width: 20
        icon.height: 20
        text: qsTr("设置")
        display: root.compact ? AbstractButton.IconOnly : AbstractButton.TextBesideIcon
        ToolTip.visible: hovered
        ToolTip.text: qsTr("设置")
        onClicked: root.openSettings()
    }
}
