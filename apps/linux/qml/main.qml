import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import QtQuick.Window 2.15
import WriterApp 1.0

ApplicationWindow {
    id: window
    visible: true
    width: 1024
    height: 768
    minimumWidth: 800
    minimumHeight: 600
    title: "Writer"
    color: bgColor

    property bool loadingChapter: false

    // --- Theme colors ---
    property string currentTheme: "dark"

    function applyTheme() {
        let mode = backend.setting_theme_mode
        if (mode === "light") {
            currentTheme = "light"
        } else if (mode === "dark") {
            currentTheme = "dark"
        } else {
            // system - default to dark on Linux for now
            currentTheme = "dark"
        }
    }

    property string bgColor: currentTheme === "light" ? "#ffffff" : "#1e1e1e"
    property string headerBgColor: currentTheme === "light" ? "#f3f3f3" : "#2d2d2d"
    property string footerBgColor: currentTheme === "light" ? "#f3f3f3" : "#2d2d2d"
    property string treeBgColor: currentTheme === "light" ? "#f8f8f8" : "#252526"
    property string textColor: currentTheme === "light" ? "#1e1e1e" : "#ffffff"
    property string textSecondaryColor: currentTheme === "light" ? "#666666" : "#999999"
    property string selectedBgColor: currentTheme === "light" ? "#e8e8e8" : "#37373d"
    property string toolbarTextColor: currentTheme === "light" ? "#1e1e1e" : "#ffffff"
    property string buttonHoverColor: currentTheme === "light" ? "#e0e0e0" : "#3e3e42"

    Component.onCompleted: applyTheme()

    Connections {
        target: backend
        function onSettings_changed() { applyTheme() }
    }

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

    function saveCurrentIfNeeded() {
        if (backend.save_status === "未保存" && backend.has_selected_chapter() && backend.selected_chapter_exists()) {
            backend.save_current_chapter(editorPage.text)
        }
    }

    function canMoveUp(index) {
        let node = treeModel.get(index)
        for (let i = 0; i < treeModel.count; i++) {
            let sib = treeModel.get(i)
            if (sib.type === node.type && sib.projectId === node.projectId && sib.volumeId === node.volumeId) {
                if (sib.id === node.id) return i !== index
            }
        }
        return false
    }

    function isFirstSibling(index) {
        let node = treeModel.get(index)
        for (let i = 0; i < index; i++) {
            let sib = treeModel.get(i)
            if (sib.type === node.type && sib.projectId === node.projectId && sib.volumeId === node.volumeId) {
                return false
            }
        }
        return true
    }

    function isLastSibling(index) {
        let node = treeModel.get(index)
        for (let i = index + 1; i < treeModel.count; i++) {
            let sib = treeModel.get(i)
            if (sib.type === node.type && sib.projectId === node.projectId && sib.volumeId === node.volumeId) {
                return false
            }
        }
        return true
    }

    function reloadTree() {
        treeModel.clear()
        let items = backend.get_tree_model()
        let selId = backend.selected_item_id
        let matchIndex = -1
        for (let i = 0; i < items.length; i++) {
            let item = items[i]
            treeModel.append({
                "title": item.title,
                "id": item.id,
                "projectId": item.projectId || "",
                "volumeId": item.volumeId || "",
                "type": item.type
            })
            if (selId !== "" && item.id === selId) {
                matchIndex = i
            }
        }
        if (matchIndex !== -1) {
            treeView.currentIndex = matchIndex
        }
        if (treeModel.count === 0) {
            treeView.currentIndex = -1
        }
    }

    // --- Dialogs ---

    Popup {
        id: inputDialog
        property string actionType: ""
        property var contextData: ({})

        width: 300
        height: 150
        modal: true
        focus: true
        anchors.centerIn: parent
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: Rectangle { color: headerBgColor; radius: 8 }

        onOpened: {
            inputField.text = contextData.initialText || ""
            inputField.forceActiveFocus()
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 10
            Label { text: "请输入名称:"; color: textColor }
            TextField {
                id: inputField
                Layout.fillWidth: true
                color: textColor
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                Button {
                    text: "取消"
                    onClicked: inputDialog.close()
                }
                Button {
                    text: "确定"
                    onClicked: {
                        if (actionType === "create_project") {
                            backend.create_new_project(inputField.text)
                        } else if (actionType === "create_volume") {
                            backend.create_new_volume(contextData.projectId, inputField.text)
                        } else if (actionType === "create_chapter") {
                            backend.create_new_chapter(contextData.projectId, contextData.volumeId, inputField.text)
                        } else if (actionType === "rename_project") {
                            backend.rename_project(contextData.id, inputField.text)
                        } else if (actionType === "rename_volume") {
                            backend.rename_volume(contextData.projectId, contextData.id, inputField.text)
                        } else if (actionType === "rename_chapter") {
                            backend.rename_chapter(contextData.projectId, contextData.volumeId, contextData.id, inputField.text)
                        }
                        inputDialog.close()
                    }
                }
            }
        }
    }

    Popup {
        id: errorDialog
        width: 400
        height: 150
        modal: true
        focus: true
        anchors.centerIn: parent
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: Rectangle { color: headerBgColor; radius: 8 }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 10
            Label {
                text: "错误"
                font.bold: true
                font.pixelSize: backend.setting_font_size > 0 ? backend.setting_font_size : 16
                color: textColor
            }
            Label {
                text: backend.error_message
                Layout.fillWidth: true
                Layout.fillHeight: true
                wrapMode: Text.Wrap
                color: textColor
            }
            Button {
                text: "确定"
                Layout.alignment: Qt.AlignRight
                onClicked: errorDialog.close()
            }
        }
    }

    Popup {
        id: confirmDialog
        property string actionType: ""
        property var contextData: ({})

        width: 300
        height: 150
        modal: true
        focus: true
        anchors.centerIn: parent
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: Rectangle { color: headerBgColor; radius: 8 }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 10
            Label {
                text: "确认删除"
                font.bold: true
                font.pixelSize: backend.setting_font_size > 0 ? backend.setting_font_size : 16
                color: textColor
            }
            Label { text: "您确定要删除此项目吗？"; color: textColor }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                Button {
                    text: "取消"
                    onClicked: confirmDialog.close()
                }
                Button {
                    text: "确定"
                    onClicked: {
                        if (actionType === "delete_project") {
                            backend.delete_project(contextData.id)
                        } else if (actionType === "delete_volume") {
                            backend.delete_volume(contextData.projectId, contextData.id)
                        } else if (actionType === "delete_chapter") {
                            backend.delete_chapter(contextData.projectId, contextData.volumeId, contextData.id)
                        }
                        confirmDialog.close()
                    }
                }
            }
        }
    }

    SettingsDialog {
        id: settingsDialog
        backendRef: backend
        editorPageRef: editorPage
        bgColor: headerBgColor
        textColor: textColor
        textSecondaryColor: textSecondaryColor
        borderColor: selectedBgColor
    }

    // --- Data ---

    ListModel {
        id: treeModel
    }

    // --- Header ---

    header: ToolBar {
        background: Rectangle { color: headerBgColor }
        RowLayout {
            anchors.fill: parent
            spacing: 0
            ToolButton {
                text: "打开/创建工作区"
                onClicked: backend.open_workspace_dialog()
                contentItem: Text {
                    text: parent.text
                    color: toolbarTextColor
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                    elide: Text.ElideRight
                }
                background: Rectangle {
                    color: parent.hovered ? buttonHoverColor : "transparent"
                    radius: 4
                }
            }
            ToolButton {
                text: "新建作品"
                onClicked: {
                    inputDialog.actionType = "create_project"
                    inputDialog.contextData = { initialText: "新作品" }
                    inputDialog.open()
                }
                contentItem: Text {
                    text: parent.text
                    color: toolbarTextColor
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                    elide: Text.ElideRight
                }
                background: Rectangle {
                    color: parent.hovered ? buttonHoverColor : "transparent"
                    radius: 4
                }
            }
            ToolButton {
                text: "保存"
                onClicked: backend.save_current_chapter(editorPage.text)
                contentItem: Text {
                    text: parent.text
                    color: toolbarTextColor
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                    elide: Text.ElideRight
                }
                background: Rectangle {
                    color: parent.hovered ? buttonHoverColor : "transparent"
                    radius: 4
                }
            }
            ToolButton {
                text: "设置"
                onClicked: settingsDialog.open()
                contentItem: Text {
                    text: parent.text
                    color: toolbarTextColor
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                    elide: Text.ElideRight
                }
                background: Rectangle {
                    color: parent.hovered ? buttonHoverColor : "transparent"
                    radius: 4
                }
            }
            Item { Layout.fillWidth: true }
        }
    }

    // --- Footer ---

    footer: ToolBar {
        background: Rectangle { color: footerBgColor }
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 10
            anchors.rightMargin: 10
            spacing: 15
            Label {
                text: backend.save_status
                color: textColor
            }
            Label {
                text: "字数: " + backend.word_count
                color: textColor
            }
            Item { Layout.fillWidth: true }
            Label {
                text: backend.chapter_path
                color: textSecondaryColor
                elide: Text.ElideRight
                Layout.maximumWidth: 250
                clip: true
            }
            Label {
                text: backend.workspace_path
                color: textSecondaryColor
                elide: Text.ElideRight
                Layout.maximumWidth: 250
                clip: true
            }
        }
    }

    // --- Content area between header and footer ---

    Item {
        id: contentArea
        anchors.top: window.header.bottom
        anchors.bottom: window.footer.top
        anchors.left: parent.left
        anchors.right: parent.right

        SplitView {
            anchors.fill: parent
            orientation: Qt.Horizontal

        // Left: directory tree
        Rectangle {
            SplitView.preferredWidth: 250
            SplitView.minimumWidth: 200
            color: treeBgColor

            ListView {
                id: treeView
                anchors.fill: parent
                model: treeModel
                clip: true

                Text {
                    anchors.centerIn: parent
                    text: "未选择作品"
                    color: textSecondaryColor
                    visible: treeModel.count === 0
                }

                delegate: Item {
                    width: ListView.view.width
                    height: 30
                    Rectangle {
                        anchors.fill: parent
                        color: treeView.currentIndex === index ? selectedBgColor : "transparent"
                        MouseArea {
                            anchors.fill: parent
                            onClicked: {
                                treeView.currentIndex = index
                                let node = treeModel.get(index)
                                saveCurrentIfNeeded()
                                loadingChapter = true
                                if (node.type === "project") {
                                    backend.select_project(node.id)
                                    editorPage.clearText()
                                    backend.save_status = "已保存"
                                } else if (node.type === "volume") {
                                    backend.select_volume(node.projectId, node.id)
                                    editorPage.clearText()
                                    backend.save_status = "已保存"
                                } else if (node.type === "chapter") {
                                    backend.select_chapter(node.projectId, node.volumeId, node.id)
                                    editorPage.loadContent(
                                        backend.get_chapter_content(node.projectId, node.volumeId, node.id)
                                    )
                                    backend.save_status = "已保存"
                                    editorPage.forceEditorFocus()
                                }
                                loadingChapter = false
                            }
                        }
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: (model.type === "project" ? 5 : model.type === "volume" ? 20 : 35)
                            spacing: 5

                            Text {
                                text: model.title
                                color: textColor
                                Layout.fillWidth: true
                                elide: Text.ElideRight
                                clip: true
                            }

                            ToolButton {
                                visible: treeView.currentIndex === index
                                text: "\u22EE"
                                onClicked: contextMenu.open()
                                background: Item {}
                                padding: 0
                                Layout.preferredWidth: 30
                                Layout.alignment: Qt.AlignVCenter

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
                                    MenuItem {
                                        text: "重命名"
                                        onTriggered: {
                                            inputDialog.actionType = "rename_" + model.type
                                            inputDialog.contextData = {
                                                id: model.id,
                                                projectId: model.projectId,
                                                volumeId: model.volumeId,
                                                initialText: model.title.trim()
                                            }
                                            inputDialog.open()
                                        }
                                    }
                                    MenuItem {
                                        text: "删除"
                                        onTriggered: {
                                            confirmDialog.actionType = "delete_" + model.type
                                            confirmDialog.contextData = {
                                                id: model.id,
                                                projectId: model.projectId,
                                                volumeId: model.volumeId
                                            }
                                            confirmDialog.open()
                                        }
                                    }
                                    MenuItem {
                                        text: "上移"
                                        visible: !isFirstSibling(index)
                                        onTriggered: {
                                            let ids = []
                                            let my_pos = -1
                                            for (let i = 0; i < treeModel.count; i++) {
                                                let node = treeModel.get(i)
                                                if (node.type === model.type && node.projectId === model.projectId && node.volumeId === model.volumeId) {
                                                    if (node.id === model.id) my_pos = ids.length
                                                    ids.push(node.id)
                                                }
                                            }
                                            if (my_pos > 0) {
                                                let temp = ids[my_pos]
                                                ids[my_pos] = ids[my_pos - 1]
                                                ids[my_pos - 1] = temp
                                                if (model.type === "project") backend.reorder_projects(ids.join(","))
                                                else if (model.type === "volume") backend.reorder_volumes(model.projectId, ids.join(","))
                                                else if (model.type === "chapter") backend.reorder_chapters(model.projectId, model.volumeId, ids.join(","))
                                            }
                                        }
                                    }
                                    MenuItem {
                                        text: "下移"
                                        visible: !isLastSibling(index)
                                        onTriggered: {
                                            let ids = []
                                            let my_pos = -1
                                            for (let i = 0; i < treeModel.count; i++) {
                                                let node = treeModel.get(i)
                                                if (node.type === model.type && node.projectId === model.projectId && node.volumeId === model.volumeId) {
                                                    if (node.id === model.id) my_pos = ids.length
                                                    ids.push(node.id)
                                                }
                                            }
                                            if (my_pos < ids.length - 1) {
                                                let temp = ids[my_pos]
                                                ids[my_pos] = ids[my_pos + 1]
                                                ids[my_pos + 1] = temp
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

        // Right: editor
        EditorPage {
            id: editorPage
            SplitView.fillWidth: true
            backendRef: backend
            bgColor: bgColor
            textColor: textColor
            placeholderColor: textSecondaryColor
            onContentChanged: {
                backend.calculate_word_count(text)
                if (backend.setting_auto_save_enabled) {
                    autoSaveTimer.interval = backend.setting_auto_save_delay_ms > 0 ? backend.setting_auto_save_delay_ms : 1500
                    autoSaveTimer.restart()
                }
                if (!loadingChapter && backend.has_selected_chapter_prop && backend.save_status !== "未保存") {
                    backend.save_status = "未保存"
                }
                if (!loadingChapter && backend.has_selected_chapter_prop) {
                    autoSaveTimer.restart()
                }
            }
        }
        }
    }

    Timer {
        id: autoSaveTimer
        interval: 1500
        repeat: false
        onTriggered: {
            if (backend.save_status === "未保存") {
                backend.save_current_chapter(editorPage.text)
            }
        }
    }
}
