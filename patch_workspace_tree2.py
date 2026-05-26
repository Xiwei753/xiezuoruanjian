import re

with open("apps/linux/qml/WorkspaceTree.qml", "r") as f:
    content = f.read()

# Replace renameItem and deleteItem triggers
old_rename = "root.renameItem(contextMenu.itemData.type, contextMenu.itemData.projectId, contextMenu.itemData.volumeId, contextMenu.itemData.id, contextMenu.itemData.title);"
new_rename = """var data = contextMenu.itemData;
                    if (!data || !data.id) { console.error("Missing node ID"); return; }
                    root.renameItem(data.type, data.projectIdForAction, data.volumeIdForAction, data.chapterIdForAction, data.title);"""

old_delete = "root.deleteItem(contextMenu.itemData.type, contextMenu.itemData.projectId, contextMenu.itemData.volumeId, contextMenu.itemData.id, contextMenu.itemData.title);"
new_delete = """var data = contextMenu.itemData;
                    if (!data || !data.id) { console.error("Missing node ID"); return; }
                    root.deleteItem(data.type, data.projectIdForAction, data.volumeIdForAction, data.chapterIdForAction, data.title);"""

content = content.replace(old_rename, new_rename)
content = content.replace(old_delete, new_delete)

with open("apps/linux/qml/WorkspaceTree.qml", "w") as f:
    f.write(content)

