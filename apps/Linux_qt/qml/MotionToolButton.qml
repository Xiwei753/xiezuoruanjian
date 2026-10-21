import QtQuick
import QtQuick.Controls

ToolButton {
    id: root

    MotionTokens { id: motion }

    // Issue #977：顶栏按钮统一最小点击区 ~40×40；带文字按钮仍按自然宽度增长。
    implicitWidth: Math.max(40, implicitContentWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(40, implicitContentHeight + topPadding + bottomPadding)

    scale: down ? 0.96 : 1.0

    Behavior on scale {
        NumberAnimation {
            duration: motion.pressMs
            easing.type: Easing.OutCubic
        }
    }
}
