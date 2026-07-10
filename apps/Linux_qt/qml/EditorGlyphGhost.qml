// =============================================================================
// EditorGlyphGhost.qml — DEPRECATED: text animation now in Scene Graph
// =============================================================================
// Text animation has been moved into SujianEditorItem's Scene Graph (child[1])
// via ActiveVisualTransactionQueue. This file is kept as a stub for QML
// compatibility but no longer renders glyph ghosts.

import QtQuick

Item {
    id: root

    property string animKind: "insert"
    property var dt: null
    property real startX: 0
    property real startY: 0
    property real endX: 0
    property real endY: 0
    property real glyphWidth: 0
    property real glyphHeight: 0
    property real glyphBaselineY: 0
    property int duration: 100
    property color ghostColor: "white"
    property string glyphText: ""
    property string glyphFontFamily: ""
    property real glyphFontPixelSize: 0

    signal animationFinished()

    function startAnimation() {
        root.animationFinished()
    }
}
