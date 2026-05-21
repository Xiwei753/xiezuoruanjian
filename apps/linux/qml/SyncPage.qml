import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    property var theme: null

    color: theme ? theme.bgDark : "#1E1E1E"

    ScrollView {
        anchors.fill: parent
        anchors.margins: 24
        contentWidth: width

        Column {
            width: Math.min(parent.width, 600)
            spacing: 24

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
                    text: backend.sync_remote_url
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
                    text: backend.sync_branch
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
                    placeholderText: backend.has_sync_token ? "已设置 (输入新 Token 以覆盖)" : "请输入 GitHub Personal Access Token"
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
                        backend.sync_remote_url = urlField.text;
                        backend.sync_branch = branchField.text;
                        if (tokenField.text.trim() !== "") {
                            backend.set_sync_token(tokenField.text.trim());
                            tokenField.text = "";
                        }
                        backend.sync_enabled = true;
                        backend.sync_backend_type = "git";
                        backend.save_sync_config();
                    }
                }

                Button {
                    text: "执行同步"
                    onClicked: {
                        backend.perform_sync();
                    }
                }

                Button {
                    text: "运行诊断"
                    onClicked: {
                        backend.perform_sync_diagnostics();
                    }
                }
            }

            Rectangle {
                width: parent.width
                height: 200
                color: theme ? theme.bgDarker : "#121212"
                border.color: theme ? theme.border : "#333333"
                radius: 4
                clip: true

                ScrollView {
                    anchors.fill: parent
                    anchors.margins: 8
                    TextArea {
                        text: backend.sync_action_result
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
