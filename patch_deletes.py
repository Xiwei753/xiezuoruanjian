import re

def insert_validation(filepath, func_name, args_to_check):
    with open(filepath, "r") as f:
        content = f.read()
    
    validation_code = ""
    for arg in args_to_check:
        validation_code += f"""
    let {arg} = {arg}.trim();
    if {arg}.is_empty() || {arg}.contains("..") || {arg}.contains("/") || {arg}.contains("\\\\") {{
        return Err(crate::error::Error::Other(format!("Invalid parameter: {{}}", {arg})));
    }}
"""
    
    pattern = f"pub fn {func_name}.*?\\) -> Result<\\(\\)> {{"
    match = re.search(pattern, content, re.DOTALL)
    if match:
        start_idx = match.end()
        new_content = content[:start_idx] + validation_code + content[start_idx:]
        with open(filepath, "w") as f:
            f.write(new_content)
        print(f"Patched {func_name} in {filepath}")

insert_validation("core/writer_core/src/project.rs", "delete_project", ["project_id"])
insert_validation("core/writer_core/src/volume.rs", "delete_volume", ["project_id", "volume_id"])
insert_validation("core/writer_core/src/chapter.rs", "delete_chapter", ["project_id", "volume_id", "chapter_id"])

