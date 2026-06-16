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

    function _createInsertAnimation(event) {
        var cursorRectX = editorItem.cursor_rect_x
        var cursorRectY = editorItem.cursor_rect_y
        var cursorRectH = editorItem.cursor_rect_height
        var duration = event.durationMs || 160
        var text = event.text || ""
        var charCount = text.length
        if (charCount === 0) return

        var highlightX, highlightY, highlightWidth, highlightHeight

        if (event.glyphRects && Array.isArray(event.glyphRects) && event.glyphRects.length > 0) {
            // 使用 Rust Core 提供的精确 glyph 矩形，计算 bounding rect
            var minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity
            for (var i = 0; i < event.glyphRects.length; i++) {
                var gr = event.glyphRects[i]
                if (gr.x < minX) minX = gr.x
                if (gr.y < minY) minY = gr.y
                if (gr.x + gr.w > maxX) maxX = gr.x + gr.w
                if (gr.y + gr.h > maxY) maxY = gr.y + gr.h
            }
            highlightX = minX
            highlightY = minY
            highlightWidth = maxX - minX
            highlightHeight = maxY - minY
        } else {
            // Fallback: 旧估算逻辑（glyphRects 不可用时）
            var fontSize = editorItem.font_pixel_size || 16
            var charWidth = fontSize * 0.6
            highlightWidth = Math.min(charCount, 8) * charWidth
            highlightX = cursorRectX - highlightWidth
            highlightY = cursorRectY
            highlightHeight = cursorRectH
        }

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

        if (anim) {
            root._activeAnimations.push(anim)
            anim.animationFinished.connect(function() {
                var idx = root._activeAnimations.indexOf(anim)
                if (idx >= 0) root._activeAnimations.splice(idx, 1)
                anim.destroy()
            })
            anim.startAnimation()
        }
    }

    function _createDeleteAnimation(event) {
        var cursorRectX = editorItem.cursor_rect_x
        var cursorRectY = editorItem.cursor_rect_y
        var cursorRectH = editorItem.cursor_rect_height
        var duration = event.durationMs || 160
        var text = event.text || ""
        var charCount = text.length
        if (charCount === 0) return

        var ghostX, ghostY, ghostWidth, ghostHeight

        if (event.glyphRects && Array.isArray(event.glyphRects) && event.glyphRects.length > 0) {
            // 使用 Rust Core 提供的精确 glyph 矩形，计算 bounding rect
            var minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity
            for (var i = 0; i < event.glyphRects.length; i++) {
                var gr = event.glyphRects[i]
                if (gr.x < minX) minX = gr.x
                if (gr.y < minY) minY = gr.y
                if (gr.x + gr.w > maxX) maxX = gr.x + gr.w
                if (gr.y + gr.h > maxY) maxY = gr.y + gr.h
            }
            ghostX = minX
            ghostY = minY
            ghostWidth = maxX - minX
            ghostHeight = maxY - minY
        } else {
            // Fallback: 旧估算逻辑（glyphRects 不可用时）
            var fontSize = editorItem.font_pixel_size || 16
            var charWidth = fontSize * 0.6
            ghostWidth = Math.min(charCount, 8) * charWidth
            ghostX = cursorRectX
            ghostY = cursorRectY
            ghostHeight = cursorRectH
        }

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

        if (anim) {
            root._activeAnimations.push(anim)
            anim.animationFinished.connect(function() {
                var idx = root._activeAnimations.indexOf(anim)
                if (idx >= 0) root._activeAnimations.splice(idx, 1)
                anim.destroy()
            })
            anim.startAnimation()
        }
    }

    function clearAll() {
        for (var i = 0; i < root._activeAnimations.length; i++) {
            if (root._activeAnimations[i]) root._activeAnimations[i].destroy()
        }
        root._activeAnimations = []
    }
}