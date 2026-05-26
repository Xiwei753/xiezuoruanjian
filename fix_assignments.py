import os

with open("apps/linux/qml/SettingsDialog.qml", "r") as f:
    content = f.read()

content = content.replace(
    "(root.backendRef ? root.backendRef.setting_theme_mode : 'system') = currentText;\n                        if(root.backendRef && root.backendRef.save_local_settings()) { root.settingsChanged(); }",
    "if (!root.backendRef) return;\n                        root.backendRef.setting_theme_mode = currentText;\n                        if (root.backendRef.save_local_settings()) root.settingsChanged();"
)

content = content.replace(
    "(root.backendRef ? root.backendRef.setting_font_size : 16) = value;\n                        if(root.backendRef && root.backendRef.save_local_settings()) { root.settingsChanged(); }",
    "if (!root.backendRef) return;\n                        root.backendRef.setting_font_size = value;\n                        if (root.backendRef.save_local_settings()) root.settingsChanged();"
)

content = content.replace(
    "(root.backendRef ? root.backendRef.setting_auto_save_enabled : false) = checked;\n                        if(root.backendRef && root.backendRef.save_local_settings()) { root.settingsChanged(); }",
    "if (!root.backendRef) return;\n                        root.backendRef.setting_auto_save_enabled = checked;\n                        if (root.backendRef.save_local_settings()) root.settingsChanged();"
)

with open("apps/linux/qml/SettingsDialog.qml", "w") as f:
    f.write(content)

with open("apps/linux/qml/SyncPage.qml", "r") as f:
    content = f.read()

old_sync_block = """                        (root.backendRef ? root.backendRef.sync_remote_url : '') = urlField.text;
                        (root.backendRef ? root.backendRef.sync_branch : '') = branchField.text;
                        if (tokenField.text.trim() !== "") {
                            if (root.backendRef) root.backendRef.set_sync_token(tokenField.text.trim());
                            tokenField.text = "";
                        }
                        if (root.backendRef) root.backendRef.sync_enabled = true;
                        if (root.backendRef) root.backendRef.sync_backend_type = "git";
                        if(root.backendRef && root.backendRef.save_sync_config()) { root.settingsChanged(); }"""

new_sync_block = """                        if (!root.backendRef) return;
                        root.backendRef.sync_remote_url = urlField.text;
                        root.backendRef.sync_branch = branchField.text.length > 0 ? branchField.text : "main";
                        if (tokenField.text.trim().length > 0) {
                            root.backendRef.set_sync_token(tokenField.text.trim());
                            tokenField.text = "";
                        }
                        root.backendRef.sync_enabled = true;
                        root.backendRef.sync_backend_type = "git";
                        if (root.backendRef.save_sync_config()) root.settingsChanged();"""

content = content.replace(old_sync_block, new_sync_block)

with open("apps/linux/qml/SyncPage.qml", "w") as f:
    f.write(content)

