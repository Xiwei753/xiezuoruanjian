import os

with open("apps/linux/qml/SyncPage.qml", "r") as f:
    content = f.read()

content = content.replace(
"""                    onClicked: {
                        if
                        if (root.backendRef) root.backendRef.perform_sync();
                    }""",
"""                    onClicked: {
                        if (root.backendRef) root.backendRef.perform_sync();
                    }"""
)

with open("apps/linux/qml/SyncPage.qml", "w") as f:
    f.write(content)
