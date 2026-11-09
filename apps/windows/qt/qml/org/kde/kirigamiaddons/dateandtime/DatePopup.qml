import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Popup {
    id: root

    property date value: new Date()
    property int selectedYear: value.getFullYear()
    property int selectedMonth: value.getMonth() + 1
    property int selectedDay: value.getDate()

    signal accepted()

    modal: true
    focus: true
    padding: 12
    implicitWidth: 320
    implicitHeight: contentColumn.implicitHeight + 24
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

    onOpened: {
        selectedYear = value.getFullYear()
        selectedMonth = value.getMonth() + 1
        selectedDay = value.getDate()
    }

    contentItem: ColumnLayout {
        id: contentColumn
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            SpinBox {
                Layout.fillWidth: true
                from: 1900
                to: 2200
                value: root.selectedYear
                onValueModified: root.selectedYear = value
            }
            SpinBox {
                Layout.fillWidth: true
                from: 1
                to: 12
                value: root.selectedMonth
                onValueModified: root.selectedMonth = value
            }
            SpinBox {
                Layout.fillWidth: true
                from: 1
                to: 31
                value: root.selectedDay
                onValueModified: root.selectedDay = value
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Item { Layout.fillWidth: true }
            Button {
                text: qsTr("取消")
                onClicked: root.close()
            }
            Button {
                text: qsTr("确定")
                onClicked: {
                    root.value = new Date(root.selectedYear, root.selectedMonth - 1, root.selectedDay)
                    root.accepted()
                    root.close()
                }
            }
        }
    }
}
