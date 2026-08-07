#[cfg(test)]
mod tests {
    use crate::project::{create_project, list_projects};
        use tempfile::tempdir;

    #[test]
    fn test_create_and_list_project() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        std::fs::create_dir_all(workspace_path.join("projects")).unwrap();

        let projects = list_projects(&workspace_path.join("projects")).unwrap();
        assert_eq!(projects.len(), 0);

        let project = create_project(&workspace_path.join("projects"), "Test Project").unwrap();
        assert_eq!(project.title, "Test Project");

        let projects = list_projects(&workspace_path.join("projects")).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].title, "Test Project");

        let volumes = crate::volume::list_volumes(&workspace_path.join("projects").join(&project.id)).unwrap();
        assert_eq!(volumes.len(), 1);
        assert_eq!(volumes[0].title, "第一卷");
    }

    #[test]
    fn test_rename_project_duplicate_title() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        std::fs::create_dir_all(workspace_path.join("projects")).unwrap();

        let _project1 = create_project(&workspace_path.join("projects"), "Project A").unwrap();
        let project2 = create_project(&workspace_path.join("projects"), "Project B").unwrap();

        let result = crate::project::rename_project(&workspace_path.join("projects"), &project2.id, "Project A");
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
        std::fs::create_dir_all(workspace_path.join("projects")).unwrap();

        let project = create_project(&workspace_path.join("projects"), "Test Stats Empty").unwrap();

        let stats = crate::project::get_project_stats(&workspace_path.join("projects").join(&project.id)).unwrap();
        assert_eq!(stats.volume_count, 1); // create_project automatically generates a default volume
        assert_eq!(stats.chapter_count, 0);
        assert_eq!(stats.total_word_count, 0);
    }

    #[test]
    fn test_get_project_stats_with_data() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        std::fs::create_dir_all(workspace_path.join("projects")).unwrap();

        let project = create_project(&workspace_path.join("projects"), "Test Stats Data").unwrap();

        // project already has 1 default volume "第一卷"
        let volumes = crate::volume::list_volumes(&workspace_path.join("projects").join(&project.id)).unwrap();
        assert_eq!(volumes.len(), 1);
        let vol1_id = &volumes[0].id;

        // Add second volume
        let vol2 = crate::volume::create_volume(&workspace_path.join("projects").join(&project.id), "第二卷").unwrap();

        // Add chapters
        let c1 = crate::chapter::create_chapter(&workspace_path.join("projects").join(&project.id), vol1_id, "Chapter 1")
            .unwrap();
        let c2 = crate::chapter::create_chapter(&workspace_path.join("projects").join(&project.id), vol1_id, "Chapter 2")
            .unwrap();
        let c3 = crate::chapter::create_chapter(&workspace_path.join("projects").join(&project.id), &vol2.id, "Chapter 3")
            .unwrap();

        // Write content
        let content1 = "This is a test content.";
        crate::chapter::save_chapter(&workspace_path.join("projects").join(&project.id), vol1_id, &c1.id, content1)
            .unwrap();

        let content2 = "More content.";
        crate::chapter::save_chapter(&workspace_path.join("projects").join(&project.id), vol1_id, &c2.id, content2)
            .unwrap();

        let content3 = "Even more testing content.";
        crate::chapter::save_chapter(&workspace_path.join("projects").join(&project.id), &vol2.id, &c3.id, content3)
            .unwrap();

        // Calculate exact word counts using the core function for robust testing

        let stats = crate::project::get_project_stats(&workspace_path.join("projects").join(&project.id)).unwrap();
        assert_eq!(stats.volume_count, 2);
        assert_eq!(stats.chapter_count, 3);

        let expected_word_count = crate::chapter::calculate_word_count(content1)
            + crate::chapter::calculate_word_count(content2)
            + crate::chapter::calculate_word_count(content3);

        assert_eq!(stats.total_word_count, expected_word_count);
    }

    #[test]
    fn test_get_project_stats_non_existent() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        std::fs::create_dir_all(workspace_path.join("projects")).unwrap();

        let stats = crate::project::get_project_stats(&workspace_path.join("projects").join("non-existent-id")).unwrap();
        assert_eq!(stats.volume_count, 0);
        assert_eq!(stats.chapter_count, 0);
        assert_eq!(stats.total_word_count, 0);
    }

    #[test]
    fn test_reorder_projects_success_and_mismatch_error() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        std::fs::create_dir_all(workspace_path.join("projects")).unwrap();

        let _project1 = create_project(&workspace_path.join("projects"), "Project 1").unwrap();
        let _project2 = create_project(&workspace_path.join("projects"), "Project 2").unwrap();

        let projects = list_projects(&workspace_path.join("projects")).unwrap();
        assert!(projects.len() >= 2);

        let mut ordered_ids = projects.iter().map(|p| p.id.clone()).collect::<Vec<_>>();

        ordered_ids.reverse();
        let result = crate::project::reorder_projects(&workspace_path.join("projects"), &ordered_ids);
        assert!(result.is_ok());

        let new_projects = list_projects(&workspace_path.join("projects")).unwrap();
        let new_ids = new_projects
            .iter()
            .map(|p| p.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(new_ids, ordered_ids);

        let missing_ids = vec![ordered_ids[0].clone()];
        let result = crate::project::reorder_projects(&workspace_path.join("projects"), &missing_ids);
        match result {
            Err(crate::error::Error::Other(msg)) => {
                assert_eq!(msg, "Invalid ordered_ids for reorder")
            }
            _ => panic!("Expected Error::Other for missing IDs"),
        }

        let mut extra_ids = ordered_ids.clone();
        extra_ids.push("non-existent-id".to_string());
        let result = crate::project::reorder_projects(&workspace_path.join("projects"), &extra_ids);
        match result {
            Err(crate::error::Error::Other(msg)) => {
                assert_eq!(msg, "Invalid ordered_ids for reorder")
            }
            _ => panic!("Expected Error::Other for extra non-existent IDs"),
        }
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

        let core = WriterCore::new(workspace_path, workspace_path.join("projects"));
        std::fs::create_dir_all(dir.path().join("projects")).unwrap();

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
