import re

with open('apps/linux/qml/WritingWorkspace.qml', 'r') as f:
    content = f.read()

# Replace SplitView preferredWidth and onWidthChanged with currentSidebarWidth and Timer
sidebar_re = r'''(SplitView\.preferredWidth: root\.backendRef && root\.backendRef\.setting_linux_sidebar_width > 0 \? root\.backendRef\.setting_linux_sidebar_width : 240)
(.*?)onWidthChanged: \{
(.*?)if \(root\.backendRef && width > 0\) \{
(.*?)root\.backendRef\.setting_linux_sidebar_width = width;
(.*?)\}
(.*?)\}'''

sidebar_replacement = r'''property real currentSidebarWidth: root.backendRef && root.backendRef.setting_linux_sidebar_width > 0 ? root.backendRef.setting_linux_sidebar_width : 240
            SplitView.preferredWidth: currentSidebarWidth
\2Timer {
                id: sidebarDebounceTimer
                interval: 300
                repeat: false
                onTriggered: {
                    if (root.backendRef && sidebarRect.width > 0 && Math.abs(root.backendRef.setting_linux_sidebar_width - sidebarRect.width) >= 1.0) {
                        root.backendRef.setting_linux_sidebar_width = sidebarRect.width;
                    }
                }
            }

            onWidthChanged: {
                if (width > 0 && width !== currentSidebarWidth) {
                    currentSidebarWidth = width;
                    sidebarDebounceTimer.restart();
                }
            }'''

content = re.sub(sidebar_re, sidebar_replacement, content, flags=re.DOTALL)

# Add SplitView.handle to SplitView
splitview_handle_re = r'''(SplitView \{
\s*anchors\.fill: parent
\s*orientation: Qt\.Horizontal)'''

splitview_handle_replacement = r'''\1

        handle: Rectangle {
            implicitWidth: 4
            color: SplitHandle.hovered || SplitHandle.pressed ? (dt ? dt.primary : "#006497") : (dt ? dt.border : "#2A2E36")
            Behavior on color { ColorAnimation { duration: 120 } }
        }'''

content = re.sub(splitview_handle_re, splitview_handle_replacement, content)

# Replace paperBg logic
paperbg_re = r'''(property real baseResponsiveWidth: Math\.min\(parent\.width, editorMaxWidth, Math\.max\(820, parent\.width \* editorWidthRatio\)\)
\s*width: root\.backendRef && root\.backendRef\.setting_linux_editor_width > 0 \? Math\.min\(parent\.width, root\.backendRef\.setting_linux_editor_width\) : baseResponsiveWidth
\s*height: parent\.height
\s*color: dt \? dt\.editorBg : "#191C21"
\s*radius: dt \? dt\.radiusMd : 12
\s*border\.color: dt \? dt\.border : "#2A2E36"
\s*border\.width: 1)
\s*// Left edge drag handle.*?(?=ScrollView \{)'''

paperbg_replacement = r'''\1
                            
                            property real currentEditorWidth: root.backendRef && root.backendRef.setting_linux_editor_width > 0 ? root.backendRef.setting_linux_editor_width : baseResponsiveWidth
                            width: currentEditorWidth
                            
                            Timer {
                                id: editorWidthDebounceTimer
                                interval: 300
                                repeat: false
                                onTriggered: {
                                    if (root.backendRef && Math.abs(root.backendRef.setting_linux_editor_width - paperBg.currentEditorWidth) >= 1.0) {
                                        root.backendRef.setting_linux_editor_width = paperBg.currentEditorWidth;
                                    }
                                }
                            }
                            
                            function resetEditorWidth() {
                                if (root.backendRef) {
                                    root.backendRef.setting_linux_editor_width = 0;
                                }
                                currentEditorWidth = baseResponsiveWidth;
                            }

                            // Left edge drag handle
                            Rectangle {
                                anchors.left: parent.left
                                anchors.top: parent.top
                                anchors.bottom: parent.bottom
                                width: 4
                                color: leftDragArea.containsMouse || leftDragArea.pressed ? (dt ? dt.primary : "#006497") : "transparent"
                                Behavior on color { ColorAnimation { duration: 120 } }

                                MouseArea {
                                    id: leftDragArea
                                    anchors.fill: parent
                                    anchors.margins: -4
                                    hoverEnabled: true
                                    cursorShape: Qt.SizeHorCursor
                                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                                    property real startX: 0
                                    property real startWidth: 0
                                    onPressed: function(mouse) {
                                        if (mouse.button === Qt.RightButton) {
                                            paperBg.resetEditorWidth();
                                            return;
                                        }
                                        startX = mouse.x;
                                        startWidth = paperBg.currentEditorWidth;
                                    }
                                    onPositionChanged: function(mouse) {
                                        if (pressed && (mouse.buttons & Qt.LeftButton)) {
                                            var dx = mouse.x - startX;
                                            var newWidth = startWidth - dx * 2;
                                            var maxPossible = Math.max(1400, paperBg.parent.width - 64);
                                            newWidth = Math.max(720, Math.min(newWidth, maxPossible));
                                            paperBg.currentEditorWidth = newWidth;
                                            editorWidthDebounceTimer.restart();
                                        }
                                    }
                                    onDoubleClicked: paperBg.resetEditorWidth()
                                }
                            }

                            // Right edge drag handle
                            Rectangle {
                                anchors.right: parent.right
                                anchors.top: parent.top
                                anchors.bottom: parent.bottom
                                width: 4
                                color: rightDragArea.containsMouse || rightDragArea.pressed ? (dt ? dt.primary : "#006497") : "transparent"
                                Behavior on color { ColorAnimation { duration: 120 } }

                                MouseArea {
                                    id: rightDragArea
                                    anchors.fill: parent
                                    anchors.margins: -4
                                    hoverEnabled: true
                                    cursorShape: Qt.SizeHorCursor
                                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                                    property real startX: 0
                                    property real startWidth: 0
                                    onPressed: function(mouse) {
                                        if (mouse.button === Qt.RightButton) {
                                            paperBg.resetEditorWidth();
                                            return;
                                        }
                                        startX = mouse.x;
                                        startWidth = paperBg.currentEditorWidth;
                                    }
                                    onPositionChanged: function(mouse) {
                                        if (pressed && (mouse.buttons & Qt.LeftButton)) {
                                            var dx = mouse.x - startX;
                                            var newWidth = startWidth + dx * 2;
                                            var maxPossible = Math.max(1400, paperBg.parent.width - 64);
                                            newWidth = Math.max(720, Math.min(newWidth, maxPossible));
                                            paperBg.currentEditorWidth = newWidth;
                                            editorWidthDebounceTimer.restart();
                                        }
                                    }
                                    onDoubleClicked: paperBg.resetEditorWidth()
                                }
                            }
                            
                            '''

content = re.sub(paperbg_re, paperbg_replacement, content, flags=re.DOTALL)

with open('apps/linux/qml/WritingWorkspace.qml', 'w') as f:
    f.write(content)
