import re

with open("apps/linux/qml/WorkspaceTree.qml", "r") as f:
    content = f.read()

replacement = """        MenuItem {
            text: "删除"
            onTriggered: {
                if (contextMenu.itemData) {
                    if (!contextMenu.itemData.id) {
                        console.error("Missing node ID, cannot delete");
                        return;
                    }
                    root.deleteItem(contextMenu.itemData.type, contextMenu.itemData.projectIdForAction, contextMenu.itemData.volumeIdForAction, contextMenu.itemData.chapterIdForAction, contextMenu.itemData.title);
                }
            }
        }"""

pattern = r"        MenuItem {\n            text: \"删除\"\n            onTriggered: {\n                if \(contextMenu.itemData\) {\n                    root.deleteItem\(contextMenu.itemData.type, contextMenu.itemData.projectIdForAction, contextMenu.itemData.volumeIdForAction, contextMenu.itemData.chapterIdForAction, contextMenu.itemData.title\);\n                }\n            }\n        }"

content = re.sub(pattern, replacement, content)

with open("apps/linux/qml/WorkspaceTree.qml", "w") as f:
    f.write(content)

