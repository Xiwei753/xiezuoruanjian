import re

with open("AGENTS.md", "r") as f:
    text = f.read()

text = text.replace("apps/desktop/src/sujian_editor.rs", "apps/desktop/src/sujian_editor_item/mod.rs")
text = text.replace("sujian_editor.rs", "sujian_editor_item/mod.rs")

with open("AGENTS.md", "w") as f:
    f.write(text)

with open("docs/editor_engine_route.md", "r") as f:
    text = f.read()

text = text.replace("apps/desktop/src/sujian_editor.rs", "apps/desktop/src/sujian_editor_item/mod.rs")
text = text.replace("sujian_editor.rs", "sujian_editor_item/mod.rs")

with open("docs/editor_engine_route.md", "w") as f:
    f.write(text)
