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
//   ┌──────────────◢  ← 标题区是一块小标题栏，最右端收成三角折角
//   │ 作品名        │     标题区底线只画到折角之前
//   ├──────────────┘
//   │ 8卷 97章      │
//   │              │
//   │        1678字 │   ← 右下总字数
//   │ 26/8/11 22:37│   ← 左下最后编辑时间
//   └──────────────┘
//
// 宽度取 Core 的 project_card_min_width_dp（经 layoutPlan.projectCardMinWidthVp 透传，
// 三端共用同一个值），不在 QML 里写死另一套卡片宽度。
// lastEdited 由 ProjectHomePage.formatProjectTime() 格式化后传入（26/8/11 22:37），
// 卡片只负责画，不再自己解析 Core 的 ISO 字符串。
// 右键菜单挂整张卡，不常驻「更多」按钮。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Shapes

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

    // Issue #827 评论 4 第 3 点：草图里标题区本身是一块小标题栏，
    // 最右端收成一个真正的三角折角，所以标题区高度固定、折角尺寸固定，
    // 标题区的底线只画到折角之前。
    readonly property int _foldSize: 16
    readonly property int _headerHeight: 40

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
        spacing: 0

        // 封面标题区：一块固定高度的小标题栏，底部画底线，正文从它下面开始。
        // 底线宽度要比整卡窄出折角宽度，才能画成草图里
        // 「标题 ──────┤」 到折角断开的那一条。
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: root._headerHeight

            AppText {
                anchors.left: parent.left
                anchors.leftMargin: _sp16
                anchors.right: cardFold.left
                anchors.rightMargin: dt.sp4
                anchors.verticalCenter: parent.verticalCenter
                dt: root.dt
                text: root.title || qsTr("未命名作品")
                color: _textPrimary
                font.pointSize: _subtitle
                font.family: _fontFamily
                font.weight: Font.DemiBold
                elide: Text.ElideRight
                maximumLineCount: 2
                wrapMode: Text.Wrap
            }

            // 标题区底线：只画到折角之前（父项宽 - 折角宽 - 左内边距）。
            Rectangle {
                anchors.left: parent.left
                anchors.leftMargin: _sp16
                anchors.right: cardFold.left
                anchors.rightMargin: dt.sp4
                anchors.bottom: parent.bottom
                height: 1
                color: _border
            }
        }

        // 卷 / 章数量（从标题区下面开始）
        AppText {
            dt: root.dt
            Layout.fillWidth: true
            Layout.leftMargin: _sp16
            Layout.rightMargin: _sp16
            Layout.topMargin: _sp12
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
            Layout.leftMargin: _sp16
            Layout.rightMargin: _sp16
            Layout.bottomMargin: _sp12
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
            Layout.leftMargin: _sp16
            Layout.rightMargin: _sp16
            Layout.bottomMargin: _sp16
            Layout.topMargin: 2
            text: root.lastEdited || ""
            color: _textMuted
            font.pointSize: _caption
            font.family: _fontFamily
            visible: text !== ""
        }
    }

    // Issue #827 评论 4 第 3 点：草图右上角是一个真正的三角折角，
    // 属于标题区本身的一部分，不是整张卡下方再挂一个圆角小方块。
    // 用 QtQuick.Shapes 画直角三角（仓库 StarMapEmbed.qml 已用同一套 API）。
    // Shape 默认不接收鼠标事件，右键菜单仍由整卡的 MouseArea 处理。
    Shape {
        id: cardFold
        width: root._foldSize
        height: root._foldSize
        anchors.top: parent.top
        anchors.right: parent.right
        z: 1

        ShapePath {
            strokeWidth: 0
            fillColor: root.hovered ? root._primary : root._border
            startX: 0
            startY: 0
            PathLine { x: root._foldSize; y: 0 }
            PathLine { x: root._foldSize; y: root._foldSize }
            PathLine { x: 0; y: 0 }
        }
    }
}