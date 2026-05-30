import re

with open('apps/linux/tests/qml_static_check.rs', 'r') as f:
    content = f.read()

# Revert forbidden_bindings
content = re.sub(r'let forbidden_bindings = \[.*?\];', 'let forbidden_bindings = ["dt ? dt.editorText : \\"#2C2E36\\"", "dt ? dt.editorText : \\"#2c2e36\\""];', content)

# Add check specifically for the 3 files
check_code = r'''
                    if file_name == "TopWritingToolbar.qml" || file_name == "WorkspaceTree.qml" || file_name == "WritingWorkspace.qml" {
                        for color in &forbidden_colors {
                            let c = color.to_lowercase();
                            if lower.contains(&format!("\"{}\"", c)) || lower.contains(&format!("'{}'", c)) || lower.contains(&c) {
                                eprintln!("{}:{}: Found hardcoded dark fallback color '{}'", file_name, line_num, color);
                                has_errors = true;
                            }
                        }
                    }
'''

with open('apps/linux/tests/qml_static_check.rs', 'w') as f:
    f.write(content)
