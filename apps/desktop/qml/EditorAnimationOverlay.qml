// =============================================================================
// EditorAnimationOverlay.qml — QML overlay animation layer for SujianEditorItem
// =============================================================================
// Consumes EditorAnimationEvent JSON from Rust core and renders short-lived
// insert/delete animations as QML items. This is the new animation route:
// Core transaction → animation_events_json signal → QML overlay rendering.
// The static text texture (Layer 0) and QML cursor remain unchanged.

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
        var cursorRectH = editorItem.cursor_rect_height
        var duration = event.durationMs || 160

        if (event.glyphRects && Array.isArray(event.glyphRects) && event.glyphRects.length > 0) {
            // ── Glyph Ghost 路径 ──
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
        } else {
            // ── Fallback: 旧矩形高亮路径 ──
            var text = event.text || ""
            var charCount = text.length
            if (charCount === 0) return

            var fontSize = editorItem.font_pixel_size || 16
            var charWidth = fontSize * 0.6
            var highlightWidth = Math.min(charCount, 8) * charWidth
            var highlightX = cursorRectX - highlightWidth
            var highlightY = cursorRectY
            var highlightHeight = cursorRectH

            var component = Qt.createComponent("EditorAnimationHighlight.qml")
            if (component.status !== Component.Ready) return

            var anim = component.createObject(root, {
                "x": highlightX,
                "y": highlightY,
                "width": highlightWidth,
                "height": highlightHeight,
                "duration": duration,
                "color": editorItem.selection_color || "#006497",
                "animKind": "insert"
            })

            _trackGhost(anim)
        }
    }

    function _createDeleteAnimation(event) {
        var cursorRectX = editorItem.cursor_rect_x
        var cursorRectY = editorItem.cursor_rect_y
        var cursorRectH = editorItem.cursor_rect_height
        var duration = event.durationMs || 160

        if (event.glyphRects && Array.isArray(event.glyphRects) && event.glyphRects.length > 0) {
            // ── Glyph Ghost 路径 ──
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
        } else {
            // ── Fallback: 旧矩形高亮路径 ──
            var text = event.text || ""
            var charCount = text.length
            if (charCount === 0) return

            var fontSize = editorItem.font_pixel_size || 16
            var charWidth = fontSize * 0.6
            var ghostWidth = Math.min(charCount, 8) * charWidth
            var ghostX = cursorRectX
            var ghostY = cursorRectY
            var ghostHeight = cursorRectH

            var component = Qt.createComponent("EditorAnimationHighlight.qml")
            if (component.status !== Component.Ready) return

            var anim = component.createObject(root, {
                "x": ghostX,
                "y": ghostY,
                "width": ghostWidth,
                "height": ghostHeight,
                "duration": duration,
                "color": editorItem.text_color || "#E2E2E5",
                "animKind": "delete"
            })

            _trackGhost(anim)
        }
    }

    function clearAll() {
        for (var i = 0; i < root._activeAnimations.length; i++) {
            if (root._activeAnimations[i]) root._activeAnimations[i].destroy()
        }
        root._activeAnimations = []
    }
}