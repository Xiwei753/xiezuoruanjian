#[cfg(test)]
mod tests {
    use crate::project::create_project;
    use crate::volume::{create_volume, delete_volume, list_volumes, normalize_rel_path, rename_volume, reorder_volumes};
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
    fn test_rename_volume_success() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let project = create_project(workspace_path, "Test Project").unwrap();
        let volume = create_volume(workspace_path, &project.id, "Old Title").unwrap();

        rename_volume(workspace_path, &project.id, &volume.id, "New Title").unwrap();

        let volumes = list_volumes(workspace_path, &project.id).unwrap();
        let updated_volume = volumes.iter().find(|v| v.id == volume.id).unwrap();
        assert_eq!(updated_volume.title, "New Title");
    }

    #[test]
    fn test_rename_volume_not_found() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let project = create_project(workspace_path, "Test Project").unwrap();

        let result = rename_volume(workspace_path, &project.id, "non-existent-volume-id", "New Title");
        match result {
            Err(crate::error::Error::VolumeNotFound) => (),
            _ => panic!("Expected Error::VolumeNotFound, got {:?}", result),
        }
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
        assert!(volumes.len() >= 2);

        let ordered_ids = vec![volume1.id.clone()];
        let result = reorder_volumes(workspace_path, &project.id, &ordered_ids);
        match result {
            Err(crate::error::Error::Other(msg)) => {
                assert_eq!(msg, "Invalid ordered_ids for reorder")
            }
            _ => panic!("Expected Error::Other for missing IDs"),
        }

        let mut extra_ids = volumes.iter().map(|v| v.id.clone()).collect::<Vec<_>>();
        extra_ids.push("non-existent-id".to_string());
        let result = reorder_volumes(workspace_path, &project.id, &extra_ids);
        match result {
            Err(crate::error::Error::Other(msg)) => {
                assert_eq!(msg, "Invalid ordered_ids for reorder")
            }
            _ => panic!("Expected Error::Other for extra non-existent IDs"),
        }
    }

    #[test]
    fn test_reorder_volumes_success() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let project = create_project(workspace_path, "Test Project").unwrap();

        let _volume1 = create_volume(workspace_path, &project.id, "Volume 1").unwrap();
        let _volume2 = create_volume(workspace_path, &project.id, "Volume 2").unwrap();

        let volumes = list_volumes(workspace_path, &project.id).unwrap();
        let mut ordered_ids = volumes.iter().map(|v| v.id.clone()).collect::<Vec<_>>();

        ordered_ids.reverse();

        let result = reorder_volumes(workspace_path, &project.id, &ordered_ids);
        assert!(result.is_ok());

        let new_volumes = list_volumes(workspace_path, &project.id).unwrap();
        let new_ids = new_volumes.iter().map(|v| v.id.clone()).collect::<Vec<_>>();
        assert_eq!(new_ids, ordered_ids);

        for (i, vol) in new_volumes.iter().enumerate() {
            assert_eq!(vol.order, i as i32);
        }
    }

    #[test]
    fn test_delete_volume() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let project = create_project(workspace_path, "Test Project").unwrap();
        let volume = create_volume(workspace_path, &project.id, "Test Volume").unwrap();

        let volumes_before = list_volumes(workspace_path, &project.id).unwrap();
        let count_before = volumes_before.len();

        delete_volume(workspace_path, &project.id, &volume.id).unwrap();

        let volumes_after = list_volumes(workspace_path, &project.id).unwrap();
        assert_eq!(volumes_after.len(), count_before - 1);

        let trash_dir = workspace_path.join("app-meta/sync/trash");
        assert!(std::fs::read_dir(trash_dir).unwrap().count() > 0);

        let state = crate::sync::SyncService::load_sync_state(workspace_path).unwrap();
        assert!(!state.tombstones.is_empty());
    }

    #[test]
    fn test_normalize_rel_path() {
        use std::path::Path;

        let base = Path::new("workspace/my_project");
        let path = Path::new("workspace/my_project/volumes/vol1");

        let rel = normalize_rel_path(path, base);
        assert_eq!(rel, "volumes/vol1");

        let out_of_bounds_path = Path::new("other/path/vol1");
        assert_eq!(
            normalize_rel_path(out_of_bounds_path, base),
            "other/path/vol1"
        );

        let path_with_backslash = Path::new("volumes\\vol1\\file.txt");
        let base_empty = Path::new("");
        assert_eq!(
            normalize_rel_path(path_with_backslash, base_empty),
            "volumes/vol1/file.txt"
        );
    }
}
