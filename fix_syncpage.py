import re

with open("apps/linux/qml/SyncPage.qml", "r") as f:
    content = f.read()

# Add Connections block to update syncResultArea.text
connections_code = """
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
"""

content = content.replace("id: root", "id: root" + connections_code)
content = content.replace("TextArea {", "TextArea {\n                        id: syncResultArea")
content = content.replace("text: (root.backendRef ? root.backendRef.sync_action_result : '')", "text: root.backendRef ? root.backendRef.sync_action_result : ''")

# Add feedback to buttons
content = content.replace("""                    onClicked: {
                        if (root.backendRef) root.backendRef.perform_sync();
                    }""", """                    onClicked: {
                        syncResultArea.text = "正在同步...";
                        if (root.backendRef) root.backendRef.perform_sync();
                    }""")
content = content.replace("""                    onClicked: {
                        if (root.backendRef) root.backendRef.perform_sync_diagnostics();
                    }""", """                    onClicked: {
                        syncResultArea.text = "正在诊断...";
                        if (root.backendRef) root.backendRef.perform_sync_diagnostics();
                    }""")

with open("apps/linux/qml/SyncPage.qml", "w") as f:
    f.write(content)

