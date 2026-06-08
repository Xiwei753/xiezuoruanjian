#[cfg(test)]
mod tests {
    use crate::project::create_project;
    use crate::volume::{create_volume, list_volumes};
    use crate::workspace::create_workspace;
    use tempfile::tempdir;

    #[test]
    fn test_create_and_list_volume() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let project = create_project(workspace_path, "Test Project").unwrap();

        let volumes = list_volumes(workspace_path, &project.id).unwrap();
        assert_eq!(volumes.len(), 1);

        let volume = create_volume(workspace_path, &project.id, "Test Volume").unwrap();
        assert_eq!(volume.title, "Test Volume");

        let volumes = list_volumes(workspace_path, &project.id).unwrap();
        assert_eq!(volumes.len(), 2);
    }

    #[test]
    fn test_normalize_rel_path() {
        use crate::volume::normalize_rel_path;
        use std::path::{Path, PathBuf};

        // Test standard prefix stripping
        let base = Path::new("workspace/my_project");
        let path = Path::new("workspace/my_project/volumes/vol1");

        let rel = normalize_rel_path(path, base);
        assert_eq!(rel, "volumes/vol1");

        // Test when path does not have base as prefix (strip_prefix fails)
        // Expected fallback is the full path.
        let out_of_bounds_path = Path::new("other/path/vol1");
        assert_eq!(
            normalize_rel_path(out_of_bounds_path, base),
            "other/path/vol1"
        );

        // Test backslash replacement (simulating Windows path serialization or broken paths)
        // Since PathBuf::from will preserve backslashes on Unix (as literal characters),
        // we can test the .replace("\\", "/") behavior explicitly.
        let path_with_backslash = PathBuf::from("volumes\\vol1\\file.txt");
        let base_empty = Path::new("");
        assert_eq!(
            normalize_rel_path(&path_with_backslash, base_empty),
            "volumes/vol1/file.txt"
        );
    }
}
