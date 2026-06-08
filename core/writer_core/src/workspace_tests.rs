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

    use crate::workspace::flush_recent_edits;

    #[test]
    fn test_flush_recent_edits() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        create_workspace(workspace_path).unwrap();

        // Initially no cache, flushing shouldn't create file
        flush_recent_edits(workspace_path).unwrap();
        let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");
        assert!(!recent_path.exists(), "Should not create file if no cached edits");

        // Record edit (this populates cache and may write file)
        record_recent_edit(workspace_path, "p1", "v1", "c1").unwrap();
        assert!(recent_path.exists());

        // Delete file to simulate unflushed cache
        fs::remove_file(&recent_path).unwrap();
        assert!(!recent_path.exists());

        // Flush edits to write cache back to file
        flush_recent_edits(workspace_path).unwrap();
        assert!(recent_path.exists(), "File should be re-created by flush_recent_edits");

        // Verify content
        let content = fs::read_to_string(&recent_path).unwrap();
        let edits: Vec<crate::workspace::RecentEdit> = serde_json::from_str(&content).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].project_id, "p1");
        assert_eq!(edits[0].volume_id, "v1");
        assert_eq!(edits[0].chapter_id, "c1");
    }

    #[test]
    fn test_record_recent_edit_limit_20() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        create_workspace(workspace_path).unwrap();

        for i in 1..=25 {
            let chapter_id = format!("ch_{}", i);
            record_recent_edit(workspace_path, "proj_1", "vol_1", &chapter_id).unwrap();
        }

        let edits = get_recent_edits(workspace_path).unwrap();

        assert_eq!(edits.len(), 20);

        assert_eq!(edits[0].chapter_id, "ch_25");

        assert_eq!(edits[19].chapter_id, "ch_6");
    }
}
