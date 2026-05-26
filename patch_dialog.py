with open("apps/linux/qml/main.qml", "r") as f:
    content = f.read()

content = content.replace('        id: syncPageDialog\n        title: "同步设置"\n', '        id: syncPageDialog\n')

with open("apps/linux/qml/main.qml", "w") as f:
    f.write(content)

