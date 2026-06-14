// =============================================================================
// EditorAnimationHighlight.qml — Single short-lived animation effect
// =============================================================================
// Renders a fade-out highlight rectangle for insert/delete animations.
// Self-destroys when animation finishes.

import QtQuick

Item {
    id: root

    property int duration: 160
    property color color: "#006497"
    property string animKind: "insert"

    signal animationFinished()

    opacity: 1.0

    Rectangle {
        id: highlightRect
        anchors.fill: parent
        color: root.color
        radius: 2
    }

    NumberAnimation on opacity {
        id: fadeOutAnim
        from: root.animKind === "insert" ? 0.35 : 0.5
        to: 0.0
        duration: root.duration
        easing.type: Easing.OutCubic
        running: false
        onFinished: {
            root.animationFinished()
        }
    }

    NumberAnimation on x {
        id: slideAnim
        from: root.x
        to: root.animKind === "delete" ? root.x + 4 : root.x
        duration: root.duration
        easing.type: Easing.OutCubic
        running: false
    }

    function startAnimation() {
        fadeOutAnim.start()
        if (root.animKind === "delete") {
            slideAnim.start()
        }
    }
}