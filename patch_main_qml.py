import re

with open("apps/linux/qml/main.qml", "r") as f:
    content = f.read()

content = content.replace("width: 720\n        height: 560", "width: Math.min(ApplicationWindow.window.width - 80, 720)\n        height: Math.min(ApplicationWindow.window.height - 120, 560)")

with open("apps/linux/qml/main.qml", "w") as f:
    f.write(content)
