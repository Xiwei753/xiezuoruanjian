#[cfg(test)]
mod tests {
    use crate::workspace::{create_workspace, validate_workspace, record_recent_edit, get_recent_edits};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_create_and_validate_workspace() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        // Initial state should be invalid
        assert!(!validate_workspace(workspace_path).unwrap());

        // Create workspace
        create_workspace(workspace_path).unwrap();

        // Should now be valid
        assert!(validate_workspace(workspace_path).unwrap());
    }

    #[test]
    fn test_record_recent_edit_corrupt_json() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        create_workspace(workspace_path).unwrap();

        let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");
        fs::write(&recent_path, "{ corrupted data ]}").unwrap();

        record_recent_edit(workspace_path, "p1", "v1", "c1").unwrap();

        let edits = get_recent_edits(workspace_path).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].project_id, "p1");
        assert_eq!(edits[0].volume_id, "v1");
        assert_eq!(edits[0].chapter_id, "c1");
    }
}
