// =============================================================================
// EditorAnimationOverlay.qml — QML overlay animation layer for SujianEditorItem
// =============================================================================
// Consumes EditorAnimationEvent JSON from Rust core and renders short-lived
// insert/delete animations as QML items. This is the only animation route:
// Core transaction → animation_events_json signal → QML overlay rendering.
// The static text texture (Layer 0) and QML cursor remain unchanged.
//
// Rules:
// - glyphRects > 8: no animation
// - contains newline: no animation
// - IME composing: no animation (only commit events)
// - paste: no animation (Core already filters this)
// - scroll/settings/load/chapter switch: clearAll()
// - No rectangle highlight fallback — if glyphRects is empty, skip animation

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

    function _log(message) {
        if (verboseLogging) console.log("[AnimOverlay] " + message)
    }

    visible: editorItem !== null

    property var _activeAnimations: []
    property Component _glyphGhostComponent: Qt.createComponent("EditorGlyphGhost.qml")

    Connections {
        target: editorItem
        function onAnimationEventsChanged() {
            if (!root.animationEnabled || root.suppressed) {
                root._log("skipped: animationEnabled=" + root.animationEnabled + " suppressed=" + root.suppressed)
                return
            }
            var jsonStr = editorItem.animation_events_json
            if (!jsonStr || jsonStr === "[]") {
                root._log("skipped: jsonStr empty or []")
                return
            }
            root._log("received: jsonLen=" + jsonStr.length)
            var events
            try {
                events = JSON.parse(jsonStr)
            } catch (e) {
                root._log("JSON parse error: " + e)
                return
            }
            if (!Array.isArray(events)) {
                root._log("skipped: events not array")
                return
            }
            root._log("events count=" + events.length)
            for (var i = 0; i < events.length; i++) {
                root._handleEvent(events[i])
            }
        }
    }

    onSuppressedChanged: {
        if (suppressed) {
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

    function _handleEvent(event) {
        if (!event) return
        var kind = event.kind
        if (kind === "insert") {
            _createInsertAnimation(event)
        } else if (kind === "delete") {
            _createDeleteAnimation(event)
        }
    }

    function _trackGhost(ghost) {
        if (!ghost) return
        root._activeAnimations.push(ghost)
        ghost.animationFinished.connect(function() {
            var idx = root._activeAnimations.indexOf(ghost)
            if (idx >= 0) root._activeAnimations.splice(idx, 1)
            ghost.destroy()
        })
        ghost.startAnimation()
    }

    function _createInsertAnimation(event) {
        var cursorRectX = editorItem.cursor_rect_x
        var cursorRectY = editorItem.cursor_rect_y
        var duration = Math.max(80, Math.min(180, event.durationMs || 160))

        if (!event.glyphRects || !Array.isArray(event.glyphRects) || event.glyphRects.length === 0) {
            root._log("insert skipped: glyphRects empty")
            return
        }

        // Multi-char limit: > 8 glyphs → no animation
        if (event.glyphRects.length > 8) {
            root._log("insert skipped: glyphRects count=" + event.glyphRects.length + " > 8")
            return
        }

        // Newline check: if any glyph char is newline, skip
        for (var ci = 0; ci < event.glyphRects.length; ci++) {
            if (event.glyphRects[ci].char === "\n" || event.glyphRects[ci].char === "\r") {
                root._log("insert skipped: contains newline at index=" + ci)
                return
            }
        }

        // ── Glyph Ghost path ──
        var component = root._glyphGhostComponent
        if (!component || component.status !== Component.Ready) {
            root._log("insert skipped: component not ready, status=" + (component ? component.status : "null") + " error=" + (component ? component.errorString() : "no component"))
            return
        }

        root._log("insert creating " + event.glyphRects.length + " ghosts, cursorRect=(" + cursorRectX + "," + cursorRectY + ")")

        for (var i = 0; i < event.glyphRects.length; i++) {
            var gr = event.glyphRects[i]
            var ghost = component.createObject(root, {
                "animKind": "insert",
                "startX": cursorRectX,
                "startY": cursorRectY,
                "endX": gr.x,
                "endY": gr.y,
                "glyphWidth": gr.w,
                "glyphHeight": gr.h,
                "width": gr.w,
                "height": gr.h,
                "duration": duration,
                "ghostColor": editorItem.text_color || "#E2E2E5",
                "glyphText": isComplexGrapheme(gr.char) ? "" : (gr.char || ""),
                "glyphFontFamily": editorItem.font_family || "",
                "glyphFontPixelSize": editorItem.font_pixel_size || 0
            })

            _trackGhost(ghost)
        }
    }

    function _createDeleteAnimation(event) {
        var cursorRectX = editorItem.cursor_rect_x
        var cursorRectY = editorItem.cursor_rect_y
        var duration = Math.max(80, Math.min(180, event.durationMs || 160))

        if (!event.glyphRects || !Array.isArray(event.glyphRects) || event.glyphRects.length === 0) {
            root._log("delete skipped: glyphRects empty")
            return
        }

        // Multi-char limit: > 8 glyphs → no animation
        if (event.glyphRects.length > 8) {
            root._log("delete skipped: glyphRects count=" + event.glyphRects.length + " > 8")
            return
        }

        // Newline check: if any glyph char is newline, skip
        for (var ci = 0; ci < event.glyphRects.length; ci++) {
            if (event.glyphRects[ci].char === "\n" || event.glyphRects[ci].char === "\r") {
                root._log("delete skipped: contains newline at index=" + ci)
                return
            }
        }

        // ── Glyph Ghost path ──
        var component = root._glyphGhostComponent
        if (!component || component.status !== Component.Ready) {
            root._log("delete skipped: component not ready, status=" + (component ? component.status : "null") + " error=" + (component ? component.errorString() : "no component"))
            return
        }

        root._log("delete creating " + event.glyphRects.length + " ghosts, cursorRect=(" + cursorRectX + "," + cursorRectY + ")")

        for (var i = 0; i < event.glyphRects.length; i++) {
            var gr = event.glyphRects[i]
            var ghost = component.createObject(root, {
                "animKind": "delete",
                "startX": gr.x,
                "startY": gr.y,
                "endX": cursorRectX,
                "endY": cursorRectY,
                "glyphWidth": gr.w,
                "glyphHeight": gr.h,
                "width": gr.w,
                "height": gr.h,
                "duration": duration,
                "ghostColor": editorItem.text_color || "#E2E2E5",
                "glyphText": isComplexGrapheme(gr.char) ? "" : (gr.char || ""),
                "glyphFontFamily": editorItem.font_family || "",
                "glyphFontPixelSize": editorItem.font_pixel_size || 0
            })

            _trackGhost(ghost)
        }
    }

    function clearAll() {
        for (var i = 0; i < root._activeAnimations.length; i++) {
            if (root._activeAnimations[i]) root._activeAnimations[i].destroy()
        }
        root._activeAnimations = []
    }
}
