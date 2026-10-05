// =============================================================================
// WritingStatusBar.qml — 写作页底部状态栏（Issue #829 手稿）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：正文列底部一条只读状态带，三段布局
//   左：当前章节字数   中：写作进度   右：时间 + 保存状态
//
// 手稿基准（docs/ui/reference/widescreen/宽屏全打开.png、左右缩回.png）：
//   底部一行横贯「章节树右缘 ~ 工具面板左缘」，也就是只属于 Editor 这一列——
//   章节树、右侧工具面板、最右工具 rail 都不参与这条带子。
//   硬约束 6：这一行只放字数 / 进度 / 时间 / 保存状态，不承担任何页面导航。
//
// 边界：本组件是纯展示件，不查后端、不持有业务状态。
// 所有数值由 WritingWorkspace 注入（那边才是持有 Core 数据的地方），
// 避免状态栏自己再维护一份字数/进度缓存。
// =============================================================================

import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root

    required property var dt

    // ── 注入的状态数据（全部由 WritingWorkspace 提供）──
    // 当前章节字数：Core 的 calculate_word_count 语义（非空白字符）。
    property int chapterWordCount: 0
    // 进度分子：Core 写作统计里的今日纯输入字数（get_writing_stats_summary）。
    property int progressCurrent: 0
    // 进度分母：手稿上的「/ 2,000」。
    // Issue #829 备注：Core 目前没有任何目标字数字段（settings 里只有
    // desktop_sidebar_width / desktop_editor_width 这类布局项），所以这里
    // 用一个显式常量占位。等 Core 加了目标字数设置项，把这个默认值换掉、
    // 由 WritingWorkspace 改成读 Core，不要在端侧另造一套目标状态。
    property int progressTarget: 2000
    // 右段：HH:mm。WritingWorkspace 每分钟推一次，不在本组件里起定时器。
    property string clockText: ""
    // 右段：保存状态（已保存 / 未保存 / 保存中 / 保存失败）。
    property string saveStatus: ""
    // 保存状态是否异常（失败）——决定用 error 还是 success 色。
    property bool saveStatusIsError: false

    // 状态带高度跟手稿的细条比例；不跟 Core 的七角色走——手稿里它只是
    // Editor 列底部的一条信息带，不是任何一个角色，所以不占 Core bounds。
    readonly property real barHeight: 28

    implicitHeight: barHeight
    color: root.dt.surface

    // 与正文纸面上缘呼应的一条分隔线。
    Rectangle {
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 1
        color: root.dt.border
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: root.dt.sp16
        anchors.rightMargin: root.dt.sp16
        spacing: root.dt.sp12

        // ── 左：当前章节字数 ──
        AppText {
            dt: root.dt
            text: root.chapterWordCount + " " + qsTr("字")
            color: root.dt.textSecondary
            font.pointSize: root.dt.captionPt
            font.family: root.dt.fontFamily
            Layout.alignment: Qt.AlignVCenter
        }

        Item { Layout.fillWidth: true }

        // ── 中：写作进度 ──
        AppText {
            dt: root.dt
            // 手稿写的是「434/2,000」，斜杠两侧都不留空格。
            text: root.progressCurrent + "/" + root.progressTarget.toLocaleString(Qt.locale(), "f", 0)
            color: root.dt.textSecondary
            font.pointSize: root.dt.captionPt
            font.family: root.dt.fontFamily
            Layout.alignment: Qt.AlignVCenter
        }

        Item { Layout.fillWidth: true }

        // ── 右：时间 + 保存状态 ──
        RowLayout {
            spacing: root.dt.sp12
            Layout.alignment: Qt.AlignVCenter

            AppText {
                dt: root.dt
                text: root.clockText
                color: root.dt.textSecondary
                font.pointSize: root.dt.captionPt
                font.family: root.dt.fontFamily
                visible: root.clockText !== ""
            }

            AppText {
                dt: root.dt
                text: root.saveStatus
                color: root.saveStatusIsError ? root.dt.error : root.dt.success
                font.pointSize: root.dt.captionPt
                font.family: root.dt.fontFamily
                visible: root.saveStatus !== ""
            }
        }
    }
}
