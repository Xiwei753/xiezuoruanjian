#[cfg(test)]
mod tests {
    use crate::workspace::{
        create_workspace, get_recent_edits, record_recent_edit, validate_workspace,
    };
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
    fn test_record_recent_edit_limit_20() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        // Create workspace
        create_workspace(workspace_path).unwrap();

        // Add 25 edits
        for i in 1..=25 {
            let chapter_id = format!("ch_{}", i);
            record_recent_edit(workspace_path, "proj_1", "vol_1", &chapter_id).unwrap();
        }

        // Get recent edits
        let edits = get_recent_edits(workspace_path).unwrap();

        // Should be limited to 20
        assert_eq!(edits.len(), 20);

        // The most recent one should be at the top
        assert_eq!(edits[0].chapter_id, "ch_25");
        // The oldest one kept should be ch_6 (since 1-5 were truncated)
        assert_eq!(edits[19].chapter_id, "ch_6");
    }
}
