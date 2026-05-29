import re

with open('apps/linux/src/main.rs', 'r') as f:
    lines = f.readlines()

api_methods_no_change = [
    "list_projects", "create_project", "rename_project", "delete_project", "reorder_projects",
    "list_volumes", "create_volume", "rename_volume", "delete_volume", "reorder_volumes",
    "list_chapters", "create_chapter", "rename_chapter", "delete_chapter", "reorder_chapters",
    "open_chapter",
    "load_local_settings", "save_local_settings",
    "load_syncable_settings", "save_syncable_settings",
    "load_sync_config", "save_sync_config",
    "load_sync_secrets", "save_sync_secrets",
    "load_sync_state",
    "perform_sync_diagnostics", "perform_sync_dry_run", "perform_sync",
    "validate_workspace", "create_workspace",
]

in_facade_block = False
block_start_idx = -1

for i in range(len(lines)):
    line = lines[i]
    if "if let Some(core) = self.core_facade() {" in line:
        in_facade_block = True
        block_start_idx = i
        continue
    
    if in_facade_block:
        # Check if it contains any of the methods
        if any(f"core.{m}(" in line for m in api_methods_no_change):
            # Replace core_facade with core_api
            lines[block_start_idx] = lines[block_start_idx].replace("self.core_facade()", "self.core_api()")
            in_facade_block = False
        
        # also for save_chapter_content
        elif "core.write_chapter_verified(" in line:
            lines[block_start_idx] = lines[block_start_idx].replace("self.core_facade()", "self.core_api()")
            lines[i] = line.replace("core.write_chapter_verified(", "core.save_chapter_content(")
            in_facade_block = False
            
        # clear_chapter_content_verified
        elif "core.clear_chapter_content_verified(" in line:
            lines[block_start_idx] = lines[block_start_idx].replace("self.core_facade()", "self.core_api()")
            lines[i] = line.replace("core.clear_chapter_content_verified(", "core.clear_chapter_content(")
            in_facade_block = False
            
        elif "}" in line and line.strip() == "}":
            in_facade_block = False

with open('apps/linux/src/main.rs', 'w') as f:
    f.writelines(lines)

