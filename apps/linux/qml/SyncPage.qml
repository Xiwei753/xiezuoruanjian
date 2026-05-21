import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import QtQuick.Window 2.15

Rectangle {
    id: root
    implicitHeight: mainCol.implicitHeight + 32
    Connections {
        target: root.backendRef
        function onSync_action_completed() {
            if (root.backendRef) {
                syncResultArea.text = root.backendRef.sync_action_result;
            }
        }
        function onSync_status_changed() {
            if (root.backendRef) {
                syncResultArea.text = root.backendRef.sync_action_result;
            }
        }
    }

    property var theme: null
    property var backendRef: null
    signal settingsChanged()

    color: theme ? theme.bgDark : "#1E1E1E"

    ScrollView {
        id: syncScroll
        anchors.fill: parent
        anchors.margins: 16
        contentWidth: availableWidth
        contentHeight: mainCol.implicitHeight
        clip: true

        Column {
            id: mainCol
            width: syncScroll.availableWidth
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
                        if (!root.backendRef) return;
                        root.backendRef.sync_remote_url = urlField.text;
                        root.backendRef.sync_branch = branchField.text.length > 0 ? branchField.text : "main";
                        if (tokenField.text.trim().length > 0) {
                            root.backendRef.set_sync_token(tokenField.text.trim());
                            tokenField.text = "";
                        }
                        root.backendRef.sync_enabled = true;
                        root.backendRef.sync_backend_type = "git";
                        if (root.backendRef.save_sync_config()) root.settingsChanged();
                    }
                }

                Button {
                    text: "执行同步"
                    onClicked: {
                        syncResultArea.text = "正在同步...";
                        if (root.backendRef) root.backendRef.perform_sync();
                    }
                }

                Button {
                    text: "运行诊断"
                    onClicked: {
                        syncResultArea.text = "正在诊断...";
                        if (root.backendRef) root.backendRef.perform_sync_diagnostics();
                    }
                }
            }

            Rectangle {
                width: parent.width
                height: Math.max(120, Math.min(200, (Window.window ? Window.window.height : 768) - 500))
                color: theme ? theme.bgDarker : "#121212"
                border.color: theme ? theme.border : "#333333"
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
                        color: theme ? theme.textDim : "#A0A0A0"
                        font.family: "monospace"
                        readOnly: true
                        background: null
                        wrapMode: TextEdit.Wrap
                    }
                }
            }
        }
    }
}
