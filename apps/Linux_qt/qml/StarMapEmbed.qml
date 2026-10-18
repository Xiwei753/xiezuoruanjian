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
    property int wobbleIndex: 0

    // 概览需要的数据入口（只读）
    property var starmapBackendRef: null

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

    // 直径（屏幕像素）：Embed 是正圆，取投影短边。
    readonly property real _diameter: Math.min(width, height)
    // 圆的内接正方形边长：标题、概览都被它约束在圆内。
    readonly property real _inscribedSide: _diameter / Math.SQRT2

    // 微缩概览：只在该入口投影足够大时创建（小到看不清就不做无谓加载）。
    // 概览的 fitScale 同时是"进入该子图后的子画布比例"：加载成功后父画布
    // 穿越子图时用同一份比例，切画布前后画面连续。
    readonly property bool previewWanted:
        targetStarmapId !== "" && width >= 32 && height >= 32
    readonly property var previewContent: previewLoader.item
    readonly property var childBounds: previewContent ? previewContent.childBounds : null
    readonly property real childFitScale: previewContent ? previewContent.fitScale : 0

    // 纯视觉偏移（wobble），不影响根 Item 的 x/y 命中测试
    property real visualOffsetX: (isSelected || isPressed) ? 0 : _wobbleAnimX
    property real visualOffsetY: (isSelected || isPressed) ? 0 : _wobbleAnimY
    property real _wobbleAnimX: 0
    property real _wobbleAnimY: 0

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
        radius: width / 2
        antialiasing: true
        // 子星图像一块独立的主题表面：浅色主题浅底 + 深色反边，反之相反。
        // 过渡期间仍继续绘制（圆环放大描边由过渡层按同一个圆壳几何画在
        // 内容之下），只有整圈离开可见视口后才停。
        color: root._surfaceLow
        border.color: root._inverseSurface
        border.width: root.isSelected ? 3 : 2
        visible: !root.revealActive || root.revealOnScreen

        transform: Translate {
            x: root.visualOffsetX
            y: root.visualOffsetY
        }

        // 近似阴影的一圈描边（不参与命中）
        Rectangle {
            anchors.fill: parent
            anchors.margins: -1
            z: -1
            color: "transparent"
            border.color: root._shadowLight
            radius: visualEmbed.radius + 1
            visible: !root.isSelected
        }
    }

    // ── 标题：约束在圆的内接正方形顶部带内，不需要圆形裁剪 ──
    Label {
        id: titleLabel
        visible: root.label.length > 0 && root._diameter >= 56
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
        visible: root.isSelected
        antialiasing: true
        z: 10
    }

    // wobble 降速，和 StarMapNode.qml 一致；选中/按下/拖动时动画暂停。
    SequentialAnimation on _wobbleAnimX {
        loops: Animation.Infinite
        running: !root.isSelected && !root.isPressed
        NumberAnimation { to: 0.6; duration: 7000 + (root.wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.6; duration: 7000 + (root.wobbleIndex % 7) * 400; easing.type: Easing.InOutSine }
    }
    SequentialAnimation on _wobbleAnimY {
        loops: Animation.Infinite
        running: !root.isSelected && !root.isPressed
        NumberAnimation { to: 0.4; duration: 8500 + (root.wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
        NumberAnimation { to: -0.4; duration: 8500 + (root.wobbleIndex % 5) * 300; easing.type: Easing.InOutSine }
    }
}
