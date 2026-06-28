// =============================================================================
// EditorGlyphGhost.qml — 单个 glyph ghost 动画效果
// =============================================================================
// 渲染吐字（insert）和吞字（delete）动画。
//
// 真吐字/吞字模式：
// insert: 正文层跳过 inserted range 不绘制，ghost 从光标位置"吐出"到 glyph 位置
//   - opacity: 0 → 1.0 → 0（动画期间可见，因为正文层跳过了它）
//   - scale: 0.72 → 1.0
//   - position: 光标 → glyph
// delete: 正文层正常绘制 new_text，ghost 从 glyph 位置"吞入"到光标位置
//   - opacity: 0.85 → 0
//   - scale: 1.0 → 0.45
//   - position: glyph → 光标
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

    // 动画时长（由外部传入，默认 100ms，范围 30~1000ms）
    property int duration: 100

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
        font.family: root.glyphFontFamily || "serif"
        font.pixelSize: root.glyphFontPixelSize > 0 ? root.glyphFontPixelSize : root.glyphHeight * 0.85
        opacity: root.animKind === "insert" ? root.insertOpacityAnim.currentOpacity : root.deleteOpacityAnim.currentOpacity
    }

    // ── Insert 并行动画组 ──
    // 真吐字模式：正文层跳过 inserted range，ghost 必须可见
    // opacity: 0 → 1.0（动画期间可见）→ 0（最后快速淡出，正文层恢复绘制）
    // scale: 0.72 → 1.0
    // position: cursor → glyph

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
        SequentialAnimation {
            // 前半段：淡入到完全可见（因为正文层跳过了 inserted range）
            NumberAnimation {
                target: root.insertOpacityAnim
                property: "currentOpacity"
                from: 0.0
                to: 1.0
                duration: root.duration * 0.4
                easing.type: Easing.OutCubic
            }
            // 后半段：保持可见然后快速淡出（正文层即将恢复绘制）
            NumberAnimation {
                target: root.insertOpacityAnim
                property: "currentOpacity"
                from: 1.0
                to: 0.0
                duration: root.duration * 0.6
                easing.type: Easing.InCubic
            }
        }
        ScaleAnimator {
            target: ghostText
            from: 0.72
            to: 1.0
            duration: root.duration
            easing.type: Easing.OutCubic
        }

        onFinished: root.animationFinished()
    }

    // ── Delete 并行动画组 ──
    // 真吞字模式：正文层正常绘制 new_text，ghost 显示被删除的 glyph "吞入"光标
    // opacity: 1.0 → 0（从完全可见到消失）
    // scale: 1.0 → 0.45
    // position: glyph → cursor

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
            from: 1.0
            to: 0.0
            duration: root.duration
            easing.type: Easing.InCubic
        }
        ScaleAnimator {
            target: ghostText
            from: 1.0
            to: 0.45
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
