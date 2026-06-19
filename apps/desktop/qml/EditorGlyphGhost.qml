// =============================================================================
// EditorGlyphGhost.qml — 单个 glyph ghost 动画效果
// =============================================================================
// 渲染吐字（insert）和吞字（delete）动画。
// insert: scale 0.6→1.0, opacity 0→0.6, 从光标位置位移到 glyph 位置
// delete: scale 1.0→0.4, opacity 0.7→0, 从 glyph 位置位移到光标位置
// 动画结束后自动 destroy。

import QtQuick

Item {
    id: root

    // 动画类型
    property string animKind: "insert"  // "insert" or "delete"

    // 起始位置（insert: 光标位置; delete: glyph 位置）
    property real startX: 0
    property real startY: 0

    // 终止位置（insert: glyph 位置; delete: 光标位置）
    property real endX: 0
    property real endY: 0

    // glyph 尺寸
    property real glyphWidth: 0
    property real glyphHeight: 0

    // 动画时长
    property int duration: 160

    // 颜色
    property color ghostColor: "#E2E2E5"

    // glyph 文字内容
    property string glyphText: ""

    // 真实字体信息（由外部传入，确保 ghost 和正文一致）
    property string glyphFontFamily
    property real glyphFontPixelSize

    signal animationFinished()

    // ── Insert 动画属性 ──

    property var insertMoveAnim: QtObject {
        property real x: root.startX
        property real y: root.startY
    }

    property var insertOpacityAnim: QtObject {
        property real currentOpacity: 0
    }

    // ── Delete 动画属性 ──

    property var deleteMoveAnim: QtObject {
        property real x: root.startX
        property real y: root.startY
    }

    property var deleteOpacityAnim: QtObject {
        property real currentOpacity: 1.0
    }

    // 当前位置（动画中间值）
    x: root.animKind === "insert" ? root.insertMoveAnim.x : root.deleteMoveAnim.x
    y: root.animKind === "insert" ? root.insertMoveAnim.y : root.deleteMoveAnim.y

    // 用 Text 渲染真实 glyph 文字
    Text {
        id: ghostText
        text: root.glyphText
        color: root.ghostColor
        font.family: root.glyphFontFamily || undefined
        font.pixelSize: root.glyphFontPixelSize > 0 ? root.glyphFontPixelSize : root.glyphHeight * 0.85
        opacity: root.animKind === "insert" ? root.insertOpacityAnim.currentOpacity : root.deleteOpacityAnim.currentOpacity
    }

    // ── Insert 并行动画组 ──

    ParallelAnimation {
        id: insertAnim
        running: false

        NumberAnimation {
            target: root.insertMoveAnim
            property: "x"
            from: root.startX
            to: root.endX
            duration: root.duration
            easing.type: Easing.OutCubic
        }
        NumberAnimation {
            target: root.insertMoveAnim
            property: "y"
            from: root.startY
            to: root.endY
            duration: root.duration
            easing.type: Easing.OutCubic
        }
        NumberAnimation {
            target: root.insertOpacityAnim
            property: "currentOpacity"
            from: 0.0
            to: 0.6
            duration: root.duration
            easing.type: Easing.OutCubic
        }
        ScaleAnimator {
            target: ghostText
            from: 0.6
            to: 1.0
            duration: root.duration
            easing.type: Easing.OutCubic
        }

        onFinished: root.animationFinished()
    }

    // ── Delete 并行动画组 ──

    ParallelAnimation {
        id: deleteAnim
        running: false

        NumberAnimation {
            target: root.deleteMoveAnim
            property: "x"
            from: root.startX
            to: root.endX
            duration: root.duration
            easing.type: Easing.InCubic
        }
        NumberAnimation {
            target: root.deleteMoveAnim
            property: "y"
            from: root.startY
            to: root.endY
            duration: root.duration
            easing.type: Easing.InCubic
        }
        NumberAnimation {
            target: root.deleteOpacityAnim
            property: "currentOpacity"
            from: 0.7
            to: 0.0
            duration: root.duration
            easing.type: Easing.InCubic
        }
        ScaleAnimator {
            target: ghostText
            from: 1.0
            to: 0.4
            duration: root.duration
            easing.type: Easing.InCubic
        }

        onFinished: root.animationFinished()
    }

    function startAnimation() {
        if (root.animKind === "insert") {
            insertAnim.start()
        } else {
            deleteAnim.start()
        }
    }
}