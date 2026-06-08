#[cfg(test)]
mod tests {
    use crate::workspace::{create_workspace, validate_workspace, record_recent_edit, get_recent_edits, flush_recent_edits};
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

    #[test]
    fn test_flush_recent_edits() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        create_workspace(workspace_path).unwrap();

        // Populate in-memory cache and write to disk
        record_recent_edit(workspace_path, "proj_1", "vol_1", "ch_1").unwrap();

        let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");

        // Assert file exists initially
        assert!(recent_path.exists());

        // Delete file to simulate desync between cache and disk
        fs::remove_file(&recent_path).unwrap();
        assert!(!recent_path.exists());

        // Flush cache to disk
        flush_recent_edits(workspace_path).unwrap();

        // Assert file exists again
        assert!(recent_path.exists());

        // Verify contents from memory cache
        let edits = get_recent_edits(workspace_path).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].project_id, "proj_1");
        assert_eq!(edits[0].volume_id, "vol_1");
        assert_eq!(edits[0].chapter_id, "ch_1");

        // Verify that flush actually wrote the expected content to disk
        let file_content = fs::read_to_string(&recent_path).unwrap();
        let parsed_edits: Vec<serde_json::Value> = serde_json::from_str(&file_content).unwrap();
        assert_eq!(parsed_edits.len(), 1);
        assert_eq!(parsed_edits[0]["project_id"], "proj_1");
        assert_eq!(parsed_edits[0]["volume_id"], "vol_1");
        assert_eq!(parsed_edits[0]["chapter_id"], "ch_1");
        assert!(parsed_edits[0]["timestamp"].is_string());
    }

    #[test]
    fn test_flush_recent_edits_empty_cache() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        create_workspace(workspace_path).unwrap();

        // Flush cache to disk without any prior edits
        flush_recent_edits(workspace_path).unwrap();

        let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");

        // Assert file is not created
        assert!(!recent_path.exists());
    }
}
