// =============================================================================
// SettingsSection.qml — 设置分区组件（可折叠 accordion）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
// 职责：设置页面的分区容器，带标题、圆角背景和折叠/展开交互
// 约束：
//   - 纯 UI 组件，设置行通过 default property 传入
//   - 使用 DesignTokens 统一样式
//   - Issue #833：改成真正的折叠分组。expanded 由外部（SettingsDialog）控制，
//     toggleRequested 通知外部切换 expandedSectionKey，不自己写 expanded。
// =============================================================================
//
// 关于 Layouts 中的 implicitHeight
//   implicitHeight: header 固定高度 + body 折叠高度 + 上下 padding。
//   收起时 body 高度 0，所有 section 只有同一套 header 高度。
//   展开时 body 高度 = rows.implicitHeight，clip 保证收起动画不溢出。

import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root
    required property var dt
    property string title: ""
    // Issue #833：折叠状态由外部控制，本组件不自己写 expanded。
    property bool expanded: false
    signal toggleRequested()
    default property alias contentData: rows.data

    radius: dt.radiusLg
    color: dt.card
    border.color: dt.border
    border.width: 1

    readonly property int _sectionPadding: dt.sp20
    readonly property int _headerHeight: dt.sp40
    // Issue #833：body 高度 = expanded ? rows.implicitHeight + spacing : 0。
    // 收起时 clip 裁掉内容，整张卡只剩 header 高度。
    readonly property real _bodyHeight: root.expanded ? (rows.implicitHeight + dt.sp12) : 0

    implicitHeight: header.height + root._bodyHeight + root._sectionPadding

    ColumnLayout {
        id: contentCol
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: root._sectionPadding
        spacing: 0

        // ── Header：固定高度，整行可点击 ──
        Rectangle {
            id: header
            Layout.fillWidth: true
            Layout.preferredHeight: root._headerHeight
            color: "transparent"

            RowLayout {
                anchors.fill: parent
                spacing: root.dt.sp8

                AppText {
                    dt: root.dt
                    text: root.title
                    color: dt.textPrimary
                    font.pointSize: dt.subtitlePt
                    font.family: dt.fontFamily
                    font.weight: Font.DemiBold
                    Layout.fillWidth: true
                }

                // 展开/收起箭头
                AppText {
                    dt: root.dt
                    text: root.expanded ? "\u2304" : "\u2303"
                    color: dt.textSecondary
                    font.pointSize: dt.fontMdPt
                    Layout.alignment: Qt.AlignVCenter
                }
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.toggleRequested()
            }
        }

        // ── Body：折叠容器 ──
        // height = expanded ? rows.implicitHeight : 0，clip: true。
        // rows 锚 left/right/top，高度由内容决定（implicitHeight）。
        Item {
            id: body
            Layout.fillWidth: true
            Layout.preferredHeight: root._bodyHeight
            clip: true

            ColumnLayout {
                id: rows
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                spacing: root.dt.sp12
            }
        }
    }
}
