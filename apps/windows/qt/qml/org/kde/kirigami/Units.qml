pragma Singleton
import QtQuick

QtObject {
    readonly property int gridUnit: 8
    readonly property int smallSpacing: 4
    readonly property int largeSpacing: 16
    readonly property int cornerRadius: 6
    readonly property int shortDuration: 100
    readonly property int longDuration: 200
    readonly property int veryLongDuration: 400

    readonly property QtObject iconSizes: QtObject {
        readonly property int small: 16
        readonly property int smallMedium: 22
        readonly property int medium: 32
        readonly property int large: 48
        readonly property int huge: 64
        readonly property int enormous: 128
    }
}
