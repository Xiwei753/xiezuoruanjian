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

        let _project1 = create_project(workspace_path, "Project A").unwrap();
        let project2 = create_project(workspace_path, "Project B").unwrap();

        let result = crate::project::rename_project(workspace_path, &project2.id, "Project A");
        assert!(result.is_err());
        if let Err(crate::error::Error::Other(msg)) = result {
            assert_eq!(msg, "Project title already exists");
        } else {
            panic!("Expected Error::Other(\"Project title already exists\")");
        }
    }

    #[test]
    fn test_get_project_stats_empty() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let project = create_project(workspace_path, "Test Stats Empty").unwrap();

        let stats = crate::project::get_project_stats(workspace_path, &project.id).unwrap();
        assert_eq!(stats.volume_count, 1); // create_project automatically generates a default volume
        assert_eq!(stats.chapter_count, 0);
        assert_eq!(stats.total_word_count, 0);
    }

    #[test]
    fn test_get_project_stats_with_data() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let project = create_project(workspace_path, "Test Stats Data").unwrap();

        // project already has 1 default volume "第一卷"
        let volumes = crate::volume::list_volumes(workspace_path, &project.id).unwrap();
        assert_eq!(volumes.len(), 1);
        let vol1_id = &volumes[0].id;

        // Add second volume
        let vol2 = crate::volume::create_volume(workspace_path, &project.id, "第二卷").unwrap();

        // Add chapters
        let c1 = crate::chapter::create_chapter(workspace_path, &project.id, vol1_id, "Chapter 1").unwrap();
        let c2 = crate::chapter::create_chapter(workspace_path, &project.id, vol1_id, "Chapter 2").unwrap();
        let c3 = crate::chapter::create_chapter(workspace_path, &project.id, &vol2.id, "Chapter 3").unwrap();

        // Write content
        let content1 = "This is a test content.";
        crate::chapter::save_chapter(workspace_path, &project.id, vol1_id, &c1.id, content1).unwrap();

        let content2 = "More content.";
        crate::chapter::save_chapter(workspace_path, &project.id, vol1_id, &c2.id, content2).unwrap();

        let content3 = "Even more testing content.";
        crate::chapter::save_chapter(workspace_path, &project.id, &vol2.id, &c3.id, content3).unwrap();

        // Calculate exact word counts using the core function for robust testing

        let stats = crate::project::get_project_stats(workspace_path, &project.id).unwrap();
        assert_eq!(stats.volume_count, 2);
        assert_eq!(stats.chapter_count, 3);

        let expected_word_count = crate::chapter::calculate_word_count(content1) +
                                  crate::chapter::calculate_word_count(content2) +
                                  crate::chapter::calculate_word_count(content3);

        assert_eq!(stats.total_word_count, expected_word_count);
    }

    #[test]
    fn test_get_project_stats_non_existent() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let stats = crate::project::get_project_stats(workspace_path, "non-existent-id").unwrap();
        assert_eq!(stats.volume_count, 0);
        assert_eq!(stats.chapter_count, 0);
        assert_eq!(stats.total_word_count, 0);
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
