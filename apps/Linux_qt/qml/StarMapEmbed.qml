// =============================================================================
// StarMapEmbed.qml — 子星图入口（父活动画布里的正圆壳 + 微缩概览）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：在**父活动画布**里画一个子星图入口：正圆壳（shell）+ 圆内微缩概览
//   （preview）。它只是父画布里的一个矢量对象，不是子视口。
//
// Issue #929：独立画布协议。
//   - Embed 保留 authored world 矩形 / 位置；世界几何恒定 = 正圆直径 200。
//     屏幕上的大小由父活动画布相机投影决定（width/height 由所属场景给出），
//     不再用 scale 改世界几何，也没有 local/owner 累计比例。
//   - 父图里未进入的子星图只画 fit-to-bounds 微缩概览：StarMapSceneContent
//     把目标图真实数据压进圆的内接安全区，没有节点拖拽、没有局部相机、
//     没有递归 Loader。
//   - 删除旧递归方案：maskedContent.layer.enabled / MultiEffect 圆形 FBO 遮罩 /
//     circleMaskTexture(ShaderEffectSource) / childContentLoader / _syncChildContent /
//     contentUsableSide 全部不再存在。概览天然留在圆的安全区内，不需要
//     任何裁剪纹理，也不会被放大成活动画布。
//   - 进入子画布由 StarMapCanvas 走 Core 导航路由（enter_starmap_embed）完成；
//     切换后圆壳 / 裁剪 / 入口边框完全消失，活动画布不是被圆截住的可见区域。
//   - 本组件不解释任何输入：单击 / 双击 / 右键 / 拖动 / 长按统一由
//     StarMapInputRouter 按父场景命中结果解释。
//
// 约束：
//   - 纯 UI 组件，数据通过 property 传入；不读写 Core
//   - 根对象是稳定 Item：x/y/width/height 与命中框恒定
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls

