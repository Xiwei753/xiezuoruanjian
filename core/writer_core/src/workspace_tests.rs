#[cfg(test)]
mod tests {
    use crate::workspace::{create_workspace, validate_workspace};
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
}
