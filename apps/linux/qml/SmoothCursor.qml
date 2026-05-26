import QtQuick 2.15

Rectangle {
    id: root

    property var targetTextArea: null
    property var dt: null

    width: 2
    height: targetTextArea ? targetTextArea.cursorRectangle.height : 0
    color: dt ? dt.accent : "#7B8CDE"
    visible: targetTextArea && targetTextArea.focus && targetTextArea.enabled && targetTextArea.cursorRectangle.height > 0
    z: 3
    
    x: targetTextArea ? targetTextArea.cursorRectangle.x : 0
    y: targetTextArea ? targetTextArea.cursorRectangle.y : 0

    Behavior on x {
        NumberAnimation {
            duration: 80
            easing.type: Easing.OutCubic
        }
    }
    Behavior on y {
        NumberAnimation {
            duration: 80
            easing.type: Easing.OutCubic
        }
    }

    // Breathing pulse animation when idle
    SequentialAnimation on opacity {
        loops: Animation.Infinite
        running: targetTextArea && targetTextArea.focus
        NumberAnimation { from: 1.0; to: 0.2; duration: 600; easing.type: Easing.InOutQuad }
        NumberAnimation { from: 0.2; to: 1.0; duration: 600; easing.type: Easing.InOutQuad }
    }
}
