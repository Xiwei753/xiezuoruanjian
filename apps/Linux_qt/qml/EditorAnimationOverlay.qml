// =============================================================================
// EditorAnimationOverlay.qml — DEPRECATED: text animation now in Scene Graph
// =============================================================================
// Text animation has been moved into SujianEditorItem's Scene Graph (child[1])
// via ActiveVisualTransactionQueue. This file is kept as a stub for QML
// compatibility but no longer renders text animations.
//
// Insert/delete animations are now driven by the Rust-held visual transaction
// queue and rendered directly in the Scene Graph, sharing the same
// QTextLayout/QGlyphRun shaping results as the static text layer.

import QtQuick

Item {
    id: root

    property var editorItem: null
    property var dt: null
    property bool animationEnabled: true
    property bool suppressed: false

    signal insertAnimationFinished(string transactionId, string rangeId, int byteStart, int byteEnd)
    signal insertAnimationSkipped(string transactionId, string rangeId, int byteStart, int byteEnd)

    visible: false
}
