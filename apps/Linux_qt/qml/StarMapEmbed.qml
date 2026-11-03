// =============================================================================
// StarMapEmbed.qml — 子星图入口（父活动画布里的正圆壳 + 微缩概览）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：在**父活动画布**里画一个子星图入口：正圆壳（shell）+ 圆内微缩概览
//   （preview）。它只是父画布里的一个矢量对象，不是子视口。
//
// Issue #1031 第四轮：父场景只提供一种投影事实——真实 world 矩形先按 f64
// 投影成屏幕矩形，再与视口求交。根对象始终是“真实屏幕矩形 ∩ 视口”的屏幕
// 局部矩形；圆壳按真实屏幕矩形放在根对象内并由根对象裁剪。即使只露出圆的
// 一小段，裁的也是圆本身，不会把裁剪矩形退化成方形圆壳。
//
// 约束：
//   - 纯 UI 组件，数据通过 property 传入；不读写 Core
//   - 根对象是稳定 Item：x/y/width/height 与命中框恒定
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import "StarMapNodeMetrics.js" as StarMapNodeMetrics

Item {
    id: root

    required property var dt

    // 父活动场景（StarMapActiveScene）：投影 / 文字口径都从它读，
    // 本组件不自己保存第二份相机状态。
    property var parentScene: null

    // Embed 身份与数据
    property string instanceId: ""
    property string targetStarmapId: ""
    property string label: ""
    property bool isSelected: false
    property bool isPressed: false

    // 概览需要的数据入口（只读）
    property var starmapBackendRef: null
    // Issue #1032：同一目标图 + graphRevision 的共享不可变预览快照。
    // 由父活动场景统一请求 / 缓存，多个入口只读同一份数据。
    property var previewSnapshot: null

    // 父场景给出的“真实屏幕矩形 ∩ 视口”和真实屏幕矩形。两者都是屏幕
    // 像素；不再存在普通/极端两套绘制分支。
    property var screenClipRect: null
    property var trueScreenRect: null

    // Issue #949：过渡演出进度（<0 = 不在过渡中）。圆壳仍然是父图里的真实
    // Embed、世界直径仍是 200。过渡期间"同一个圆壳"由过渡层的圆环接手。
    property real revealProgress: -1
    readonly property bool revealActive: revealProgress >= 0
    readonly property bool revealOnScreen: revealActive && revealProgress < 1

    readonly property color _border: dt.border
    readonly property color _surfaceLow: dt.surfaceContainerLow
    readonly property color _inverseSurface: dt.inverseSurface
    readonly property color _shadowLight: dt.shadowLight
    readonly property color _textPrimary: dt.textPrimary

    // 父场景的文字口径：和活动画布同一份"按用户字号策略"。
    readonly property real _textScale: parentScene ? parentScene._textScale : 1

    readonly property var _clip: screenClipRect || ({
        x: 0, y: 0, width: root.width, height: root.height
    })
    readonly property var _trueRect: trueScreenRect || ({
        x: 0, y: 0, width: root.width, height: root.height
    })
    readonly property bool _fullyVisible:
        Math.abs(_clip.x - _trueRect.x) < 0.5
        && Math.abs(_clip.y - _trueRect.y) < 0.5
        && Math.abs(_clip.width - _trueRect.width) < 0.5
        && Math.abs(_clip.height - _trueRect.height) < 0.5

    // 圆壳在根对象内的真实位置。根对象只裁视口，不改变圆的形状。
    readonly property real _circleX: _trueRect.x - _clip.x
    readonly property real _circleY: _trueRect.y - _clip.y
    readonly property real _circleWidth: Math.max(0, _trueRect.width)
    readonly property real _circleHeight: Math.max(0, _trueRect.height)
    // 直径（屏幕像素）：Embed 是正圆，取投影短边。
    readonly property real _diameter: Math.min(_circleWidth, _circleHeight)
    // 圆的内接正方形边长：标题、概览都被它约束在圆内。
    readonly property real _inscribedSide: _diameter / Math.SQRT2

    // Issue #1031/1032：预览实例化按视口交集与可读直径控制。进入阈值 32px、
    // 退出阈值 20px 形成滞回，避免镜头在阈值附近抖动时反复销毁 / 重建整棵
    // 预览对象树；快照本身与相机无关，缩放只改变投影。
    property bool _previewLatched: false
    function _updatePreviewLatch() {
        if (screenClipRect === null || targetStarmapId === "") {
            _previewLatched = false
            return
        }
        if (_diameter >= 32)
            _previewLatched = true
        else if (_diameter < 20)
            _previewLatched = false
    }
    onWidthChanged: _updatePreviewLatch()
    onHeightChanged: _updatePreviewLatch()
    onTrueScreenRectChanged: _updatePreviewLatch()
    onScreenClipRectChanged: _updatePreviewLatch()
    onTargetStarmapIdChanged: _updatePreviewLatch()
    Component.onCompleted: _updatePreviewLatch()

    readonly property bool previewWanted: _previewLatched
    readonly property var previewContent: previewLoader.item
    readonly property bool childLoaded: previewSnapshot !== null
    readonly property var childBounds: previewSnapshot ? previewSnapshot.bounds : null
    readonly property real childFitScale: childBounds
            ? StarMapNodeMetrics.embedFitScale(childBounds, _diameter) : 0

    clip: true

    Loader {
        id: previewLoader
        // 真实屏幕圆，而不是根对象的裁剪矩形：fitScale 必须由入口圆直径决定。
        x: root._circleX
        y: root._circleY
        width: root._circleWidth
        height: root._circleHeight
        active: root.previewWanted
        // active 已要求可读尺寸；根对象继续裁剪部分可见的圆。
        visible: root.previewWanted
        sourceComponent: previewComponent
        // 概览画在圆壳之上、标题之下；圆壳本身是不透明表面。
        z: 1
    }

    Component {
        id: previewComponent
        StarMapSceneContent {
            dt: root.dt
            snapshot: root.previewSnapshot
            detailReveal: root.revealActive ? root.revealProgress : 1
        }
    }

    // ── 圆壳：真实屏幕圆形，由根对象的视口矩形裁剪 ──
    Rectangle {
        id: visualEmbed
        x: root._circleX
        y: root._circleY
        width: root._circleWidth
        height: root._circleHeight
        radius: width / 2
        antialiasing: true
        color: root._surfaceLow
        border.color: root._inverseSurface
        border.width: root.isSelected ? 3 : 2
        visible: !root.revealActive || root.revealOnScreen

        // 近似阴影的一圈描边（不参与命中）
        Rectangle {
            anchors.fill: parent
            anchors.margins: -1
            z: -1
            color: "transparent"
            border.color: root._shadowLight
            border.width: 1
            radius: visualEmbed.radius + 1
            visible: !root.isSelected && root._fullyVisible
        }
    }

    // ── 标题：约束在圆的内接正方形顶部带内，不需要圆形裁剪 ──
    Label {
        id: titleLabel
        visible: root._fullyVisible && root.label.length > 0
                 && root._diameter >= 56
        x: (root.width - width) / 2
        y: (root.height - root._inscribedSide) / 2 + root._inscribedSide * 0.04
        width: root._inscribedSide
        height: Math.max(14, root._inscribedSide * 0.16)
        text: root.label
        color: root._textPrimary
        font.pointSize: root.dt.fontSmPt * root._textScale
        wrapMode: Text.NoWrap
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        z: 2
    }

    // ── 选中 ring：只叠一圈主题强调色，不遮住概览 ──
    Rectangle {
        anchors.centerIn: parent
        width: root.width
        height: root.height
        radius: width / 2
        color: "transparent"
        border.color: root.dt.accent
        border.width: 2
        visible: root.isSelected && root._fullyVisible
        antialiasing: true
        z: 10
    }
}
