import re

with open("apps/linux/src/main.rs", "r") as f:
    content = f.read()

if '"qml/WorkspaceTree.qml" as "WorkspaceTree.qml",' not in content:
    content = content.replace('"qml/SidebarItem.qml" as "SidebarItem.qml",', 
        '"qml/SidebarItem.qml" as "SidebarItem.qml",\n    "qml/WorkspaceTree.qml" as "WorkspaceTree.qml",\n    "qml/CreateProjectDialog.qml" as "CreateProjectDialog.qml",')

with open("apps/linux/src/main.rs", "w") as f:
    f.write(content)
print("Updated main.rs qrc successfully")
