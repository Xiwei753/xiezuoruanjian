// =============================================================================
// ProjectCard.qml — 作品卡片组件（书封面式竖卡）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：单个作品的封面式卡片展示（作品名 / 卷章数 / 总字数 / 最后编辑时间）
// 约束：
//   - 纯展示组件，数据通过 property 传入
//   - 点击和右键菜单通过 signal 传递给 ProjectHomePage
//   - 使用 DesignTokens 统一样式
//
// Issue #827 评论 2：形态从 220×180 横卡改为竖向「书封面」卡。
// 结构对应 2026-10-05 手绘稿：
//
//   ┌──────────────┐
//   │ 作品名      ◢ │   ← 右上折角是书封面的视觉身份
//   ├──────────────┤
//   │ 8卷 97章      │
//   │              │
//   │        1678字 │   ← 右下总字数
//   │ 26/8/11 22:37│   ← 左下最后编辑时间
//   └──────────────┘
//
// 宽度取 Core 的 project_card_min_width_dp（经 layoutPlan.projectCardMinWidthVp 透传，
// 三端共用同一个值），不在 QML 里写死另一套卡片宽度。
// 右键菜单挂整张卡，不常驻「更多」按钮。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    required property var dt

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
    readonly property real _subtitle: dt.subtitlePt
    readonly property real _body: dt.bodyPt
    readonly property real _caption: dt.captionPt
    readonly property string _fontFamily: dt.fontFamily
    readonly property int _animFast: dt.animFast

    // Issue #827 评论 2：卷数与章数直接用 Core summary 的值，不跨 FFI 重算。
    property string projectId: ""
    property string title: ""
    property int wordCount: 0
    property int volumeCount: 0
    property int chapterCount: 0
    property string lastEdited: ""

    // 卡片宽高由 ProjectHomePage 从 Core 的 projectCardMinWidthVp 传入，
    // 「+」卡复用同一组尺寸，不允许自己另有一套。
    property int cardWidth: 180
    property int cardHeight: 240

    signal clicked()
    signal rightClicked()

    width: cardWidth
    height: cardHeight
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
        anchors.margins: _sp16
        spacing: 0

        // 作品名（封面顶部）
        AppText {
            dt: root.dt
            Layout.fillWidth: true
            text: root.title || qsTr("未命名作品")
            color: _textPrimary
            font.pointSize: _subtitle
            font.family: _fontFamily
            font.weight: Font.DemiBold
            elide: Text.ElideRight
            maximumLineCount: 2
            wrapMode: Text.Wrap
        }

        // 书脊横线：分隔标题与信息区
        Rectangle {
            Layout.fillWidth: true
            Layout.topMargin: _sp12
            Layout.bottomMargin: _sp12
            height: 1
            color: _border
        }

        // 卷 / 章数量
        AppText {
            dt: root.dt
            Layout.fillWidth: true
            text: qsTr("%1卷 %2章").arg(root.volumeCount).arg(root.chapterCount)
            color: _textMuted
            font.pointSize: _body
            font.family: _fontFamily
        }

        Item { Layout.fillHeight: true }

        // 右下：总字数
        AppText {
            dt: root.dt
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignRight
            text: root.wordCount >= 10000
                  ? qsTr("%1万字").arg((root.wordCount / 10000).toFixed(1))
                  : qsTr("%1字").arg(root.wordCount.toLocaleString())
            color: _textPrimary
            font.pointSize: _body
            font.family: _fontFamily
            font.weight: Font.Medium
        }

        // 左下：最后编辑时间
        AppText {
            dt: root.dt
            Layout.fillWidth: true
            Layout.topMargin: 2
            text: root.lastEdited || ""
            color: _textMuted
            font.pointSize: _caption
            font.family: _fontFamily
            visible: text !== ""
        }
    }

    // 右上折角（书封面身份标识）。纯装饰，不吃鼠标事件：
    // 右键菜单必须挂在整张卡上，所以折角不能阻断 MouseArea。
    Rectangle {
        width: 16
        height: 16
        anchors.top: parent.top
        anchors.right: parent.right
        color: root.hovered ? root._primary : root._border
        radius: 4
        topLeftRadius: root._cardRadius
        opacity: 0.9
    }
}