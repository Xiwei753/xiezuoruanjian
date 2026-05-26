import re

with open("apps/linux/qml/main.qml", "r") as f:
    content = f.read()

# Fix rename item
rename_pattern = r"onRenameItem: function\(type, projectId, volumeId, chapterId, currentTitle\) \{.*?\} \n            \}"
rename_replace = """onRenameItem: function(type, projectId, volumeId, chapterId, currentTitle) {
                    errorDialog.message = "重命名功能尚未实现";
                    errorDialog.open();
                }
            }"""
content = re.sub(rename_pattern, rename_replace, content, flags=re.DOTALL)

# Fix syncPageDialog width/height and padding
sync_pattern = r"        id: syncPageDialog\n        modal: true\n        width: Math\.min\(window\.width - 80, 720\)\n        height: Math\.min\(window\.height - 120, 560\)\n        anchors\.centerIn: Overlay\.overlay\n        background: Rectangle \{ color: theme\.bgDark; border\.color: theme\.border;\n radius: 8; border\.width: 1 \}\n        contentItem: Item \{\n            anchors\.fill: parent\n            SyncPage \{\n                anchors\.fill: parent"

sync_replace = """        id: syncPageDialog
        modal: true
        width: Math.max(360, Math.min(window.width - 80, 720))
        height: Math.max(420, Math.min(window.height - 120, 560))
        anchors.centerIn: Overlay.overlay
        background: Rectangle { color: theme.bgDark; border.color: theme.border; radius: 8; border.width: 1 }
        padding: 16
        contentItem: Item {
            anchors.fill: parent
            SyncPage {
                anchors.fill: parent"""
                
content = re.sub(sync_pattern, sync_replace, content)

with open("apps/linux/qml/main.qml", "w") as f:
    f.write(content)

