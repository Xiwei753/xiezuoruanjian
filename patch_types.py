import re

with open('core/writer_core/src/api/types.rs', 'r') as f:
    content = f.read()

# Add editor_line_spacing_multiplier to LocalSettingsDto
content = content.replace(
    "pub editor_font_size: f32,",
    "pub editor_font_size: f32,\n    pub editor_line_spacing_multiplier: f32,"
)

content = content.replace(
    "editor_font_size: s.editor_font_size,",
    "editor_font_size: s.editor_font_size,\n            editor_line_spacing_multiplier: s.editor_line_spacing_multiplier,"
)

# Replace #[derive(Debug, Clone)] with #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)] for all DTOs
# Or just add serde::Serialize to all of them.
content = re.sub(
    r'#\[derive\(Debug, Clone\)\]',
    r'#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]',
    content
)

with open('core/writer_core/src/api/types.rs', 'w') as f:
    f.write(content)
