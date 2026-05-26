import re

with open("apps/linux/qml/main.qml", "r") as f:
    content = f.read()

content = content.replace("Math.min(ApplicationWindow.window.width", "Math.min(mainWindow.width")
content = content.replace("Math.min(ApplicationWindow.window.height", "Math.min(mainWindow.height")

with open("apps/linux/qml/main.qml", "w") as f:
    f.write(content)
