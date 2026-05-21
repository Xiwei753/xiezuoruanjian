import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    property var theme: null
    property var backendRef: null
    signal settingsChanged()

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
                        if (root.backendRef) root.backendRef.perform_sync();
                    }
                }

                Button {
                    text: "运行诊断"
                    onClicked: {
                        if (root.backendRef) root.backendRef.perform_sync_diagnostics();
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
                        text: (root.backendRef ? root.backendRef.sync_action_result : '')
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
