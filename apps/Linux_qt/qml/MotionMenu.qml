import QtQuick
import QtQuick.Controls

Menu {
    MotionTokens { id: motion }

    enter: Transition {
        NumberAnimation {
            property: "opacity"
            from: 0
            to: 1
            duration: motion.menuMs
            easing.type: Easing.OutCubic
        }
    }

    exit: Transition {
        NumberAnimation {
            property: "opacity"
            from: 1
            to: 0
            duration: motion.menuMs
            easing.type: Easing.OutCubic
        }
    }
}
