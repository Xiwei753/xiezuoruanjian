import os
import glob
import re

def fix_arrows(filepath):
    with open(filepath, "r") as f:
        content = f.read()

    # Replace specific known arrow function signatures
    content = content.replace("onItemActivated: (type, projectId, volumeId, chapterId) => {", "onItemActivated: function(type, projectId, volumeId, chapterId) {")
    content = content.replace("onCreateVolume: (projectId) => {", "onCreateVolume: function(projectId) {")
    content = content.replace("onCreateChapter: (projectId, volumeId) => {", "onCreateChapter: function(projectId, volumeId) {")
    content = content.replace("onAccepted: (title) => {", "onAccepted: function(title) {")
    content = content.replace("onClicked: (mouse) => {", "onClicked: function(mouse) {")

    with open(filepath, "w") as f:
        f.write(content)

for qml in glob.glob("apps/linux/qml/*.qml"):
    fix_arrows(qml)

