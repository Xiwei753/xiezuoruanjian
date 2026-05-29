import re

def process_file():
    with open('apps/linux/src/main.rs', 'r') as f:
        code = f.read()

    # Step 1: Replace all `if let Some(core_ref) = &self.core { let core = core_ref.borrow();`
    code = re.sub(
        r'if let Some\(core_ref\) = &self\.core\s*\{\s*let core = core_ref\.borrow\(\);',
        r'if let Some(core) = self.core_facade() {',
        code
    )

    # Replace specific `WriterCore` instantiation:
    code = code.replace(
        "let core = WriterCore::new(path);",
        "let core = writer_core::facade::WriterCore::new(path);"
    )
    code = code.replace(
        "let core = WriterCore::new(&path);",
        "let core = writer_core::facade::WriterCore::new(&path);"
    )
    code = code.replace(
        "let core = WriterCore::new(ws_path);",
        "let core = writer_core::facade::WriterCore::new(ws_path);"
    )

    # Step 2: Now we find functions that we want to migrate to core_api().
    # We can just do a regex replace for the specific method names.
    api_methods = [
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
        "get_mind_map_snapshot", "save_mind_map_layout",
        "validate_workspace", "create_workspace",
    ]
    
    # In `main.rs`, the calls look like `core.list_projects()`
    # If the block contains `core.<method>`, we can try to change `core_facade` to `core_api`.
    # Actually, it's easier to just do it manually for these since it's just replacing `core_facade` with `core_api`.
    
    # Wait, `get_mind_map_snapshot` in api is `get_mindmap_snapshot_json`.
    
    with open('apps/linux/src/main.rs', 'w') as f:
        f.write(code)

process_file()
