// =============================================================================
// ProjectCard.qml — 作品卡片组件（书封面式竖卡）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：单个作品的封面式卡片展示（作品名 / 卷章数 / 总字数 / 最后编辑时间）
// 约束：
//   - 纯展示组件，数据通过 property 传入
//   - 点击和右键菜单通过 signal 传递给 ProjectHomePage
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
// 宽度取 Qt 平台本地常量（ProjectHomePage._cardWidth = 180vp，Issue #920 起
// 不再经 Core 布局契约透传），卡片只做展示，不在组件内另写一套尺寸。
// lastEdited 由 ProjectHomePage.formatProjectTime() 格式化后传入（26/8/11 22:37），
// 卡片只负责画，不再自己解析 Core 的 ISO 字符串。
// 右键菜单挂整张卡，不常驻「更多」按钮。
//
// Issue #900：颜色/间距迁移到 Kirigami.Theme / Kirigami.Units，文本改用
// Controls.Label；不再读 DesignTokens 的 M3 字段，也移除自绘投影层。
// 公共 API（dt / elevation / appShadow / projectId / title / wordCount /
// volumeCount / chapterCount / lastEdited / cardWidth / cardHeight /
// clicked / rightClicked）保持不变。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Shapes
import org.kde.kirigami as Kirigami

Rectangle {
    id: root
    required property var dt

    // Elevation shadow support（保留属性以兼容既有注入，视觉交由 Breeze 边框承担）
    property int elevation: 1
    property var appShadow: null

    // Issue #827 评论 2：卷数与章数直接用 Core summary 的值，不跨 FFI 重算。
    property string projectId: ""
    property string title: ""
    property int wordCount: 0
    property int volumeCount: 0
    property int chapterCount: 0
    property string lastEdited: ""

    // 卡片宽高由 ProjectHomePage 传入，「+」卡复用同一组尺寸，不允许自己另有一套。
    property int cardWidth: 180
    property int cardHeight: 240

    // Issue #827 评论 4 第 3 点：草图里标题区本身是一块小标题栏，
    // 最右端收成一个真正的三角折角，所以标题区高度固定、折角尺寸固定，
    // 标题区的底线只画到折角之前。
    readonly property int _foldSize: 16
    readonly property int _headerHeight: 40

    signal clicked()
    signal rightClicked()

    property bool pressVisual: false

    width: cardWidth
    height: cardHeight
    radius: Kirigami.Units.cornerRadius
    color: hovered ? Kirigami.Theme.activeBackgroundColor : Kirigami.Theme.alternateBackgroundColor
    border.color: hovered ? Kirigami.Theme.focusColor : Kirigami.Theme.disabledTextColor
    border.width: 1

    property bool hovered: false

    MotionTokens { id: motion }

    scale: pressVisual ? 0.985 : (hovered ? 1.01 : 1.0)

    Behavior on color { ColorAnimation { duration: motion.hoverMs } }
    Behavior on border.color { ColorAnimation { duration: motion.hoverMs } }
    Behavior on scale {
        NumberAnimation {
            duration: motion.hoverMs
            easing.type: Easing.OutCubic
        }
    }

    MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onContainsMouseChanged: root.hovered = containsMouse
        onPressed: root.pressVisual = true
        onReleased: root.pressVisual = false
        onCanceled: root.pressVisual = false
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

            Label {
                // Issue #833：不跨层锚 cardFold（cardFold 是外层 root 的子项，
                // 不是本 Item 的父项或兄弟项）。标题 Item 内直接 anchors.right:
                // parent.right，rightMargin 留出折角宽 + 间距。
                anchors.left: parent.left
                anchors.leftMargin: Kirigami.Units.largeSpacing
                anchors.right: parent.right
                anchors.rightMargin: root._foldSize + Kirigami.Units.smallSpacing
                anchors.verticalCenter: parent.verticalCenter
                text: root.title || qsTr("未命名作品")
                color: Kirigami.Theme.textColor
                font.weight: Font.DemiBold
                elide: Text.ElideRight
                maximumLineCount: 2
                wrapMode: Text.Wrap
            }

            // 标题区底线：只画到折角之前（父项宽 - 折角宽 - 左内边距）。
            // Issue #833：同样按 parent.right + fold margin 收口，不跨层锚 cardFold。
            Rectangle {
                anchors.left: parent.left
                anchors.leftMargin: Kirigami.Units.largeSpacing
                anchors.right: parent.right
                anchors.rightMargin: root._foldSize + Kirigami.Units.smallSpacing
                anchors.bottom: parent.bottom
                height: 1
                color: Kirigami.Theme.disabledTextColor
            }
        }

        // 卷 / 章数量（从标题区下面开始）
        Label {
            Layout.fillWidth: true
            Layout.leftMargin: Kirigami.Units.largeSpacing
            Layout.rightMargin: Kirigami.Units.largeSpacing
            Layout.topMargin: Kirigami.Units.smallSpacing
            text: qsTr("%1卷 %2章").arg(root.volumeCount).arg(root.chapterCount)
            color: Kirigami.Theme.disabledTextColor
        }

        Item { Layout.fillHeight: true }

        // 右下：总字数
        Label {
            Layout.fillWidth: true
            Layout.leftMargin: Kirigami.Units.largeSpacing
            Layout.rightMargin: Kirigami.Units.largeSpacing
            Layout.bottomMargin: Kirigami.Units.smallSpacing
            horizontalAlignment: Text.AlignRight
            text: root.wordCount >= 10000
                  ? qsTr("%1万字").arg((root.wordCount / 10000).toFixed(1))
                  : qsTr("%1字").arg(root.wordCount.toLocaleString())
            color: Kirigami.Theme.textColor
            font.weight: Font.Medium
        }

        // 左下：最后编辑时间
        Label {
            Layout.fillWidth: true
            Layout.leftMargin: Kirigami.Units.largeSpacing
            Layout.rightMargin: Kirigami.Units.largeSpacing
            Layout.bottomMargin: Kirigami.Units.largeSpacing
            Layout.topMargin: 2
            text: root.lastEdited || ""
            color: Kirigami.Theme.disabledTextColor
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
            fillColor: root.hovered ? Kirigami.Theme.focusColor : Kirigami.Theme.disabledTextColor
            startX: 0
            startY: 0
            PathLine { x: root._foldSize; y: 0 }
            PathLine { x: root._foldSize; y: root._foldSize }
            PathLine { x: 0; y: 0 }
        }
    }
}
