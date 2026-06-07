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
    fn test_create_volume_empty_list() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        // Initialize project properly without using the high level create_project because that creates a default volume
        let project_id = "test_empty_project";
        let project_dir = workspace_path.join("projects").join(project_id);
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::create_dir_all(project_dir.join("volumes")).unwrap();

        // Check list_volumes is empty
        let volumes = list_volumes(workspace_path, project_id).unwrap();
        assert_eq!(volumes.len(), 0);

        let volume = create_volume(workspace_path, project_id, "First Volume").unwrap();
        assert_eq!(volume.order, 0);
        assert_eq!(volume.title, "First Volume");
    }
}
