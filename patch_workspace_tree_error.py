import re

with open("apps/linux/qml/WorkspaceTree.qml", "r") as f:
    content = f.read()

# Add a showError signal
if "signal showError(string message)" not in content:
    content = content.replace("    signal deleteItem", "    signal showError(string message)\n    signal deleteItem")

# Replace console.error with root.showError
content = content.replace('console.error("Missing node ID");', 'root.showError("删除失败：缺失节点 ID");')
content = content.replace('console.error("Missing projectId");', 'root.showError("删除失败：缺失项目 ID");')
content = content.replace('console.error("Missing projectId or volumeId");', 'root.showError("删除失败：缺失卷的归属 ID");')
content = content.replace('console.error("Missing projectId, volumeId or chapterId");', 'root.showError("删除失败：缺失章节的归属 ID");')

with open("apps/linux/qml/WorkspaceTree.qml", "w") as f:
    f.write(content)

with open("apps/linux/qml/main.qml", "r") as f:
    main_content = f.read()

# Connect showError to errorDialog
main_pattern = r"            WorkspaceTree \{\n                id: workspaceTree\n                anchors\.fill: parent"
main_replace = """            WorkspaceTree {
                id: workspaceTree
                anchors.fill: parent
                onShowError: function(msg) {
                    errorDialog.message = msg;
                    errorDialog.open();
                }"""
main_content = re.sub(main_pattern, main_replace, main_content)

with open("apps/linux/qml/main.qml", "w") as f:
    f.write(main_content)

