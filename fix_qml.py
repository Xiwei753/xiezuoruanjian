import re

with open("apps/linux/qml/SettingsDialog.qml", "r") as f:
    content = f.read()

content = content.replace("property var theme: null", "property var theme: null\n    property var backendRef: null")
content = content.replace("backend.", "root.backendRef.")

with open("apps/linux/qml/SettingsDialog.qml", "w") as f:
    f.write(content)


with open("apps/linux/qml/SyncPage.qml", "r") as f:
    content = f.read()

content = content.replace("property var theme: null", "property var theme: null\n    property var backendRef: null")
content = content.replace("backend.", "root.backendRef.")

with open("apps/linux/qml/SyncPage.qml", "w") as f:
    f.write(content)


with open("apps/linux/qml/main.qml", "r") as f:
    content = f.read()

content = content.replace("""    SettingsDialog {
        id: settingsDialog
        theme: theme
    }""", """    SettingsDialog {
        id: settingsDialog
        theme: theme
        backendRef: backend
    }""")

content = content.replace("""        SyncPage {
            anchors.fill: parent
            theme: theme
        }""", """        SyncPage {
            anchors.fill: parent
            theme: theme
            backendRef: backend
        }""")

content = content.replace("""        EmptyWorkspace {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: !appState.hasWorkspace
            onCreateWorkspace: {
                backend.create_new_workspace();
                applyState(JSON.parse(backend.refresh_app_state_json()));
            }
            onOpenWorkspace: {
                backend.open_existing_workspace();
                applyState(JSON.parse(backend.refresh_app_state_json()));
            }
        }""", """        EmptyWorkspace {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: !appState.hasWorkspace
            backendRef: backend
            appTheme: theme
            onCreateWorkspace: {
                backend.create_new_workspace();
                applyState(JSON.parse(backend.refresh_app_state_json()));
            }
            onOpenWorkspace: {
                backend.open_existing_workspace();
                applyState(JSON.parse(backend.refresh_app_state_json()));
            }
        }""")


with open("apps/linux/qml/main.qml", "w") as f:
    f.write(content)


with open("apps/linux/qml/WorkspaceTree.qml", "r") as f:
    content = f.read()

new_item_data = """                property var itemData: {
                    var out = {
                        "id": model.id || "",
                        "type": model.type || "",
                        "title": model.title || "",
                        "projectId": model.projectId || "",
                        "volumeId": model.volumeId || ""
                    };
                    if (out.type === "project") {
                        out.projectIdForAction = out.id;
                        out.volumeIdForAction = "";
                        out.chapterIdForAction = "";
                    } else if (out.type === "volume") {
                        out.projectIdForAction = out.projectId;
                        out.volumeIdForAction = out.id;
                        out.chapterIdForAction = "";
                    } else if (out.type === "chapter") {
                        out.projectIdForAction = out.projectId;
                        out.volumeIdForAction = out.volumeId;
                        out.chapterIdForAction = out.id;
                    }
                    return out;
                }"""
content = re.sub(r'property var itemData: \{\s*return \{\s*"id": model\.id \|\| "",\s*"type": model\.type \|\| "",\s*"title": model\.title \|\| "",\s*"projectId": model\.projectId \|\| "",\s*"volumeId": model\.volumeId \|\| ""\s*\}\s*\}', new_item_data, content)

content = content.replace("root.itemActivated(model.type, model.projectId, model.volumeId, model.id);", "root.itemActivated(delegateRect.itemData.type, delegateRect.itemData.projectIdForAction, delegateRect.itemData.volumeIdForAction, delegateRect.itemData.chapterIdForAction);")

with open("apps/linux/qml/WorkspaceTree.qml", "w") as f:
    f.write(content)

