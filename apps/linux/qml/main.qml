import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import QtQuick.Window 2.15
import WriterApp 1.0

ApplicationWindow {
    id: window
    visible: true
    width: 1100
    height: 768
    minimumWidth: 680
    minimumHeight: 500
    title: "Writer"
    color: theme.bg

    // ── Design System ──────────────────────────────────────────

    QtObject {
        id: theme
        property string mode: "dark"

        property color bg:           mode === "light" ? "#f5f5f5" : "#1a1a2e"
        property color surface:      mode === "light" ? "#ffffff" : "#16213e"
        property color surfaceAlt:   mode === "light" ? "#fafafa" : "#1a1a2e"
        property color sidebarBg:    mode === "light" ? "#ffffff" : "#0f3460"
        property color sidebarAlt:   mode === "light" ? "#f0f0f0" : "#16213e"
        property color topbarBg:     mode === "light" ? "#ffffff" : "#0f3460"
        property color footerBg:     mode === "light" ? "#fafafa" : "#0f3460"
        property color editorBg:     mode === "light" ? "#ffffff" : "#1a1a2e"

        property color primary:      "#0ea5e9"
        property color primaryHover: "#38bdf8"
        property color primaryText:  "#ffffff"
        property color secondary:    mode === "light" ? "#64748b" : "#94a3b8"
        property color danger:       "#ef4444"
        property color dangerHover:  "#f87171"
        property color success:      "#22c55e"
        property color warning:      "#f59e0b"

        property color text:         mode === "light" ? "#0f172a" : "#e2e8f0"
        property color textDim:      mode === "light" ? "#64748b" : "#94a3b8"
        property color textInverse:  mode === "light" ? "#ffffff" : "#0f172a"
        property color border:       mode === "light" ? "#e2e8f0" : "#334155"
        property color borderFocus:  "#0ea5e9"
        property color divider:      mode === "light" ? "#e2e8f0" : "#1e293b"

        property color hover:        mode === "light" ? "#f1f5f9" : "#1e293b"
        property color selected:     mode === "light" ? "#e0f2fe" : "#0c4a6e"
        property color selectedText: mode === "light" ? "#0f172a" : "#e2e8f0"

        property color scrollbarBg:  mode === "light" ? "#e2e8f0" : "#334155"
        property color scrollbarFg:  mode === "light" ? "#94a3b8" : "#64748b"

        property int sp2:  2
        property int sp4:  4
        property int sp8:  8
        property int sp12: 12
        property int sp16: 16
        property int sp24: 24
        property int sp32: 32
        property int sp48: 48
        property int radiusSm: 4
        property int radiusMd: 8
        property int radiusLg: 12

        property real fontXs:  11
        property real fontSm:  12
        property real fontMd:  13
        property real fontLg:  15
        property real fontXl:  18
        property real fontXxl: 22

        function apply(modeStr) {
            if (modeStr === "light") mode = "light"
            else if (modeStr === "dark") mode = "dark"
            else mode = "dark"
        }
    }

    // ── Backend ───────────────────────────────────────────────

    AppBackend {
        id: backend
        onWorkspace_opened: reloadTree()
        onProjects_reloaded: reloadTree()
        onError_occurred: errorDialog.open()
        onClear_editor: {
            loadingChapter = true
            editorPage.clearText()
            backend.save_status = "未选择章节"
            loadingChapter = false
        }
    }

    Component.onCompleted: {
        theme.apply(backend.setting_theme_mode)
        reloadTree()
    }

    Connections {
        target: backend
        function onSettings_changed() { theme.apply(backend.setting_theme_mode) }
    }

    // ── State ─────────────────────────────────────────────────

    property bool loadingChapter: false
    property int sidebarWidth: 260
    property string navSection: "tree"

    // ── Functions ─────────────────────────────────────────────

    function saveCurrentIfNeeded() {
        if (backend.save_status === "未保存" && backend.has_selected_chapter() && backend.selected_chapter_exists()) {
            backend.save_current_chapter(editorPage.text)
        }
    }

    function reloadTree() {
        treeModel.clear()
        let items = backend.get_tree_model()
        let selId = backend.selected_item_id
        let matchIndex = -1
        for (let i = 0; i < items.length; i++) {
            let item = items[i]
            treeModel.append({
                "title": item.title, "id": item.id,
                "projectId": item.projectId || "",
                "volumeId": item.volumeId || "",
                "type": item.type
            })
            if (selId !== "" && item.id === selId) matchIndex = i
        }
        if (matchIndex !== -1) treeView.currentIndex = matchIndex
        if (treeModel.count === 0) treeView.currentIndex = -1
    }

    function isFirstSibling(index) {
        let node = treeModel.get(index)
        for (let i = 0; i < index; i++) {
            let sib = treeModel.get(i)
            if (sib.type === node.type && sib.projectId === node.projectId && sib.volumeId === node.volumeId) return false
        }
        return true
    }

    function isLastSibling(index) {
        let node = treeModel.get(index)
        for (let i = index + 1; i < treeModel.count; i++) {
            let sib = treeModel.get(i)
            if (sib.type === node.type && sib.projectId === node.projectId && sib.volumeId === node.volumeId) return false
        }
        return true
    }

    // ── Dialogs ───────────────────────────────────────────────

    Popup {
        id: inputDialog
        property string actionType: ""
        property var contextData: ({})
        x: Math.round((window.width - width) / 2)
        y: Math.round((window.height - height) / 2)
        width: 340; height: 160
        modal: true; focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: Rectangle { color: theme.surface; radius: theme.radiusMd; border.color: theme.border; border.width: 1 }

        onOpened: { inputField.text = contextData.initialText || ""; inputField.forceActiveFocus() }

        ColumnLayout {
            anchors.fill: parent; anchors.margins: theme.sp16; spacing: theme.sp12
            Label { text: "请输入名称"; font.pixelSize: theme.fontLg; font.weight: Font.DemiBold; color: theme.text }
            TextField {
                id: inputField; Layout.fillWidth: true; color: theme.text
                background: Rectangle {
                    color: theme.surfaceAlt; border.color: inputField.activeFocus ? theme.borderFocus : theme.border
                    border.width: 1; radius: theme.radiusSm
                }
                font.pixelSize: theme.fontMd; leftPadding: theme.sp8; topPadding: theme.sp8; bottomPadding: theme.sp8
            }
            RowLayout {
                Layout.fillWidth: true; Layout.alignment: Qt.AlignRight; spacing: theme.sp8
                Item { Layout.fillWidth: true }
                Button {
                    text: "取消"; flat: true; onClicked: inputDialog.close()
                    contentItem: Text { text: parent.text; color: theme.textDim; font.pixelSize: theme.fontMd; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                    background: Rectangle { color: parent.hovered ? theme.hover : "transparent"; radius: theme.radiusSm }
                    implicitWidth: 64; implicitHeight: 32
                }
                Button {
                    text: "确定"
                    onClicked: {
                        if (actionType === "create_project") backend.create_new_project(inputField.text)
                        else if (actionType === "create_volume") backend.create_new_volume(contextData.projectId, inputField.text)
                        else if (actionType === "create_chapter") backend.create_new_chapter(contextData.projectId, contextData.volumeId, inputField.text)
                        else if (actionType === "rename_project") backend.rename_project(contextData.id, inputField.text)
                        else if (actionType === "rename_volume") backend.rename_volume(contextData.projectId, contextData.id, inputField.text)
                        else if (actionType === "rename_chapter") backend.rename_chapter(contextData.projectId, contextData.volumeId, contextData.id, inputField.text)
                        inputDialog.close()
                    }
                    contentItem: Text { text: parent.text; color: theme.primaryText; font.pixelSize: theme.fontMd; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                    background: Rectangle { color: parent.hovered ? theme.primaryHover : theme.primary; radius: theme.radiusSm }
                    implicitWidth: 64; implicitHeight: 32
                }
            }
        }
    }

    Popup {
        id: errorDialog
        x: Math.round((window.width - width) / 2)
        y: Math.round((window.height - height) / 2)
        width: 420; height: 180
        modal: true; focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: Rectangle { color: theme.surface; radius: theme.radiusMd; border.color: theme.border; border.width: 1 }
        ColumnLayout {
            anchors.fill: parent; anchors.margins: theme.sp16; spacing: theme.sp12
            Label { text: "错误"; font.pixelSize: theme.fontXl; font.weight: Font.DemiBold; color: theme.danger }
            Label { text: backend.error_message; Layout.fillWidth: true; Layout.fillHeight: true; wrapMode: Text.Wrap; color: theme.text; font.pixelSize: theme.fontMd }
            Button {
                text: "确定"; Layout.alignment: Qt.AlignRight; onClicked: errorDialog.close()
                contentItem: Text { text: parent.text; color: theme.primaryText; font.pixelSize: theme.fontMd; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                background: Rectangle { color: theme.primary; radius: theme.radiusSm }
                implicitWidth: 64; implicitHeight: 32
            }
        }
    }

    Popup {
        id: confirmDialog
        property string actionType: ""
        property var contextData: ({})
        x: Math.round((window.width - width) / 2)
        y: Math.round((window.height - height) / 2)
        width: 340; height: 160
        modal: true; focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: Rectangle { color: theme.surface; radius: theme.radiusMd; border.color: theme.border; border.width: 1 }
        ColumnLayout {
            anchors.fill: parent; anchors.margins: theme.sp16; spacing: theme.sp12
            Label { text: "确认删除"; font.pixelSize: theme.fontXl; font.weight: Font.DemiBold; color: theme.danger }
            Label { text: "您确定要删除此项目吗？此操作不可撤销。"; color: theme.text; font.pixelSize: theme.fontMd; wrapMode: Text.Wrap }
            RowLayout {
                Layout.fillWidth: true; Layout.alignment: Qt.AlignRight; spacing: theme.sp8
                Item { Layout.fillWidth: true }
                Button {
                    text: "取消"; flat: true; onClicked: confirmDialog.close()
                    contentItem: Text { text: parent.text; color: theme.textDim; font.pixelSize: theme.fontMd; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                    background: Rectangle { color: parent.hovered ? theme.hover : "transparent"; radius: theme.radiusSm }
                    implicitWidth: 64; implicitHeight: 32
                }
                Button {
                    text: "删除"
                    onClicked: {
                        if (actionType === "delete_project") backend.delete_project(contextData.id)
                        else if (actionType === "delete_volume") backend.delete_volume(contextData.projectId, contextData.id)
                        else if (actionType === "delete_chapter") backend.delete_chapter(contextData.projectId, contextData.volumeId, contextData.id)
                        confirmDialog.close()
                    }
                    contentItem: Text { text: parent.text; color: "#ffffff"; font.pixelSize: theme.fontMd; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                    background: Rectangle { color: parent.hovered ? theme.dangerHover : theme.danger; radius: theme.radiusSm }
                    implicitWidth: 64; implicitHeight: 32
                }
            }
        }
    }

    SettingsDialog {
        id: settingsDialog
        backendRef: backend
        editorPageRef: editorPage
        theme: theme
    }

    // ── Models ────────────────────────────────────────────────

    ListModel { id: treeModel }

    // ══════════════════════════════════════════════════════════
    //  TOP BAR
    // ══════════════════════════════════════════════════════════

    header: Rectangle {
        height: 48
        color: theme.topbarBg

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: theme.sp16
            anchors.rightMargin: theme.sp8
            spacing: theme.sp4

            Label {
                text: "Writer"
                font.pixelSize: theme.fontXl; font.weight: Font.Bold; color: theme.primary
                Layout.preferredWidth: 80
            }

            Rectangle { width: 1; height: 24; color: theme.divider; Layout.leftMargin: 4; Layout.rightMargin: 4 }

            Label {
                text: {
                    let ws = backend.workspace_path
                    if (ws.length > 0) { let parts = ws.split("/"); return parts[parts.length - 1] }
                    return "未打开工作区"
                }
                font.pixelSize: theme.fontMd; color: theme.textDim
                Layout.fillWidth: true; elide: Text.ElideRight
            }

            Item { Layout.fillWidth: true }

            AppButton {
                text: "新建作品"; theme: theme
                onClicked: {
                    inputDialog.actionType = "create_project"
                    inputDialog.contextData = { initialText: "新作品" }
                    inputDialog.open()
                }
            }

            AppButton {
                text: "保存"; theme: theme
                enabled: backend.has_selected_chapter_prop
                onClicked: backend.save_current_chapter(editorPage.text)
            }

            ToolbarButton { text: "设置"; theme: theme; onClicked: settingsDialog.open() }
            ToolbarButton { text: "同步"; theme: theme; onClicked: { settingsDialog.switchToCategory(2); settingsDialog.open() } }
            ToolbarButton { text: "调试"; theme: theme; onClicked: { settingsDialog.switchToCategory(4); settingsDialog.open() } }

            Item { width: theme.sp8 }
        }
    }

    // ══════════════════════════════════════════════════════════
    //  CONTENT
    // ══════════════════════════════════════════════════════════

    RowLayout {
        anchors.top: window.header.bottom
        anchors.bottom: window.footer.top
        anchors.left: parent.left
        anchors.right: parent.right
        spacing: 0

        // ── Left Sidebar ────────────────────────────────────────

        Rectangle {
            id: sidebar
            Layout.preferredWidth: sidebarVisible ? sidebarWidth : 0
            Layout.maximumWidth: sidebarVisible ? sidebarWidth : 0
            Layout.minimumWidth: sidebarVisible ? sidebarWidth : 0
            color: theme.sidebarBg
            clip: true

            Behavior on Layout.preferredWidth { NumberAnimation { duration: 200; easing.type: Easing.OutCubic } }

            property bool sidebarVisible: true

            ColumnLayout {
                anchors.fill: parent
                spacing: 0

                Rectangle {
                    Layout.fillWidth: true; height: 40; color: theme.sidebarAlt
                    RowLayout {
                        anchors.fill: parent; anchors.leftMargin: theme.sp12; anchors.rightMargin: theme.sp8; spacing: theme.sp4
                        Label {
                            text: "导航"; font.pixelSize: theme.fontSm; font.weight: Font.DemiBold; color: theme.textDim
                            Layout.fillWidth: true
                        }
                        Button {
                            text: "☰"; implicitWidth: 28; implicitHeight: 28
                            onClicked: sidebarVisible = !sidebarVisible
                            contentItem: Text { text: parent.text; color: theme.textDim; font.pixelSize: theme.fontLg; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                            background: Rectangle { color: parent.hovered ? theme.hover : "transparent"; radius: theme.radiusSm }
                        }
                    }
                }

                SidebarItem {
                    text: "作品"
                    icon: "📁"
                    active: navSection === "tree"
                    theme: theme
                    onClicked: {
                        navSection = "tree"
                    }
                }
                SidebarItem {
                    text: "设置"
                    icon: "⚙"
                    active: navSection === "settings"
                    theme: theme
                    onClicked: {
                        navSection = "settings"
                        settingsDialog.open()
                    }
                }
                SidebarItem {
                    text: "同步"
                    icon: "🔄"
                    active: navSection === "sync"
                    theme: theme
                    onClicked: {
                        navSection = "sync"
                        settingsDialog.switchToCategory(2)
                        settingsDialog.open()
                    }
                }
                SidebarItem {
                    text: "调试"
                    icon: "🐛"
                    active: navSection === "debug"
                    theme: theme
                    onClicked: {
                        navSection = "debug"
                        settingsDialog.switchToCategory(4)
                        settingsDialog.open()
                    }
                }

                Rectangle { height: 1; color: theme.divider; Layout.fillWidth: true; Layout.leftMargin: theme.sp12; Layout.rightMargin: theme.sp12 }

                Rectangle {
                    Layout.fillWidth: true; Layout.fillHeight: true; color: "transparent"
                    ListView {
                        id: treeView
                        anchors.fill: parent; anchors.topMargin: theme.sp4
                        model: treeModel; clip: true
                        ScrollBar.vertical: ScrollBar { parent: treeView.parent; anchors.top: treeView.top; anchors.bottom: treeView.bottom; anchors.right: treeView.right }

                        Text {
                            anchors.centerIn: parent
                            text: "未选择作品"; color: theme.textDim; font.pixelSize: theme.fontMd
                            visible: treeModel.count === 0
                        }

                        delegate: Item {
                            width: ListView.view.width; height: 32
                            Rectangle {
                                anchors.fill: parent; anchors.leftMargin: theme.sp4; anchors.rightMargin: theme.sp4
                                radius: theme.radiusSm; color: treeView.currentIndex === index ? theme.selected : "transparent"

                                MouseArea {
                                    anchors.fill: parent
                                    onClicked: {
                                        treeView.currentIndex = index
                                        let node = treeModel.get(index)
                                        saveCurrentIfNeeded(); loadingChapter = true
                                        if (node.type === "project") {
                                            backend.select_project(node.id); editorPage.clearText(); backend.save_status = "已保存"
                                        } else if (node.type === "volume") {
                                            backend.select_volume(node.projectId, node.id); editorPage.clearText(); backend.save_status = "已保存"
                                        } else if (node.type === "chapter") {
                                            backend.select_chapter(node.projectId, node.volumeId, node.id)
                                            editorPage.loadContent(backend.get_chapter_content(node.projectId, node.volumeId, node.id))
                                            backend.save_status = "已保存"; editorPage.forceEditorFocus()
                                        }
                                        loadingChapter = false
                                    }
                                }

                                RowLayout {
                                    anchors.fill: parent
                                    anchors.leftMargin: model.type === "project" ? theme.sp8 : model.type === "volume" ? theme.sp24 : theme.sp40
                                    anchors.rightMargin: theme.sp4; spacing: theme.sp4

                                    Text {
                                        text: model.type === "project" ? "📁" : model.type === "volume" ? "📂" : "📄"
                                        font.pixelSize: theme.fontSm
                                    }
                                    Text {
                                        text: model.title
                                        color: treeView.currentIndex === index ? theme.selectedText : theme.text
                                        font.pixelSize: theme.fontMd; Layout.fillWidth: true; elide: Text.ElideRight; clip: true
                                    }
                                    Button {
                                        visible: treeView.currentIndex === index
                                        text: "⋮"; implicitWidth: 24; implicitHeight: 24
                                        onClicked: contextMenu.open()
                                        contentItem: Text { text: parent.text; color: theme.textDim; font.pixelSize: theme.fontLg; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                                        background: Rectangle { color: parent.hovered ? theme.hover : "transparent"; radius: theme.radiusSm }
                                        Menu {
                                            id: contextMenu
                                            MenuItem {
                                                text: model.type === "project" ? "新建分卷" : "新建章节"
                                                visible: model.type !== "chapter"
                                                onTriggered: {
                                                    if (model.type === "project") {
                                                        inputDialog.actionType = "create_volume"
                                                        inputDialog.contextData = { projectId: model.id, initialText: "新分卷" }
                                                        inputDialog.open()
                                                    } else if (model.type === "volume") {
                                                        inputDialog.actionType = "create_chapter"
                                                        inputDialog.contextData = { projectId: model.projectId, volumeId: model.id, initialText: "新章节" }
                                                        inputDialog.open()
                                                    }
                                                }
                                            }
                                            MenuItem { text: "重命名"; onTriggered: { inputDialog.actionType = "rename_" + model.type; inputDialog.contextData = { id: model.id, projectId: model.projectId, volumeId: model.volumeId, initialText: model.title.trim() }; inputDialog.open() } }
                                            MenuItem { text: "删除"; onTriggered: { confirmDialog.actionType = "delete_" + model.type; confirmDialog.contextData = { id: model.id, projectId: model.projectId, volumeId: model.volumeId }; confirmDialog.open() } }
                                            MenuItem {
                                                text: "上移"; visible: !isFirstSibling(index)
                                                onTriggered: {
                                                    let ids = []; let my_pos = -1
                                                    for (let i = 0; i < treeModel.count; i++) {
                                                        let n = treeModel.get(i)
                                                        if (n.type === model.type && n.projectId === model.projectId && n.volumeId === model.volumeId) {
                                                            if (n.id === model.id) my_pos = ids.length; ids.push(n.id)
                                                        }
                                                    }
                                                    if (my_pos > 0) { let t = ids[my_pos]; ids[my_pos] = ids[my_pos - 1]; ids[my_pos - 1] = t
                                                        if (model.type === "project") backend.reorder_projects(ids.join(","))
                                                        else if (model.type === "volume") backend.reorder_volumes(model.projectId, ids.join(","))
                                                        else if (model.type === "chapter") backend.reorder_chapters(model.projectId, model.volumeId, ids.join(","))
                                                    }
                                                }
                                            }
                                            MenuItem {
                                                text: "下移"; visible: !isLastSibling(index)
                                                onTriggered: {
                                                    let ids = []; let my_pos = -1
                                                    for (let i = 0; i < treeModel.count; i++) {
                                                        let n = treeModel.get(i)
                                                        if (n.type === model.type && n.projectId === model.projectId && n.volumeId === model.volumeId) {
                                                            if (n.id === model.id) my_pos = ids.length; ids.push(n.id)
                                                        }
                                                    }
                                                    if (my_pos < ids.length - 1) { let t = ids[my_pos]; ids[my_pos] = ids[my_pos + 1]; ids[my_pos + 1] = t
                                                        if (model.type === "project") backend.reorder_projects(ids.join(","))
                                                        else if (model.type === "volume") backend.reorder_volumes(model.projectId, ids.join(","))
                                                        else if (model.type === "chapter") backend.reorder_chapters(model.projectId, model.volumeId, ids.join(","))
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Sidebar resize handle
        Rectangle {
            id: sidebarHandle
            Layout.preferredWidth: 4; Layout.fillHeight: true
            color: theme.divider
            MouseArea {
                anchors.fill: parent; anchors.leftMargin: -2; anchors.rightMargin: -2; cursorShape: Qt.SizeHorCursor
                onPressed: { sidebarDrag.startX = mouseX; sidebarDrag.origWidth = sidebarWidth }
                onPositionChanged: { if (pressed) { let delta = mouseX - sidebarDrag.startX; sidebarWidth = Math.max(160, Math.min(400, sidebarDrag.origWidth + delta)) } }
                property real startX: 0; property int origWidth: 260
            }
        }

        // ── Editor Area ─────────────────────────────────────────

        EditorPage {
            id: editorPage
            Layout.fillWidth: true; Layout.fillHeight: true
            backendRef: backend; theme: theme
            onContentChanged: {
                backend.calculate_word_count(text)
                if (backend.setting_auto_save_enabled) {
                    autoSaveTimer.interval = backend.setting_auto_save_delay_ms > 0 ? backend.setting_auto_save_delay_ms : 1500
                    autoSaveTimer.restart()
                }
                if (!loadingChapter && backend.has_selected_chapter_prop && backend.save_status !== "未保存")
                    backend.save_status = "未保存"
                if (!loadingChapter && backend.has_selected_chapter_prop)
                    autoSaveTimer.restart()
            }
        }
    }

    // ══════════════════════════════════════════════════════════
    //  FOOTER
    // ══════════════════════════════════════════════════════════

    footer: Rectangle {
        height: 28; color: theme.footerBg
        RowLayout {
            anchors.fill: parent; anchors.leftMargin: theme.sp12; anchors.rightMargin: theme.sp12; spacing: theme.sp16

            StatusPill {
                theme: theme
                pillColor: backend.save_status === "已保存" || backend.save_status === "未选择章节" ? (theme ? theme.success : "#22c55e") : (theme ? theme.warning : "#f59e0b")
            }

            Label { text: backend.save_status; color: theme.textDim; font.pixelSize: theme.fontXs }
            Rectangle { width: 1; height: 14; color: theme.divider }
            Label { text: "字数: " + backend.word_count; color: theme.textDim; font.pixelSize: theme.fontXs }
            Item { Layout.fillWidth: true }
            Label { text: backend.chapter_path; color: theme.textDim; font.pixelSize: theme.fontXs; elide: Text.ElideRight; Layout.maximumWidth: 300; clip: true }
            Label { text: backend.workspace_path; color: theme.textDim; font.pixelSize: theme.fontXs; elide: Text.ElideRight; Layout.maximumWidth: 250; clip: true }
        }
    }

    // ══════════════════════════════════════════════════════════
    //  Auto-save Timer
    // ══════════════════════════════════════════════════════════

    Timer {
        id: autoSaveTimer
        interval: 1500; repeat: false
        onTriggered: { if (backend.save_status === "未保存") backend.save_current_chapter(editorPage.text) }
    }
}
