// =============================================================================
// EditorTypingAnimator.qml — 文字吐字动画占位层
// =============================================================================

import QtQuick

Item {
    id: root

    property var targetTextArea: null
    property var documentHandler: null
    property var overlayItem: parent
    property var dt: null
    property bool animationEnabled: false
    property bool suppressed: false
    property int animationDuration: 160
    property int maxAnimatedChars: 8

    anchors.fill: parent
    z: 2
    visible: false

    // Phase 1 intentionally disables Linux character animation. The old path
    // mutated QTextDocument character formats and could pollute formatting,
    // undo stacks and cursor synchronization. Future animation must consume
    // Core editor transactions and be rendered by SujianEditorItem.
    function clearHiddenRanges() {
    }

    function resetTextSnapshot() {
    }
}
