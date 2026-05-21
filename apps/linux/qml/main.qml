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
    color: tokens.bg

    // ── Design Tokens (semantic colors) ──────────────────────
    // Contrast targets (WCAG 2.1 AA):
    //   Normal text >= 4.5:1, Large text >= 3:1
    // Light mode verification (on #ffffff surface):
    //   textPrimary   #0f172a -> ~15.4:1  PASS
    //   textSecondary #475569 ->  ~6.4:1  PASS
    //   textDisabled  #94a3b8 ->  ~2.8:1  OK (disabled only)
    //   textDim       #475569 ->  ~6.4:1  PASS (same as secondary)
    // Dark mode verification (on #0f172a bg / #1e293b surface):
    //   textPrimary   #e2e8f0 -> ~12.3:1  PASS
    //   textSecondary #94a3b8 ->  ~7.1:1  PASS
    //   textDisabled  #475569 ->  ~2.6:1  OK (disabled only)

    QtObject {
        id: tokens
        property string mode: "light"

        // Backgrounds
        property color bg:           mode === "light" ? "#f8fafc" : "#0f172a"
        property color surface:      mode === "light" ? "#ffffff" : "#1e293b"
        property color surfaceAlt:   mode === "light" ? "#f1f5f9" : "#162032"
        property color sidebarBg:    mode === "light" ? "#ffffff" : "#1e293b"
        property color sidebarAlt:   mode === "light" ? "#f1f5f9" : "#162032"
        property color topbarBg:     mode === "light" ? "#ffffff" : "#1e293b"
        property color footerBg:     mode === "light" ? "#f1f5f9" : "#162032"
        property color editorBg:     mode === "light" ? "#ffffff" : "#0f172a"
        property color overlay:      mode === "light" ? "#00000033" : "#00000066"

        // Semantic
        property color primary:      "#3b82f6"
        property color primaryHover: "#60a5fa"
        property color primaryText:  "#ffffff"
        property color secondary:    mode === "light" ? "#475569" : "#94a3b8"
        property color danger:       "#ef4444"
        property color dangerHover:  "#f87171"
        property color success:      "#22c55e"
        property color warning:      "#f59e0b"

        // Text (WCAG AA compliant)
        property color textPrimary:   mode === "light" ? "#0f172a" : "#e2e8f0"
        property color textSecondary: mode === "light" ? "#475569" : "#94a3b8"
        property color textDisabled:  mode === "light" ? "#94a3b8" : "#475569"
        // Legacy aliases (point to proper tokens)
        property color text:         mode === "light" ? "#0f172a" : "#e2e8f0"
        property color textDim:      mode === "light" ? "#475569" : "#94a3b8"
        property color textInverse:  mode === "light" ? "#ffffff" : "#0f172a"
        property color border:       mode === "light" ? "#e2e8f0" : "#334155"
        property color borderFocus:  "#3b82f6"
        property color divider:      mode === "light" ? "#e2e8f0" : "#1e293b"

        // Interaction
        property color hover:        mode === "light" ? "#f1f5f9" : "#1e293b"
        property color selected:     mode === "light" ? "#dbeafe" : "#1e3a5f"
        property color selectedText: mode === "light" ? "#0f172a" : "#e2e8f0"

        // Scrollbar
        property color scrollbarBg:  mode === "light" ? "#e2e8f0" : "#334155"
        property color scrollbarFg:  mode === "light" ? "#94a3b8" : "#64748b"

        // Spacing
        property int sp2:  2
        property int sp4:  4
        property int sp6:  6
        property int sp8:  8
        property int sp12: 12
        property int sp16: 16
        property int sp24: 24
        property int sp32: 32
        property int sp48: 48
        property int radiusSm: 6
        property int radiusMd: 8
        property int radiusLg: 12

        // Font sizes
        property real fontXs:  11
        property real fontSm:  12
        property real fontMd:  13
        property real fontLg:  15
        property real fontXl:  18
        property real fontXxl: 22

        function apply(modeStr) {
            if (modeStr === "light") mode = "light"
            else if (modeStr === "dark") mode = "dark"
            else applySystemTheme()
        }

        function applySystemTheme() {
            var sh = Qt.styleHints
            if (sh !== undefined && sh.colorScheme !== undefined) {
                try {
                    if (sh.colorScheme === Qt.styleHints.ColorScheme.Dark) {
                        mode = "dark"
                        return
                    }
                } catch(e) {}
            }
            var sysColorScheme = backend ? backend.system_color_scheme : ""
            if (sysColorScheme === "dark") {
                mode = "dark"
            } else if (sysColorScheme === "light") {
                mode = "light"
            } else {
                mode = "light"
            }
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
        backend.try_restore_last_workspace()
        backend.query_system_color_scheme()
        // Apply system theme FIRST before checking workspace settings
        tokens.applySystemTheme()
        effectiveTheme = tokens.mode
        // Then apply workspace theme mode if available
        var themeMode = backend.setting_theme_mode
        if (themeMode === "system") {
            tokens.applySystemTheme()
        } else if (themeMode === "light" || themeMode === "dark") {
            tokens.apply(themeMode)
        }
        effectiveTheme = tokens.mode
        reloadTree()
    }

    Connections {
        target: backend
        function onSettings_changed() {
            var mode = backend.setting_theme_mode
            if (mode === "system") {
                tokens.applySystemTheme()
            } else {
                tokens.apply(mode)
            }
            effectiveTheme = tokens.mode
        }
        function onSystem_color_scheme_changed() {
            if (backend.setting_theme_mode === "system") {
                tokens.applySystemTheme()
                effectiveTheme = tokens.mode
            }
        }
    }

    onEffectiveThemeChanged: {
        // Ensure token mode stays in sync
        tokens.mode = effectiveTheme
    }

    // ── State ─────────────────────────────────────────────────

    property bool loadingChapter: false
    property int sidebarWidth: 260
    property string navSection: "tree"
    property string effectiveTheme: tokens.mode

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
        property string dialogErrorMessage: ""
        property string dialogDiagnosticsJson: ""
        property bool dialogProcessing: false
        x: Math.round((window.width - width) / 2)
        y: Math.round((window.height - height) / 2)
        width: 420
        implicitHeight: 280
        modal: true; focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: Rectangle { color: tokens.surface; radius: tokens.radiusMd; border.color: tokens.border; border.width: 1 }

        onOpened: {
            inputField.text = inputDialog.contextData.initialText || ""
            inputDialog.dialogErrorMessage = ""
            inputDialog.dialogDiagnosticsJson = ""
            inputDialog.dialogProcessing = false
            inputField.forceActiveFocus()
        }

        ColumnLayout {
            id: contentCol
            anchors.fill: parent; anchors.margins: tokens.sp16; spacing: tokens.sp12
            Label { text: "请输入名称"; font.pixelSize: tokens.fontLg; font.weight: Font.DemiBold; color: tokens.textPrimary }
            TextField {
                id: inputField; Layout.fillWidth: true; color: tokens.textPrimary
                background: Rectangle {
                    color: tokens.surfaceAlt; border.color: inputField.activeFocus ? tokens.borderFocus : tokens.border
                    border.width: 1; radius: tokens.radiusSm
                }
                font.pixelSize: tokens.fontMd; leftPadding: tokens.sp8; topPadding: tokens.sp8; bottomPadding: tokens.sp8
                onAccepted: confirmButton.clicked()
            }
            Label {
                text: inputDialog.dialogErrorMessage
                visible: inputDialog.dialogErrorMessage.length > 0
                color: tokens.danger
                font.pixelSize: tokens.fontSm
                wrapMode: Text.WordWrap
                Layout.fillWidth: true
            }
            RowLayout {
                Layout.fillWidth: true; Layout.alignment: Qt.AlignRight; spacing: tokens.sp8
                Item { Layout.fillWidth: true }
                Button {
                    text: "复制诊断"
                    visible: inputDialog.dialogDiagnosticsJson.length > 0
                    flat: true
                    onClicked: {
                        var result = JSON.parse(backend.copy_text_to_clipboard(inputDialog.dialogDiagnosticsJson))
                        if (!result.success) {
                            inputDialog.dialogErrorMessage = result.message
                        } else {
                            backend.save_status = result.message
                        }
                    }
                    contentItem: Text { text: parent.text; color: tokens.primary; font.pixelSize: tokens.fontSm; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                    background: Rectangle { color: parent.hovered ? tokens.hover : "transparent"; radius: tokens.radiusSm }
                    implicitWidth: 64; implicitHeight: 28
                }
                Button {
                    text: "取消"; flat: true; enabled: !inputDialog.dialogProcessing
                    onClicked: inputDialog.close()
                    contentItem: Text { text: parent.text; color: tokens.textSecondary; font.pixelSize: tokens.fontMd; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                    background: Rectangle { color: parent.hovered ? tokens.hover : "transparent"; radius: tokens.radiusSm }
                    implicitWidth: 64; implicitHeight: 32
                }
                Button {
                    id: confirmButton
                    text: inputDialog.dialogProcessing ? "处理中..." : "确定"
                    enabled: !inputDialog.dialogProcessing && inputField.text.trim().length > 0
                    onClicked: {
                        if (inputDialog.actionType === "create_volume") {
                            backend.create_new_volume(inputDialog.contextData.projectId, inputField.text.trim())
                            inputDialog.close()
                        } else if (inputDialog.actionType === "create_chapter") {
                            backend.create_new_chapter(inputDialog.contextData.projectId, inputDialog.contextData.volumeId, inputField.text.trim())
                            inputDialog.close()
                        } else if (inputDialog.actionType === "rename_project") {
                            backend.rename_project(inputDialog.contextData.id, inputField.text.trim())
                            inputDialog.close()
                        } else if (inputDialog.actionType === "rename_volume") {
                            backend.rename_volume(inputDialog.contextData.projectId, inputDialog.contextData.id, inputField.text.trim())
                            inputDialog.close()
                        } else if (inputDialog.actionType === "rename_chapter") {
                            backend.rename_chapter(inputDialog.contextData.projectId, inputDialog.contextData.volumeId, inputDialog.contextData.id, inputField.text.trim())
                            inputDialog.close()
                        }
                    }
                    contentItem: Text { text: parent.text; color: tokens.primaryText; font.pixelSize: tokens.fontMd; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                    background: Rectangle { color: parent.hovered ? tokens.primaryHover : tokens.primary; radius: tokens.radiusSm }
                    implicitWidth: 64; implicitHeight: 32
                }
            }
        }
    }
    // ── Create Project Dialog (dedicated) ─────────────────────

    Popup {
        id: createProjectDialog
        property string projectTitle: ""
        property string errorMessage: ""
        property string diagnosticsJson: ""
        property bool processing: false
        x: Math.round((window.width - width) / 2)
        y: Math.round((window.height - height) / 2)
        width: 420
        implicitHeight: 320
        modal: true; focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: Rectangle { color: tokens.surface; radius: tokens.radiusMd; border.color: tokens.border; border.width: 1 }

        onOpened: {
            projectTitle = ""
            errorMessage = ""
            diagnosticsJson = ""
            processing = false
            projectTitleInput.forceActiveFocus()
        }

        ColumnLayout {
            anchors.fill: parent; anchors.margins: tokens.sp16; spacing: tokens.sp12

            Label { text: "新建作品"; font.pixelSize: tokens.fontLg; font.weight: Font.DemiBold; color: tokens.textPrimary }

            TextField {
                id: projectTitleInput
                Layout.fillWidth: true
                placeholderText: "输入作品名称"
                color: tokens.textPrimary
                background: Rectangle {
                    color: tokens.surfaceAlt
                    border.color: projectTitleInput.activeFocus ? tokens.borderFocus : tokens.border
                    border.width: 1; radius: tokens.radiusSm
                }
                font.pixelSize: tokens.fontMd
                leftPadding: tokens.sp8; topPadding: tokens.sp8; bottomPadding: tokens.sp8
                onAccepted: {
                    if (!processing && projectTitleInput.text.trim().length > 0)
                        createProjectBtn.clicked()
                }
            }

            Label {
                text: createProjectDialog.errorMessage
                visible: createProjectDialog.errorMessage.length > 0
                color: tokens.danger
                font.pixelSize: tokens.fontSm
                wrapMode: Text.WordWrap
                Layout.fillWidth: true
            }

            RowLayout {
                Layout.fillWidth: true; Layout.alignment: Qt.AlignRight; spacing: tokens.sp8
                Item { Layout.fillWidth: true }
                Button {
                    text: "复制诊断"
                    visible: createProjectDialog.diagnosticsJson.length > 0
                    flat: true
                    onClicked: {
                        var result = JSON.parse(backend.copy_text_to_clipboard(createProjectDialog.diagnosticsJson))
                        if (result.success) {
                            backend.save_status = result.message
                        } else {
                            createProjectDialog.errorMessage = result.message
                        }
                    }
                    contentItem: Text { text: parent.text; color: tokens.primary; font.pixelSize: tokens.fontSm; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                    background: Rectangle { color: parent.hovered ? tokens.hover : "transparent"; radius: tokens.radiusSm }
                    implicitWidth: 64; implicitHeight: 28
                }
                Button {
                    text: "取消"; flat: true; enabled: !createProjectDialog.processing
                    onClicked: createProjectDialog.close()
                    contentItem: Text { text: parent.text; color: tokens.textSecondary; font.pixelSize: tokens.fontMd; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                    background: Rectangle { color: parent.hovered ? tokens.hover : "transparent"; radius: tokens.radiusSm }
                    implicitWidth: 64; implicitHeight: 32
                }
                Button {
                    id: createProjectBtn
                    text: createProjectDialog.processing ? "处理中..." : "确定"
                    enabled: !createProjectDialog.processing && projectTitleInput.text.trim().length > 0
                    onClicked: {
                        createProjectDialog.processing = true
                        var result = backend.create_new_project(projectTitleInput.text.trim())
                        try {
                            var r = JSON.parse(result)
                            if (r.success) {
                                createProjectDialog.errorMessage = ""
                                createProjectDialog.diagnosticsJson = ""
                                backend.save_status = r.message
                                createProjectDialog.close()
                                reloadTree()
                            } else {
                                createProjectDialog.errorMessage = r.message || "创建失败"
                                createProjectDialog.diagnosticsJson = JSON.stringify(r, null, 2)
                                createProjectDialog.processing = false
                            }
                        } catch(e) {
                            createProjectDialog.errorMessage = "解析返回结果失败: " + e
                            createProjectDialog.processing = false
                        }
                    }
                    contentItem: Text { text: parent.text; color: tokens.primaryText; font.pixelSize: tokens.fontMd; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                    background: Rectangle { color: parent.hovered ? tokens.primaryHover : tokens.primary; radius: tokens.radiusSm }
                    implicitWidth: 64; implicitHeight: 32
                }
            }
        }
    }

    Popup {
        id: errorDialog
        x: Math.round((window.width - width) / 2)
        y: Math.round((window.height - height) / 2)
        width: 420; height: 200
        modal: true; focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: Rectangle { color: tokens.surface; radius: tokens.radiusMd; border.color: tokens.border; border.width: 1 }
        ColumnLayout {
            anchors.fill: parent; anchors.margins: tokens.sp16; spacing: tokens.sp12
            Label { text: "错误"; font.pixelSize: tokens.fontXl; font.weight: Font.DemiBold; color: tokens.danger }
            Label { text: backend.error_message; Layout.fillWidth: true; Layout.fillHeight: true; wrapMode: Text.Wrap; color: tokens.textPrimary; font.pixelSize: tokens.fontMd }
            Button {
                text: "确定"; Layout.alignment: Qt.AlignRight; onClicked: errorDialog.close()
                contentItem: Text { text: parent.text; color: tokens.primaryText; font.pixelSize: tokens.fontMd; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                background: Rectangle { color: tokens.primary; radius: tokens.radiusSm }
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
        width: 340; height: 180
        modal: true; focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: Rectangle { color: tokens.surface; radius: tokens.radiusMd; border.color: tokens.border; border.width: 1 }
        ColumnLayout {
            anchors.fill: parent; anchors.margins: tokens.sp16; spacing: tokens.sp12
            Label { text: "确认删除"; font.pixelSize: tokens.fontXl; font.weight: Font.DemiBold; color: tokens.danger }
            Label { text: "您确定要删除此项目吗？此操作不可撤销。"; color: tokens.textPrimary; font.pixelSize: tokens.fontMd; wrapMode: Text.Wrap }
            RowLayout {
                Layout.fillWidth: true; Layout.alignment: Qt.AlignRight; spacing: tokens.sp8
                Item { Layout.fillWidth: true }
                Button {
                    text: "取消"; flat: true; onClicked: confirmDialog.close()
                    contentItem: Text { text: parent.text; color: tokens.textSecondary; font.pixelSize: tokens.fontMd; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                    background: Rectangle { color: parent.hovered ? tokens.hover : "transparent"; radius: tokens.radiusSm }
                    implicitWidth: 64; implicitHeight: 32
                }
                Button {
                    text: "删除"
                    onClicked: {
                        if (confirmDialog.actionType === "delete_project") backend.delete_project(confirmDialog.contextData.id)
                        else if (confirmDialog.actionType === "delete_volume") backend.delete_volume(confirmDialog.contextData.projectId, confirmDialog.contextData.id)
                        else if (confirmDialog.actionType === "delete_chapter") backend.delete_chapter(confirmDialog.contextData.projectId, confirmDialog.contextData.volumeId, confirmDialog.contextData.id)
                        confirmDialog.close()
                    }
                    contentItem: Text { text: parent.text; color: "#ffffff"; font.pixelSize: tokens.fontMd; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                    background: Rectangle { color: parent.hovered ? tokens.dangerHover : tokens.danger; radius: tokens.radiusSm }
                    implicitWidth: 64; implicitHeight: 32
                }
            }
        }
    }

    SettingsDialog {
        id: settingsDialog
        backendRef: backend
        editorPageRef: editorPage
        appTheme: tokens
    }

    // ── Models ────────────────────────────────────────────────

    ListModel { id: treeModel }

    // ══════════════════════════════════════════════════════════
    //  TOP BAR
    // ══════════════════════════════════════════════════════════

    header: Rectangle {
        height: 48
        color: tokens.topbarBg

        Rectangle {
            anchors.left: parent.left; anchors.right: parent.right; anchors.bottom: parent.bottom
            height: 1; color: tokens.border
        }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: tokens.sp16
            anchors.rightMargin: tokens.sp8
            spacing: tokens.sp4

            Label {
                text: "Writer"
                font.pixelSize: tokens.fontXl; font.weight: Font.Bold; color: tokens.primary
                Layout.preferredWidth: 80
            }

            Rectangle { width: 1; height: 24; color: tokens.divider; Layout.leftMargin: 4; Layout.rightMargin: 4 }

            Label {
                text: {
                    let ws = backend.workspace_path
                    if (ws.length > 0) { let parts = ws.split("/"); return parts[parts.length - 1] }
                    return "未打开工作区"
                }
                font.pixelSize: tokens.fontMd; color: tokens.textSecondary
                Layout.fillWidth: true; elide: Text.ElideRight
            }

            Item { Layout.fillWidth: true }

            AppButton {
                text: "新建作品"; theme: tokens
                visible: backend.has_workspace
                onClicked: {
                    createProjectDialog.open()
                }
            }

            AppButton {
                text: "保存"; theme: tokens
                visible: backend.has_workspace
                enabled: backend.has_selected_chapter_prop
                onClicked: backend.save_current_chapter(editorPage.text)
            }

            ToolbarButton { text: "设置"; theme: tokens; onClicked: settingsDialog.open() }
            ToolbarButton { text: "同步"; theme: tokens; onClicked: { settingsDialog.switchToCategory(2); settingsDialog.open() } }
            ToolbarButton { text: "调试"; theme: tokens; onClicked: { settingsDialog.switchToCategory(4); settingsDialog.open() } }

            Item { width: tokens.sp8 }

            // Workspace switcher entry
            AppButton {
                text: "切换工作区"; theme: tokens
                visible: backend.has_workspace
                small: true
                onClicked: {
                    backend.switch_workspace()
                    reloadTree()
                }
            }
        }
    }

    // ══════════════════════════════════════════════════════════
    //  CONTENT
    // ══════════════════════════════════════════════════════════

    StackLayout {
        anchors.top: window.header.bottom
        anchors.bottom: window.footer.top
        anchors.left: parent.left
        anchors.right: parent.right
        currentIndex: backend.has_workspace ? 1 : 0

        // Page 0: Empty Workspace
        EmptyWorkspace {
            appTheme: tokens
            backendRef: backend
            onCreateWorkspace: backend.create_new_workspace()
            onOpenWorkspace: backend.open_existing_workspace()
            onInitFromGithub: {
                // Open sync settings so user can configure remote/branch/token/proxy
                // and see the "初始化/克隆" button
                settingsDialog.switchToCategory(2)
                settingsDialog.open()
            }
        }

        // Page 1: Main Workspace View
        RowLayout {
            spacing: 0

            // ── Left Sidebar ────────────────────────────────────────

            Rectangle {
                id: sidebar
                Layout.preferredWidth: sidebarVisible ? sidebarWidth : 0
                Layout.maximumWidth: sidebarVisible ? sidebarWidth : 0
                Layout.minimumWidth: sidebarVisible ? sidebarWidth : 0
                color: tokens.sidebarBg
                clip: true

                Behavior on Layout.preferredWidth { NumberAnimation { duration: 200; easing.type: Easing.OutCubic } }

                property bool sidebarVisible: true

                ColumnLayout {
                    anchors.fill: parent
                    spacing: 0

                    // Sidebar header
                    Rectangle {
                        Layout.fillWidth: true; height: 40; color: tokens.sidebarAlt
                        RowLayout {
                            anchors.fill: parent; anchors.leftMargin: tokens.sp12; anchors.rightMargin: tokens.sp8; spacing: tokens.sp4
                            Label {
                                text: "导航"; font.pixelSize: tokens.fontSm; font.weight: Font.DemiBold; color: tokens.textSecondary
                                Layout.fillWidth: true
                            }
                            Button {
                                text: "☰"; implicitWidth: 28; implicitHeight: 28
                                onClicked: sidebar.sidebarVisible = !sidebar.sidebarVisible
                                contentItem: Text { text: parent.text; color: tokens.textSecondary; font.pixelSize: tokens.fontLg; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                                background: Rectangle { color: parent.hovered ? tokens.hover : "transparent"; radius: tokens.radiusSm }
                            }
                        }
                    }

                    SidebarItem {
                        text: "作品"
                        icon: "📁"
                        active: navSection === "tree"
                        theme: tokens
                        onClicked: navSection = "tree"
                    }
                    SidebarItem {
                        text: "设置"
                        icon: "⚙"
                        active: navSection === "settings"
                        theme: tokens
                        onClicked: {
                            navSection = "settings"
                            settingsDialog.open()
                        }
                    }
                    SidebarItem {
                        text: "同步"
                        icon: "🔄"
                        active: navSection === "sync"
                        theme: tokens
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
                        theme: tokens
                        onClicked: {
                            navSection = "debug"
                            settingsDialog.switchToCategory(4)
                            settingsDialog.open()
                        }
                    }

                    Rectangle { height: 1; color: tokens.divider; Layout.fillWidth: true; Layout.leftMargin: tokens.sp12; Layout.rightMargin: tokens.sp12 }

                    // Tree view
                    Rectangle {
                        Layout.fillWidth: true; Layout.fillHeight: true; color: "transparent"
                        ListView {
                            id: treeView
                            anchors.fill: parent; anchors.topMargin: tokens.sp4
                            model: treeModel; clip: true
                            ScrollBar.vertical: ScrollBar { parent: treeView.parent; anchors.top: treeView.top; anchors.bottom: treeView.bottom; anchors.right: treeView.right }

                            // Empty state for tree
                            ColumnLayout {
                                anchors.centerIn: parent
                                spacing: tokens.sp8
                                visible: treeModel.count === 0
                                width: parent.width - tokens.sp24

                                Text {
                                    Layout.alignment: Qt.AlignHCenter
                                    text: "暂无作品"
                                    color: tokens.textSecondary
                                    font.pixelSize: tokens.fontMd
                                }
                                Text {
                                    Layout.alignment: Qt.AlignHCenter
                                    text: "点击上方「新建作品」开始创作"
                                    color: tokens.textSecondary
                                    font.pixelSize: tokens.fontSm
                                    horizontalAlignment: Text.AlignHCenter
                                    wrapMode: Text.Wrap
                                    Layout.fillWidth: true
                                }
                            }

                            delegate: Item {
                                width: ListView.view.width; height: 32
                                Rectangle {
                                    anchors.fill: parent; anchors.leftMargin: tokens.sp4; anchors.rightMargin: tokens.sp4
                                    radius: tokens.radiusSm; color: treeView.currentIndex === index ? tokens.selected : "transparent"

                                    Behavior on color {
                                        ColorAnimation { duration: 100 }
                                    }

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
                                        anchors.leftMargin: model.type === "project" ? tokens.sp8 : model.type === "volume" ? tokens.sp24 : tokens.sp40
                                        anchors.rightMargin: tokens.sp4; spacing: tokens.sp4

                                        Text {
                                            text: model.type === "project" ? "📁" : model.type === "volume" ? "📂" : "📄"
                                            font.pixelSize: tokens.fontSm
                                        }
                                        Text {
                                            text: model.title
                                            color: treeView.currentIndex === index ? tokens.selectedText : tokens.text
                                            font.pixelSize: tokens.fontMd; Layout.fillWidth: true; elide: Text.ElideRight; clip: true
                                        }
                                        Button {
                                            visible: treeView.currentIndex === index
                                            text: "⋮"; implicitWidth: 24; implicitHeight: 24
                                            onClicked: contextMenu.open()
                                            contentItem: Text { text: parent.text; color: tokens.textSecondary; font.pixelSize: tokens.fontLg; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                                            background: Rectangle { color: parent.hovered ? tokens.hover : "transparent"; radius: tokens.radiusSm }
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
                color: tokens.divider
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
                backendRef: backend; appTheme: tokens
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
    }

    // ══════════════════════════════════════════════════════════
    //  FOOTER
    // ══════════════════════════════════════════════════════════

    footer: Rectangle {
        height: 28; color: tokens.footerBg

        Rectangle {
            anchors.left: parent.left; anchors.right: parent.right; anchors.top: parent.top
            height: 1; color: tokens.border
        }

        RowLayout {
            anchors.fill: parent; anchors.leftMargin: tokens.sp12; anchors.rightMargin: tokens.sp12; spacing: tokens.sp16

            StatusPill {
                theme: tokens
                pillColor: backend.save_status === "已保存" || backend.save_status === "未选择章节" ? tokens.success : tokens.warning
            }

            Label { text: backend.save_status; color: tokens.textSecondary; font.pixelSize: tokens.fontXs }
            Rectangle { width: 1; height: 14; color: tokens.divider }
            Label { text: "字数: " + backend.word_count; color: tokens.textSecondary; font.pixelSize: tokens.fontXs }
            Rectangle { width: 1; height: 14; color: tokens.divider }

            // Sync status indicator
            RowLayout {
                spacing: tokens.sp4
                visible: backend.has_workspace
                Rectangle {
                    width: 6; height: 6; radius: 3
                    color: {
                        var ss = backend.sync_status
                        if (ss === "success") return tokens.success
                        if (ss === "syncing") return tokens.warning
                        if (ss === "auth_failed") return tokens.danger
                        if (ss === "network_failed") return tokens.danger
                        if (ss === "conflict") return tokens.danger
                        if (ss === "branch_missing") return tokens.warning
                        if (ss === "non_fast_forward") return tokens.danger
                        return tokens.textSecondary
                    }
                }
                Label { text: "同步"; color: tokens.textSecondary; font.pixelSize: tokens.fontXs }
            }

            Item { Layout.fillWidth: true }
            Button {
                text: "诊断"
                visible: backend.has_workspace
                flat: true
                implicitHeight: 22; implicitWidth: 40
                font.pixelSize: tokens.fontXs
                onClicked: {
                    var diag = backend.get_workspace_diagnostics()
                    var result = JSON.parse(backend.copy_text_to_clipboard(diag))
                    backend.save_status = result.success ? "诊断已复制" : result.message
                }
                contentItem: Text {
                    text: parent.text
                    color: tokens.primary
                    font.pixelSize: tokens.fontXs
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
                background: Rectangle {
                    color: parent.hovered ? tokens.hover : "transparent"
                    radius: tokens.radiusSm
                }
            }
            Label { text: backend.chapter_path; color: tokens.textSecondary; font.pixelSize: tokens.fontXs; elide: Text.ElideRight; Layout.maximumWidth: 300; clip: true }
            Label { text: backend.workspace_path; color: tokens.textSecondary; font.pixelSize: tokens.fontXs; elide: Text.ElideRight; Layout.maximumWidth: 250; clip: true }
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
