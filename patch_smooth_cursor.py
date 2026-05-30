import re

with open('apps/linux/qml/SmoothCursor.qml', 'r') as f:
    content = f.read()

# Replace typing pulse animation and connections
pulse_re = r'''(    Rectangle \{
\s*id: typingPulse
.*?
    Connections \{
.*?
    \})'''

pulse_replacement = r'''    property bool isTyping: false

    Behavior on x {
        id: xBehavior
        enabled: !root.isTyping
        NumberAnimation {
            duration: root.cursorAnimationDuration
            easing.type: Easing.OutCubic
        }
    }
    Behavior on y {
        id: yBehavior
        enabled: !root.isTyping
        NumberAnimation {
            duration: root.cursorAnimationDuration
            easing.type: Easing.OutCubic
        }
    }

    Rectangle {
        id: typingPulse
        x: -4
        y: -4
        width: 12
        height: Math.max(18, root.height + 8)
        radius: 4
        color: root.dt ? root.dt.accent : "#7B8CDE"
        opacity: 0
        visible: opacity > 0
        z: -1
    }

    ParallelAnimation {
        id: typingPulseAnimation
        NumberAnimation {
            target: typingPulse
            property: "opacity"
            from: 0.4
            to: 0
            duration: root.typingAnimationDuration * 1.5
            easing.type: Easing.OutSine
        }
        NumberAnimation {
            target: typingPulse
            property: "width"
            from: 12
            to: 32
            duration: root.typingAnimationDuration * 1.5
            easing.type: Easing.OutSine
        }
        NumberAnimation {
            target: typingPulse
            property: "x"
            from: -4
            to: -14
            duration: root.typingAnimationDuration * 1.5
            easing.type: Easing.OutSine
        }
    }

    Timer {
        id: typingResetTimer
        interval: 32
        repeat: false
        onTriggered: {
            root.isTyping = false;
        }
    }

    Connections {
        target: root.targetTextArea
        function onTextChanged() {
            var len = root.targetTextArea ? root.targetTextArea.length : 0;
            if (root.typingAnimationEnabled && root.targetTextArea && root.targetTextArea.activeFocus && len > root.lastTextLength) {
                root.isTyping = true;
                typingResetTimer.restart();
                
                typingPulseAnimation.stop();
                typingPulse.opacity = 0.4;
                typingPulse.width = 12;
                typingPulse.x = -4;
                typingPulseAnimation.start();
            }
            root.lastTextLength = len;
        }
    }'''

# Note: In the original file there are already Behavior on x and y.
# So I should just replace them along with the pulse.
behavior_re = r'''(    Behavior on x \{.*?    Behavior on y \{.*?    \})'''
content = re.sub(behavior_re, "", content, flags=re.DOTALL)
content = re.sub(pulse_re, pulse_replacement, content, flags=re.DOTALL)

with open('apps/linux/qml/SmoothCursor.qml', 'w') as f:
    f.write(content)

