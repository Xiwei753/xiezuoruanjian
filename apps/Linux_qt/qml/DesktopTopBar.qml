// =============================================================================
// DesktopTopBar.qml — Linux 全窗口桌面顶栏
// =============================================================================
//
// 层级：Linux_qt UI 层（应用壳层组件）
// 职责：所有一级 / 二级主页面共用的唯一顶栏，挂在 main.qml 的 workspaceBody
//   之上，而不是由 CreativeHub / WritingWorkspace / StarMapWorkspace 各自绘制。
//   - hub：左侧与一级导航同宽的品牌区，右侧同步 / 搜索 / 设置；
//   - writing：左返回 / 撤销 / 重做，中间排版组，右侧公共入口；
//   - starmap：左返回 + 星图标题，右侧公共入口。
// 约束（Issue #977）：
//   - 顶栏面固定 64，底线是独立的 1px 兄弟项并贯通整个客户区；
//     不允许把底线画进 64 高背景里。
//   - 右侧 GlobalTopActions 始终锚定窗口右缘，不随写作区工具 pane 收放漂移。
//   - 本组件只发信号，不直接访问 editor / sync backend；具体动作由 main.qml
//     转交给当前工作区的公共方法。
//   - 宽度只通过 Layout.preferredWidth/minimumWidth/maximumWidth 表达，
//     不把 width 和 Layout 宽度绑在一起。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: root

    required property var dt
    property string mode: "hub"
    // Issue #1014：顶栏不再从写作区实际宽度反推，统一读页面骨架状态。
    property var layoutState: null
    property string pageKey: "hub.projects"
    property var appState: ({})
    property string starmapTitle: ""
    property bool starmapFocusIsRoot: true
    property real currentFontSize: 16
    property real currentLineSpacing: 1.5
    property bool firstLineIndent: false
    property string saveStatus: ""

    signal backRequested()
    signal undoRequested()
    signal redoRequested()
    signal fontSizeChanged(real size)
    signal lineSpacingChanged(real spacing)
    signal firstLineIndentToggled()
    signal formatOneClick()
    signal requestSync()
    signal requestSearch()
    signal openSettings()

    spacing: 0
    implicitHeight: barFace.implicitHeight + barDivider.implicitHeight

    // Issue #1014：顶栏高度也取整窗唯一骨架常量，未注入时回退 64。
    readonly property int topBarHeight:
        root.layoutState ? root.layoutState.topBarHeight : 64
    // Issue #1016：读取下方 SplitView 本次实际渲染宽度；无记录时才回退到
    // 期望宽度。窗口缩窄导致的钳制不会再让顶栏与实际 pane 错位。
    readonly property real effectiveLeftWidth:
        root.layoutState ? root.layoutState.renderedPaneWidth(root.pageKey, "left") : 240
    readonly property real effectiveRightWidth:
        root.layoutState ? root.layoutState.renderedPaneWidth(root.pageKey, "right") : 296
    readonly property bool leftCollapsed:
        root.layoutState ? root.layoutState.isCollapsed(root.pageKey, "left") : false
    readonly property bool rightCollapsed:
        root.layoutState ? root.layoutState.isCollapsed(root.pageKey, "right") : false

    readonly property real writingLeadingNaturalWidth:
        writingLeadingInner.implicitWidth + Kirigami.Units.largeSpacing + Kirigami.Units.smallSpacing
    readonly property real writingTrailingNaturalWidth:
        topActions.implicitWidth + Kirigami.Units.largeSpacing * 2
    // 展开阈值只使用不受 compactActions 影响的完整态预算，避免
    // compactActions → GlobalTopActions.compact → implicitWidth → 阈值
    // 反过来改变 compactActions 的绑定环。完整态右侧至少按 14 个
    // gridUnit 预留，覆盖同步状态文案 / 搜索 / 设置同时显示的情况。
    readonly property real writingExpandedWidth:
        Math.max(root.leftCollapsed ? 0 : root.effectiveLeftWidth, writingLeadingNaturalWidth)
        + Math.max(root.rightCollapsed ? 0 : root.effectiveRightWidth,
                   Kirigami.Units.gridUnit * 14)
        + formatGroup.implicitWidth
        + Kirigami.Units.largeSpacing * 2

    // 仅写作模式在空间不足时隐藏中间排版组；hub / starmap 只在极窄窗口收起
    // 文字标签，保留 40px 图标点击区。
    readonly property bool compactActions: root.mode === "writing"
        ? root.width < writingExpandedWidth
        : root.width < Kirigami.Units.gridUnit * 30

    // 顶栏面：统一 64 高。底线不在这个 Rectangle 内绘制。
    Rectangle {
        id: barFace
        Layout.fillWidth: true
        Layout.preferredHeight: root.topBarHeight
        implicitHeight: root.topBarHeight
        color: Kirigami.Theme.alternateBackgroundColor

        RowLayout {
            anchors.fill: parent
            spacing: 0

            // ── hub：品牌区与左侧一级导航同宽 ──
            Item {
                id: hubBrandSlot
                visible: root.mode === "hub"
                Layout.fillHeight: true
                Layout.preferredWidth: root.effectiveLeftWidth
                Layout.minimumWidth: root.effectiveLeftWidth
                Layout.maximumWidth: root.effectiveLeftWidth

                Kirigami.Heading {
                    anchors.left: parent.left
                    anchors.leftMargin: Kirigami.Units.largeSpacing
                    anchors.verticalCenter: parent.verticalCenter
                    level: 3
                    text: qsTr("素笺")
                    color: Kirigami.Theme.highlightColor
                }
            }

            // ── writing：返回 / 撤销 / 重做，宽度与左侧章节栏对齐 ──
            Item {
                id: writingLeadingSlot
                visible: root.mode === "writing"
                Layout.fillHeight: true
                Layout.preferredWidth: root.compactActions || root.leftCollapsed
                    ? root.writingLeadingNaturalWidth
                    : Math.max(root.effectiveLeftWidth, root.writingLeadingNaturalWidth)
                Layout.minimumWidth: Layout.preferredWidth
                Layout.maximumWidth: Layout.preferredWidth

                RowLayout {
                    id: writingLeadingInner
                    anchors.fill: parent
                    anchors.leftMargin: Kirigami.Units.largeSpacing
                    anchors.rightMargin: Kirigami.Units.smallSpacing
                    spacing: Kirigami.Units.smallSpacing

                    MotionToolButton {
                        icon.name: "go-previous"
                        icon.width: 20
                        icon.height: 20
                        display: AbstractButton.IconOnly
                        flat: true
                        text: qsTr("返回")
                        ToolTip.visible: hovered
                        ToolTip.text: qsTr("返回作品列表")
                        onClicked: root.backRequested()
                    }

                    MotionToolButton {
                        icon.name: "edit-undo"
                        icon.width: 20
                        icon.height: 20
                        display: AbstractButton.IconOnly
                        flat: true
                        text: qsTr("撤销")
                        ToolTip.visible: hovered
                        ToolTip.text: qsTr("撤销")
                        onClicked: root.undoRequested()
                    }

                    MotionToolButton {
                        icon.name: "edit-redo"
                        icon.width: 20
                        icon.height: 20
                        display: AbstractButton.IconOnly
                        flat: true
                        text: qsTr("重做")
                        ToolTip.visible: hovered
                        ToolTip.text: qsTr("重做")
                        onClicked: root.redoRequested()
                    }
                }
            }

            // ── starmap：返回 + 当前画布标题 ──
            Item {
                id: starmapLeadingSlot
                visible: root.mode === "starmap"
                Layout.fillHeight: true
                Layout.preferredWidth: root.compactActions || root.leftCollapsed
                    ? starmapLeadingInner.implicitWidth + Kirigami.Units.largeSpacing
                      + Kirigami.Units.smallSpacing
                    : Math.max(root.effectiveLeftWidth,
                               starmapLeadingInner.implicitWidth + Kirigami.Units.largeSpacing
                               + Kirigami.Units.smallSpacing)
                Layout.minimumWidth: Layout.preferredWidth
                Layout.maximumWidth: Layout.preferredWidth

                RowLayout {
                    id: starmapLeadingInner
                    anchors.fill: parent
                    anchors.leftMargin: Kirigami.Units.largeSpacing
                    anchors.rightMargin: Kirigami.Units.smallSpacing
                    spacing: Kirigami.Units.smallSpacing

                    MotionToolButton {
                        icon.name: "go-previous"
                        icon.width: 20
                        icon.height: 20
                        display: AbstractButton.TextBesideIcon
                        flat: true
                        text: root.starmapFocusIsRoot ? qsTr("返回") : qsTr("返回父星图")
                        onClicked: root.backRequested()
                    }

                    Kirigami.Heading {
                        level: 3
                        text: root.starmapTitle
                        color: Kirigami.Theme.textColor
                        elide: Text.ElideRight
                        Layout.alignment: Qt.AlignVCenter
                        Layout.maximumWidth: Kirigami.Units.gridUnit * 24
                    }
                }
            }

            // ── writing：中间排版组，只有足够宽时显示 ──
            Item {
                id: centerSlot
                visible: root.mode === "writing" && !root.compactActions
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.minimumWidth: formatGroup.implicitWidth + Kirigami.Units.smallSpacing * 2

                WritingFormatGroup {
                    id: formatGroup
                    anchors.fill: parent
                    anchors.leftMargin: Kirigami.Units.smallSpacing
                    anchors.rightMargin: Kirigami.Units.smallSpacing
                    dt: root.dt
                    currentFontSize: root.currentFontSize
                    currentLineSpacing: root.currentLineSpacing
                    firstLineIndent: root.firstLineIndent
                    saveStatus: root.saveStatus
                    onFontSizeChanged: function(size) { root.fontSizeChanged(size) }
                    onLineSpacingChanged: function(spacing) { root.lineSpacingChanged(spacing) }
                    onFirstLineIndentToggled: root.firstLineIndentToggled()
                    onFormatOneClick: root.formatOneClick()
                }
            }

            // 中间组隐藏时用它把右侧公共入口推到窗口右缘。
            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: root.mode !== "writing" || root.compactActions
            }

            // ── 右侧公共入口：所有模式共用同一个容器和右缘间距 ──
            Item {
                id: trailingSlot
                Layout.fillHeight: true
                Layout.preferredWidth: root.mode === "writing" && !root.compactActions
                    && !root.rightCollapsed
                    ? Math.max(root.effectiveRightWidth, root.writingTrailingNaturalWidth)
                    : root.writingTrailingNaturalWidth
                Layout.minimumWidth: Layout.preferredWidth
                Layout.maximumWidth: Layout.preferredWidth

                GlobalTopActions {
                    id: topActions
                    anchors.right: parent.right
                    anchors.rightMargin: Kirigami.Units.largeSpacing
                    anchors.verticalCenter: parent.verticalCenter
                    dt: root.dt
                    appState: root.appState
                    syncSource: root.mode
                    compact: root.compactActions
                    onRequestSync: root.requestSync()
                    onRequestSearch: root.requestSearch()
                    onOpenSettings: root.openSettings()
                }
            }
        }
    }

    // 独立 1px 底线：从窗口 x=0 贯通到右缘，和 64 高顶栏面分开。
    Rectangle {
        id: barDivider
        Layout.fillWidth: true
        Layout.preferredHeight: 1
        implicitHeight: 1
        color: Kirigami.Theme.disabledTextColor
    }
}
