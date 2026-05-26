import re

with open("apps/linux/qml/SyncPage.qml", "r") as f:
    content = f.read()

pattern = r"    ScrollView \{\n        anchors\.fill: parent\n        anchors\.margins: 24\n        contentWidth: width\n\n        Column \{"
replacement = """    ScrollView {
        anchors.fill: parent
        anchors.margins: 24
        contentWidth: width
        contentHeight: mainCol.height

        Column {
            id: mainCol"""
            
content = re.sub(pattern, replacement, content)

with open("apps/linux/qml/SyncPage.qml", "w") as f:
    f.write(content)
