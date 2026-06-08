#[cfg(test)]
mod tests {
    use crate::project::{create_project, get_project_stats, list_projects};
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
    fn test_get_project_stats() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        // 1. Create a project. It automatically gets one default volume.
        let project = create_project(workspace_path, "Stats Test Project").unwrap();

        // Initially, 1 volume, 0 chapters, 0 words
        let initial_stats = get_project_stats(workspace_path, &project.id).unwrap();
        assert_eq!(initial_stats.volume_count, 1);
        assert_eq!(initial_stats.chapter_count, 0);
        assert_eq!(initial_stats.total_word_count, 0);

        // Get the default volume ID
        let volumes = crate::volume::list_volumes(workspace_path, &project.id).unwrap();
        let vol1_id = &volumes[0].id;

        // 2. Add a chapter to the default volume and save content to it
        let chap1 = crate::chapter::create_chapter(workspace_path, &project.id, vol1_id, "Chapter 1").unwrap();
        // 7 chars
        crate::chapter::save_chapter(workspace_path, &project.id, vol1_id, &chap1.id, "测试章节内容一").unwrap();

        // 3. Create a second volume
        let vol2 = crate::volume::create_volume(workspace_path, &project.id, "Volume 2").unwrap();

        // 4. Add a chapter to the second volume and save content to it
        let chap2 = crate::chapter::create_chapter(workspace_path, &project.id, &vol2.id, "Chapter 2").unwrap();
        // 12 chars
        crate::chapter::save_chapter(workspace_path, &project.id, &vol2.id, &chap2.id, "这是第二个测试章节的内容").unwrap();

        // 5. Assert the stats
        let stats = get_project_stats(workspace_path, &project.id).unwrap();
        assert_eq!(stats.volume_count, 2);
        assert_eq!(stats.chapter_count, 2);
        // "测试章节内容一" (7 chars) + "这是第二个测试章节的内容" (12 chars) = 19
        assert_eq!(stats.total_word_count, 19);
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
