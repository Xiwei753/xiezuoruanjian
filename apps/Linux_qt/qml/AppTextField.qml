import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: control
    property var dt: null
    // Issue #701 评论 5699565102: 删除内部 fallbackDt。组件必须消费调用方
    // 传入的根 dt；漏传就是调用错误，不偷偷生成独立主题。
    readonly property var resolvedDt: dt
    property string label: ""
    property string placeholder: ""
    property alias placeholderText: control.placeholder
    property alias text: inputField.text
    property alias echoMode: inputField.echoMode
    property alias validator: inputField.validator
    property bool fieldTabFocus: true

    readonly property color normalTextColor: control.resolvedDt.onSurface
    readonly property color placeholderColor: control.resolvedDt.textMuted
    readonly property color backgroundColor: control.resolvedDt.surfaceContainerLow
    readonly property color labelColor: control.resolvedDt.onSurfaceVariant
    readonly property color activeBorderColor: control.resolvedDt.primary
    readonly property color inactiveBorderColor: control.resolvedDt.outline
    readonly property color highlightColor: control.resolvedDt.primary
    readonly property color highlightTextColor: control.resolvedDt.onPrimary

    signal accepted()
    signal editingFinished()

    implicitHeight: inputField.height + (control.label.length > 0 ? 24 : 0)
    implicitWidth: 200

    ColumnLayout {
        anchors.fill: parent
        spacing: control.resolvedDt.sp4

        AppText {
            dt: control.resolvedDt
            text: control.label
            variant: "onSurfaceVariant"
            font.pointSize: control.resolvedDt.labelPt
            font.weight: Font.Medium
            font.family: control.resolvedDt.fontFamily
            visible: control.label.length > 0
        }

        TextField {
            id: inputField
            Layout.fillWidth: true
            implicitHeight: control.resolvedDt.settingsControlHeight
            placeholderText: control.placeholder
            color: control.normalTextColor
            placeholderTextColor: control.placeholderColor
            selectionColor: control.highlightColor
            selectedTextColor: control.highlightTextColor
            activeFocusOnTab: control.fieldTabFocus
            onAccepted: control.accepted()
            onEditingFinished: control.editingFinished()
            background: Rectangle {
                color: control.backgroundColor
                border.color: inputField.activeFocus ? control.activeBorderColor : control.inactiveBorderColor
                border.width: inputField.activeFocus ? 2 : 1
                radius: control.resolvedDt.radiusMd
            }
            font.pointSize: control.resolvedDt.bodyPt
            font.family: control.resolvedDt.fontFamily
            leftPadding: control.resolvedDt.sp12
            rightPadding: control.resolvedDt.sp12
            topPadding: control.resolvedDt.sp8
            bottomPadding: control.resolvedDt.sp8
        }
    }
}
