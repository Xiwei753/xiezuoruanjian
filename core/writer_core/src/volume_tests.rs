#[cfg(test)]
mod tests {
    use crate::project::create_project;
    use crate::volume::{create_volume, list_volumes, reorder_volumes};
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
    fn test_reorder_volumes_mismatch_error() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let project = create_project(workspace_path, "Test Project").unwrap();

        let volume1 = create_volume(workspace_path, &project.id, "Volume 1").unwrap();
        let _volume2 = create_volume(workspace_path, &project.id, "Volume 2").unwrap();

        let volumes = list_volumes(workspace_path, &project.id).unwrap();
        // Since project creation might create a default volume, let's verify length
        assert!(volumes.len() >= 2);

        // Try missing IDs
        let ordered_ids = vec![volume1.id.clone()];
        let result = reorder_volumes(workspace_path, &project.id, &ordered_ids);
        match result {
            Err(crate::error::Error::Other(msg)) => assert_eq!(msg, "Invalid ordered_ids for reorder"),
            _ => panic!("Expected Error::Other for missing IDs"),
        }

        // Try extra non-existent IDs
        let mut extra_ids = volumes.iter().map(|v| v.id.clone()).collect::<Vec<_>>();
        extra_ids.push("non-existent-id".to_string());
        let result = reorder_volumes(workspace_path, &project.id, &extra_ids);
        match result {
            Err(crate::error::Error::Other(msg)) => assert_eq!(msg, "Invalid ordered_ids for reorder"),
            _ => panic!("Expected Error::Other for extra non-existent IDs"),
        }
    }
}
