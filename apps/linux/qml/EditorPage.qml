import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    color: appTheme ? appTheme.editorBg : "#1a1a2e"

    property var backendRef: null
    property var appTheme: null
    property double lastTextChangeLogTime: 0
    readonly property string text: editorArea.text

    signal contentChanged()

    function clearText() {
        if (typeof window !== "undefined" && typeof window.debugLog === "function") {
            window.debugLog("editor", "clear_text", "");
        }
        editorArea.text = ""
    }
    function loadContent(content) {
        if (typeof window !== "undefined" && typeof window.debugLog === "function") {
            window.debugLog("editor", "load_content", "len=" + (content ? content.length : 0));
        }
        editorArea.text = content
    }
    function forceEditorFocus() { editorArea.forceActiveFocus() }

    Rectangle {
        anchors.fill: parent
        color: root.appTheme ? root.appTheme.editorBg : "#1a1a2e"
        clip: true

        // Placeholder
        Text {
            anchors.centerIn: parent
            text: "请在左侧选择或创建一个章节"
            color: root.appTheme ? root.appTheme.textSecondary : "#475569"
            font.pixelSize: root.appTheme ? root.appTheme.fontLg : 15
            visible: !root.backendRef || !root.backendRef.has_selected_chapter_prop
        }

        // Editor area with max-width
        Item {
            anchors.fill: parent
            anchors.margins: root.appTheme ? root.appTheme.sp24 : 24
            visible: root.backendRef && root.backendRef.has_selected_chapter_prop

            Rectangle {
                width: Math.min(parent.width, 800)
                height: parent.height
                anchors.horizontalCenter: parent.horizontalCenter
                color: "transparent"

                ScrollView {
                    id: editorScroll
                    anchors.fill: parent
                    clip: true
                    ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                    ScrollBar.vertical: ScrollBar {
                        policy: ScrollBar.AsNeeded
                        parent: editorScroll
                        anchors.top: editorScroll.top
                        anchors.bottom: editorScroll.bottom
                        anchors.right: editorScroll.right
                    }

                    TextArea {
                        id: editorArea
                        width: editorScroll.availableWidth
                        color: root.appTheme ? root.appTheme.textPrimary : "#0f172a"
                        font.pixelSize: {
                            if (root.backendRef && root.backendRef.setting_font_size > 0) {
                                return root.backendRef.setting_font_size
                            }
                            let def = root.appTheme ? root.appTheme.fontLg : 15
                            return def
                        }
                        font.family: "serif"
                        wrapMode: TextArea.Wrap
                        background: Rectangle { color: "transparent" }
                        enabled: root.backendRef && root.backendRef.has_selected_chapter_prop
                        focus: true
                        activeFocusOnTab: true
                        selectByMouse: true
                        persistentSelection: true
                        leftPadding: root.appTheme ? root.appTheme.sp4 : 4
                        rightPadding: root.appTheme ? root.appTheme.sp4 : 4
                        topPadding: root.appTheme ? root.appTheme.sp4 : 4
                        bottomPadding: root.appTheme ? root.appTheme.sp4 : 4

                        onTextChanged: {
                            var now = Date.now();
                            if (now - root.lastTextChangeLogTime > 2000) {
                                root.lastTextChangeLogTime = now;
                                if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                                    window.debugLog("editor", "content_changed", "len=" + editorArea.text.length);
                                }
                            }
                            root.contentChanged()
                        }
                    }
                }
            }
        }
    }
}
