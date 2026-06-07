#[cfg(test)]
mod tests {
    use crate::workspace::{create_workspace, validate_workspace, record_recent_edit};
    use tempfile::tempdir;
    use std::fs;
    use std::time::Duration;
    use std::thread;

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
    fn test_record_recent_edit_debounce() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");

        // The debounce timer uses a static OnceLock across all tests.
        // Wait 5.1s to ensure the 5-second window has definitely elapsed,
        // even if another test just triggered a flush.
        thread::sleep(Duration::from_millis(5100));

        // First call should flush
        record_recent_edit(workspace_path, "p1", "v1", "c1").unwrap();

        let content1 = fs::read_to_string(&recent_path).unwrap();
        assert!(content1.contains("c1"));

        // Next rapid calls should NOT flush to disk
        for i in 2..=6 {
            record_recent_edit(workspace_path, "p1", "v1", &format!("c{}", i)).unwrap();
        }

        let content2 = fs::read_to_string(&recent_path).unwrap();
        assert_eq!(content1, content2, "Disk file should not update due to debounce");

        // Wait another 5.1 seconds
        thread::sleep(Duration::from_millis(5100));

        // Call again, should flush now
        record_recent_edit(workspace_path, "p1", "v1", "c7").unwrap();

        let content3 = fs::read_to_string(&recent_path).unwrap();
        assert_ne!(content2, content3, "Disk file should update after wait");
        assert!(content3.contains("c7"));
        assert!(content3.contains("c6")); // cache should have accumulated c2-c6
    }
}