Item {
    id: root

    required property var dt

    // 父活动场景（StarMapActiveScene）：投影 / 文字口径 / 相机比例都从它读，
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

    // Issue #1031：极端镜头下父场景交给本组件的不是 world 尺寸，而是
    // “真实屏幕投影矩形 ∩ 视口”的屏幕局部矩形。trueScreenRect 保留原始
    // 边界，用来判断圆壳是否只是被裁掉一部分。
    property bool extremeClipped: false
    property var screenClipRect: null
    property var trueScreenRect: null

    // Issue #949：过渡演出进度（<0 = 不在过渡中）。圆壳仍然是父图里的真实
    // Embed、世界直径仍是 200。过渡期间"同一个圆壳"由过渡层的圆环接手：
    // 进场时圆环在进度 0 与本圆壳完全重合（没有一帧突变），之后按完整
    // progress 放大到整圈离开可见视口；出场时圆环从当前帧入口圆投影收回到
    // 目标父帧的 Embed 圆壳投影，过渡结束（进度 1）再交回真实圆壳。
    // revealProgress 是内容态（跟随 contentProgress）：圆内概览的圆点 →
    // 节点在两层里同步展开，不用覆盖率阈值决定圆壳可见性。
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
    // 常规镜头：本组件位于父场景统一的 worldContent 变换内，宽度 / 字号 /
    // 边框都按 authored world 单位计算，屏幕口径通过除以父级 _safeZoom 保持
    // 恒定。极端镜头：根对象已是屏幕坐标，所有口径直接用像素。
    readonly property real _cameraZoom: parentScene && parentScene._safeZoom > 0
                                        ? parentScene._safeZoom : 1
    readonly property bool _extreme: extremeClipped
    readonly property bool _fullyVisible: !_extreme
        || (screenClipRect !== null && trueScreenRect !== null
            && Math.abs(screenClipRect.x - trueScreenRect.x) < 0.5
            && Math.abs(screenClipRect.y - trueScreenRect.y) < 0.5
            && Math.abs(screenClipRect.width - trueScreenRect.width) < 0.5
            && Math.abs(screenClipRect.height - trueScreenRect.height) < 0.5)

    // 直径（世界单位）：Embed 是正圆，取投影短边。
    readonly property real _diameter: Math.min(width, height)
    // 圆的内接正方形边长：标题、概览都被它约束在圆内。
    readonly property real _inscribedSide: _diameter / Math.SQRT2

    // 微缩概览：只在该入口投影足够大时创建（小到看不清就不做无谓加载）。
    // 概览的 fitScale 同时是"进入该子图后的子画布比例"：加载成功后父画布
    // 穿越子图时用同一份比例，切画布前后画面连续。
    readonly property bool previewWanted:
        (!_extreme || _fullyVisible)
        && targetStarmapId !== "" && width * _cameraZoom >= 32
        && height * _cameraZoom >= 32
    readonly property var previewContent: previewLoader.item
    readonly property var childBounds: previewContent ? previewContent.childBounds : null
    readonly property real childFitScale: previewContent ? previewContent.fitScale : 0

    // Issue #1029：逐节点无限 wobble 动画已移除；不再保留视觉偏移属性。

    Loader {
        id: previewLoader
        anchors.fill: parent
        active: root.previewWanted
        sourceComponent: previewComponent
        // 概览画在圆壳之上、标题之下；圆壳本身是不透明表面。
        z: 1
    }

    Component {
        id: previewComponent
        StarMapSceneContent {
            dt: root.dt
            starmapId: root.targetStarmapId
            starmapBackendRef: root.starmapBackendRef
            detailReveal: root.revealActive ? root.revealProgress : 1
        }
    }

    // ── 圆壳：父画布里的入口本体（世界几何恒定，屏幕大小由父相机投影）──
    Rectangle {
        id: visualEmbed
        anchors.fill: parent
        // 极端镜头被视口裁掉时只填相交区域，不把裁剪矩形伪装成圆壳边界；
        // 圆壳真实边线交给父场景的边框投影处理（此处不再画假圆角）。
        radius: root._fullyVisible ? width / 2 : 0
        antialiasing: true
        // 子星图像一块独立的主题表面：浅色主题浅底 + 深色反边，反之相反。
        // 过渡期间仍继续绘制（圆环放大描边由过渡层按同一个圆壳几何画在
        // 内容之下），只有整圈离开可见视口后才停。
        color: root._surfaceLow
        border.color: root._inverseSurface
        border.width: root._extreme
                      ? (root._fullyVisible ? (root.isSelected ? 3 : 2) : 0)
                      : (root.isSelected ? 3 : 2) / root._cameraZoom
        visible: !root.revealActive || root.revealOnScreen

        // 近似阴影的一圈描边（不参与命中）
        Rectangle {
            anchors.fill: parent
            anchors.margins: root._extreme ? -1 : -1 / root._cameraZoom
            z: -1
            color: "transparent"
            border.color: root._shadowLight
            radius: root._fullyVisible
                    ? visualEmbed.radius + (root._extreme ? 1 : 1 / root._cameraZoom) : 0
            visible: !root.isSelected && (!root._extreme || root._fullyVisible)
        }
    }

    // ── 标题：约束在圆的内接正方形顶部带内，不需要圆形裁剪 ──
    Label {
        id: titleLabel
        visible: (!root._extreme || root._fullyVisible)
                 && root.label.length > 0 && root._diameter * root._cameraZoom >= 56
        x: (root.width - width) / 2
        y: (root.height - root._inscribedSide) / 2 + root._inscribedSide * 0.04
        width: root._inscribedSide
        height: Math.max(root._extreme ? 14 : 14 / root._cameraZoom,
                         root._inscribedSide * 0.16)
        text: root.label
        color: root._textPrimary
        font.pointSize: root.dt.fontSmPt * root._textScale
                         / (root._extreme ? 1 : root._cameraZoom)
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
        border.width: root._extreme ? 2 : 2 / root._cameraZoom
        visible: root.isSelected && (!root._extreme || root._fullyVisible)
        antialiasing: true
        z: 10
    }
}
