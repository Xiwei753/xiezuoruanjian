// =============================================================================
// WritingWorkspace.qml — 写作工作区
// =============================================================================
//
// 层级：Linux UI 层（QML 页面）
// 职责：写作区整体布局（侧栏树 + 编辑区 + 工具栏）
// 约束：
//   - 纯布局容器，业务逻辑委托给 EditorController
//   - 通过 WritingTreeController 管理树结构
//   - 不直接操作文件系统
//
// 组成：
//   WorkspaceTree (侧栏) + EditorPage (编辑区) + TopWritingToolbar (工具栏)
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    property var dt: null
    property var backendRef: null
    property var appState: ({})
    property var tree: []
    property string projectTitle: ""
    property bool drawerOpen: false
    property int drawerTab: 0
    property bool aiCapable: false
    property bool aiEnabled: false

    // Project-level ID — set by main.qml, used for tree and create volume/chapter
    property string workspaceProjectId: ""

    signal backToProjects()
    signal openSettings()
    signal openSync()
    signal createVolumeRequested(string projectId)
    signal createChapterRequested(string projectId, string volumeId)

    WritingTreeController {
        id: writingTree
        tree: root.tree
        projectId: root.workspaceProjectId
        onItemsChanged: root.populateTreeModel()
    }

    function populateTreeModel() {
        treeModel.clear();
        var items = writingTree.items || [];
        for (var i = 0; i < items.length; i++) {
            treeModel.append({
                "itemId": items[i].id || "",
                "itemType": items[i].type || "",
                "itemTitle": items[i].title || "",
                "itemProjectId": items[i].projectId || "",
                "itemVolumeId": items[i].volumeId || ""
            });
        }
    }

    EditorController {
        id: editorController
        targetTextArea: editorArea
        backendRef: root.backendRef
        dt: root.dt
        onEmptySaveBlocked: function(msg) {
            emptySaveDialogText.text = msg;
            emptySaveDialog.open();
        }
    }

    Dialog {
        id: emptySaveDialog
        modal: true
        width: 360
        height: 180
        anchors.centerIn: parent
        background: Rectangle { color: dt ? dt.surface : "#FCFCFF"; border.color: dt ? dt.border : "#CBD5E1"; radius: dt ? dt.radiusXl : 24; border.width: 1 }
        header: null

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: dt ? dt.sp24 : 24
            spacing: dt ? dt.sp16 : 16
            AppText {
                text: qsTr("保存被阻止")
                color: dt ? dt.textPrimary : "#E2E2E5"
                font.pixelSize: dt ? dt.subtitle : 18
                font.family: dt ? dt.fontFamily : "sans-serif"
                font.weight: Font.DemiBold
            }
            AppText {
                id: emptySaveDialogText
                Layout.fillWidth: true
                text: qsTr("检测到异常空内容覆盖，已阻止保存。")
                color: dt ? dt.textSecondary : "#8C9198"
                font.pixelSize: dt ? dt.body : 14
                font.family: dt ? dt.fontFamily : "sans-serif"
                wrapMode: Text.Wrap
            }
            AppButton {
                text: qsTr("确定")
                theme: dt
                variant: "primary"
                Layout.alignment: Qt.AlignRight
                onClicked: emptySaveDialog.close()
            }
        }
    }

    onTreeChanged: populateTreeModel()
    Component.onCompleted: {
        populateTreeModel();
        if (root.backendRef && root.backendRef.selected_chapter_id) {
            root.openChapter(
                root.backendRef.selected_project_id,
                root.backendRef.selected_volume_id,
                root.backendRef.selected_chapter_id,
                ""
            );
        }
    }

    function openChapter(pId, vId, cId, cTitle) {
        if (!pId || !vId || !cId) return;
        // Prevent re-opening the same chapter (anti-loop guard)
        if (editorController.projectId === pId &&
            editorController.volumeId === vId &&
            editorController.chapterId === cId &&
            !editorController.isLoadingChapter) {
            return;
        }

        // loadChapterContentWithIds returns null on failure, result object on success.
        // State is only updated after content is successfully loaded.
        var result = editorController.loadChapterContentWithIds(pId, vId, cId);
        if (result) {
            editorController.projectId = result.projectId || pId;
            editorController.volumeId = result.volumeId || vId;
            editorController.chapterId = result.chapterId || cId;
            editorController.chapterTitle = result.title || cTitle || "";
        }
    }

    color: dt ? dt.bg : "#111318"

    SplitView {
        anchors.fill: parent
        orientation: Qt.Horizontal

        handle: Rectangle {
            implicitWidth: 4
            color: SplitHandle.hovered || SplitHandle.pressed ? (dt ? dt.primary : "#006497") : (dt ? dt.border : "#2A2E36")
            Behavior on color { ColorAnimation { duration: 120 } }
        }

        // Left sidebar: volume/chapter tree
        Rectangle {
            id: sidebarRect
            SplitView.preferredWidth: root.backendRef && root.backendRef.setting_linux_sidebar_width > 0 ? root.backendRef.setting_linux_sidebar_width : 240
            SplitView.minimumWidth: 180
            SplitView.maximumWidth: 420
            color: dt ? dt.sidebar : "#14161B"
            border.color: dt ? dt.border : "#2A2E36"
            border.width: 1

            Timer {
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
                if (width > 0) {
                    sidebarDebounceTimer.restart();
                }
            }

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
                                radius: dt ? dt.radiusPill : 999
                                color: backHover.containsMouse ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"

                                AppText {
                                    anchors.centerIn: parent
                                    text: "\u2190"
                                    color: dt ? dt.textSecondary : "#8C9198"
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

                            AppText {
                                text: root.projectTitle || qsTr("作品")
                                color: dt ? dt.textPrimary : "#E2E2E5"
                                font.pixelSize: dt ? dt.fontMd : 14
                                font.family: dt ? dt.fontFamily : "sans-serif"
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
                                    radius: dt ? dt.radiusPill : 999
                                    color: isSelected ?
                                           (dt ? dt.primaryContainer : "#CCE5FF") :
                                           delegateHover.containsMouse ?
                                           (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"

                                    property bool isSelected: model.itemId === editorController.chapterId

                                    RowLayout {
                                        anchors.fill: parent
                                        anchors.leftMargin: model.itemType === "chapter" ? (dt ? dt.sp32 : 32) : (dt ? dt.sp12 : 12)
                                        spacing: dt ? dt.sp6 : 6

                                        Rectangle {
                                            width: 6; height: 6
                                            radius: model.itemType === "volume" ? 0 : 3
                                            color: delegateBg.isSelected ? (dt ? dt.selectedText : "#CCE5FF") : (dt ? dt.textSecondary : "#8C9198")
                                            Layout.alignment: Qt.AlignVCenter
                                            opacity: 0.6
                                        }

                                        AppText {
                                            text: model.itemTitle || ""
                                            color: delegateBg.isSelected ?
                                                   (dt ? dt.onPrimaryContainer : "#CCE5FF") :
                                                   (dt ? dt.textPrimary : "#E2E2E5")
                                            font.pixelSize: dt ? dt.label : 13
                                            font.family: dt ? dt.fontFamily : "sans-serif"
                                            font.weight: delegateBg.isSelected ? Font.DemiBold : Font.Normal
                                            Layout.fillWidth: true
                                            elide: Text.ElideRight
                                        }

                                        // "+" button for volumes (create chapter)
                                        Rectangle {
                                            visible: model.itemType === "volume"
                                            width: 20; height: 20
                                            radius: 10
                                            color: addChapterHover.containsMouse ? (dt ? dt.primaryContainer : "#CCE5FF") : "transparent"

                                            AppText {
                                                anchors.centerIn: parent
                                                text: "+"
                                                color: dt ? dt.primary : "#006497"
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
                                                     root.openChapter(model.itemProjectId || root.workspaceProjectId, model.itemVolumeId, model.itemId, model.itemTitle);
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
                        radius: dt ? dt.radiusPill : 999
                        color: addVolumeHover.containsMouse ? (dt ? dt.primaryContainer : "#CCE5FF") : "transparent"

                        RowLayout {
                            anchors.centerIn: parent
                            spacing: dt ? dt.sp4 : 4
                            AppText {
                                text: "+"
                                color: dt ? dt.primary : "#006497"
                                font.pixelSize: dt ? dt.fontMd : 14
                                font.weight: Font.Bold
                            }
                            AppText {
                                text: qsTr("新建卷")
                                color: dt ? dt.primary : "#006497"
                                font.pixelSize: dt ? dt.label : 13
                                font.family: dt ? dt.fontFamily : "sans-serif"
                            }
                        }

                        MouseArea {
                            id: addVolumeHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.createVolumeRequested(root.workspaceProjectId)
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
                        background: Rectangle {
                            color: dt ? dt.surface : "#1A1D23"
                            border.color: dt ? dt.border : "#2A2E36"
                            radius: dt ? dt.radiusMd : 12
                            border.width: 1
                        }

                        MenuItem {
                            id: createVolumeMenuItem
                            text: qsTr("新建卷")
                            visible: treeContextMenu.itemType === "project"
                            contentItem: AppText {
                                text: createVolumeMenuItem.text
                                color: dt ? dt.textPrimary : "#E2E2E5"
                                font.pixelSize: dt ? dt.label : 13
                                font.family: dt ? dt.fontFamily : "sans-serif"
                                verticalAlignment: Text.AlignVCenter
                            }
                            background: Rectangle {
                                color: createVolumeMenuItem.highlighted ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"
                            }
                            onTriggered: root.createVolumeRequested(treeContextMenu.itemProjectId || root.workspaceProjectId)
                        }
                        MenuItem {
                            id: createChapterMenuItem
                            text: qsTr("新建章节")
                            visible: treeContextMenu.itemType === "volume"
                            contentItem: AppText {
                                text: createChapterMenuItem.text
                                color: dt ? dt.textPrimary : "#E2E2E5"
                                font.pixelSize: dt ? dt.label : 13
                                font.family: dt ? dt.fontFamily : "sans-serif"
                                verticalAlignment: Text.AlignVCenter
                            }
                            background: Rectangle {
                                color: createChapterMenuItem.highlighted ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"
                            }
                            onTriggered: root.createChapterRequested(treeContextMenu.itemProjectId, treeContextMenu.itemId)
                        }
                    }
                }
            }

        // Middle Area: Toolbar + Editor
        ColumnLayout {
            SplitView.fillWidth: true
            spacing: 0

            // Top toolbar
            TopWritingToolbar {
                Layout.fillWidth: true
                dt: root.dt
                backendRef: root.backendRef
                currentFontSize: root.backendRef ? root.backendRef.setting_font_size : 16
                currentLineSpacing: root.backendRef ? root.backendRef.setting_line_spacing : 1.5
                firstLineIndent: root.backendRef ? root.backendRef.setting_auto_indent_enabled : false
                saveStatus: editorController.saveStatus
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
                onFirstLineIndentToggled: {
                    if (root.backendRef) {
                        root.backendRef.setting_auto_indent_enabled = !root.backendRef.setting_auto_indent_enabled;
                    }
                }
                onFormatOneClick: editorController.formatText()
                onLinkToStarMap: { root.drawerTab = 0; root.drawerOpen = true; }
                onOpenStats: { root.drawerTab = 2; root.drawerOpen = true; }
            }

            // Editor Container Area
            Rectangle {
                Layout.fillWidth: true
                Layout.fillHeight: true
                color: dt ? dt.bg : "#111318"

                // Centered paper container
                Item {
                    anchors.fill: parent
                    anchors.leftMargin: dt ? dt.sp8 : 8
                    anchors.rightMargin: dt ? dt.sp8 : 8
                    anchors.topMargin: dt ? dt.sp8 : 8
                    anchors.bottomMargin: dt ? dt.sp8 : 8

                    // Paper background - adapts to available space up to 820px, leaving a small side gap
                    Rectangle {
                        id: paperBg
                        width: Math.min(parent.width, 820)
                        height: parent.height
                        anchors.horizontalCenter: parent.horizontalCenter
                        color: dt ? dt.editorBg : "#191C21"
                        radius: dt ? dt.radiusMd : 12
                        border.color: dt ? dt.border : "#2A2E36"
                        border.width: 1
                    }
                    
                    ScrollView {
                        id: editorScroll
                        anchors.fill: paperBg
                        anchors.margins: dt ? dt.sp20 : 20
                        clip: true
                        contentWidth: availableWidth
                        contentHeight: Math.max(editorArea.implicitHeight, editorArea.emptyContentMinimumHeight, availableHeight)
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
                            property real emptyContentMinimumHeight: Math.max(font.pixelSize * 2.4 + topPadding + bottomPadding, editorScroll.availableHeight)
                            width: editorScroll.availableWidth
                            height: Math.max(implicitHeight, emptyContentMinimumHeight)
                            color: dt ? dt.editorText : "#E2E2E5"
                            selectedTextColor: dt ? dt.selectedText : "#CCE5FF"
                            selectionColor: dt ? dt.primary : "#006497"
                            font.pixelSize: root.backendRef ? root.backendRef.setting_font_size : 16
                            font.family: "serif"
                            wrapMode: TextArea.Wrap
                            verticalAlignment: TextInput.AlignTop
                            background: Rectangle { color: "transparent" }
                            enabled: editorController.chapterId !== ""
                            focus: true
                            activeFocusOnTab: true
                            selectByMouse: true
                            persistentSelection: true
                            leftPadding: dt ? dt.sp16 : 16
                            rightPadding: dt ? dt.sp16 : 16
                            topPadding: dt ? dt.sp16 : 16
                            bottomPadding: dt ? dt.sp16 : 16
                            implicitHeight: Math.max(contentHeight + topPadding + bottomPadding, emptyContentMinimumHeight)

                            cursorVisible: false
                            cursorDelegate: Component { Item {} }
                            onCursorVisibleChanged: {
                                if (cursorVisible) {
                                    cursorVisible = false;
                                }
                            }

                            text: ""

                            Keys.onPressed: function(event) {
                                if (event.key === Qt.Key_Backspace ||
                                    event.key === Qt.Key_Delete ||
                                    (event.key === Qt.Key_X && (event.modifiers & Qt.ControlModifier))) {
                                    editorController.markPotentialExplicitClear();
                                }
                            }
                        }
                    }

                    Item {
                        id: cursorOverlay
                        anchors.fill: editorScroll
                        clip: false
                        z: editorScroll.z + 1

                        SmoothCursor {
                            targetTextArea: editorArea
                            overlayItem: cursorOverlay
                            dt: root.dt
                            smoothCursorEnabled: root.backendRef ? root.backendRef.setting_smooth_cursor_enabled : true
                            typingAnimationEnabled: root.backendRef ? root.backendRef.setting_typing_animation_enabled : true
                            cursorAnimationDuration: root.backendRef ? root.backendRef.setting_smooth_cursor_duration_ms : 80
                            typingAnimationDuration: root.backendRef ? root.backendRef.setting_typing_animation_duration_ms : 100
                        }
                    }
                }

                // Empty state
                ColumnLayout {
                    anchors.centerIn: parent
                    spacing: dt ? dt.sp12 : 12
                    visible: !editorController.chapterId

                    Rectangle {
                        width: 32; height: 32
                        radius: 16
                        color: dt ? dt.textSecondary : "#8C9198"
                        opacity: 0.1
                        Layout.alignment: Qt.AlignHCenter
                    }
                    AppText {
                        text: qsTr("选择一个章节开始写作")
                        color: dt ? dt.textSecondary : "#8C9198"
                        font.pixelSize: dt ? dt.fontLg : 16
                        Layout.alignment: Qt.AlignHCenter
                    }
                }

                // Right drawer button (when closed)
                Rectangle {
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.bottom: parent.bottom
                    width: 36
                    visible: !root.drawerOpen
                    color: "transparent"

                    ColumnLayout {
                        anchors.centerIn: parent
                        spacing: dt ? dt.sp8 : 8

                        Rectangle {
                            width: 28; height: 28
                            radius: dt ? dt.radiusPill : 999
                            color: drawerBtnHover.containsMouse ? (dt ? dt.surfaceVariant : "#DFE3EB") : "transparent"

                            AppText {
                                anchors.centerIn: parent
                                text: "\u25C0" // Left arrow to indicate it opens from the right
                                color: dt ? dt.textMuted : "#8C9198"
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
            }
        }

        // Right drawer - direct sibling in SplitView
        RightDrawer {
            id: rightDrawerRect
            SplitView.preferredWidth: 320
            SplitView.minimumWidth: 240
            SplitView.maximumWidth: 480
            visible: root.drawerOpen
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

    Connections {
        target: root.backendRef
        function onChapter_path_changed() {
            // Path changed visually, do NOT load chapter content to avoid infinite loop.
            // Just update title if needed
        }
        function onSettings_changed() {
            editorController.applyCurrentSettings();
        }
    }

    Connections {
        target: root.dt
        function onIsDarkChanged() {
            editorController.applyCurrentSettings();
        }
    }
}
