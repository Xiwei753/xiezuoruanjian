import re

def fix_file(filename):
    with open(filename, "r") as f:
        content = f.read()

    # Replace things like root.backendRef.setting_theme_mode with (root.backendRef ? root.backendRef.setting_theme_mode : "")
    # But only in places where it's reading a property. Let's do it manually for the few properties:
    
    # SettingsDialog.qml
    content = content.replace("root.backendRef.setting_theme_mode", "(root.backendRef ? root.backendRef.setting_theme_mode : 'system')")
    content = content.replace("root.backendRef.setting_font_size", "(root.backendRef ? root.backendRef.setting_font_size : 16)")
    content = content.replace("root.backendRef.setting_auto_save_enabled", "(root.backendRef ? root.backendRef.setting_auto_save_enabled : false)")
    
    # SyncPage.qml
    content = content.replace("root.backendRef.sync_remote_url", "(root.backendRef ? root.backendRef.sync_remote_url : '')")
    content = content.replace("root.backendRef.sync_branch", "(root.backendRef ? root.backendRef.sync_branch : '')")
    content = content.replace("root.backendRef.has_sync_token", "(root.backendRef ? root.backendRef.has_sync_token : false)")
    content = content.replace("root.backendRef.sync_action_result", "(root.backendRef ? root.backendRef.sync_action_result : '')")
    
    # Handle the assignments/method calls to ensure safety
    # In SettingsDialog:
    # root.backendRef.setting_theme_mode = currentText; -> if (root.backendRef) root.backendRef.setting_theme_mode = currentText;
    content = re.sub(r'root\.backendRef\.setting_([a-z_]+)\s*=\s*(.+);', r'if (root.backendRef) root.backendRef.setting_\1 = \2;', content)

    # In SyncPage:
    content = re.sub(r'root\.backendRef\.sync_([a-z_]+)\s*=\s*(.+);', r'if (root.backendRef) root.backendRef.sync_\1 = \2;', content)
    content = re.sub(r'root\.backendRef\.set_sync_token\((.+)\);', r'if (root.backendRef) root.backendRef.set_sync_token(\1);', content)
    content = re.sub(r'root\.backendRef\.perform_([a-z_]+)\(\);', r'if (root.backendRef) root.backendRef.perform_\1();', content)
    
    with open(filename, "w") as f:
        f.write(content)

fix_file("apps/linux/qml/SettingsDialog.qml")
fix_file("apps/linux/qml/SyncPage.qml")

