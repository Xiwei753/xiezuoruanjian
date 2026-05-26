import re

with open("apps/linux/qml/SettingsDialog.qml", "r") as f:
    content = f.read()

if "signal settingsChanged()" not in content:
    content = content.replace("property var backendRef: null", "property var backendRef: null\n    signal settingsChanged()")
    content = content.replace("root.backendRef.save_local_settings();", "if(root.backendRef && root.backendRef.save_local_settings()) { root.settingsChanged(); }")

with open("apps/linux/qml/SettingsDialog.qml", "w") as f:
    f.write(content)

with open("apps/linux/qml/SyncPage.qml", "r") as f:
    content = f.read()

if "signal settingsChanged()" not in content:
    content = content.replace("property var backendRef: null", "property var backendRef: null\n    signal settingsChanged()")
    content = content.replace("root.backendRef.save_sync_config();", "if(root.backendRef && root.backendRef.save_sync_config()) { root.settingsChanged(); }")

with open("apps/linux/qml/SyncPage.qml", "w") as f:
    f.write(content)

with open("apps/linux/qml/main.qml", "r") as f:
    content = f.read()

if "onSettingsChanged:" not in content:
    content = content.replace("""    SettingsDialog {
        id: settingsDialog
        theme: theme
        backendRef: backend
    }""", """    SettingsDialog {
        id: settingsDialog
        theme: theme
        backendRef: backend
        onSettingsChanged: {
            applyState(JSON.parse(backend.refresh_app_state_json()));
        }
    }""")
    
    content = content.replace("""        SyncPage {
            anchors.fill: parent
            theme: theme
            backendRef: backend
        }""", """        SyncPage {
            anchors.fill: parent
            theme: theme
            backendRef: backend
            onSettingsChanged: {
                applyState(JSON.parse(backend.refresh_app_state_json()));
            }
        }""")

with open("apps/linux/qml/main.qml", "w") as f:
    f.write(content)
