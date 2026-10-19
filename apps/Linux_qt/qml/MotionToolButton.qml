import QtQuick
import QtQuick.Controls

ToolButton {
    id: root

    MotionTokens { id: motion }

    scale: down ? 0.96 : 1.0

    Behavior on scale {
        NumberAnimation {
            duration: motion.pressMs
            easing.type: Easing.OutCubic
        }
    }
}
