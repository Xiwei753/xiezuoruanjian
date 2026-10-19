import QtQuick
import QtQuick.Controls

Dialog {
    MotionTokens { id: motion }

    enter: Transition {
        NumberAnimation {
            property: "opacity"
            from: 0
            to: 1
            duration: motion.dialogMs
            easing.type: Easing.OutCubic
        }
        NumberAnimation {
            property: "scale"
            from: 0.98
            to: 1
            duration: motion.dialogMs
            easing.type: Easing.OutCubic
        }
    }

    exit: Transition {
        NumberAnimation {
            property: "opacity"
            from: 1
            to: 0
            duration: motion.dialogMs
            easing.type: Easing.OutCubic
        }
        NumberAnimation {
            property: "scale"
            from: 1
            to: 0.98
            duration: motion.dialogMs
            easing.type: Easing.OutCubic
        }
    }
}
