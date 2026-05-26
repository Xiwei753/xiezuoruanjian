import re

with open("apps/linux/qml/WorkspaceTree.qml", "r") as f:
    content = f.read()

replacement = """var data = contextMenu.itemData;
                    if (!data) return;
                    if (data.type === "project" && !data.projectIdForAction) { console.error("Missing projectId"); return; }
                    if (data.type === "volume" && (!data.projectIdForAction || !data.volumeIdForAction)) { console.error("Missing projectId or volumeId"); return; }
                    if (data.type === "chapter" && (!data.projectIdForAction || !data.volumeIdForAction || !data.chapterIdForAction)) { console.error("Missing projectId, volumeId or chapterId"); return; }
                    root.deleteItem(data.type, data.projectIdForAction, data.volumeIdForAction, data.chapterIdForAction, data.title);"""

pattern = r"var data = contextMenu\.itemData;\n                    if \(\!data \|\| \!data\.id\) \{ console\.error\(\"Missing node ID\"\); return; \}\n                    root\.deleteItem\(data\.type, data\.projectIdForAction, data\.volumeIdForAction, data\.chapterIdForAction, data\.title\);"

content = re.sub(pattern, replacement, content)

with open("apps/linux/qml/WorkspaceTree.qml", "w") as f:
    f.write(content)
