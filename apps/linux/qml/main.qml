import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import QtQuick.Window 2.15
import WriterApp 1.0

ApplicationWindow {
    id: window
    onClosing: {
        saveCurrentIfNeeded();
    }
    visible: true
    width: 1024
    height: 768
    minimumWidth: 800
    minimumHeight: 600
    title: "Writer"
    color: "#1e1e1e"

    property bool loadingChapter: false

    AppBackend {
        id: backend
        onWorkspace_opened: reloadTree()
        onProjects_reloaded: reloadTree()
        onError_occurred: {
            errorDialog.open();
        }
        onClear_editor: {
            loadingChapter = true;
            editorArea.text = "";
            backend.save_status = "未选择章节";
            loadingChapter = false;
        }
    }

    function saveCurrentIfNeeded() {
        if (backend.save_status === "未保存" && backend.has_selected_chapter() && backend.selected_chapter_exists()) {
            backend.save_current_chapter(editorArea.text);
        }
    }

    function canMoveUp(index) {
        let node = treeModel.get(index);
        for (let i = 0; i < treeModel.count; i++) {
            let sib = treeModel.get(i);
            if (sib.type === node.type && sib.projectId === node.projectId && sib.volumeId === node.volumeId) {
                if (sib.id === node.id) return i !== index;
            }
        }
        return false;
    }

    function isFirstSibling(index) {
        let node = treeModel.get(index);
        for (let i = 0; i < index; i++) {
            let sib = treeModel.get(i);
            if (sib.type === node.type && sib.projectId === node.projectId && sib.volumeId === node.volumeId) {
                return false;
            }
        }
        return true;
    }

    function isLastSibling(index) {
        let node = treeModel.get(index);
        for (let i = index + 1; i < treeModel.count; i++) {
            let sib = treeModel.get(i);
            if (sib.type === node.type && sib.projectId === node.projectId && sib.volumeId === node.volumeId) {
                return false;
            }
        }
        return true;
    }

    function reloadTree() {
        treeModel.clear();
        let items = backend.get_tree_model();
        let selId = backend.selected_item_id;
        let matchIndex = -1;
        for (let i = 0; i < items.length; i++) {
            let item = items[i];
            treeModel.append({
                "title": item.title,
                "id": item.id,
                "projectId": item.projectId || "",
                "volumeId": item.volumeId || "",
                "type": item.type
            });
            if (selId !== "" && item.id === selId) {
                matchIndex = i;
            }
        }
        if (matchIndex !== -1) {
            treeView.currentIndex = matchIndex;
        }

        if (treeModel.count === 0) {
            treeView.currentIndex = -1;
        }
    }

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

        onOpened: {
            inputField.text = contextData.initialText || "";
            inputField.forceActiveFocus();
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 10
            Label { text: "请输入名称:" }
            TextField {
                id: inputField
                Layout.fillWidth: true
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
                            backend.create_new_project(inputField.text);
                        } else if (actionType === "create_volume") {
                            backend.create_new_volume(contextData.projectId, inputField.text);
                        } else if (actionType === "create_chapter") {
                            backend.create_new_chapter(contextData.projectId, contextData.volumeId, inputField.text);
                        } else if (actionType === "rename_project") {
                            backend.rename_project(contextData.id, inputField.text);
                        } else if (actionType === "rename_volume") {
                            backend.rename_volume(contextData.projectId, contextData.id, inputField.text);
                        } else if (actionType === "rename_chapter") {
                            backend.rename_chapter(contextData.projectId, contextData.volumeId, contextData.id, inputField.text);
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

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 10
            Label {
                text: "错误"
                font.bold: true
                font.pixelSize: backend.setting_font_size > 0 ? backend.setting_font_size : 16
            }
            Label {
                text: backend.error_message
                Layout.fillWidth: true
                Layout.fillHeight: true
                wrapMode: Text.Wrap
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
        width: 300
        height: 150
        modal: true
        focus: true
        anchors.centerIn: parent
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

        property string actionType: ""
        property var contextData: ({})

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 10
            Label {
                text: "确认删除"
                font.bold: true
                font.pixelSize: backend.setting_font_size > 0 ? backend.setting_font_size : 16
            }
            Label { text: "您确定要删除此项目吗？" }
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
                            backend.delete_project(contextData.id);
                        } else if (actionType === "delete_volume") {
                            backend.delete_volume(contextData.projectId, contextData.id);
                        } else if (actionType === "delete_chapter") {
                            let wasSelected = (backend.selected_item_id === contextData.id);
                            backend.delete_chapter(contextData.projectId, contextData.volumeId, contextData.id);
                            if (wasSelected) {
                                editorArea.text = "";
                                backend.save_status = "已保存";
                            }
                        }
                        confirmDialog.close();
                    }
                }
            }
        }
    }

    ListModel {
        id: treeModel
    }

    Popup {
        id: settingsDialog
        property bool actionInProgress: false

        width: 600
        height: 700
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        anchors.centerIn: parent

        function applySyncFormToBackend() {
            backend.sync_enabled = syncEnabledCheck.checked;
            backend.sync_remote_url = remoteUrlInput.text;
            backend.sync_branch = branchInput.text;
            backend.sync_auto_sync = autoSyncCheck.checked;
            backend.sync_interval = parseInt(syncIntervalInput.text) || 300;
            backend.sync_proxy_type = proxyTypeCombo.currentText;
            backend.sync_proxy_host = proxyHostInput.text;
            backend.sync_proxy_port = parseInt(proxyPortInput.text) || 0;
            backend.sync_token = tokenInput.text;
        }

        function applyEditorFormToBackend() {
            backend.setting_font_size = fontSizeSpin.value;
            backend.setting_line_spacing = lineSpacingSpin.value / 100.0;
            backend.setting_auto_save_enabled = autoSaveCheck.checked;
            backend.setting_auto_save_delay_ms = autoSaveDelaySpin.value;
            backend.setting_auto_indent_enabled = autoIndentCheck.checked;
            backend.setting_auto_indent_width = autoIndentWidthSpin.value / 100.0;
            backend.setting_theme_mode = themeModeCombo.currentText;
        }

        onAboutToShow: {
            backend.load_local_settings();
            fontSizeSpin.value = backend.setting_font_size > 0 ? backend.setting_font_size : 16;
            lineSpacingSpin.value = backend.setting_line_spacing > 0 ? backend.setting_line_spacing * 100 : 150;
            autoSaveCheck.checked = backend.setting_auto_save_enabled;
            autoSaveDelaySpin.value = backend.setting_auto_save_delay_ms > 0 ? backend.setting_auto_save_delay_ms : 1500;
            autoIndentCheck.checked = backend.setting_auto_indent_enabled;
            autoIndentWidthSpin.value = backend.setting_auto_indent_width > 0 ? backend.setting_auto_indent_width * 100 : 200;

            var modes = ["system", "light", "dark"];
            themeModeCombo.currentIndex = modes.indexOf(backend.setting_theme_mode);
            if (themeModeCombo.currentIndex === -1) themeModeCombo.currentIndex = 0;

            backend.load_sync_config();
            syncEnabledCheck.checked = backend.sync_enabled;
            remoteUrlInput.text = backend.sync_remote_url;
            branchInput.text = backend.sync_branch;
            autoSyncCheck.checked = backend.sync_auto_sync;
            syncIntervalInput.text = backend.sync_interval.toString();
            proxyTypeCombo.currentIndex = proxyTypeCombo.indexOfValue(backend.sync_proxy_type);
            proxyHostInput.text = backend.sync_proxy_host;
            proxyPortInput.text = backend.sync_proxy_port.toString();
            tokenInput.text = backend.sync_token;
            syncResultLabel.text = backend.sync_action_result;
        }

        Connections {
            target: backend
            function onSync_action_completed() {
                syncResultLabel.text = backend.sync_action_result;
                settingsDialog.actionInProgress = false;
            }
        }

        ColumnLayout {
            anchors.fill: parent

            TabBar {
                id: settingsTabBar
                Layout.fillWidth: true
                TabButton { text: "编辑器设置" }
                TabButton { text: "同步设置" }
                TabButton { text: "关于" }
            }

            StackLayout {
                currentIndex: settingsTabBar.currentIndex
                Layout.fillWidth: true
                Layout.fillHeight: true

                ScrollView {
                    clip: true
                    ColumnLayout {
                        width: parent.width - 20
                        spacing: 15

                        Label { text: "字号:" }
                        SpinBox {
                            id: fontSizeSpin
                            from: 10
                            to: 72
                            value: 16
                        }

                        Label { text: "行距 (Linux端暂未实现视觉应用):" }
                        SpinBox {
                            id: lineSpacingSpin
                            from: 100
                            to: 300
                            value: 150
                            stepSize: 10
                        }

                        CheckBox {
                            id: autoSaveCheck
                            text: "自动保存"
                        }

                        Label { text: "自动保存延迟 (毫秒):" }
                        SpinBox {
                            id: autoSaveDelaySpin
                            from: 500
                            to: 60000
                            stepSize: 500
                            value: 1500
                        }

                        CheckBox {
                            id: autoIndentCheck
                            text: "自动缩进"
                        }
                        Label {
                            text: "Linux 端暂未实现自动缩进效果"
                            color: "gray"
                            font.pixelSize: 12
                        }

                        Label { text: "自动缩进宽度:" }
                        SpinBox {
                            id: autoIndentWidthSpin
                            from: 0
                            to: 800
                            value: 200
                            stepSize: 50
                        }

                        Label { text: "主题模式:" }
                        ComboBox {
                            id: themeModeCombo
                            Layout.fillWidth: true
                            model: ["system", "light", "dark"]
                        }

                        Button {
                            text: "保存设置"
                            enabled: !settingsDialog.actionInProgress
                            onClicked: {
                                applyEditorFormToBackend();
                                backend.save_local_settings();
                            }
                        }
                    }
                }

                ScrollView {
                    clip: true
                    ColumnLayout {
                        width: parent.width - 20
                        spacing: 10

                        CheckBox {
                            id: syncEnabledCheck
                            text: "启用同步"
                        }

                        Label { text: "GitHub 仓库地址:" }
                        TextField {
                            id: remoteUrlInput
                            Layout.fillWidth: true
                            placeholderText: "https://github.com/user/repo.git"
                        }

                        Label { text: "分支:" }
                        TextField {
                            id: branchInput
                            Layout.fillWidth: true
                            placeholderText: "main"
                        }

                        CheckBox {
                            id: autoSyncCheck
                            text: "自动同步"
                        }

                        Label { text: "同步间隔 (秒):" }
                        TextField {
                            id: syncIntervalInput
                            Layout.fillWidth: true
                            validator: IntValidator { bottom: 60 }
                        }

                        Label { text: "代理类型:" }
                        ComboBox {
                            id: proxyTypeCombo
                            Layout.fillWidth: true
                            model: ["none", "auto", "http", "socks5"]
                        }

                        Label { text: "代理 Host:" }
                        TextField {
                            id: proxyHostInput
                            Layout.fillWidth: true
                            placeholderText: "127.0.0.1"
                        }

                        Label { text: "代理 Port:" }
                        TextField {
                            id: proxyPortInput
                            Layout.fillWidth: true
                            validator: IntValidator { bottom: 0; top: 65535 }
                        }

                        Label { text: "Token (Personal Access Token):" }
                        TextField {
                            id: tokenInput
                            Layout.fillWidth: true
                            echoMode: TextInput.Password
                        }

                        RowLayout {
                            Layout.fillWidth: true

                            Button {
                                text: "保存"
                                enabled: !settingsDialog.actionInProgress
                                onClicked: {
                                    applySyncFormToBackend();
                                    settingsDialog.actionInProgress = true;
                                    if (backend.save_sync_config()) {
                                        settingsDialog.actionInProgress = false;
                                        syncResultLabel.text = "配置已保存";
                                    } else {
                                        settingsDialog.actionInProgress = false;
                                    }
                                }
                            }

                            Button {
                                text: "测试 GitHub 连接"
                                enabled: !settingsDialog.actionInProgress
                                onClicked: {
                                    applySyncFormToBackend();
                                    settingsDialog.actionInProgress = true;
                                    if (backend.save_sync_config()) {
                                        backend.perform_sync_diagnostics();
                                    } else {
                                        settingsDialog.actionInProgress = false;
                                    }
                                }
                            }

                            Button {
                                text: "检查同步计划"
                                enabled: !settingsDialog.actionInProgress
                                onClicked: {
                                    applySyncFormToBackend();
                                    settingsDialog.actionInProgress = true;
                                    if (backend.save_sync_config()) {
                                        backend.perform_sync_dry_run();
                                    } else {
                                        settingsDialog.actionInProgress = false;
                                    }
                                }
                            }
                            Button {
                                text: "立即同步"
                                enabled: !settingsDialog.actionInProgress
                                onClicked: {
                                    applySyncFormToBackend();
                                    settingsDialog.actionInProgress = true;
                                    if (backend.save_sync_config()) {
                                        backend.perform_sync();
                                    } else {
                                        settingsDialog.actionInProgress = false;
                                    }
                                }
                            }
                        }

                        TextArea {
                            id: syncResultLabel
                            Layout.fillWidth: true
                            readOnly: true
                            wrapMode: Text.Wrap
                            background: Rectangle { color: "#eeeeee" }
                            color: "black"
                        }
                    }
                }

                Item {
                    ColumnLayout {
                        anchors.centerIn: parent
                        Label {
                            text: "Writer Application (Linux)"
                            font.pixelSize: 20
                            font.bold: true
                            horizontalAlignment: Text.AlignHCenter
                        }
                        Label {
                            text: "Version 1.0.0"
                            horizontalAlignment: Text.AlignHCenter
                        }
                    }
                }
            }
        }
    }

    header: ToolBar {
        background: Rectangle { color: "#2d2d2d" }
        RowLayout {
            anchors.fill: parent
            ToolButton {
                text: "打开/创建工作区"
                onClicked: backend.open_workspace_dialog()
                contentItem: Text {
                    text: parent.text
                    color: "white"
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
            }
            ToolButton {
                text: "新建作品"
                onClicked: { inputDialog.actionType = "create_project"; inputDialog.contextData = { initialText: "新作品" }; inputDialog.open(); }
                contentItem: Text {
                    text: parent.text
                    color: "white"
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
            }
            ToolButton {
                text: "保存"
                onClicked: backend.save_current_chapter(editorArea.text)
                contentItem: Text {
                    text: parent.text
                    color: "white"
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
            }
            ToolButton {
                text: "设置"
                onClicked: settingsDialog.open()
                contentItem: Text {
                    text: parent.text
                    color: "white"
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
            }
            Item { Layout.fillWidth: true }
        }
    }

    footer: ToolBar {
        background: Rectangle { color: "#2d2d2d" }
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 10
            anchors.rightMargin: 10
            spacing: 15
            Label {
                id: statusLabel
                text: backend.save_status
                color: "white"
            }
            Label {
                id: wordCountLabel
                text: "字数: " + backend.word_count
                color: "white"
            }
            Item { Layout.fillWidth: true }
            Label {
                text: backend.chapter_path
                color: "gray"
                elide: Text.ElideRight
                Layout.maximumWidth: 250
                clip: true
            }
            Label {
                id: workspacePathLabel
                text: backend.workspace_path
                color: "gray"
                elide: Text.ElideRight
                Layout.maximumWidth: 250
                clip: true
            }
        }
    }

    SplitView {
        anchors.fill: parent
        orientation: Qt.Horizontal

        Rectangle {
            SplitView.preferredWidth: 250
            SplitView.minimumWidth: 200
            color: "#252526"

            ListView {
                id: treeView
                anchors.fill: parent
                model: treeModel
                clip: true

                Text {
                    anchors.centerIn: parent
                    text: "未选择作品"
                    color: "gray"
                    visible: treeModel.count === 0
                }

                delegate: Item {
                    width: ListView.view.width
                    height: 30
                    Rectangle {
                        anchors.fill: parent
                        color: treeView.currentIndex === index ? "#37373d" : "transparent"
                        MouseArea {
                            anchors.fill: parent
                            onClicked: {
                                treeView.currentIndex = index;
                                let node = treeModel.get(index);
                                saveCurrentIfNeeded();
                                loadingChapter = true;
                                if (node.type === "project") {
                                    backend.select_project(node.id);
                                    editorArea.text = "";
                                    backend.save_status = "已保存";
                                } else if (node.type === "volume") {
                                    backend.select_volume(node.projectId, node.id);
                                    editorArea.text = "";
                                    backend.save_status = "已保存";
                                } else if (node.type === "chapter") {
                                    backend.select_chapter(node.projectId, node.volumeId, node.id);
                                    editorArea.text = backend.get_chapter_content(node.projectId, node.volumeId, node.id);
                                    backend.save_status = "已保存";
                                    editorArea.forceActiveFocus();
                                }
                                loadingChapter = false;
                            }
                        }
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: (model.type === "project" ? 5 : model.type === "volume" ? 20 : 35)
                            spacing: 5

                            Text {
                                text: model.title
                                color: "white"
                                Layout.fillWidth: true
                                elide: Text.ElideRight
                                clip: true
                            }

                            ToolButton {
                                visible: treeView.currentIndex === index
                                text: "⋮"
                                onClicked: contextMenu.open()
                                contentItem: Text { text: parent.text; color: "white"; font.pixelSize: 18; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
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
                                                inputDialog.actionType = "create_volume";
                                                inputDialog.contextData = { projectId: model.id, initialText: "新分卷" };
                                                inputDialog.open();
                                            } else if (model.type === "volume") {
                                                inputDialog.actionType = "create_chapter";
                                                inputDialog.contextData = { projectId: model.projectId, volumeId: model.id, initialText: "新章节" };
                                                inputDialog.open();
                                            }
                                        }
                                    }
                                    MenuItem {
                                        text: "重命名"
                                        onTriggered: {
                                            inputDialog.actionType = "rename_" + model.type;
                                            inputDialog.contextData = { id: model.id, projectId: model.projectId, volumeId: model.volumeId, initialText: model.title.trim() };
                                            inputDialog.open();
                                        }
                                    }
                                    MenuItem {
                                        text: "删除"
                                        onTriggered: {
                                            if (model.type === "volume") {
                                                confirmDialog.text = "确定要删除此分卷及其包含的所有章节吗？";
                                            } else {
                                                confirmDialog.text = "确定要删除吗？";
                                            }
                                            confirmDialog.actionType = "delete_" + model.type;
                                            confirmDialog.contextData = { id: model.id, projectId: model.projectId, volumeId: model.volumeId };
                                            confirmDialog.open();
                                        }
                                    }
                                    MenuItem {
                                        text: "上移"
                                        visible: !isFirstSibling(index)
                                        onTriggered: {
                                            let ids = [];
                                            let my_pos = -1;
                                            for (let i = 0; i < treeModel.count; i++) {
                                                let node = treeModel.get(i);
                                                if (node.type === model.type && node.projectId === model.projectId && node.volumeId === model.volumeId) {
                                                    if (node.id === model.id) my_pos = ids.length;
                                                    ids.push(node.id);
                                                }
                                            }
                                            if (my_pos > 0) {
                                                let temp = ids[my_pos];
                                                ids[my_pos] = ids[my_pos - 1];
                                                ids[my_pos - 1] = temp;

                                                if (model.type === "project") backend.reorder_projects(ids.join(","));
                                                else if (model.type === "volume") backend.reorder_volumes(model.projectId, ids.join(","));
                                                else if (model.type === "chapter") backend.reorder_chapters(model.projectId, model.volumeId, ids.join(","));
                                            }
                                        }
                                    }
                                    MenuItem {
                                        text: "下移"
                                        visible: !isLastSibling(index)
                                        onTriggered: {
                                            let ids = [];
                                            let my_pos = -1;
                                            for (let i = 0; i < treeModel.count; i++) {
                                                let node = treeModel.get(i);
                                                if (node.type === model.type && node.projectId === model.projectId && node.volumeId === model.volumeId) {
                                                    if (node.id === model.id) my_pos = ids.length;
                                                    ids.push(node.id);
                                                }
                                            }
                                            if (my_pos < ids.length - 1) {
                                                let temp = ids[my_pos];
                                                ids[my_pos] = ids[my_pos + 1];
                                                ids[my_pos + 1] = temp;

                                                if (model.type === "project") backend.reorder_projects(ids.join(","));
                                                else if (model.type === "volume") backend.reorder_volumes(model.projectId, ids.join(","));
                                                else if (model.type === "chapter") backend.reorder_chapters(model.projectId, model.volumeId, ids.join(","));
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

        Rectangle {
            SplitView.fillWidth: true
            color: "#1e1e1e"

            Timer {
                id: autoSaveTimer
                interval: 1500
                repeat: false
                onTriggered: {
                    if (backend.save_status === "未保存") {
                        backend.save_current_chapter(editorArea.text);
                    }
                }
            }

            Rectangle {
                anchors.fill: parent
                color: "#1e1e1e"
                clip: true

                Text {
                    anchors.centerIn: parent
                    text: "请在左侧选择或创建一个章节"
                    color: "gray"
                    visible: !backend.has_selected_chapter_prop
                }

                ScrollView {
                    id: editorScroll
                    anchors.fill: parent
                    anchors.margins: 20
                    clip: true
                    visible: backend.has_selected_chapter_prop
                    ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

                    TextArea {
                        id: editorArea
                        color: "#d4d4d4"
                        font.pixelSize: backend.setting_font_size > 0 ? backend.setting_font_size : 16
                        wrapMode: TextArea.Wrap
                        background: Rectangle { color: "transparent" }
                        enabled: backend.has_selected_chapter_prop
                        width: editorScroll.availableWidth
                        implicitWidth: editorScroll.availableWidth
                        implicitHeight: Math.max(editorScroll.availableHeight, contentHeight + topPadding + bottomPadding)
                        focus: true
                        activeFocusOnTab: true
                        selectByMouse: true
                        persistentSelection: true

                        onTextChanged: {
                            backend.calculate_word_count(text);
                            if (backend.setting_auto_save_enabled) {
                                autoSaveTimer.interval = backend.setting_auto_save_delay_ms > 0 ? backend.setting_auto_save_delay_ms : 1500;
                                autoSaveTimer.restart();
                            }
                            if (!loadingChapter && backend.has_selected_chapter_prop && backend.save_status !== "未保存") {
                                backend.save_status = "未保存";
                            }
                            if (!loadingChapter && backend.has_selected_chapter_prop) {
                                autoSaveTimer.restart();
                            }
                        }
                    }
                }
            }
        }
    }
}
