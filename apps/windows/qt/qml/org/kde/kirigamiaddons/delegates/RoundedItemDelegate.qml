import QtQuick
import QtQuick.Controls
import org.kde.kirigami as Kirigami

ItemDelegate {
    id: delegate

    background: Rectangle {
        radius: Kirigami.Units.cornerRadius
        color: delegate.highlighted ? Kirigami.Theme.highlightColor
                                    : delegate.hovered ? Kirigami.Theme.hoverColor
                                                       : "transparent"
    }
}
