#[cfg(test)]
mod tests {
    use crate::project::{create_project, list_projects};
    use crate::workspace::create_workspace;
    use tempfile::tempdir;

    #[test]
    fn test_create_and_list_project() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let projects = list_projects(workspace_path).unwrap();
        assert_eq!(projects.len(), 0);

        let project = create_project(workspace_path, "Test Project").unwrap();
        assert_eq!(project.title, "Test Project");

        let projects = list_projects(workspace_path).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].title, "Test Project");

        let volumes = crate::volume::list_volumes(workspace_path, &project.id).unwrap();
        assert_eq!(volumes.len(), 1);
        assert_eq!(volumes[0].title, "第一卷");
    }

    #[test]
    fn test_rename_project_duplicate_title() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let project1 = create_project(workspace_path, "First Project").unwrap();
        let project2 = create_project(workspace_path, "Second Project").unwrap();

        // Rename the second project to the same title as the first project.
        // This should succeed because projects use UUIDs as directory names.
        crate::project::rename_project(workspace_path, &project2.id, "First Project").unwrap();

        let projects = list_projects(workspace_path).unwrap();
        assert_eq!(projects.len(), 2);

        let p1 = projects.iter().find(|p| p.id == project1.id).unwrap();
        let p2 = projects.iter().find(|p| p.id == project2.id).unwrap();

        assert_eq!(p1.title, "First Project");
        assert_eq!(p2.title, "First Project");
    }

    #[test]
    fn test_rename_project_not_found() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let result = crate::project::rename_project(workspace_path, "non-existent-id", "New Title");
        assert!(matches!(result, Err(crate::error::Error::ProjectNotFound)));
    }
}

#[cfg(test)]
mod tests_facade {
    use crate::facade::WriterCore;
    use tempfile::tempdir;

    #[test]
    fn test_facade_create_project_updates_workspace_tree() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        let core = WriterCore::new(workspace_path);
        core.create_workspace().unwrap();

        let tree_before = core.list_projects().unwrap();
        assert_eq!(tree_before.len(), 0, "Tree should be empty initially");

        let _proj = core.create_project("Test Project Facade").unwrap();

        let tree_after = core.list_projects().unwrap();
        assert_eq!(
            tree_after.len(),
            1,
            "Tree should have 1 project after creation"
        );
        assert_eq!(tree_after[0].title, "Test Project Facade");
    }
}
