// =============================================================================
// WritingWorkspace.qml - 写作工作区
// =============================================================================
//
// 职责：Linux_qt UI 层（QML 组件）
// 边界：只负责布局和导航（编辑区 + 侧栏 + 工具栏）
// 约束：
//   - 所有业务逻辑委托 EditorController
//   - 通过 WritingTreeController 管理章节树
//   - 不包含保存、格式化等业务操作
//
// 关于 LayoutPlan 和布局策略
//
// LayoutPlan 是布局策略对象，控制：
//   - 侧栏宽度（sidebarWidth）
//   - 内容区最大宽度（contentMaxWidthVp）
//   - 外壳模式（shellMode）
//   - 内容区内边距（contentPaddingVp）
//
// LayoutPlan 不直接控制以下内容：
//   - 自研 SujianEditorItem 的 QSG 渲染
//   - 光标和选区的 IME 交互逻辑
//   - QTextLayout 的排版细节
//   - Rust Coordinator → Scene Graph 动画渲染参数
//
// 编辑器交互（包括IME处理）由 EditorController 和 SujianEditorItem
// 直接管理，不走 Qt QSG 渲染管线，不受 LayoutPlan 约束
//
// 组成：
//   WorkspaceTree (侧栏) + SujianEditorItem (编辑区) + TopWritingToolbar (工具栏)
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Sujian 1.0

