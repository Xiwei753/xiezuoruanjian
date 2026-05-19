import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    color: "#1e1e1e"

    property var backendRef: null
    readonly property string text: editorArea.text

    signal contentChanged()

    function clearText() {
        editorArea.text = ""
    }

    function loadContent(content) {
        editorArea.text = content
    }

    function forceEditorFocus() {
        editorArea.forceActiveFocus()
    }

    Rectangle {
        anchors.fill: parent
        color: "#1e1e1e"
        clip: true

        Text {
            anchors.centerIn: parent
            text: "请在左侧选择或创建一个章节"
            color: "gray"
            visible: !root.backendRef || !root.backendRef.has_selected_chapter_prop
        }

        ScrollView {
            id: editorScroll
            anchors.fill: parent
            anchors.margins: 20
            clip: true
            visible: root.backendRef && root.backendRef.has_selected_chapter_prop
            ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

            TextArea {
                id: editorArea
                color: "#d4d4d4"
                font.pixelSize: root.backendRef && root.backendRef.setting_font_size > 0 ? root.backendRef.setting_font_size : 16
                wrapMode: TextArea.Wrap
                background: Rectangle { color: "transparent" }
                enabled: root.backendRef && root.backendRef.has_selected_chapter_prop
                focus: true
                activeFocusOnTab: true
                selectByMouse: true
                persistentSelection: true

                onTextChanged: {
                    root.contentChanged()
                }
            }
        }
    }
}
