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

    anchors.fill: parent
    visible: editorItem !== null

    property var _activeAnimations: []

    Connections {
        target: editorItem
        function onAnimationEventsChanged() {
            if (!root.animationEnabled || root.suppressed) return
            var jsonStr = editorItem.animation_events_json
            if (!jsonStr || jsonStr === "[]") return
            var events
            try {
                events = JSON.parse(jsonStr)
            } catch (e) {
                return
            }
            if (!Array.isArray(events)) return
            for (var i = 0; i < events.length; i++) {
                root._handleEvent(events[i])
            }
        }

        // Clear all ghosts when chapter loads or settings change
        function onPlain_text_changed() {
            root.clearAll()
        }
    }

    onSuppressedChanged: {
        if (suppressed) {
            root.clearAll()
        }
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
            // No glyph rects — skip animation entirely, no rectangle fallback
            return
        }

        // Multi-char limit: > 8 glyphs → no animation
        if (event.glyphRects.length > 8) return

        // Newline check: if any glyph char is newline, skip
        for (var ci = 0; ci < event.glyphRects.length; ci++) {
            if (event.glyphRects[ci].char === "\n" || event.glyphRects[ci].char === "\r") return
        }

        // ── Glyph Ghost path ──
        var component = Qt.createComponent("EditorGlyphGhost.qml")
        if (component.status !== Component.Ready) return

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
                "glyphText": gr.char || "",
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
            // No glyph rects — skip animation entirely, no rectangle fallback
            return
        }

        // Multi-char limit: > 8 glyphs → no animation
        if (event.glyphRects.length > 8) return

        // Newline check: if any glyph char is newline, skip
        for (var ci = 0; ci < event.glyphRects.length; ci++) {
            if (event.glyphRects[ci].char === "\n" || event.glyphRects[ci].char === "\r") return
        }

        // ── Glyph Ghost path ──
        var component = Qt.createComponent("EditorGlyphGhost.qml")
        if (component.status !== Component.Ready) return

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
                "glyphText": gr.char || "",
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
