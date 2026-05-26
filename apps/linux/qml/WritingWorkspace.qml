import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    property var dt: null
    property var backendRef: null
    property var appState: ({})
    property var tree: []
    property string projectId: ""
    property string volumeId: ""
    property string chapterId: ""
    property string chapterTitle: ""
    property string projectTitle: ""
    property bool isLoadingChapter: false
    property string previousEditorText: ""
    property bool drawerOpen: false
    property int drawerTab: 0
    property bool aiCapable: false
    property bool aiEnabled: false

    signal backToProjects()
    signal openSettings()
    signal openSync()
    signal createVolumeRequested(string projectId)
    signal createChapterRequested(string projectId, string volumeId)

    function populateTreeModel() {
        treeModel.clear();
        if (!tree) return;
        for (var i = 0; i < tree.length; i++) {
            var item = tree[i];
            if (item.type === "project" && item.id === root.projectId) {
                treeModel.append({
                    "itemId": item.id || "",
                    "itemType": item.type || "",
                    "itemTitle": item.title || "",
                    "itemProjectId": item.id || "",
                    "itemVolumeId": ""
                });
                // Add volumes for this project
                for (var j = 0; j < tree.length; j++) {
                    var vol = tree[j];
                    if (vol.type === "volume" && vol.projectId === item.id) {
                        treeModel.append({
                            "itemId": vol.id || "",
                            "itemType": "volume",
                            "itemTitle": vol.title || "",
                            "itemProjectId": vol.projectId || "",
                            "itemVolumeId": vol.id || ""
                        });
                        // Add chapters for this volume
                        for (var k = 0; k < tree.length; k++) {
                            var chap = tree[k];
                            if (chap.type === "chapter" && chap.volumeId === vol.id && chap.projectId === vol.projectId) {
                                treeModel.append({
                                    "itemId": chap.id || "",
                                    "itemType": "chapter",
                                    "itemTitle": chap.title || "",
                                    "itemProjectId": chap.projectId || "",
                                    "itemVolumeId": chap.volumeId || ""
                                });
                            }
                        }
                    }
                }
                break;
            }
        }
    }

    onTreeChanged: populateTreeModel()
    Component.onCompleted: populateTreeModel()

    color: dt ? dt.bg : "#111318"

    function loadChapterContent() {
        if (!chapterId || !backendRef) return;
        isLoadingChapter = true;
        var content = backendRef.get_chapter_content(projectId, volumeId, chapterId);
        if (typeof window !== "undefined" && typeof window.debugLog === "function") {
            window.debugLog("editor", "workspace_load_chapter", "projectId=" + projectId + ", volumeId=" + volumeId + ", chapterId=" + chapterId + ", len=" + content.length);
        }
        editorPage.loadContent(content);
        previousEditorText = content;
        isLoadingChapter = false;
    }

    function computeWordCount(text) {
        if (!text) return 0;
        var count = 0;
        var chars = text.replace(/\s/g, '');
        count = chars.length;
        return count;
    }

    function reportStatsIfChanged() {
        if (isLoadingChapter || !chapterId) return;
        var newText = editorPage.text;
        if (previousEditorText === newText) return;

        var oldLen = previousEditorText.length;
        var newLen = newText.length;
        var diff = newLen - oldLen;

        if (diff > 0) {
            var source = "human_typed";
            var inserted = diff;
            var deleted = 0;
            var pasted = 0;
            if (diff > 20) {
                source = "pasted";
                pasted = diff;
                inserted = 0;
            }
            backendRef.report_writing_event(projectId, volumeId, chapterId, source, inserted, deleted, pasted);
        } else if (diff < 0) {
            backendRef.report_writing_event(projectId, volumeId, chapterId, "deleted", 0, Math.abs(diff), 0);
        }

        previousEditorText = newText;
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Top toolbar
        TopWritingToolbar {
            Layout.fillWidth: true
            dt: root.dt
            backendRef: root.backendRef
            currentFontSize: root.backendRef ? root.backendRef.setting_font_size : 16
            currentLineSpacing: root.backendRef ? root.backendRef.setting_line_spacing : 1.5
            firstLineIndent: false
            saveStatus: root.backendRef ? root.backendRef.save_status : ""
            onFontSizeChanged: function(size) {
                if (root.backendRef) {
                    root.backendRef.setting_font_size = size;
                }
            }
            onLineSpacingChanged: function(spacing) {
                if (root.backendRef) {
                    root.backendRef.setting_line_spacing = spacing;
                }
            }
            onFirstLineIndentToggled: firstLineIndent = !firstLineIndent
            onFormatOneClick: { /* placeholder */ }
            onLinkToStarMap: { root.drawerTab = 0; root.drawerOpen = true; }
            onOpenStats: { root.drawerTab = 2; root.drawerOpen = true; }
        }

        // Main content area
        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            // Left sidebar: volume/chapter tree
            Rectangle {
                Layout.preferredWidth: 240
                Layout.fillHeight: true
                color: dt ? dt.sidebar : "#14161B"
                border.color: dt ? dt.border : "#2A2E36"
                border.width: 1

                ColumnLayout {
                    anchors.fill: parent
                    spacing: 0

                    // Back button + project title
                    Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 48
                        color: "transparent"

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: dt ? dt.sp12 : 12
                            anchors.rightMargin: dt ? dt.sp8 : 8
                            spacing: dt ? dt.sp8 : 8

                            Rectangle {
                                width: 28; height: 28
                                radius: dt ? dt.radiusSm : 8
                                color: backHover.containsMouse ? (dt ? dt.card : "#1E2128") : "transparent"

                                Text {
                                    anchors.centerIn: parent
                                    text: "\u2190"
                                    color: dt ? dt.textSecondary : "#5C6070"
                                    font.pixelSize: dt ? dt.fontLg : 16
                                }

                                MouseArea {
                                    id: backHover
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: root.backToProjects()
                                }
                            }

                            Text {
                                text: root.projectTitle || "作品"
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontMd : 14
                                font.weight: Font.DemiBold
                                Layout.fillWidth: true
                                elide: Text.ElideRight
                            }
                        }
                    }

                    // Divider
                    Rectangle { Layout.fillWidth: true; height: 1; color: dt ? dt.border : "#2A2E36" }

                    // Tree list
                    ScrollView {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true

                        ListView {
                            id: treeListView
                            model: ListModel { id: treeModel }
                            delegate: Item {
                                width: treeListView.width
                                height: model.itemType === "volume" ? 36 : 32

                                Rectangle {
                                    id: delegateBg
                                    anchors.fill: parent
                                    anchors.leftMargin: dt ? dt.sp8 : 8
                                    anchors.rightMargin: dt ? dt.sp8 : 8
                                    radius: dt ? dt.radiusSm : 8
                                    color: isSelected ?
                                           (dt ? dt.accentSoft : "rgba(123,140,222,0.12)") :
                                           delegateHover.containsMouse ?
                                           (dt ? dt.card : "#1E2128") : "transparent"

                                    property bool isSelected: model.itemId === root.chapterId

                                    RowLayout {
                                        anchors.fill: parent
                                        anchors.leftMargin: model.itemType === "chapter" ? (dt ? dt.sp32 : 32) : (dt ? dt.sp12 : 12)
                                        spacing: dt ? dt.sp6 : 6

                                        Text {
                                            text: model.itemType === "volume" ? "\uD83D\uDCC1" : "\uD83D\uDCC4"
                                            font.pixelSize: dt ? dt.fontSm : 12
                                            opacity: 0.6
                                        }

                                        Text {
                                            text: model.itemTitle || ""
                                            color: delegateBg.isSelected ?
                                                   (dt ? dt.accentText : "#3D4D9E") :
                                                   (dt ? dt.textPrimary : "#E2E4E9")
                                            font.pixelSize: dt ? dt.fontSm : 12
                                            font.weight: delegateBg.isSelected ? Font.DemiBold : Font.Normal
                                            Layout.fillWidth: true
                                            elide: Text.ElideRight
                                        }

                                        // "+" button for volumes (create chapter)
                                        Rectangle {
                                            visible: model.itemType === "volume"
                                            width: 20; height: 20
                                            radius: 10
                                            color: addChapterHover.containsMouse ? (dt ? dt.accentSoft : "rgba(123,140,222,0.12)") : "transparent"

                                            Text {
                                                anchors.centerIn: parent
                                                text: "+"
                                                color: dt ? dt.textMuted : "#606470"
                                                font.pixelSize: dt ? dt.fontSm : 12
                                                font.weight: Font.Bold
                                            }

                                            MouseArea {
                                                id: addChapterHover
                                                anchors.fill: parent
                                                hoverEnabled: true
                                                cursorShape: Qt.PointingHandCursor
                                                onClicked: root.createChapterRequested(model.itemProjectId || "", model.itemId)
                                            }
                                        }
                                    }

                                    MouseArea {
                                        id: delegateHover
                                        anchors.fill: parent
                                        hoverEnabled: true
                                        acceptedButtons: Qt.LeftButton | Qt.RightButton
                                        cursorShape: Qt.PointingHandCursor
                                        onClicked: function(mouse) {
                                            if (mouse.button === Qt.LeftButton) {
                                                if (model.itemType === "chapter") {
                                                    root.chapterId = model.itemId;
                                                    root.volumeId = model.itemVolumeId;
                                                    root.chapterTitle = model.itemTitle;
                                                    root.loadChapterContent();
                                                }
                                            } else if (mouse.button === Qt.RightButton) {
                                                treeContextMenu.itemType = model.itemType;
                                                treeContextMenu.itemId = model.itemId;
                                                treeContextMenu.itemTitle = model.itemTitle;
                                                treeContextMenu.itemProjectId = model.itemProjectId || "";
                                                treeContextMenu.itemVolumeId = model.itemVolumeId || "";
                                                treeContextMenu.popup();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // "+" button for project (create volume)
                    Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 36
                        Layout.leftMargin: dt ? dt.sp8 : 8
                        Layout.rightMargin: dt ? dt.sp8 : 8
                        radius: dt ? dt.radiusSm : 8
                        color: addVolumeHover.containsMouse ? (dt ? dt.accentSoft : "rgba(123,140,222,0.12)") : "transparent"

                        RowLayout {
                            anchors.centerIn: parent
                            spacing: dt ? dt.sp4 : 4
                            Text {
                                text: "+"
                                color: dt ? dt.accent : "#7B8CDE"
                                font.pixelSize: dt ? dt.fontMd : 14
                                font.weight: Font.Bold
                            }
                            Text {
                                text: "新建卷"
                                color: dt ? dt.accent : "#7B8CDE"
                                font.pixelSize: dt ? dt.fontSm : 12
                            }
                        }

                        MouseArea {
                            id: addVolumeHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.createVolumeRequested(root.projectId)
                        }
                    }

                    // Tree context menu
                    Menu {
                        id: treeContextMenu
                        property string itemType: ""
                        property string itemId: ""
                        property string itemTitle: ""
                        property string itemProjectId: ""
                        property string itemVolumeId: ""

                        MenuItem {
                            text: "新建卷"
                            visible: treeContextMenu.itemType === "project"
                            onTriggered: root.createVolumeRequested(treeContextMenu.itemProjectId || root.projectId)
                        }
                        MenuItem {
                            text: "新建章节"
                            visible: treeContextMenu.itemType === "volume"
                            onTriggered: root.createChapterRequested(treeContextMenu.itemProjectId, treeContextMenu.itemId)
                        }
                    }
                }
            }

            // Center: Editor paper area
            Rectangle {
                Layout.fillWidth: true
                Layout.fillHeight: true
                color: dt ? dt.bg : "#111318"

                // Paper background
                Rectangle {
                    anchors.fill: editorScroll
                    anchors.margins: -1
                    color: dt ? dt.editorBg : "#191C21"
                    radius: dt ? dt.radiusMd : 12
                    border.color: dt ? dt.border : "#2A2E36"
                    border.width: 1
                }

                ScrollView {
                    id: editorScroll
                    anchors.fill: parent
                    anchors.margins: dt ? dt.sp32 : 32
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
                        color: dt ? dt.editorText : "#2C2E36"
                        font.pixelSize: root.backendRef ? root.backendRef.setting_font_size : 16
                        font.family: "serif"
                        wrapMode: TextArea.Wrap
                        background: Rectangle { color: "transparent" }
                        enabled: root.chapterId !== ""
                        focus: true
                        activeFocusOnTab: true
                        selectByMouse: true
                        persistentSelection: true
                        leftPadding: dt ? dt.sp8 : 8
                        rightPadding: dt ? dt.sp8 : 8
                        topPadding: dt ? dt.sp16 : 16
                        bottomPadding: dt ? dt.sp16 : 16

                        text: ""
                    }
                }

                // Empty state
                ColumnLayout {
                    anchors.centerIn: parent
                    spacing: dt ? dt.sp12 : 12
                    visible: !root.chapterId

                    Text {
                        text: "\uD83D\uDCDD"
                        font.pixelSize: 40
                        Layout.alignment: Qt.AlignHCenter
                    }
                    Text {
                        text: "选择一个章节开始写作"
                        color: dt ? dt.textSecondary : "#9CA0AB"
                        font.pixelSize: dt ? dt.fontLg : 16
                        Layout.alignment: Qt.AlignHCenter
                    }
                }
            }

            // Right drawer button (when closed)
            Rectangle {
                visible: !root.drawerOpen
                Layout.fillHeight: true
                Layout.preferredWidth: 36
                color: "transparent"

                ColumnLayout {
                    anchors.centerIn: parent
                    spacing: dt ? dt.sp8 : 8

                    Rectangle {
                        width: 28; height: 28
                        radius: dt ? dt.radiusSm : 8
                        color: drawerBtnHover.containsMouse ? (dt ? dt.card : "#1E2128") : "transparent"

                        Text {
                            anchors.centerIn: parent
                            text: "\u25B6"
                            color: dt ? dt.textMuted : "#606470"
                            font.pixelSize: dt ? dt.fontXs : 11
                        }

                        MouseArea {
                            id: drawerBtnHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.drawerOpen = true
                        }
                    }
                }
            }

            // Right drawer
            RightDrawer {
                Layout.fillHeight: true
                dt: root.dt
                backendRef: root.backendRef
                isOpen: root.drawerOpen
                currentTab: root.drawerTab
                aiCapable: root.aiCapable
                aiEnabled: root.aiEnabled
                onCloseRequested: root.drawerOpen = false
                onOpenStarMap: { root.drawerTab = 0; root.drawerOpen = true; }
                onOpenSettings: root.openSettings()
            }
        }
    }

    Connections {
        target: root.backendRef
        function onChapter_path_changed() {
            if (root.chapterId) {
                root.loadChapterContent();
            }
        }
    }

    Timer {
        id: autoSaveTimer
        interval: root.backendRef ? root.backendRef.setting_auto_save_delay_ms : 1500
        repeat: false
        onTriggered: {
            if (!root.backendRef || !root.chapterId) return;
            if (!root.backendRef.setting_auto_save_enabled) return;
            root.backendRef.save_current_chapter(editorArea.text);
        }
    }

    Timer {
        id: autoSyncTimer
        interval: 20000
        repeat: false
        onTriggered: {
            if (!root.backendRef || !root.backendRef.has_workspace) return;
            if (!root.backendRef.sync_auto_sync || !root.backendRef.sync_enabled) return;
            root.backendRef.request_auto_sync("auto_sync_after_save");
        }
    }

    Connections {
        target: editorArea
        function onTextChanged() {
            root.reportStatsIfChanged();
            if (root.chapterId && root.backendRef) {
                root.backendRef.calculate_word_count(editorArea.text);
                if (root.backendRef.setting_auto_save_enabled) {
                    autoSaveTimer.restart();
                }
            }
        }
    }
}
