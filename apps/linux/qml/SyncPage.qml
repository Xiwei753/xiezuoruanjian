import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import QtQuick.Window 2.15

Rectangle {
    id: root
    implicitHeight: mainCol.implicitHeight + 32
    property int lastSyncResultLen: -1
    property double lastSyncStatusLogTime: 0

    Connections {
        target: root.backendRef
        function onSync_action_completed() {
            if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                var resLen = root.backendRef ? root.backendRef.sync_action_result.length : 0;
                window.debugLog("sync", "action_completed_callback", "resultLength=" + resLen);
            }
            if (root.backendRef) {
                syncResultArea.text = root.backendRef.sync_action_result;
            }
        }
        function onSync_status_changed() {
            var resLen = root.backendRef ? root.backendRef.sync_action_result.length : 0;
            var now = Date.now();
            var shouldLog = true;
            if (resLen === root.lastSyncResultLen) {
                if (now - root.lastSyncStatusLogTime < 5000) {
                    shouldLog = false;
                }
            }
            if (shouldLog) {
                root.lastSyncResultLen = resLen;
                root.lastSyncStatusLogTime = now;
                if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                    window.debugLog("sync", "status_changed_callback", "resultLength=" + resLen);
                }
            }
            if (root.backendRef) {
                syncResultArea.text = root.backendRef.sync_action_result;
            }
        }
    }

    property var theme: null
    property var backendRef: null
    signal settingsChanged()

    color: theme ? theme.bgDark : "#1E1E1E"

    Column {
        id: mainCol
        width: parent.width - 32
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.top
        anchors.topMargin: 16
        spacing: 20

        Text {
            text: "同步设置"
            color: theme ? theme.textMain : "#FFFFFF"
            font.pixelSize: 24
            font.bold: true
        }

        Column {
            width: parent.width
            spacing: 8
            Text { text: "远程仓库地址"; color: theme ? theme.textMain : "#E0E0E0"; font.pixelSize: 14 }
            TextField {
                id: urlField
                width: parent.width
                text: (root.backendRef ? root.backendRef.sync_remote_url : '')
                placeholderText: "https://github.com/user/repo"
                color: theme ? theme.textMain : "#E0E0E0"
                background: Rectangle {
                    color: theme ? theme.inputBg : "#2A2A2A"
                    border.color: theme ? theme.border : "#444444"
                    radius: 4
                }
            }
        }

        Column {
            width: parent.width
            spacing: 8
            Text { text: "分支名"; color: theme ? theme.textMain : "#E0E0E0"; font.pixelSize: 14 }
            TextField {
                id: branchField
                width: parent.width
                text: (root.backendRef ? root.backendRef.sync_branch : '')
                placeholderText: "main"
                color: theme ? theme.textMain : "#E0E0E0"
                background: Rectangle {
                    color: theme ? theme.inputBg : "#2A2A2A"
                    border.color: theme ? theme.border : "#444444"
                    radius: 4
                }
            }
        }

        Column {
            width: parent.width
            spacing: 8
            Text { text: "访问 Token"; color: theme ? theme.textMain : "#E0E0E0"; font.pixelSize: 14 }
            TextField {
                id: tokenField
                width: parent.width
                placeholderText: (root.backendRef ? root.backendRef.has_sync_token : false) ? "已设置 (输入新 Token 以覆盖)" : "请输入 GitHub Personal Access Token"
                echoMode: TextInput.Password
                color: theme ? theme.textMain : "#E0E0E0"
                background: Rectangle {
                    color: theme ? theme.inputBg : "#2A2A2A"
                    border.color: theme ? theme.border : "#444444"
                    radius: 4
                }
            }
        }

        Row {
            spacing: 16
            Button {
                text: "保存配置"
                onClicked: {
                    if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                        var hasNewToken = tokenField.text.trim().length > 0;
                        window.debugLog("sync", "save_config_clicked", "url=" + urlField.text + ", branch=" + branchField.text + ", hasNewToken=" + hasNewToken);
                    }
                    if (!root.backendRef) return;
                    root.backendRef.sync_remote_url = urlField.text;
                    root.backendRef.sync_branch = branchField.text.length > 0 ? branchField.text : "main";
                    if (tokenField.text.trim().length > 0) {
                        root.backendRef.set_sync_token(tokenField.text.trim());
                        tokenField.text = "";
                    }
                    root.backendRef.sync_enabled = true;
                    root.backendRef.sync_backend_type = "github_api";
                    
                    var success = root.backendRef.save_sync_config();
                    if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                        window.debugLog("sync", "save_config_finished", "success=" + success);
                    }
                    if (success) root.settingsChanged();
                }
            }

            Button {
                text: "执行同步"
                onClicked: {
                    if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                        window.debugLog("sync", "perform_sync_clicked", "");
                    }
                    syncResultArea.text = "正在同步...";
                    if (root.backendRef) root.backendRef.perform_sync();
                }
            }

            Button {
                text: "运行诊断"
                onClicked: {
                    if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                        window.debugLog("sync", "perform_diagnostics_clicked", "");
                    }
                    syncResultArea.text = "正在诊断...";
                    if (root.backendRef) root.backendRef.perform_sync_diagnostics();
                }
            }

            Button {
                text: "打开工作区目录"
                visible: root.backendRef && root.backendRef.has_workspace
                onClicked: {
                    if (root.backendRef) {
                        root.backendRef.open_workspace_dir();
                    }
                }
            }

            Button {
                text: "复制冲突信息"
                visible: root.backendRef && root.backendRef.sync_status === "conflict"
                onClicked: {
                    if (root.backendRef) {
                        root.backendRef.copy_text_to_clipboard(syncResultArea.text);
                    }
                }
            }
        }

        Rectangle {
            width: parent.width
            height: Math.max(120, Math.min(200, (Window.window ? Window.window.height : 768) - 500))
            color: (root.backendRef && root.backendRef.sync_status === "conflict") ? "#331a1a" : (theme ? theme.bgDarker : "#121212")
            border.color: (root.backendRef && root.backendRef.sync_status === "conflict") ? "#cc3333" : (theme ? theme.border : "#333333")
            border.width: (root.backendRef && root.backendRef.sync_status === "conflict") ? 2 : 1
            radius: 4
            clip: true

            ScrollView {
                id: logScroll
                anchors.fill: parent
                anchors.margins: 8
                TextArea {
                    id: syncResultArea
                    width: logScroll.availableWidth
                    text: root.backendRef ? root.backendRef.sync_action_result : ''
                    color: (root.backendRef && root.backendRef.sync_status === "conflict") ? "#ff9999" : (theme ? theme.textDim : "#A0A0A0")
                    font.family: "monospace"
                    readOnly: true
                    background: null
                    wrapMode: TextEdit.Wrap
                }
            }
        }
    }
}