Rectangle {
    id: root
    required property var dt
    property var backendRef: null
    property var starMapController: null
    // Issue #709 评论 issue-body-709: 传入 themeController 给 EditorController，
    // 使 logRenderColorProbe 能读取 ThemeController runtime state。
    property var themeController: null
    property var appState: ({})
    property var tree: []
    property string projectTitle: ""
    property bool drawerOpen: false
    property int drawerTab: 0
    property bool aiCapable: false
    property bool aiEnabled: false
    // 关于 LayoutPlan 和布局策略
    // layoutPlan 是外部注入的布局策略（由上层根据屏幕尺寸和设置决定）
    // 编辑器交互（包括IME处理）由 EditorController 和 SujianEditorItem
    // 直接管理，不走 Qt QSG 渲染管线，不受 LayoutPlan 约束
    property var layoutPlan: null

    // Project-level ID - set by main.qml, used for tree and create volume/chapter
    property string workspaceProjectId: ""

    signal backToProjects()
    signal openSettings()

    function flushActiveEditorBeforeSync() {
        if (!editorController.chapterId || !editorController.projectId || !editorController.volumeId) return true;
        return editorController.flushActiveEditorBeforeSync();
    }

    signal createVolumeRequested(string projectId)
    signal createChapterRequested(string projectId, string volumeId)
    signal renameItemRequested(var itemData)
    signal deleteItemRequested(var itemData)

    function requestEditorFocus() {
        Qt.callLater(function() {
            if (sujianEditor && sujianEditor.visible && sujianEditor.editor_enabled) {
                sujianEditor.request_text_input_focus();
            }
        });
    }

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
        targetEditorItem: sujianEditor
        backendRef: root.backendRef
        dt: root.dt
        // Issue #709 评论 issue-body-709: 传入 themeController 使
        // logRenderColorProbe 能读取 runtime state。
        themeControllerRef: root.themeController
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
        background: Rectangle { color: dt.surface; border.color: dt.border; radius: dt.radiusXl; border.width: 1 }
        header: null

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: dt.sp24
            spacing: dt.sp16
            AppText {
                dt: root.dt
                text: qsTr("保存被阻止")
                color: dt.textPrimary
                font.pointSize: dt.subtitlePt
                font.family: dt.fontFamily
                font.weight: Font.DemiBold
            }
            AppText {
                dt: root.dt
                id: emptySaveDialogText
                Layout.fillWidth: true
                text: qsTr("空内容保存被阻止，请输入内容后重试")
                color: dt.textSecondary
                font.pointSize: dt.bodyPt
                font.family: dt.fontFamily
                wrapMode: Text.Wrap
            }
            AppButton {
                            text: qsTr("确定")
                dt: root.dt
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
        root.requestEditorFocus();
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

        sujianEditor.snap_next_cursor_update();
        // loadChapterContentWithIds returns null on failure, result object on success.
        // State is only updated after content is successfully loaded.
        var result = editorController.loadChapterContentWithIds(pId, vId, cId);
        if (result) {
            var d = result.data || {};
            editorController.projectId = d.projectId || pId;
            editorController.volumeId = d.volumeId || vId;
            editorController.chapterId = d.chapterId || cId;
            editorController.chapterTitle = d.title || cTitle || "";
            sujianEditor.snap_next_cursor_update();
            // Issue #715: 切章后明确把新章节视口设为顶部。
            // 旧章节的 contentY 不应继承到新章节，否则动画层会画到错误 Y 坐标。
            // contentY -> scroll_y 绑定会同步给 Rust，确保两层使用同一滚动坐标。
            if (editorScroll.contentItem) {
                editorScroll.contentItem.contentY = 0;
            }
            root.requestEditorFocus();
        }
    }

    function reloadActiveChapter() {
        if (editorController.projectId && editorController.volumeId && editorController.chapterId) {
            sujianEditor.snap_next_cursor_update();
            editorController.loadChapterContentWithIds(
                editorController.projectId,
                editorController.volumeId,
                editorController.chapterId
            );
        }
    }

    color: dt.bg

    SplitView {
        anchors.fill: parent
        orientation: Qt.Horizontal

        handle: Rectangle {
            implicitWidth: 4
            color: SplitHandle.hovered || SplitHandle.pressed ? dt.primary : dt.border
            Behavior on color { ColorAnimation { duration: 120 } }
        }

        // Left sidebar: volume/chapter tree
        Rectangle {
            id: sidebarRect
            SplitView.preferredWidth: root.backendRef && root.backendRef.setting_linux_qt_sidebar_width > 0 ? root.backendRef.setting_linux_qt_sidebar_width : 240
            SplitView.minimumWidth: 180
            SplitView.maximumWidth: 420
            color: dt.sidebar
            border.color: dt.border
            border.width: 1

            Timer {
                id: sidebarDebounceTimer
                interval: 300
                repeat: false
                onTriggered: {
                    if (root.backendRef && sidebarRect.width > 0 && Math.abs(root.backendRef.setting_linux_qt_sidebar_width - sidebarRect.width) >= 1.0) {
                        root.backendRef.setting_linux_qt_sidebar_width = sidebarRect.width;
                        if (settingsBackend) settingsBackend.debounced_save_local_settings();
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
                            anchors.leftMargin: dt.sp12
                            anchors.rightMargin: dt.sp8
                            spacing: dt.sp8

                            Rectangle {
                                width: 28; height: 28
                                radius: dt.radiusPill
                                color: backHover.containsMouse ? dt.surfaceVariant : "transparent"

                                AppText {
                                    dt: root.dt
                                    anchors.centerIn: parent
                                    text: "\u2190"
                                    color: dt.textSecondary
                                    font.pointSize: dt.fontLgPt
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
                                dt: root.dt
                                text: root.projectTitle || qsTr("作品")
                                color: dt.textPrimary
                                font.pointSize: dt.fontMdPt
                                font.family: dt.fontFamily
                                font.weight: Font.DemiBold
                                Layout.fillWidth: true
                                elide: Text.ElideRight
                            }
                        }
                }

                // Divider
                Rectangle { Layout.fillWidth: true; height: 1; color: dt.border }

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
                                    anchors.leftMargin: dt.sp8
                                    anchors.rightMargin: dt.sp8
                                    radius: dt.radiusPill
                                    color: {
                                        if (isSelected) return dt.primaryContainer;
                                        if (delegateHover.containsMouse) return dt.surfaceVariant;
                                        return "transparent";
                                    }

                                    property bool isSelected: model.itemId === editorController.chapterId

                                    RowLayout {
                                        anchors.fill: parent
                                        anchors.leftMargin: model.itemType === "chapter" ? dt.sp32 : dt.sp12
                                        spacing: dt.sp6

                                        Rectangle {
                                            width: 6; height: 6
                                            radius: model.itemType === "volume" ? 0 : 3
                                            color: delegateBg.isSelected ? dt.selectedText : dt.textSecondary
                                            Layout.alignment: Qt.AlignVCenter
                                            opacity: 0.6
                                        }

                                        AppText {
                                            dt: root.dt
                                            text: model.itemTitle || ""
                                            color: {
                                                if (delegateBg.isSelected) return dt.onPrimaryContainer;
                                                return dt.textPrimary;
                                            }
                                            font.pointSize: dt.labelPt
                                            font.family: dt.fontFamily
                                            font.weight: delegateBg.isSelected ? Font.DemiBold : Font.Normal
                                            Layout.fillWidth: true
                                            elide: Text.ElideRight
                                        }

                                        // "⋯" menu button — visible for both volume and chapter
                                        Rectangle {
                                            z: 10
                                            width: 24; height: 24
                                            radius: 12
                                            color: menuBtnHover.containsMouse ? dt.surfaceVariant : "transparent"
                                            Layout.alignment: Qt.AlignVCenter

                                            AppText {
                                                dt: root.dt
                                                anchors.centerIn: parent
                                                text: "⋯"
                                                color: dt.textSecondary
                                                font.pointSize: dt.fontMdPt
                                            }

                                            MouseArea {
                                                id: menuBtnHover
                                                anchors.fill: parent
                                                hoverEnabled: true
                                                cursorShape: Qt.PointingHandCursor
                                                onClicked: {
                                                    treeContextMenu.itemType = model.itemType;
                                                    treeContextMenu.itemId = model.itemId;
                                                    treeContextMenu.itemTitle = model.itemTitle;
                                                    treeContextMenu.itemProjectId = model.itemProjectId || "";
                                                    treeContextMenu.itemVolumeId = model.itemVolumeId || "";
                                                    treeContextMenu.popup(menuBtnHover, 0, menuBtnHover.height);
                                                }
                                            }
                                        }
                                    }

                                    MouseArea {
                                        id: delegateHover
                                        anchors.left: parent.left
                                        anchors.top: parent.top
                                        anchors.bottom: parent.bottom
                                        anchors.right: parent.right
                                        anchors.rightMargin: 32
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
                                                treeContextMenu.popup(delegateHover, mouse.x, mouse.y);
                                            }
                                        }
                                    }

                                    // 长按弹出菜单（触屏支持）
                                    TapHandler {
                                        onLongPressed: {
                                            treeContextMenu.itemType = model.itemType;
                                            treeContextMenu.itemId = model.itemId;
                                            treeContextMenu.itemTitle = model.itemTitle;
                                            treeContextMenu.itemProjectId = model.itemProjectId || "";
                                            treeContextMenu.itemVolumeId = model.itemVolumeId || "";
                                            treeContextMenu.popup(delegateBg, point.position.x, point.position.y);
                                        }
                                    }

                                    // "+" button for volumes (create chapter)
                                    Rectangle {
                                        visible: model.itemType === "volume"
                                        width: 20; height: 20
                                        radius: 10
                                        color: addChapterHover.containsMouse ? dt.primaryContainer : "transparent"
                                        anchors {
                                            right: parent.right
                                            rightMargin: dt.sp8
                                        }
                                        anchors.verticalCenter: parent.verticalCenter

                                        AppText {
                                            dt: root.dt
                                            anchors.centerIn: parent
                                            text: "+"
                                            color: dt.primary
                                            font.pointSize: dt.fontSmPt
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
                            }
                        }
                    }

                    // "+" button for project (create volume)
                    Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 36
                        Layout.leftMargin: dt.sp8
                        Layout.rightMargin: dt.sp8
                        radius: dt.radiusPill
                        color: addVolumeHover.containsMouse ? dt.primaryContainer : "transparent"

                        RowLayout {
                            anchors.centerIn: parent
                            spacing: dt.sp4
                            AppText {
                                dt: root.dt
                                text: "+"
                                color: dt.primary
                                font.pointSize: dt.fontMdPt
                                font.weight: Font.Bold
                            }
                            AppText {
                                dt: root.dt
                                text: qsTr("新卷")
                                color: dt.primary
                                font.pointSize: dt.labelPt
                                font.family: dt.fontFamily
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
                            color: dt.surface
                            border.color: dt.border
                            radius: dt.radiusMd
                            border.width: 1
                        }

                        MenuItem {
                            id: createVolumeMenuItem
                            text: qsTr("新建卷")
                            visible: treeContextMenu.itemType === "project"
                            contentItem: AppText {
                                dt: root.dt
                                text: createVolumeMenuItem.text
                                color: dt.textPrimary
                                font.pointSize: dt.labelPt
                                font.family: dt.fontFamily
                                verticalAlignment: Text.AlignVCenter
                            }
                            background: Rectangle {
                                color: createVolumeMenuItem.highlighted ? dt.surfaceVariant : "transparent"
                            }
                            onTriggered: root.createVolumeRequested(treeContextMenu.itemProjectId || root.workspaceProjectId)
                        }
                        MenuItem {
                            id: createChapterMenuItem
                            text: qsTr("新建章节")
                            visible: treeContextMenu.itemType === "volume"
                            contentItem: AppText {
                                dt: root.dt
                                text: createChapterMenuItem.text
                                color: dt.textPrimary
                                font.pointSize: dt.labelPt
                                font.family: dt.fontFamily
                                verticalAlignment: Text.AlignVCenter
                            }
                            background: Rectangle {
                                color: createChapterMenuItem.highlighted ? dt.surfaceVariant : "transparent"
                            }
                            onTriggered: root.createChapterRequested(treeContextMenu.itemProjectId, treeContextMenu.itemId)
                        }
                        MenuSeparator {
                            visible: treeContextMenu.itemType === "project" || treeContextMenu.itemType === "volume" || treeContextMenu.itemType === "chapter"
                        }
                        MenuItem {
                            id: renameMenuItem
                            text: qsTr("重命名")
                            visible: treeContextMenu.itemType === "project" || treeContextMenu.itemType === "volume" || treeContextMenu.itemType === "chapter"
                            contentItem: AppText {
                                dt: root.dt
                                text: renameMenuItem.text
                                color: dt.textPrimary
                                font.pointSize: dt.labelPt
                                font.family: dt.fontFamily
                                verticalAlignment: Text.AlignVCenter
                            }
                            background: Rectangle {
                                color: renameMenuItem.highlighted ? dt.surfaceVariant : "transparent"
                            }
                            onTriggered: root.renameItemRequested({
                                type: treeContextMenu.itemType,
                                id: treeContextMenu.itemId,
                                projectId: treeContextMenu.itemProjectId,
                                volumeId: treeContextMenu.itemVolumeId,
                                title: treeContextMenu.itemTitle
                            })
                        }
                        MenuItem {
                            id: deleteMenuItem
                text: qsTr("删除")
                            visible: treeContextMenu.itemType === "project" || treeContextMenu.itemType === "volume" || treeContextMenu.itemType === "chapter"
                            contentItem: AppText {
                                dt: root.dt
                                text: deleteMenuItem.text
                                color: dt.error
                                font.pointSize: dt.labelPt
                                font.family: dt.fontFamily
                                verticalAlignment: Text.AlignVCenter
                            }
                            background: Rectangle {
                                color: deleteMenuItem.highlighted ? dt.surfaceVariant : "transparent"
                            }
                            onTriggered: root.deleteItemRequested({
                                type: treeContextMenu.itemType,
                                id: treeContextMenu.itemId,
                                projectId: treeContextMenu.itemProjectId,
                                volumeId: treeContextMenu.itemVolumeId,
                                title: treeContextMenu.itemTitle
                            })
                        }
                    }
                }
            }

        // Middle Area: Toolbar + Editor
        ColumnLayout {
            SplitView.fillWidth: true
            SplitView.preferredWidth: 800
            SplitView.minimumWidth: 480
            spacing: 0

            // Top toolbar
            TopWritingToolbar {
                Layout.fillWidth: true
                dt: root.dt
                backendRef: root.backendRef
                currentFontSize: settingsBackend ? settingsBackend.setting_font_size : 16
                currentLineSpacing: settingsBackend ? settingsBackend.setting_line_spacing : 1.5
                firstLineIndent: settingsBackend ? settingsBackend.setting_auto_indent_enabled : false
                saveStatus: editorController.saveStatus
                onFontSizeChanged: function(size) {
                    if (settingsBackend) {
                        settingsBackend.setting_font_size = size;
                        settingsBackend.debounced_save_local_settings();
                    }
                }
                onLineSpacingChanged: function(spacing) {
                    if (settingsBackend) {
                        settingsBackend.setting_line_spacing = spacing;
                        settingsBackend.debounced_save_local_settings();
                    }
                }
                onFirstLineIndentToggled: {
                    if (settingsBackend) {
                        settingsBackend.setting_auto_indent_enabled = !settingsBackend.setting_auto_indent_enabled;
                        settingsBackend.debounced_save_local_settings();
                    }
                }
                onFormatOneClick: editorController.formatText()
                onLinkToStarMap: { root.drawerTab = 0; root.drawerOpen = true; }
                onOpenStats: { root.drawerTab = 2; root.drawerOpen = true; }
                onOpenSettings: root.openSettings()
            }

            // Editor Container Area
            Rectangle {
                Layout.fillWidth: true
                Layout.fillHeight: true
                color: dt.bg

                // Centered paper container
                Item {
                    anchors.fill: parent
                    anchors.leftMargin: dt.sp8
                    anchors.rightMargin: dt.sp8
                    anchors.topMargin: dt.sp8
                    anchors.bottomMargin: dt.sp8

                    // Paper background - adapts to available space up to contentMaxWidthVp from LayoutPlan
                    // 关于 LayoutPlan：contentMaxWidthVp 控制 paperBg 最大宽度，
                    // 编辑区组件（SujianEditorItem）跟随 paperBg 宽度
                    // 编辑器交互由 EditorController + SujianEditorItem 直接管理
                    Rectangle {
                        id: paperBg
                        width: {
                            // 用户拖拽宽度优先；未拖过才用布局策略默认宽度
                            var planW = root.layoutPlan && root.layoutPlan.contentMaxWidthVp > 0
                                    ? root.layoutPlan.contentMaxWidthVp
                                    : 820
                            // Issue #687: 统一走 backendRef.setting_desktop_editor_width
                            var userW = root.backendRef && root.backendRef.setting_desktop_editor_width > 0
                                    ? root.backendRef.setting_desktop_editor_width
                                    : 0
                            var targetW = userW > 0 ? userW : planW
                            return Math.max(480, Math.min(parent.width, targetW))
                        }
                        height: parent.height
                        anchors.horizontalCenter: parent.horizontalCenter
                        color: dt.editorBackground
                        radius: dt.radiusMd
                        border.color: dt.border
                        border.width: 1
                    }

                    // Left drag resize handle
                    MouseArea {
                        id: leftResizeHandle
                        width: dt.sp8
                        anchors.left: paperBg.left
                        anchors.leftMargin: -(width / 2)
                        anchors.top: parent.top
                        anchors.bottom: parent.bottom
                        cursorShape: Qt.SizeHorCursor
                        hoverEnabled: true

                        Rectangle {
                            anchors.centerIn: parent
                            width: 2
                            height: parent.height
                            color: parent.containsMouse || parent.pressed ? dt.primary : "transparent"
                            opacity: parent.pressed ? 0.9 : 0.4
                            Behavior on color { ColorAnimation { duration: 120 } }
                        }

                        property real startX: 0
                        property real startWidth: 0

                        onPressed: function(mouse) {
                            startX = mouse.x;
                            startWidth = paperBg.width;
                        }

                        onPositionChanged: function(mouse) {
                            if (pressed && root.backendRef) {
                                var dx = mouse.x - startX;
                                var newWidth = Math.max(480, Math.min(parent.width - 16, startWidth - dx * 2));
                                // Issue #687: 统一走 backendRef.setting_desktop_editor_width
                                root.backendRef.setting_desktop_editor_width = newWidth;
                                if (settingsBackend) settingsBackend.debounced_save_local_settings();
                            }
                        }
                    }

                    // Right drag resize handle
                    MouseArea {
                        id: rightResizeHandle
                        width: dt.sp8
                        anchors.right: paperBg.right
                        anchors.rightMargin: -(width / 2)
                        anchors.top: parent.top
                        anchors.bottom: parent.bottom
                        cursorShape: Qt.SizeHorCursor
                        hoverEnabled: true

                        Rectangle {
                            anchors.centerIn: parent
                            width: 2
                            height: parent.height
                            color: parent.containsMouse || parent.pressed ? dt.primary : "transparent"
                            opacity: parent.pressed ? 0.9 : 0.4
                            Behavior on color { ColorAnimation { duration: 120 } }
                        }

                        property real startX: 0
                        property real startWidth: 0

                        onPressed: function(mouse) {
                            startX = mouse.x;
                            startWidth = paperBg.width;
                        }

                        onPositionChanged: function(mouse) {
                            if (pressed && root.backendRef) {
                                var dx = mouse.x - startX;
                                var newWidth = Math.max(480, Math.min(parent.width - 16, startWidth + dx * 2));
                                // Issue #687: 统一走 backendRef.setting_desktop_editor_width
                                root.backendRef.setting_desktop_editor_width = newWidth;
                                if (settingsBackend) settingsBackend.debounced_save_local_settings();
                            }
                        }
                    }

                    ScrollView {
                        id: editorScroll
                        // Issue #695 评论 5693346400: editorIsScrolling 把
                        // desktopWheelHandler.active 算进去，这样直接修改 contentY
                        // 时现有的滚动期间动画暂停逻辑仍然有效。
                        readonly property bool editorIsScrolling: ScrollBar.vertical.active || desktopWheelHandler.active || (contentItem && ((contentItem.moving !== undefined && contentItem.moving) || (contentItem.flicking !== undefined && contentItem.flicking)))
                        property bool editorAnimationSuppressed: false
                        anchors.fill: paperBg
                        anchors.margins: dt.sp20
                        clip: true
                        contentWidth: availableWidth
                        contentHeight: Math.max(sujianEditor.content_height, editorCanvas.emptyContentMinimumHeight)

                        function clampScroll() {
                            if (contentItem) {
                                var maxScroll = Math.max(0, contentHeight - height);
                                // Only clamp if contentY exceeds the valid range.
                                // Do NOT force contentY to 0 when contentHeight is still updating.
                                if (maxScroll > 0 && contentItem.contentY > maxScroll) {
                                    contentItem.contentY = maxScroll;
                                }
                            }
                        }

                        // Issue #693 评论 5689819383: 光标自动跟随滚动。
                        // 语义同 QPlainTextEdit::ensureCursorVisible()（centerOnScroll=false）：
                        // 只滚刚好够让 caret 回到可视区，不每打一字就强制居中。
                        // cursor_rect_y 是目标 caret 的 viewport 坐标（Rust 已减过 scroll_y），
                        // 直接用它做最小滚动量。contentY 改后仍通过 scroll_y 绑定回 Rust，
                        // Scene Graph 和 IME 继续使用同一滚动位置。
                        function ensureCursorVisible() {
                            const flick = contentItem
                            if (!flick)
                                return

                            const margin = Math.max(dt.sp12, sujianEditor.cursor_rect_height * 0.5)
                            const top = sujianEditor.cursor_rect_y
                            const bottom = top + sujianEditor.cursor_rect_height
                            const visibleBottom = availableHeight - margin

                            let nextY = flick.contentY
                            if (top < margin) {
                                nextY += top - margin
                            } else if (bottom > visibleBottom) {
                                nextY += bottom - visibleBottom
                            } else {
                                return
                            }

                            const maxY = Math.max(0, contentHeight - height)
                            flick.contentY = Math.max(0, Math.min(maxY, nextY))
                        }

                        function scheduleEnsureCursorVisible() {
                            Qt.callLater(ensureCursorVisible)
                        }

                        onContentHeightChanged: clampScroll()
                        onHeightChanged: clampScroll()
                        Component.onCompleted: {
                            // Issue #695: Qt 6.9+ 建议桌面 Flickable 设 acceptedButtons: Qt.NoButton，
                            // 鼠标按住拖动不当触屏甩动；触屏 flick 不受 mouse-button 限制。
                            // 让 Qt 原生处理 wheel/scrollbar，ScrollView/Flickable 继续持有 contentY。
                            if (contentItem) {
                                contentItem.acceptedButtons = Qt.NoButton;
                            }
                        }
                        onEditorIsScrollingChanged: {
                            if (editorIsScrolling) {
                                scrollAnimationReleaseTimer.stop();
                                editorAnimationSuppressed = true;
                            } else {
                                scrollAnimationReleaseTimer.restart();
                            }
                        }

                        Timer {
                            id: scrollAnimationReleaseTimer
                            interval: 80
                            repeat: false
                            onTriggered: editorScroll.editorAnimationSuppressed = false
                        }

                        ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                        ScrollBar.vertical: ScrollBar {
                            policy: ScrollBar.AsNeeded
                            parent: editorScroll
                            anchors.top: editorScroll.top
                            anchors.bottom: editorScroll.bottom
                            anchors.right: editorScroll.right
                        }

                        Item {
                            id: editorCanvas
                            readonly property real emptyContentMinimumHeight: Math.max((settingsBackend ? settingsBackend.setting_font_size : 16) * 2.4 + dt.sp16 * 2, editorScroll.availableHeight)
                            width: editorScroll.availableWidth
                            height: editorScroll.availableHeight
                            implicitHeight: Math.max(sujianEditor.content_height, emptyContentMinimumHeight)

                            // NOTE: SujianEditorItem is a "viewport renderer" - it must be
                            // a FIXED overlay on paperBg, NOT inside the Flickable contentItem.
                            // The Flickable only holds a transparent spacer for scrollbar / contentHeight.
                            // scroll_y is passed to the Rust renderer for viewport clipping.
                        }
                    }

                    // SujianEditorItem: viewport renderer - fixed overlay on paperBg,
                    // NOT inside Flickable. scroll_y passes contentY to Rust renderer
                    // for viewport clipping. Flickable only holds a transparent spacer
                    // for scrollbar / contentHeight.
                    //
                    // 关于渲染：LayoutPlan 只控制布局策略（如最大宽度和padding），
                    // 不控制 SujianEditorItem 的渲染细节，渲染由 QML layout 自动处理
                    //
                    // 具体分工：
                    //   - LayoutPlan 策略（contentMaxWidthVp, shellMode, contentPaddingVp）
                    //     通过 paperBg 宽度和 SujianEditorItem 的 Q_PROPERTY 传递
                    //   - 编辑器参数（font_pixel_size, line_spacing, text_indent,
                    //     cursor_color, scroll_y 等）由 settingsBackend 和 EditorController
                    //     直接管理，不走 LayoutPlan
                    //   - 编辑区尺寸跟随 paperBg 宽度和 QML layout 自动调整
                    //     不需要 SujianEditorItem 的 geometry_changed 依赖 LayoutPlan
                    //   - updatePaintNode / QSG 渲染完全由 Rust 侧管理
                    // Issue #677 评论 5653315696 约束:
                    //   - 写作区只拿 DesignTokens 已经算好的最终颜色（editorText、
                    //     textPrimaryHex、primary、selectedText 等），不在写作区里
                    //     解释主题 JSON。主题 JSON 的 snake_case key 解析只在
                    //     DesignTokens.qml 里完成。
                    //   - text_color 最终来自 dt.editorText → dt.textPrimary →
                    //     on_surface（Core DTO snake_case 字段）。
                    //   - QML 的 loading/scrolling 状态（is_loading、is_scrolling）
                    //     只用于动画抑制等，不能决定 Scene Graph 根节点和正文层是否
                    //     存在。visible: true 固定不受 loading/scrolling 影响；
                    //     update_paint_node 通过 ensure_editor_root 在第一帧创建根
                    //     节点，不再因 root 为空整帧跳过。
                    SujianEditorItem {
                        id: sujianEditor
                        // Issue #693 评论 5689819383: SujianEditorItem 是 ScrollView
                        // 外的固定 overlay，editorScroll.clip 裁不到这个 sibling。
                        // 自定义 Scene Graph 默认不裁剪，会画到 item 边界外盖住顶栏。
                        // 打开 clip 把自身绘制和子节点限制在 bounding rect 内。
                        // 参考 https://doc.qt.io/qt-6/qquickitem.html#clip-prop
                        clip: true
                        x: editorScroll.x
                        y: editorScroll.y
                        width: editorScroll.availableWidth
                        height: editorScroll.availableHeight
                        visible: true
                        focus: true
                        editor_enabled: editorController.chapterId !== ""
                        font_pixel_size: settingsBackend ? settingsBackend.setting_font_size : (root.backendRef ? root.backendRef.setting_font_size : 16)
                        font_family: "serif"
                        line_spacing: settingsBackend ? settingsBackend.setting_line_spacing : 1.5
                        text_indent: (settingsBackend && settingsBackend.setting_auto_indent_enabled)
                            ? Math.max(0, (settingsBackend.setting_font_size || 16) * settingsBackend.setting_auto_indent_width)
                            : 0
                        padding: dt.sp16
                        // Issue #710 评论 5731145076: text_color 只绑定 dt.editorText，
                        // 去掉 fallback 到 dt.textPrimaryHex 的第二套逻辑。
                        // editorText = textPrimary = onSurface（DesignTokens 单一事实源），
                        // 主题变化只触发正文节点颜色重建，不重新创建另一份编辑器主题状态。
                        // Issue #714: 颜色绑定改为 dt.xxx.toString()，
                        // 不再经过 editorController.colorToHex() 转换。
                        // DesignTokens 的 color 属性现在直接从 themeController.*_hex
                        // (QString) 绑定，toString() 得到 "#RRGGBB" 字符串，
                        // 形成 QString → QML color 单一链。
                        text_color: dt.editorText.toString()
                        selection_color: dt.primary.toString()
                        selected_text_color: dt.selectedText.toString()
                        cursor_color: dt.primary.toString()
                        smooth_cursor_enabled: settingsBackend ? settingsBackend.setting_smooth_cursor_enabled : true
                        cursor_animation_duration_ms: settingsBackend ? settingsBackend.setting_smooth_cursor_duration_ms : 80
                        typing_animation_enabled: settingsBackend ? settingsBackend.setting_typing_animation_enabled : true
                        typing_animation_duration_ms: settingsBackend ? settingsBackend.setting_typing_animation_duration_ms : 100
                        coordinated_text_cursor_animation_enabled: settingsBackend ? settingsBackend.setting_coordinated_text_cursor_animation_enabled : true
                        scroll_y: editorScroll.contentItem ? editorScroll.contentItem.contentY : 0
                        viewport_height: sujianEditor.height
                        is_scrolling: editorScroll.editorAnimationSuppressed
                        is_loading: editorController.isLoadingChapter
                        is_applying_format: editorController.isApplyingFormat
                        // Issue #721: 不再把保存 guard (settingsSaveGuardActive) 传成
                        // 编辑器视觉抑制状态。主题切换不再触发 applyCurrentSettings()，
                        // 正文/选区/光标颜色已通过 text_color/selection_color/
                        // selected_text_color/cursor_color 绑定自动下发，Rust 侧各自
                        // 走 color_only_changed() -> request_scene_rebuild()，不会清空动画。

                        // Issue #693 评论 5689819383: 光标自动跟随滚动。
                        // 只在真正的编辑/selection 变化时调度，不监听 scroll_y 或
                        // 每次 cursor_rect_changed，避免用户手动滚离光标被立刻拽回。
                        // Rust 侧先发信号再 update_cursor_visual_position()，故用
                        // Qt.callLater() 等本轮 caret target 算完再读 cursor_rect_y。
                        // 同一轮即使排了两次 callLater，第二次看到 caret 已可见会直接返回。
                        onCursor_position_changed: editorScroll.scheduleEnsureCursorVisible()
                        onText_changed: editorScroll.scheduleEnsureCursorVisible()

                        onWidthChanged: {
                            Qt.callLater(sujianEditor.flush_content_height)
                        }

                        Component.onCompleted: {
                            sujianEditor.verify_animation_signal_meta_object()
                        }

                        onExplicit_clear_requested: editorController.markPotentialExplicitClear()

                        onContext_menu_requested: function(cx, cy) {
                            // 将局部坐标映射为全局坐标后弹出菜单
                            var globalPos = sujianEditor.mapToGlobal(cx, cy)
                            editorContextMenu.popup(globalPos.x, globalPos.y)
                        }

                        TapHandler {
                            acceptedButtons: Qt.RightButton
                            onTapped: function(eventPoint) {
                                sujianEditor.click_at(eventPoint.position.x, eventPoint.position.y, false)
                                editorContextMenu.popup()
                            }
                        }

                        TapHandler {
                            acceptedButtons: Qt.LeftButton
                            // Issue #714: 限制长按只对触屏/手写笔生效，不对桌面鼠标生效
                            // 桌面鼠标的长按等同于右键菜单，不需要触发 long_press_at
                            acceptedDevices: PointerDevice.TouchScreen | PointerDevice.Stylus
                            onLongPressed: {
                                sujianEditor.long_press_at(point.position.x, point.position.y)
                            }
                        }

                        // Cursor is now rendered in SujianEditorItem Scene Graph (child[3])
                    }

                    // 编辑器上下文菜单
                    EditorContextMenu {
                        id: editorContextMenu
                        editorItem: sujianEditor
                        dt: root.dt
                    }

                    // Issue #690 评论 5675007226 步骤 4: 光标闪烁改用低频 Timer，
                    // 不再用 FrameAnimation 每帧回调。正文位置动画已由 RenderPlan 同帧计算，
                    // 不再需要 QML 每帧推进。空闲闪烁只需要低频定时触发 blink，
                    // 并且每次切换显式 request_frame_update()。
                    Timer {
                        id: cursorBlinkTimer
                        interval: 265
                        repeat: true
                        running: sujianEditor.editor_enabled
                                 && sujianEditor.focus
                        onTriggered: {
                            sujianEditor.tick_cursor_animation()
                        }
                    }

                    // Animation overlay removed — text animation is now handled in
                    // SujianEditorItem Scene Graph (child[1]) via ActiveVisualTransactionQueue

                    // 文字动画唯一主路径：Rust Coordinator → Scene Graph (child[1])
                    // 不再使用 QML overlay 路线

                    // Issue #695 评论 5693346400: 桌面滚轮事件直译器
                    // 用 WheelHandler 拦截 wheel 事件并直接修改 contentY，阻止
                    // Qt 6.10 QQuickFlickable::wheelEvent() 自带的 wheel acceleration。
                    // 放在 editorScroll/sujianEditor 之后声明（z 更高），wheel 事件
                    // 先到本组件；不拦截鼠标点击/拖拽（Item 默认不处理鼠标事件）。
                    DesktopWheelScrollHandler {
                        id: desktopWheelHandler
                        targetFlickable: editorScroll.contentItem
                        // 每格滚动距离 = wheelScrollLines × 当前行高
                        // 行高 = 字体大小 × 行距倍数
                        lineSpacingPx: (settingsBackend ? settingsBackend.setting_font_size : 16) * (settingsBackend ? settingsBackend.setting_line_spacing : 1.5)
                        anchors.fill: editorScroll
                    }

                }

                // Empty state
                ColumnLayout {
                    anchors.centerIn: parent
                    spacing: dt.sp12
                    visible: !editorController.chapterId

                    Rectangle {
                        width: 32; height: 32
                        radius: 16
                        color: dt.textSecondary
                        opacity: 0.1
                        Layout.alignment: Qt.AlignHCenter
                    }
                    AppText {
                        dt: root.dt
                        text: qsTr("请选择或新建章节")
                        color: dt.textSecondary
                        font.pointSize: dt.fontLgPt
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
                        spacing: dt.sp8

                        Rectangle {
                            width: 28; height: 28
                            radius: dt.radiusPill
                            color: drawerBtnHover.containsMouse ? dt.surfaceVariant : "transparent"

                            AppText {
                                dt: root.dt
                                anchors.centerIn: parent
                                text: "\u25C0" // Left arrow to indicate it opens from the right
                                color: dt.textMuted
                                font.pointSize: dt.fontXsPt
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
            starMapController: root.starMapController
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
    }

    Connections {
        target: settingsBackend
        function onSettings_changed() {
            editorController.applyCurrentSettings();
        }
    }

    // Issue #721: 删除 onIsDarkChanged -> applyCurrentSettings() 连接。
    // 主题切换不需要进入 applyCurrentSettings()——正文/选区/光标颜色已通过
    // text_color/selection_color/selected_text_color/cursor_color 绑定自动下发，
    // Rust 侧各自走 color_only_changed() -> request_scene_rebuild()，不会清空动画。
    // 上面 settingsBackend.onSettings_changed 仍保留，它只保护保存，不控制编辑器动画。

    Connections {
        target: editorController
        function onChapterIdChanged() {
            if (editorController.chapterId) {
                root.requestEditorFocus();
                // 保存导航状态（包含当前章节信息）
                if (workspaceBackend) {
                    workspaceBackend.save_last_navigation_state(
                        "writing",
                        root.workspaceProjectId || "",
                        editorController.volumeId || "",
                        editorController.chapterId || "",
                        ""
                    );
                }
            }
        }
    }

}
