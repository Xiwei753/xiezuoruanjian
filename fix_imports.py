import os
import glob
import re

qml_files = glob.glob("apps/linux/qml/*.qml")

for f in qml_files:
    with open(f, "r") as file:
        content = file.read()
    
    modified = False
    
    new_content = re.sub(r'^import QtQuick$', 'import QtQuick 2.15', content, flags=re.MULTILINE)
    if new_content != content:
        modified = True
        content = new_content
        
    new_content = re.sub(r'^import QtQuick\.Controls$', 'import QtQuick.Controls 2.15', content, flags=re.MULTILINE)
    if new_content != content:
        modified = True
        content = new_content
        
    new_content = re.sub(r'^import QtQuick\.Layouts$', 'import QtQuick.Layouts 1.15', content, flags=re.MULTILINE)
    if new_content != content:
        modified = True
        content = new_content
        
    new_content = re.sub(r'^import QtQuick\.Window$', 'import QtQuick.Window 2.15', content, flags=re.MULTILINE)
    if new_content != content:
        modified = True
        content = new_content
        
    if modified:
        with open(f, "w") as file:
            file.write(content)
        print(f"Fixed imports in {f}")

