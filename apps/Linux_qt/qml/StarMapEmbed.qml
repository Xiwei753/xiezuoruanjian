// =============================================================================
// StarMapEmbed.qml — 子星图入口（父活动画布里的正圆壳 + 微缩概览）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：在**父活动画布**里画一个子星图入口的微缩概览与标题。
//
// Issue #1035：入口圆壳（填充 + 描边 + 选中描边）由 StarMapObjectLayer 用
// 完整 world 几何批量绘制；本组件不再接收“视口 ∩ 圆”的裁剪矩形，也不再
// 用它改写根对象尺寸。根对象就是真实投影圆，部分可见时只由场景视口裁像素。
// 预览内容需要真正的圆形边界，使用 StarMapCircleClip 自己的圆形 clip node，
// 而不是把整个入口矩形 clip 掉。
// =============================================================================

import QtQuick
import QtQuick.Controls
import Sujian 1.0
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

    // Issue #949：过渡演出进度（<0 = 不在过渡中）。圆壳仍然是父图里的真实
    // Embed、世界直径仍是 200。过渡期间“同一个圆壳”由过渡层的圆环接手。
    property real revealProgress: -1
    readonly property bool revealActive: revealProgress >= 0
    readonly property bool revealOnScreen: revealActive && revealProgress < 1

    readonly property color _textPrimary: dt.textPrimary

    // 父场景的文字口径：和活动画布同一份“按用户字号策略”。
    readonly property real _textScale: parentScene ? parentScene._textScale : 1

    // 直径（屏幕像素）：Embed 是正圆，取投影短边。尺寸来自完整 world 几何，
    // 不是视口交集。
    readonly property real _diameter: Math.min(root.width, root.height)
    // 圆的内接正方形边长：标题、概览都被它约束在圆内。
    readonly property real _inscribedSide: _diameter / Math.SQRT2

    // Issue #1031/1032：预览实例化按可读直径控制。进入阈值 32px、退出阈值
    // 20px 形成滞回，避免镜头在阈值附近抖动时反复销毁 / 重建整棵预览对象树；
    // 快照本身与相机无关，缩放只改变投影。
    property bool _previewLatched: false
    function _updatePreviewLatch() {
        if (targetStarmapId === "") {
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
    onTargetStarmapIdChanged: _updatePreviewLatch()
    Component.onCompleted: _updatePreviewLatch()

    readonly property bool previewWanted: _previewLatched && !revealOnScreen
    readonly property var previewContent: previewLoader.item
    readonly property bool childLoaded: previewSnapshot !== null
    readonly property var childBounds: previewSnapshot ? previewSnapshot.bounds : null
    readonly property real childFitScale: childBounds
            ? StarMapNodeMetrics.embedFitScale(childBounds, _diameter) : 0

    // 预览用真正的圆形 clip node 约束在入口圆内，并留出原生描边宽度，
    // 不把整个入口矩形当作裁剪边界。
    StarMapCircleClip {
        id: previewMask
        anchors.fill: parent
        anchors.margins: root.isSelected ? 3 : 2
        clipEnabled: root.previewWanted
        clipCenterX: width / 2
        clipCenterY: height / 2
        clipRadius: Math.min(width, height) / 2
        z: 1

        Loader {
            id: previewLoader
            anchors.fill: parent
            active: root.previewWanted
            visible: root.previewWanted
            sourceComponent: previewComponent
        }
    }

    Component {
        id: previewComponent
        StarMapSceneContent {
            dt: root.dt
            snapshot: root.previewSnapshot
            detailReveal: root.revealActive ? root.revealProgress : 1
        }
    }

    // ── 标题：约束在圆的内接正方形顶部带内，不需要圆形裁剪 ──
    Label {
        id: titleLabel
        visible: root.label.length > 0 && root._diameter >= 56
                 && !root.revealOnScreen
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
}
