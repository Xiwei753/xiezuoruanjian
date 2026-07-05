// =============================================================================
// EditorAnimationOverlay.qml — QML overlay animation layer for SujianEditorItem
// =============================================================================
// Consumes EditorVisualTransaction JSON from Rust core and renders short-lived
// insert/delete animations as QML items. This is the only animation route:
// Core transaction → visual_transaction_json signal → QML overlay rendering.
// The static text texture (Layer 0) and QML cursor remain unchanged.
//
// Unified animation mode rules (must match Rust choose_animation_mode):
// - cluster 为空：systemSuppressed
// - 系统抑制条件（动画关闭/滚动/加载/格式化/设置变化）：systemSuppressed
// - 包含换行：lineReflowAnimation（创建 hidden range + reflow 动画）
// - 包含复杂 grapheme：clusterAnimation（整簇作为单个 ghost）
// - cluster 数量 1–8：glyphAnimation
// - cluster 数量 9–40：runAnimation
// - cluster 数量 > 40：snapshotAnimation
// - systemSuppressed 时不创建 hidden range
// - 非 systemSuppressed 时都创建 hidden range
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

    /// 统一动画模式选择函数 — 与 Rust choose_animation_mode 使用相同规则
    /// 返回值: "glyphAnimation" / "clusterAnimation" / "runAnimation" / "lineReflowAnimation" / "snapshotAnimation" / "systemSuppressed"
    function chooseAnimationMode(clusterCount, containsNewline, containsComplexGrapheme, isScrolling, isLoading, isApplyingFormat, isApplyingSettings, animEnabled, componentReady) {
        if (!animEnabled) return "systemSuppressed"
        if (isScrolling || isLoading || isApplyingFormat || isApplyingSettings) return "systemSuppressed"
        if (!componentReady) return "systemSuppressed"
        if (clusterCount === 0) return "systemSuppressed"
        if (containsNewline) return "lineReflowAnimation"
        if (containsComplexGrapheme) return "clusterAnimation"
        if (clusterCount <= 8) return "glyphAnimation"
        if (clusterCount <= 40) return "runAnimation"
        return "snapshotAnimation"
    }

    /// 公共 ghost 创建函数 — 统一参数构建，避免漂移
    function createGlyphGhost(animKind, startX, startY, endX, endY, glyphWidth, glyphHeight, glyphBaselineY, duration, ghostColor, glyphText, glyphFontFamily, glyphFontPixelSize) {
        var component = root._glyphGhostComponent
        if (!component || component.status !== Component.Ready) return null
        return component.createObject(root, {
            "animKind": animKind,
            "startX": startX,
            "startY": startY,
            "endX": endX,
            "endY": endY,
            "glyphWidth": glyphWidth,
            "glyphHeight": glyphHeight,
            "glyphBaselineY": glyphBaselineY,
            "width": glyphWidth,
            "height": glyphHeight,
            "duration": duration,
            "ghostColor": ghostColor,
            "glyphText": glyphText,
            "glyphFontFamily": glyphFontFamily,
            "glyphFontPixelSize": glyphFontPixelSize
        })
    }

    Connections {
        target: editorItem
        function onVisualTransactionChanged() {
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
            if (!root.animationEnabled || root.suppressed) {
                root._log("skipped: animationEnabled=" + root.animationEnabled + " suppressed=" + root.suppressed)
                // 动画被跳过，但 Rust 可能已创建 hidden range，必须立即通知清除
                // 否则正文层会永久跳过该 range 导致文字消失
                if (vt.kind === "insert") {
                    _notifySkippedIfHasRange(vt)
                }
                return
            }
            root._log("vt kind=" + vt.kind + " durationMs=" + vt.durationMs)
            root._handleTransaction(vt)
        }

        function onPreeditVisualTransactionChanged() {
            if (!root.animationEnabled || root.suppressed) return
            var jsonStr = editorItem.preedit_visual_transaction_json
            if (!jsonStr || jsonStr === "{}") return
            var vt
            try { vt = JSON.parse(jsonStr) } catch(e) { return }
            if (!vt) return
            root._handlePreeditTransaction(vt)
        }
    }

    onAnimationEnabledChanged: {
        if (!animationEnabled) {
            // 动画被关闭时必须立即清理所有活跃动画和 pending 状态
            // 通知 Rust 清除 hidden range，避免文字永久消失
            root._log("animationEnabled changed to false, clearing all animations")
            root.clearAll()
        }
    }

    onSuppressedChanged: {
        if (suppressed) {
            // suppressed 时必须立即清理所有动画和 pending 状态
            // clearAll 会为每个 insert 事务发送 insertAnimationSkipped 通知 Rust 清除 hidden range
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

        // 检测是否包含复杂 grapheme
        var containsComplexGrapheme = false
        for (var ci = 0; ci < glyphRects.length; ci++) {
            if (isComplexGrapheme(glyphRects[ci].char)) {
                containsComplexGrapheme = true
                break
            }
        }

        // ── 统一动画模式选择 ──
        // 必须与 Rust choose_animation_mode 使用相同规则
        var component = root._glyphGhostComponent
        var componentReady = component && component.status === Component.Ready

        var mode = root.chooseAnimationMode(
            glyphRects.length,    // clusterCount
            containsNewline,      // containsNewline
            containsComplexGrapheme, // containsComplexGrapheme
            false,                // isScrolling (handled via suppressed on QML side)
            root.suppressed,      // isLoading (suppressed covers loading)
            false,                // isApplyingFormat (suppressed covers this)
            false,                // isApplyingSettings (suppressed covers this)
            root.animationEnabled, // animationEnabled
            componentReady        // componentReady
        )

        root._log("insert mode=" + mode + " clusterCount=" + glyphRects.length + " containsNewline=" + containsNewline + " containsComplexGrapheme=" + containsComplexGrapheme + " componentReady=" + componentReady)

        if (mode === "systemSuppressed") {
            root._log("insert skipped: mode=systemSuppressed")
            // 通知 Rust 清除可能已创建的 hidden range
            _notifySkippedIfHasRange(vt)
            return
        }

        // 非 systemSuppressed 模式都创建动画

        // ── 阶段1：创建 ghost 对象但不启动 ──
        var createdGhosts = []

        if (mode === "clusterAnimation") {
            // ClusterAnimation：从 vt.clusterRects 读取 cluster 信息，整簇作为单个 ghost
            var clusterRects = vt.clusterRects
            if (clusterRects && Array.isArray(clusterRects)) {
                for (var ci = 0; ci < clusterRects.length; ci++) {
                    var cluster = clusterRects[ci]
                    // 从 insertGlyphRects 中找该 cluster byte range 对应的 glyph rects
                    var clusterGlyphs = []
                    for (var gi = 0; gi < glyphRects.length; gi++) {
                        var gr = glyphRects[gi]
                        if (gr.byteStart >= cluster.byteStart && gr.byteEnd <= cluster.byteEnd) {
                            clusterGlyphs.push(gr)
                        }
                    }
                    if (clusterGlyphs.length === 0) continue

                    // 计算 cluster bounding rect
                    var minX = Infinity, minY = Infinity, maxRight = -Infinity, maxBottom = -Infinity
                    var clusterBaselineY = 0
                    for (var k = 0; k < clusterGlyphs.length; k++) {
                        var cg = clusterGlyphs[k]
                        if (cg.x < minX) minX = cg.x
                        if (cg.y < minY) minY = cg.y
                        if (cg.x + cg.w > maxRight) maxRight = cg.x + cg.w
                        if (cg.y + cg.h > maxBottom) maxBottom = cg.y + cg.h
                        clusterBaselineY = cg.baselineY || 0
                    }

                    var ghost = createGlyphGhost(
                        "insert",
                        startX,
                        startY,
                        minX,
                        clusterBaselineY || minY,
                        maxRight - minX,
                        maxBottom - minY,
                        clusterBaselineY,
                        duration,
                        editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                        cluster.text || "",
                        editorItem.font_family || "",
                        editorItem.font_pixel_size || 0
                    )
                    if (ghost !== null) createdGhosts.push(ghost)
                }
            } else {
                // fallback: 逐 glyph（旧逻辑）
                for (var i = 0; i < glyphRects.length; i++) {
                    var gr = glyphRects[i]
                    var ghost = createGlyphGhost(
                        "insert",
                        startX,
                        startY,
                        gr.x,
                        gr.baselineY || gr.y,
                        gr.w,
                        gr.h,
                        gr.baselineY || 0,
                        duration,
                        editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                        gr.char || "",
                        editorItem.font_family || "",
                        editorItem.font_pixel_size || 0
                    )
                    if (ghost !== null) createdGhosts.push(ghost)
                }
            }
        } else if (mode === "runAnimation") {
            // RunAnimation：从 vt.clusterRuns 读取 run 信息，每个 run 一个 ghost
            var clusterRuns = vt.clusterRuns
            if (clusterRuns && Array.isArray(clusterRuns)) {
                for (var ri = 0; ri < clusterRuns.length; ri++) {
                    var run = clusterRuns[ri]
                    // 从 insertGlyphRects 中找该 run byte range 对应的 glyph rects
                    var runGlyphs = []
                    for (var gi = 0; gi < glyphRects.length; gi++) {
                        var gr = glyphRects[gi]
                        if (gr.byteStart >= run.byteStart && gr.byteEnd <= run.byteEnd) {
                            runGlyphs.push(gr)
                        }
                    }
                    if (runGlyphs.length === 0) continue

                    // 计算 run bounding rect
                    var minX = Infinity, minY = Infinity, maxRight = -Infinity, maxBottom = -Infinity
                    var runBaselineY = 0
                    for (var k = 0; k < runGlyphs.length; k++) {
                        var rg = runGlyphs[k]
                        if (rg.x < minX) minX = rg.x
                        if (rg.y < minY) minY = rg.y
                        if (rg.x + rg.w > maxRight) maxRight = rg.x + rg.w
                        if (rg.y + rg.h > maxBottom) maxBottom = rg.y + rg.h
                        runBaselineY = rg.baselineY || 0
                    }

                    var ghost = createGlyphGhost(
                        "insert",
                        startX,
                        startY,
                        minX,
                        runBaselineY || minY,
                        maxRight - minX,
                        maxBottom - minY,
                        runBaselineY,
                        duration,
                        editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                        run.text || "",
                        editorItem.font_family || "",
                        editorItem.font_pixel_size || 0
                    )
                    if (ghost !== null) createdGhosts.push(ghost)
                }
            } else {
                // fallback: 逐 glyph
                for (var i = 0; i < glyphRects.length; i++) {
                    var gr = glyphRects[i]
                    if (isComplexGrapheme(gr.char)) continue
                    var ghost = createGlyphGhost(
                        "insert",
                        startX,
                        startY,
                        gr.x,
                        gr.baselineY || gr.y,
                        gr.w,
                        gr.h,
                        gr.baselineY || 0,
                        duration,
                        editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                        gr.char || "",
                        editorItem.font_family || "",
                        editorItem.font_pixel_size || 0
                    )
                    if (ghost !== null) createdGhosts.push(ghost)
                }
            }
        } else if (mode === "snapshotAnimation") {
            // SnapshotAnimation：从 vt.hiddenVisualRanges 读取 snapshot payload
            var hiddenRanges = vt.hiddenVisualRanges
            if (hiddenRanges && Array.isArray(hiddenRanges) && hiddenRanges.length > 0) {
                for (var hi = 0; hi < hiddenRanges.length; hi++) {
                    var hr = hiddenRanges[hi]
                    // 从 insertGlyphRects 中找该 range 对应的 glyph rects
                    var snapGlyphs = []
                    for (var gi = 0; gi < glyphRects.length; gi++) {
                        var gr = glyphRects[gi]
                        if (gr.byteStart >= hr.rangeStart && gr.byteEnd <= hr.rangeEnd) {
                            snapGlyphs.push(gr)
                        }
                    }
                    if (snapGlyphs.length === 0) continue

                    // 计算 snapshot bounding rect
                    var minX = Infinity, minY = Infinity, maxRight = -Infinity, maxBottom = -Infinity
                    var snapBaselineY = 0
                    for (var k = 0; k < snapGlyphs.length; k++) {
                        var sg = snapGlyphs[k]
                        if (sg.x < minX) minX = sg.x
                        if (sg.y < minY) minY = sg.y
                        if (sg.x + sg.w > maxRight) maxRight = sg.x + sg.w
                        if (sg.y + sg.h > maxBottom) maxBottom = sg.y + sg.h
                        snapBaselineY = sg.baselineY || 0
                    }

                    var ghost = createGlyphGhost(
                        "insert",
                        startX,
                        startY,
                        minX,
                        snapBaselineY || minY,
                        maxRight - minX,
                        maxBottom - minY,
                        snapBaselineY,
                        duration,
                        editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                        "",  // snapshot: no single char text needed
                        editorItem.font_family || "",
                        editorItem.font_pixel_size || 0
                    )
                    if (ghost !== null) createdGhosts.push(ghost)
                }
            } else {
                // fallback: 逐 glyph
                for (var i = 0; i < glyphRects.length; i++) {
                    var gr = glyphRects[i]
                    if (isComplexGrapheme(gr.char)) continue
                    var ghost = createGlyphGhost(
                        "insert",
                        startX,
                        startY,
                        gr.x,
                        gr.baselineY || gr.y,
                        gr.w,
                        gr.h,
                        gr.baselineY || 0,
                        duration,
                        editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                        gr.char || "",
                        editorItem.font_family || "",
                        editorItem.font_pixel_size || 0
                    )
                    if (ghost !== null) createdGhosts.push(ghost)
                }
            }
        } else if (mode === "lineReflowAnimation") {
            // LineReflowAnimation：换行场景，做行级 reflow 动画
            // 从 vt.clusterRects 收集 cluster 信息创建 ghost，复杂字符不跳过
            var clusterRects = vt.clusterRects
            if (clusterRects && Array.isArray(clusterRects)) {
                for (var ci = 0; ci < clusterRects.length; ci++) {
                    var cluster = clusterRects[ci]
                    var clusterGlyphs = []
                    for (var gi = 0; gi < glyphRects.length; gi++) {
                        var gr = glyphRects[gi]
                        if (gr.byteStart >= cluster.byteStart && gr.byteEnd <= cluster.byteEnd) {
                            clusterGlyphs.push(gr)
                        }
                    }
                    if (clusterGlyphs.length === 0) continue

                    var minX = Infinity, minY = Infinity, maxRight = -Infinity, maxBottom = -Infinity
                    var clusterBaselineY = 0
                    for (var k = 0; k < clusterGlyphs.length; k++) {
                        var cg = clusterGlyphs[k]
                        if (cg.x < minX) minX = cg.x
                        if (cg.y < minY) minY = cg.y
                        if (cg.x + cg.w > maxRight) maxRight = cg.x + cg.w
                        if (cg.y + cg.h > maxBottom) maxBottom = cg.y + cg.h
                        clusterBaselineY = cg.baselineY || 0
                    }

                    var ghost = createGlyphGhost(
                        "insert",
                        startX,
                        startY,
                        minX,
                        clusterBaselineY || minY,
                        maxRight - minX,
                        maxBottom - minY,
                        clusterBaselineY,
                        duration,
                        editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                        cluster.text || "",
                        editorItem.font_family || "",
                        editorItem.font_pixel_size || 0
                    )
                    if (ghost !== null) createdGhosts.push(ghost)
                }
            } else {
                // fallback: 逐 glyph，换行字符不创建 glyph ghost，复杂字符不跳过
                for (var i = 0; i < glyphRects.length; i++) {
                    var gr = glyphRects[i]
                    if (gr.char === "\n" || gr.char === "\r") continue
                    var ghost = createGlyphGhost(
                        "insert",
                        startX,
                        startY,
                        gr.x,
                        gr.baselineY || gr.y,
                        gr.w,
                        gr.h,
                        gr.baselineY || 0,
                        duration,
                        editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                        gr.char || "",
                        editorItem.font_family || "",
                        editorItem.font_pixel_size || 0
                    )
                    if (ghost !== null) createdGhosts.push(ghost)
                }
            }
        } else {
            // GlyphAnimation：逐字形动画
            for (var i = 0; i < glyphRects.length; i++) {
                var gr = glyphRects[i]
                var ghost = createGlyphGhost(
                    "insert",
                    startX,
                    startY,
                    gr.x,
                    gr.baselineY || gr.y,
                    gr.w,
                    gr.h,
                    gr.baselineY || 0,
                    duration,
                    editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                    gr.char || "",
                    editorItem.font_family || "",
                    editorItem.font_pixel_size || 0
                )

                if (ghost !== null) {
                    createdGhosts.push(ghost)
                } else {
                    root._log("insert ghost createObject returned null at index=" + i)
                }
            }
        }

        // ── 局部 reflow 动画：方案 A — reflow ghost ──
        // 中间插入时，插入点右侧文字做轻量位移动画（局部挤开）。
        // 静态正文层在动画期间跳过 reflow ranges，由 overlay reflow ghost 显示位移动画。
        // 动画结束后清除 reflow hidden ranges，正文层恢复完整绘制。
        // 复杂字符也参与 reflow 动画
        var reflowRects = vt.reflowGlyphRects
        if (reflowRects && Array.isArray(reflowRects) && reflowRects.length > 0) {
            root._log("insert creating " + reflowRects.length + " reflow ghosts")
            for (var ri = 0; ri < reflowRects.length; ri++) {
                var rr = reflowRects[ri]
                // 只有位置真正变化时才创建 reflow ghost
                var dx = Math.abs(rr.newX - rr.oldX)
                var dy = Math.abs(rr.newY - rr.oldY)
                if (dx < 0.5 && dy < 0.5) {
                    continue
                }
                var reflowGhost = createGlyphGhost(
                    "reflow",
                    rr.oldX,
                    rr.oldBaselineY || rr.oldY,
                    rr.newX,
                    rr.newBaselineY || rr.newY,
                    rr.w,
                    rr.h,
                    rr.newBaselineY || 0,
                    duration,
                    editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                    rr.char || "",
                    editorItem.font_family || "",
                    editorItem.font_pixel_size || 0
                )

                if (reflowGhost !== null) {
                    createdGhosts.push(reflowGhost)
                } else {
                    root._log("reflow ghost createObject returned null at index=" + ri)
                }
            }
        }

        // 如果没有 ghost 被创建，立即通知 Rust 清除 hidden range
        if (createdGhosts.length === 0) {
            root._log("insert skipped: no ghosts actually created (createdGhosts.length=0)")
            _notifySkippedIfHasRange(vt)
            return
        }

        // ── 阶段2：先注册 transaction，再统一连接 finished 并 start ──
        root._activeTransactions.push({ pendingCount: createdGhosts.length, byteStart: insertByteStart, byteEnd: insertByteEnd })
        root._log("insert pendingCount=" + createdGhosts.length + " byteRange=(" + insertByteStart + "," + insertByteEnd + ")")

        for (var gi = 0; gi < createdGhosts.length; gi++) {
            var g = createdGhosts[gi]
            root._activeAnimations.push(g)
            // 使用 IIFE 捕获当前 ghost 引用和参数
            ;(function(ghost, animKind, byteStart, byteEnd) {
                ghost.animationFinished.connect(function() {
                    var idx = root._activeAnimations.indexOf(ghost)
                    if (idx >= 0) root._activeAnimations.splice(idx, 1)

                    if (animKind === "insert" || animKind === "reflow") {
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
            })(g, g.animKind, insertByteStart, insertByteEnd)
        }
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

        // ── 统一动画模式选择 ──
        var containsNewline = false
        for (var ci = 0; ci < glyphRects.length; ci++) {
            if (glyphRects[ci].char === "\n" || glyphRects[ci].char === "\r") {
                containsNewline = true
                break
            }
        }

        // 检测是否包含复杂 grapheme
        var containsComplexGrapheme = false
        for (var ci = 0; ci < glyphRects.length; ci++) {
            if (isComplexGrapheme(glyphRects[ci].char)) {
                containsComplexGrapheme = true
                break
            }
        }

        var component = root._glyphGhostComponent
        var componentReady = component && component.status === Component.Ready

        var mode = root.chooseAnimationMode(
            glyphRects.length,
            containsNewline,
            containsComplexGrapheme,
            false,
            root.suppressed,
            false,
            false,
            root.animationEnabled,
            componentReady
        )

        root._log("delete mode=" + mode + " clusterCount=" + glyphRects.length + " containsNewline=" + containsNewline + " componentReady=" + componentReady)

        if (mode === "systemSuppressed") {
            root._log("delete skipped: mode=systemSuppressed")
            return
        }

        // 非 systemSuppressed 模式都创建删除动画
        root._log("delete creating " + glyphRects.length + " ghosts, cursorRect=(" + endX + "," + endY + ")")

        if (mode === "clusterAnimation") {
            // ClusterAnimation：从 vt.clusterRects 读取 cluster 信息，整簇作为单个 ghost
            var clusterRects = vt.clusterRects
            if (clusterRects && Array.isArray(clusterRects)) {
                for (var ci = 0; ci < clusterRects.length; ci++) {
                    var cluster = clusterRects[ci]
                    // 从 deletedGlyphRects 中找该 cluster byte range 对应的 glyph rects
                    var clusterGlyphs = []
                    for (var gi = 0; gi < glyphRects.length; gi++) {
                        var gr = glyphRects[gi]
                        if (gr.byteStart >= cluster.byteStart && gr.byteEnd <= cluster.byteEnd) {
                            clusterGlyphs.push(gr)
                        }
                    }
                    if (clusterGlyphs.length === 0) continue

                    // 计算 cluster bounding rect
                    var minX = Infinity, minY = Infinity, maxRight = -Infinity, maxBottom = -Infinity
                    var clusterBaselineY = 0
                    for (var k = 0; k < clusterGlyphs.length; k++) {
                        var cg = clusterGlyphs[k]
                        if (cg.x < minX) minX = cg.x
                        if (cg.y < minY) minY = cg.y
                        if (cg.x + cg.w > maxRight) maxRight = cg.x + cg.w
                        if (cg.y + cg.h > maxBottom) maxBottom = cg.y + cg.h
                        clusterBaselineY = cg.baselineY || 0
                    }

                    var ghost = createGlyphGhost(
                        "delete",
                        minX,
                        clusterBaselineY || minY,
                        endX,
                        endY,
                        maxRight - minX,
                        maxBottom - minY,
                        clusterBaselineY,
                        duration,
                        editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                        cluster.text || "",
                        editorItem.font_family || "",
                        editorItem.font_pixel_size || 0
                    )
                    _trackGhost(ghost, "delete", 0, 0)
                }
            } else {
                // fallback: 逐 glyph
                for (var i = 0; i < glyphRects.length; i++) {
                    var gr = glyphRects[i]
                    var ghost = createGlyphGhost(
                        "delete",
                        gr.x,
                        gr.baselineY || gr.y,
                        endX,
                        endY,
                        gr.w,
                        gr.h,
                        gr.baselineY || 0,
                        duration,
                        editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                        gr.char || "",
                        editorItem.font_family || "",
                        editorItem.font_pixel_size || 0
                    )
                    _trackGhost(ghost, "delete", 0, 0)
                }
            }
        } else if (mode === "runAnimation") {
            // RunAnimation：从 vt.clusterRuns 读取 run 信息，每个 run 一个 ghost
            var clusterRuns = vt.clusterRuns
            if (clusterRuns && Array.isArray(clusterRuns)) {
                for (var ri = 0; ri < clusterRuns.length; ri++) {
                    var run = clusterRuns[ri]
                    var runGlyphs = []
                    for (var gi = 0; gi < glyphRects.length; gi++) {
                        var gr = glyphRects[gi]
                        if (gr.byteStart >= run.byteStart && gr.byteEnd <= run.byteEnd) {
                            runGlyphs.push(gr)
                        }
                    }
                    if (runGlyphs.length === 0) continue

                    var minX = Infinity, minY = Infinity, maxRight = -Infinity, maxBottom = -Infinity
                    var runBaselineY = 0
                    for (var k = 0; k < runGlyphs.length; k++) {
                        var rg = runGlyphs[k]
                        if (rg.x < minX) minX = rg.x
                        if (rg.y < minY) minY = rg.y
                        if (rg.x + rg.w > maxRight) maxRight = rg.x + rg.w
                        if (rg.y + rg.h > maxBottom) maxBottom = rg.y + rg.h
                        runBaselineY = rg.baselineY || 0
                    }

                    var ghost = createGlyphGhost(
                        "delete",
                        minX,
                        runBaselineY || minY,
                        endX,
                        endY,
                        maxRight - minX,
                        maxBottom - minY,
                        runBaselineY,
                        duration,
                        editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                        run.text || "",
                        editorItem.font_family || "",
                        editorItem.font_pixel_size || 0
                    )
                    _trackGhost(ghost, "delete", 0, 0)
                }
            } else {
                // fallback: 逐 glyph
                for (var i = 0; i < glyphRects.length; i++) {
                    var gr = glyphRects[i]
                    if (isComplexGrapheme(gr.char)) continue
                    var ghost = createGlyphGhost(
                        "delete",
                        gr.x,
                        gr.baselineY || gr.y,
                        endX,
                        endY,
                        gr.w,
                        gr.h,
                        gr.baselineY || 0,
                        duration,
                        editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                        gr.char || "",
                        editorItem.font_family || "",
                        editorItem.font_pixel_size || 0
                    )
                    _trackGhost(ghost, "delete", 0, 0)
                }
            }
        } else if (mode === "snapshotAnimation") {
            // SnapshotAnimation：从 vt.hiddenVisualRanges 读取 snapshot payload
            var hiddenRanges = vt.hiddenVisualRanges
            if (hiddenRanges && Array.isArray(hiddenRanges) && hiddenRanges.length > 0) {
                for (var hi = 0; hi < hiddenRanges.length; hi++) {
                    var hr = hiddenRanges[hi]
                    var snapGlyphs = []
                    for (var gi = 0; gi < glyphRects.length; gi++) {
                        var gr = glyphRects[gi]
                        if (gr.byteStart >= hr.rangeStart && gr.byteEnd <= hr.rangeEnd) {
                            snapGlyphs.push(gr)
                        }
                    }
                    if (snapGlyphs.length === 0) continue

                    var minX = Infinity, minY = Infinity, maxRight = -Infinity, maxBottom = -Infinity
                    var snapBaselineY = 0
                    for (var k = 0; k < snapGlyphs.length; k++) {
                        var sg = snapGlyphs[k]
                        if (sg.x < minX) minX = sg.x
                        if (sg.y < minY) minY = sg.y
                        if (sg.x + sg.w > maxRight) maxRight = sg.x + sg.w
                        if (sg.y + sg.h > maxBottom) maxBottom = sg.y + sg.h
                        snapBaselineY = sg.baselineY || 0
                    }

                    var ghost = createGlyphGhost(
                        "delete",
                        minX,
                        snapBaselineY || minY,
                        endX,
                        endY,
                        maxRight - minX,
                        maxBottom - minY,
                        snapBaselineY,
                        duration,
                        editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                        "",
                        editorItem.font_family || "",
                        editorItem.font_pixel_size || 0
                    )
                    _trackGhost(ghost, "delete", 0, 0)
                }
            } else {
                // fallback: 逐 glyph
                for (var i = 0; i < glyphRects.length; i++) {
                    var gr = glyphRects[i]
                    if (isComplexGrapheme(gr.char)) continue
                    var ghost = createGlyphGhost(
                        "delete",
                        gr.x,
                        gr.baselineY || gr.y,
                        endX,
                        endY,
                        gr.w,
                        gr.h,
                        gr.baselineY || 0,
                        duration,
                        editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                        gr.char || "",
                        editorItem.font_family || "",
                        editorItem.font_pixel_size || 0
                    )
                    _trackGhost(ghost, "delete", 0, 0)
                }
            }
        } else {
            // GlyphAnimation / lineReflowAnimation / 其他：逐 glyph，不跳过复杂字符
            for (var i = 0; i < glyphRects.length; i++) {
                var gr = glyphRects[i]
                var ghost = createGlyphGhost(
                    "delete",
                    gr.x,
                    gr.baselineY || gr.y,
                    endX,
                    endY,
                    gr.w,
                    gr.h,
                    gr.baselineY || 0,
                    duration,
                    editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                    gr.char || "",
                    editorItem.font_family || "",
                    editorItem.font_pixel_size || 0
                )
                _trackGhost(ghost, "delete", 0, 0)
            }
        }
    }

    function _handlePreeditTransaction(vt) {
        // Preedit transaction 只做轻量动画
        // 新增字符 → 从 preedit cursor 位置飞到目标位置
        // 删除字符 → 从当前位置飞回 preedit cursor
        // 不涉及 hidden range（preedit 是独立视觉层）

        var newLen = vt.newPreeditText ? vt.newPreeditText.length : 0
        var oldLen = vt.oldPreeditText ? vt.oldPreeditText.length : 0
        var oldText = vt.oldPreeditText || ""
        var newText = vt.newPreeditText || ""

        root._log("preedit transaction: oldLen=" + oldLen + " newLen=" + newLen + " oldText=" + oldText + " newText=" + newText)

        if (newLen > oldLen) {
            // 插入动画
            _createPreeditInsertAnimation(vt)
        } else if (newLen < oldLen) {
            // 删除动画
            _createPreeditDeleteAnimation(vt)
        } else if (oldText !== newText) {
            // 长度相同但内容不同（如 ni→你、候选替换）：先删旧再插新
            _createPreeditDeleteAnimation(vt)
            _createPreeditInsertAnimation(vt)
        }
    }

    function _createPreeditInsertAnimation(vt) {
        // Preedit insert: new characters fly from preedit cursor position
        var startX = vt.preeditCursorRect ? vt.preeditCursorRect.x : editorItem.cursor_rect_x
        var startY = vt.preeditCursorRect ? (vt.preeditCursorRect.baselineY || vt.preeditCursorRect.top) : editorItem.cursor_rect_y
        var duration = Math.max(30, Math.min(1000, vt.durationMs || 100))

        // Use insertedPreeditGlyphRects if available
        var glyphRects = vt.insertedPreeditGlyphRects
        if (!glyphRects || !Array.isArray(glyphRects) || glyphRects.length === 0) {
            root._log("preedit insert skipped: insertedPreeditGlyphRects empty")
            return
        }

        var component = root._glyphGhostComponent
        if (!component || component.status !== Component.Ready) {
            root._log("preedit insert skipped: component not ready")
            return
        }

        for (var i = 0; i < glyphRects.length; i++) {
            var gr = glyphRects[i]
            if (isComplexGrapheme(gr.char)) continue

            var ghost = createGlyphGhost(
                "insert",
                startX,
                startY,
                gr.x,
                gr.baselineY || gr.y,
                gr.w,
                gr.h,
                gr.baselineY || 0,
                duration,
                editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                gr.char || "",
                editorItem.font_family || "",
                editorItem.font_pixel_size || 0
            )

            if (ghost !== null) {
                root._activeAnimations.push(ghost)
                ;(function(g) {
                    g.animationFinished.connect(function() {
                        var idx = root._activeAnimations.indexOf(g)
                        if (idx >= 0) root._activeAnimations.splice(idx, 1)
                        g.destroy()
                    })
                    g.startAnimation()
                })(ghost)
            }
        }
    }

    function _createPreeditDeleteAnimation(vt) {
        // Preedit delete: removed characters fly back toward preedit cursor
        var endX = vt.preeditCursorRect ? vt.preeditCursorRect.x : editorItem.cursor_rect_x
        var endY = vt.preeditCursorRect ? (vt.preeditCursorRect.baselineY || vt.preeditCursorRect.top) : editorItem.cursor_rect_y
        var duration = Math.max(30, Math.min(1000, vt.durationMs || 100))

        var glyphRects = vt.deletedPreeditGlyphRects
        if (!glyphRects || !Array.isArray(glyphRects) || glyphRects.length === 0) {
            root._log("preedit delete skipped: deletedPreeditGlyphRects empty")
            return
        }

        var component = root._glyphGhostComponent
        if (!component || component.status !== Component.Ready) {
            root._log("preedit delete skipped: component not ready")
            return
        }

        for (var i = 0; i < glyphRects.length; i++) {
            var gr = glyphRects[i]
            if (isComplexGrapheme(gr.char)) continue

            var ghost = createGlyphGhost(
                "delete",
                gr.x,
                gr.baselineY || gr.y,
                endX,
                endY,
                gr.w,
                gr.h,
                gr.baselineY || 0,
                duration,
                editorItem.text_color || (root.dt ? root.dt.editorText : "#E2E2E5"),
                gr.char || "",
                editorItem.font_family || "",
                editorItem.font_pixel_size || 0
            )

            if (ghost !== null) {
                root._activeAnimations.push(ghost)
                ;(function(g) {
                    g.animationFinished.connect(function() {
                        var idx = root._activeAnimations.indexOf(g)
                        if (idx >= 0) root._activeAnimations.splice(idx, 1)
                        g.destroy()
                    })
                    g.startAnimation()
                })(ghost)
            }
        }
    }

    function clearAll() {
        // 遍历所有活跃事务，对每个 insert byte range 发 insertAnimationSkipped
        // 通知 Rust 清除 hidden range，避免文字短暂消失
        for (var ti = 0; ti < root._activeTransactions.length; ti++) {
            var tx = root._activeTransactions[ti]
            if (tx.byteStart !== undefined && tx.byteEnd !== undefined && tx.byteEnd > tx.byteStart) {
                root._log("clearAll: sending insertAnimationSkipped for byteRange=(" + tx.byteStart + "," + tx.byteEnd + ")")
                root.insertAnimationSkipped(tx.byteStart, tx.byteEnd)
            }
        }
        // 销毁所有活跃动画
        for (var i = 0; i < root._activeAnimations.length; i++) {
            if (root._activeAnimations[i]) root._activeAnimations[i].destroy()
        }
        root._activeAnimations = []
        root._activeTransactions = []
    }
}
