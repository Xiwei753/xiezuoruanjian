import re

# Fix types.rs duplicate field
with open('core/writer_core/src/api/types.rs', 'r') as f:
    content = f.read()

content = content.replace("editor_line_spacing_multiplier: 1.5,", "")

with open('core/writer_core/src/api/types.rs', 'w') as f:
    f.write(content)

# Update api.udl to add editor_line_spacing_multiplier
with open('core/writer_core/src/api.udl', 'r') as f:
    udl = f.read()

udl = udl.replace(
    "f32 editor_font_size;",
    "f32 editor_font_size;\n    f32 editor_line_spacing_multiplier;"
)

with open('core/writer_core/src/api.udl', 'w') as f:
    f.write(udl)

