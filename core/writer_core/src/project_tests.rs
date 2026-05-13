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
}
