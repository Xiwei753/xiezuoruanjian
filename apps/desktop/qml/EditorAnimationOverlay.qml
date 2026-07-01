// =============================================================================
// EditorAnimationOverlay.qml — QML overlay animation layer for SujianEditorItem
// =============================================================================
// Consumes EditorVisualTransaction JSON from Rust core and renders short-lived
// insert/delete animations as QML items. This is the only animation route:
// Core transaction → visual_transaction_json signal → QML overlay rendering.
// The static text texture (Layer 0) and QML cursor remain unchanged.
//
// Unified animation decision rules (must match Rust should_create_text_animation):
// - glyph 为空：不动画
// - glyph 超过上限（8）：不动画
// - 包含换行：文字不动画，但光标仍可动画
// - scrolling/loading/settings/applyingFormat：不动画
// - component not ready：不动画
// - 不动画时绝对不能 start_insert hidden range
// - 如果已经 start_insert 但 overlay 跳过，必须立即通知 Rust 清除 hidden range
//
// Animation mechanism:
// - Insert: during animation, the static text layer (Layer 0) temporarily skips the inserted range.
//   This is an internal rendering state of the self-rendered editor, NOT text data pollution.
//   The overlay renders ghost glyphs that fly from the cursor to their final positions.
//   When animation finishes, insertAnimationFinished signal notifies Rust to clear the hidden range
//   and restore full text rendering.
// - Delete: the overlay holds old glyph snapshots (pre-deletion positions) and renders them
//   flying back toward the cursor. The static text layer already reflects the post-deletion state.
// - The overlay is an animation layer, NOT a full text overlay masquerading as real text rendering.

import QtQuick

