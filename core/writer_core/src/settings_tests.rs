#[cfg(test)]
mod tests {
    use crate::settings::{
        load_local_settings, load_syncable_settings, save_local_settings, save_syncable_settings,
    };
    use crate::workspace::create_workspace;
    use tempfile::tempdir;

    #[test]
    fn test_settings() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let mut local = load_local_settings(workspace_path).unwrap();
        local.window_width = 800.0;
        save_local_settings(workspace_path, &local).unwrap();

        let loaded_local = load_local_settings(workspace_path).unwrap();
        assert_eq!(loaded_local.window_width, 800.0);

        let mut syncable = load_syncable_settings(workspace_path).unwrap();
        syncable.font_size = 20.0;
        save_syncable_settings(workspace_path, &syncable).unwrap();

        let loaded_syncable = load_syncable_settings(workspace_path).unwrap();
        assert_eq!(loaded_syncable.font_size, 20.0);
    }
}
