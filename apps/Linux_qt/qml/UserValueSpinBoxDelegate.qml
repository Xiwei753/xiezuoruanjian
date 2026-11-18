import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

import org.kde.kirigami as Kirigami
import org.kde.kirigamiaddons.formcard as FormCard

FormCard.AbstractFormDelegate {
    id: root

    property string label: ""
    property alias value: spinbox.value
    property alias from: spinbox.from
    property alias to: spinbox.to
    property alias stepSize: spinbox.stepSize
    property alias textFromValue: spinbox.textFromValue
    property alias valueFromText: spinbox.valueFromText

    signal valueModified()

    onClicked: spinbox.forceActiveFocus()

    contentItem: ColumnLayout {
        spacing: Kirigami.Units.smallSpacing

        QQC2.Label {
            Layout.fillWidth: true
            text: root.label
            elide: Text.ElideRight
            color: root.enabled ? Kirigami.Theme.textColor : Kirigami.Theme.disabledTextColor
        }

        QQC2.SpinBox {
            id: spinbox
            Layout.fillWidth: true
            onValueModified: root.valueModified()
        }
    }
}
