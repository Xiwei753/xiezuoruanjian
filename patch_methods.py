import re

with open("apps/linux/src/main.rs", "r") as f:
    content = f.read()

target = "    select_tree_item_json: qt_method!(fn(&mut self, item_type: QString, project_id: QString, volume_id: QString, chapter_id: QString) -> QString),\n"
methods = """    delete_project_json: qt_method!(fn(&mut self, project_id: QString) -> QString),
    delete_volume_json: qt_method!(fn(&mut self, project_id: QString, volume_id: QString) -> QString),
    delete_chapter_json: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString),
"""

if "delete_project_json: qt_method!" not in content:
    content = content.replace(target, target + methods)
    with open("apps/linux/src/main.rs", "w") as f:
        f.write(content)

