import QtQuick
import QtQuick.Controls as Controls

Controls.ApplicationWindow {
    id: root

    // Linux main.qml 只写入 pageStack.globalToolBar.style。
    property QtObject pageStack: QtObject {
        property QtObject globalToolBar: QtObject {
            property int style: 0
        }
    }
}
