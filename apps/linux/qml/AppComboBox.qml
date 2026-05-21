import QtQuick 2.15
import QtQuick.Controls 2.15

ComboBox {
    id: control
    property var theme: null

    contentItem: Text {
        text: control.displayText
        color: control.enabled
            ? (control.theme ? control.theme.textPrimary : "#0f172a")
            : (control.theme ? control.theme.textDisabled : "#94a3b8")
        font.pixelSize: control.theme ? control.theme.fontMd : 13
        leftPadding: control.theme ? control.theme.sp8 : 8
        verticalAlignment: Text.AlignVCenter
    }

    background: Rectangle {
        color: control.theme ? control.theme.surfaceAlt : "#f1f5f9"
        border.color: {
            if (!control.theme) return "#e2e8f0"
            if (control.activeFocus) return control.theme.borderFocus
            if (control.hovered) return control.theme.secondary
            return control.theme.border
        }
        border.width: 1
        radius: control.theme ? control.theme.radiusSm : 6
    }

    delegate: ItemDelegate {
        width: control.width
        contentItem: Text {
            text: modelData
            color: control.theme ? control.theme.textPrimary : "#0f172a"
            font.pixelSize: control.theme ? control.theme.fontMd : 13
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            color: control.highlightedIndex === index
                ? (control.theme ? control.theme.selected : "#dbeafe")
                : "transparent"
        }
    }

    popup: Popup {
        y: control.height - 1
        width: control.width
        implicitHeight: contentItem.implicitHeight
        padding: control.theme ? control.theme.sp4 : 4

        contentItem: ListView {
            clip: true
            implicitHeight: contentHeight
            model: control.popup.visible ? control.delegateModel : null
            currentIndex: control.highlightedIndex
            ScrollIndicator.vertical: ScrollIndicator {}
        }

        background: Rectangle {
            color: control.theme ? control.theme.surface : "#ffffff"
            border.color: control.theme ? control.theme.border : "#e2e8f0"
            border.width: 1
            radius: control.theme ? control.theme.radiusSm : 6
        }
    }
}