Item {
    id: root

    property var editorItem: null
    property var dt: null
    property bool animationEnabled: true
    property bool suppressed: false

    // 动画调试日志控制。默认关闭，避免正式包刷屏。
    // 可通过设置 root.verboseLogging = true 或环境变量 SUJIAN_EDITOR_DEBUG 开启。
    property bool verboseLogging: false

    // ── 真吐字/吞字动画信号 ──
    // Insert 动画完成时通知 Rust 侧清除 hidden range（临时渲染状态），恢复正文完整绘制
    // hidden range 是自研渲染层的内部渲染状态，不是正文数据污染
    signal insertAnimationFinished(int byteStart, int byteEnd)

    // Insert 动画被跳过时通知 Rust 侧清除 hidden range
    // 当 QML overlay 因 component not ready / glyph 超限 / 换行等原因跳过动画时，
    // Rust 侧可能已经创建了 hidden range，必须立即清除否则文字消失
    signal insertAnimationSkipped(int byteStart, int byteEnd)

    function _log(message) {
        if (verboseLogging) console.log("[AnimOverlay] " + message)
    }

    visible: editorItem !== null

    property var _activeAnimations: []
    // 当前活跃事务列表（每个 insert 动画一个事务，支持连续输入不覆盖）
    property var _activeTransactions: []
    property Component _glyphGhostComponent: Qt.createComponent("EditorGlyphGhost.qml")

    /// 统一动画判定函数 — 与 Rust should_create_text_animation 使用相同规则
    /// 返回值: "noAnimation" / "cursorOnly" / "fullAnimation"
    function shouldCreateTextAnimation(glyphCount, containsNewline, isScrolling, isLoading, isApplyingFormat, isApplyingSettings, animEnabled, componentReady) {
        var MAX_GLYPH_COUNT = 8

        if (!animEnabled) return "noAnimation"
        if (isScrolling || isLoading || isApplyingFormat || isApplyingSettings) return "noAnimation"
        if (!componentReady) return "noAnimation"
        if (glyphCount === 0) return "noAnimation"
        if (glyphCount > MAX_GLYPH_COUNT) return "noAnimation"
        if (containsNewline) return "cursorOnly"
        return "fullAnimation"
    }

    Connections {
        target: editorItem
        function onVisualTransactionChanged() {
            if (!root.animationEnabled || root.suppressed) {
                root._log("skipped: animationEnabled=" + root.animationEnabled + " suppressed=" + root.suppressed)
                // 当动画被跳过时，不会有 insert 动画创建，因此不会有 hidden range 需要清理
                // Rust 侧在 suppressed/animationDisabled 场景下不应创建 hidden range
                return
            }
            var jsonStr = editorItem.visual_transaction_json
            if (!jsonStr || jsonStr === "{}") {
                root._log("skipped: jsonStr empty or {}")
                return
            }
            root._log("received: jsonLen=" + jsonStr.length)
            var vt
            try {
                vt = JSON.parse(jsonStr)
            } catch (e) {
                root._log("JSON parse error: " + e)
                return
            }
            if (!vt || !vt.kind) {
                root._log("skipped: vt missing kind field")
                return
            }
            root._log("vt kind=" + vt.kind + " durationMs=" + vt.durationMs)
            root._handleTransaction(vt)
        }
    }

    onSuppressedChanged: {
        if (suppressed) {
            // suppressed 时必须立即清理所有动画和 pending 状态
            // 确保 Rust 侧的 hidden range 也被清除（通过 clearAll → 不发射 insertAnimationFinished）
            // 注意：clearAll 不会发射 insertAnimationFinished，所以 Rust 侧的 hidden range
            // 需要在 suppressed 场景下由 Rust 侧自行清理
            root.clearAll()
        }
    }

    function isComplexGrapheme(ch) {
        if (!ch || ch.length === 0) return false
        // Check for surrogate pairs (code point > 0xFFFF)
        var cp = ch.codePointAt(0)
        if (cp > 0xFFFF) return true
        // Zero Width Joiner
        if (cp === 0x200D) return true
        // Variation selectors
        if ((cp >= 0xFE00 && cp <= 0xFE0F) || (cp >= 0xE0100 && cp <= 0xE01EF)) return true
        // Combining marks (general category Mn, Mc, Me)
        if (cp >= 0x0300 && cp <= 0x036F) return true  // Combining Diacritical Marks
        if (cp >= 0x1AB0 && cp <= 0x1AFF) return true  // Combining Diacritical Marks Extended
        if (cp >= 0x1DC0 && cp <= 0x1DFF) return true  // Combining Diacritical Marks Supplement
        if (cp >= 0x20D0 && cp <= 0x20FF) return true  // Combining Diacritical Marks for Symbols
        if (cp >= 0xFE20 && cp <= 0xFE2F) return true  // Combining Half Marks
        return false
    }

    function _handleTransaction(vt) {
        if (!vt) return
        var kind = vt.kind
        if (kind === "insert") {
            _createInsertAnimation(vt)
        } else if (kind === "delete") {
            _createDeleteAnimation(vt)
        }
        // Cursor kind: 不需要动画
    }

    function _trackGhost(ghost, animKind, byteStart, byteEnd) {
        if (!ghost) return
        root._activeAnimations.push(ghost)
        ghost.animationFinished.connect(function() {
            var idx = root._activeAnimations.indexOf(ghost)
            if (idx >= 0) root._activeAnimations.splice(idx, 1)

            if (animKind === "insert") {
                // 找到对应的事务（按 byteStart/byteEnd 匹配）
                for (var ti = 0; ti < root._activeTransactions.length; ti++) {
                    var tx = root._activeTransactions[ti]
                    if (tx.byteStart === byteStart && tx.byteEnd === byteEnd) {
                        tx.pendingCount--
                        root._log("insert ghost finished, pendingCount=" + tx.pendingCount + " for byteRange=(" + byteStart + "," + byteEnd + ")")
                        if (tx.pendingCount <= 0) {
                            root._log("insert transaction finished, notifying Rust: byteStart=" + byteStart + " byteEnd=" + byteEnd)
                            root.insertAnimationFinished(byteStart, byteEnd)
                            root._activeTransactions.splice(ti, 1)
                        }
                        break
                    }
                }
            }
            ghost.destroy()
        })
        ghost.startAnimation()
    }

    function _createInsertAnimation(vt) {
        // 使用 vt.oldCursorRect 作为起点（插入前的光标位置）
        // 使用 baselineY 作为 Y 坐标（文字基线），回退到 top（兼容旧数据）
        var startX = vt.oldCursorRect ? vt.oldCursorRect.x : editorItem.cursor_rect_x
        var startY = vt.oldCursorRect ? (vt.oldCursorRect.baselineY || vt.oldCursorRect.top) : editorItem.cursor_rect_y
        var duration = Math.max(30, Math.min(1000, vt.durationMs || 100))

        // 从 vt.insertGlyphRects 读取 glyph 位置
        var glyphRects = vt.insertGlyphRects
        if (!glyphRects || !Array.isArray(glyphRects) || glyphRects.length === 0) {
            root._log("insert skipped: insertGlyphRects empty")
            // 通知 Rust 清除可能已创建的 hidden range
            _notifySkippedIfHasRange(vt)
            return
        }

        // byte range for insert animation — used to notify Rust when animation finishes
        // 从 vt.insertedRange 读取 hidden range
        var insertByteStart = 0
        var insertByteEnd = 0
        if (vt.insertedRange && Array.isArray(vt.insertedRange) && vt.insertedRange.length === 2) {
            insertByteStart = vt.insertedRange[0]
            insertByteEnd = vt.insertedRange[1]
        }

        // 检测是否包含换行
        var containsNewline = false
        for (var ci = 0; ci < glyphRects.length; ci++) {
            if (glyphRects[ci].char === "\n" || glyphRects[ci].char === "\r") {
                containsNewline = true
                break
            }
        }

        // ── 统一动画判定 ──
        // 必须与 Rust should_create_text_animation 使用相同规则
        var component = root._glyphGhostComponent
        var componentReady = component && component.status === Component.Ready

        var decision = root.shouldCreateTextAnimation(
            glyphRects.length,    // glyphCount
            containsNewline,      // containsNewline
            false,                // isScrolling (handled via suppressed on QML side)
            root.suppressed,      // isLoading (suppressed covers loading)
            false,                // isApplyingFormat (suppressed covers this)
            false,                // isApplyingSettings (suppressed covers this)
            root.animationEnabled, // animationEnabled
            componentReady        // componentReady
        )

        root._log("insert decision=" + decision + " glyphCount=" + glyphRects.length + " containsNewline=" + containsNewline + " componentReady=" + componentReady)

        if (decision === "noAnimation") {
            root._log("insert skipped: decision=noAnimation")
            // 通知 Rust 清除可能已创建的 hidden range
            _notifySkippedIfHasRange(vt)
            return
        }

        if (decision === "cursorOnly") {
            root._log("insert skipped: decision=cursorOnly (newline), cursor still animates")
            // 换行场景：不创建文字 ghost，但光标仍可动画
            // 通知 Rust 清除可能已创建的 hidden range
            _notifySkippedIfHasRange(vt)
            return
        }

        // decision === "fullAnimation"

        // 统计实际会创建的 ghost 数量（排除复杂 grapheme）
        var ghostCount = 0
        for (var i = 0; i < glyphRects.length; i++) {
            var gr = glyphRects[i]
            if (!isComplexGrapheme(gr.char)) {
                ghostCount++
            }
        }

        if (ghostCount === 0) {
            root._log("insert skipped: no valid ghosts after filtering")
            // 通知 Rust 清除可能已创建的 hidden range
            _notifySkippedIfHasRange(vt)
            return
        }

        // 记录当前事务的 pending ghost 数量（push 到事务列表，支持连续输入不覆盖）
        root._activeTransactions.push({ pendingCount: ghostCount, byteStart: insertByteStart, byteEnd: insertByteEnd })

        root._log("insert creating " + ghostCount + " ghosts, cursorRect=(" + startX + "," + startY + ") byteRange=(" + insertByteStart + "," + insertByteEnd + ")")

        for (var i = 0; i < glyphRects.length; i++) {
            var gr = glyphRects[i]
            // 防御性检查：Rust 侧已过滤复杂字符，这里双重保险
            if (isComplexGrapheme(gr.char)) {
                root._log("insert skipped: complex grapheme at index=" + i)
                continue
            }
            var ghost = component.createObject(root, {
                "animKind": "insert",
                "startX": startX,
                "startY": startY,
                "endX": gr.x,
                "endY": gr.baselineY || gr.y,
                "glyphWidth": gr.w,
                "glyphHeight": gr.h,
                "glyphBaselineY": gr.baselineY || 0,
                "width": gr.w,
                "height": gr.h,
                "duration": duration,
                "ghostColor": editorItem.text_color || root.dt.editorText,
                "glyphText": isComplexGrapheme(gr.char) ? "" : (gr.char || ""),
                "glyphFontFamily": editorItem.font_family || "",
                "glyphFontPixelSize": editorItem.font_pixel_size || 0
            })

            _trackGhost(ghost, "insert", insertByteStart, insertByteEnd)
        }

        // ── 局部 reflow 动画：方案 B — 暂不创建 reflow ghost ──
        // 中间插入时，右侧文字直接 snap 到最终位置，不做位移动画。
        // 原因：reflow ghost（opacity=1）和静态层最终 glyph 同时可见，产生重影。
        // 未来如需恢复 reflow 动画，需要同时让静态层跳过 reflow range。
    }

    /// 当 QML overlay 跳过 Insert 动画时，通知 Rust 清除可能已创建的 hidden range。
    /// 这是防止文字短暂消失的关键：Rust 可能已经在 record_transaction 中
    /// 创建了 hidden range，但 QML 因各种原因跳过了动画。
    function _notifySkippedIfHasRange(vt) {
        if (vt.insertedRange && Array.isArray(vt.insertedRange) && vt.insertedRange.length === 2) {
            var byteStart = vt.insertedRange[0]
            var byteEnd = vt.insertedRange[1]
            root._log("insert skipped, notifying Rust to clear hidden range: byteStart=" + byteStart + " byteEnd=" + byteEnd)
            root.insertAnimationSkipped(byteStart, byteEnd)
        }
    }

    function _createDeleteAnimation(vt) {
        // 使用 vt.newCursorRect 作为终点（删除后的新光标位置）
        // 使用 baselineY 作为 Y 坐标（文字基线），回退到 top（兼容旧数据）
        var endX = vt.newCursorRect ? vt.newCursorRect.x : editorItem.cursor_rect_x
        var endY = vt.newCursorRect ? (vt.newCursorRect.baselineY || vt.newCursorRect.top) : editorItem.cursor_rect_y
        var duration = Math.max(30, Math.min(1000, vt.durationMs || 100))

        // 从 vt.deletedGlyphRects 读取被删 glyph 位置
        var glyphRects = vt.deletedGlyphRects
        if (!glyphRects || !Array.isArray(glyphRects) || glyphRects.length === 0) {
            root._log("delete skipped: deletedGlyphRects empty")
            return
        }

        // ── 统一动画判定 ──
        var containsNewline = false
        for (var ci = 0; ci < glyphRects.length; ci++) {
            if (glyphRects[ci].char === "\n" || glyphRects[ci].char === "\r") {
                containsNewline = true
                break
            }
        }

        var component = root._glyphGhostComponent
        var componentReady = component && component.status === Component.Ready

        var decision = root.shouldCreateTextAnimation(
            glyphRects.length,
            containsNewline,
            false,
            root.suppressed,
            false,
            false,
            root.animationEnabled,
            componentReady
        )

        root._log("delete decision=" + decision + " glyphCount=" + glyphRects.length + " containsNewline=" + containsNewline + " componentReady=" + componentReady)

        if (decision !== "fullAnimation") {
            root._log("delete skipped: decision=" + decision)
            return
        }

        root._log("delete creating " + glyphRects.length + " ghosts, cursorRect=(" + endX + "," + endY + ")")

        for (var i = 0; i < glyphRects.length; i++) {
            var gr = glyphRects[i]
            // 防御性检查：Rust 侧已过滤复杂字符，这里双重保险
            if (isComplexGrapheme(gr.char)) {
                root._log("delete skipped: complex grapheme at index=" + i)
                continue
            }
            var ghost = component.createObject(root, {
                "animKind": "delete",
                "startX": gr.x,
                "startY": gr.baselineY || gr.y,
                "endX": endX,
                "endY": endY,
                "glyphWidth": gr.w,
                "glyphHeight": gr.h,
                "glyphBaselineY": gr.baselineY || 0,
                "glyphWidth": gr.w,
                "glyphHeight": gr.h,
                "width": gr.w,
                "height": gr.h,
                "duration": duration,
                "ghostColor": editorItem.text_color || root.dt.editorText,
                "glyphText": isComplexGrapheme(gr.char) ? "" : (gr.char || ""),
                "glyphFontFamily": editorItem.font_family || "",
                "glyphFontPixelSize": editorItem.font_pixel_size || 0
            })

            _trackGhost(ghost, "delete", 0, 0)
        }
    }

    function clearAll() {
        // 销毁所有活跃动画
        // 注意：此函数不发射 insertAnimationFinished 信号
        // Rust 侧需要在 suppressed/component not ready/animation disabled 场景下
        // 自行确保不创建或立即清理 hidden range
        for (var i = 0; i < root._activeAnimations.length; i++) {
            if (root._activeAnimations[i]) root._activeAnimations[i].destroy()
        }
        root._activeAnimations = []
        root._activeTransactions = []
    }
}
