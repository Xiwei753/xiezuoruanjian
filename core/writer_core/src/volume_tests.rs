#[cfg(test)]
mod tests {
    use crate::project::create_project;
    use crate::volume::{create_volume, list_volumes, rename_volume};
    use crate::workspace::create_workspace;
    use std::fs;
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
    fn test_rename_volume_missing_meta() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let project = create_project(workspace_path, "Test Project").unwrap();
        let volume = create_volume(workspace_path, &project.id, "Test Volume").unwrap();

        let volume_dir = workspace_path
            .join("projects")
            .join(&project.id)
            .join("volumes")
            .join(&volume.id);
        let meta_path = volume_dir.join("volume.json");

        // Remove the meta file manually to simulate missing file
        fs::remove_file(&meta_path).unwrap();

        let result = rename_volume(workspace_path, &project.id, &volume.id, "New Title");
        match result {
            Err(crate::error::Error::VolumeNotFound) => {}
            _ => panic!("Expected VolumeNotFound error, got {:?}", result),
        }
    }
}
