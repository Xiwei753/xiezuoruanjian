#[cfg(test)]
mod tests {
    use crate::workspace::{create_workspace, validate_workspace, record_recent_edit, get_recent_edits, flush_recent_edits};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_create_and_validate_workspace() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        // Initial state should be invalid
        assert!(!validate_workspace(workspace_path).unwrap());

        // Create workspace
        create_workspace(workspace_path).unwrap();

        // Should now be valid
        assert!(validate_workspace(workspace_path).unwrap());
    }

    #[test]
    fn test_record_recent_edit_corrupt_json() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        create_workspace(workspace_path).unwrap();

        let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");
        fs::write(&recent_path, "{ corrupted data ]}").unwrap();

        record_recent_edit(workspace_path, "p1", "v1", "c1").unwrap();

        let edits = get_recent_edits(workspace_path).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].project_id, "p1");
        assert_eq!(edits[0].volume_id, "v1");
        assert_eq!(edits[0].chapter_id, "c1");
    }

    #[test]
    fn test_flush_recent_edits() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        create_workspace(workspace_path).unwrap();

        // Initially no cache, flushing shouldn't create file
        flush_recent_edits(workspace_path).unwrap();
        let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");
        assert!(!recent_path.exists(), "Should not create file if no cached edits");

        // Record edit (this populates cache and may write file depending on global debounce)
        record_recent_edit(workspace_path, "p1", "v1", "c1").unwrap();

        // Delete file to simulate unflushed cache, if the debounce happened to write it
        if recent_path.exists() {
            fs::remove_file(&recent_path).unwrap();
        }
        assert!(!recent_path.exists());

        // Flush edits to write cache back to file
        flush_recent_edits(workspace_path).unwrap();
        assert!(recent_path.exists(), "File should be re-created by flush_recent_edits");

        // Verify content
        let content = fs::read_to_string(&recent_path).unwrap();
        let edits: Vec<crate::workspace::RecentEdit> = serde_json::from_str(&content).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].project_id, "p1");
        assert_eq!(edits[0].volume_id, "v1");
        assert_eq!(edits[0].chapter_id, "c1");
    }

    #[test]
    fn test_record_recent_edit_limit_20() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        create_workspace(workspace_path).unwrap();

        for i in 1..=25 {
            let chapter_id = format!("ch_{}", i);
            record_recent_edit(workspace_path, "proj_1", "vol_1", &chapter_id).unwrap();
        }

        let edits = get_recent_edits(workspace_path).unwrap();

        assert_eq!(edits.len(), 20);

        assert_eq!(edits[0].chapter_id, "ch_25");

        assert_eq!(edits[19].chapter_id, "ch_6");
    }

    #[test]
    fn test_flush_recent_edits_manual_cache() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        create_workspace(workspace_path).unwrap();

        // Populate in-memory cache
        // Instead of calling record_recent_edit which uses debounced write,
        // we manually push to cache to simulate memory-only state.
        {
            let edits = get_recent_edits(workspace_path).unwrap(); // initializes cache
            assert!(edits.is_empty());
        }

        {
            let mutex = crate::workspace::RECENT_EDITS_CACHE.get().unwrap();
            let mut cache = mutex.lock().unwrap();
            cache.insert(
                workspace_path.to_path_buf(),
                vec![crate::workspace::RecentEdit {
                    project_id: "proj_1".to_string(),
                    volume_id: "vol_1".to_string(),
                    chapter_id: "ch_1".to_string(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                }],
            );
        }

        let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");
        if recent_path.exists() {
            fs::remove_file(&recent_path).unwrap();
        }

        // Flush cache to disk
        flush_recent_edits(workspace_path).unwrap();

        // Assert file exists
        assert!(recent_path.exists());

        // Verify contents
        let edits = get_recent_edits(workspace_path).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].project_id, "proj_1");
        assert_eq!(edits[0].volume_id, "vol_1");
        assert_eq!(edits[0].chapter_id, "ch_1");
    }

    #[test]
    fn test_flush_recent_edits_empty_cache() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");
        assert!(!recent_path.exists());

        // Flushing when cache has nothing for this workspace shouldn't fail
        flush_recent_edits(workspace_path).unwrap();

        // And it shouldn't create the file if it had no data
        assert!(!recent_path.exists());
    }

    #[test]
    fn test_flush_recent_edits_creates_directory() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        // DO NOT call create_workspace so the directory structure is missing
        let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");
        assert!(!recent_path.exists());
        assert!(!workspace_path.join("app-meta/settings").exists());

        // Populate in-memory cache directly
        {
            let _ = get_recent_edits(workspace_path); // initializes cache
            let mutex = crate::workspace::RECENT_EDITS_CACHE.get().unwrap();
            let mut cache = mutex.lock().unwrap();
            cache.insert(
                workspace_path.to_path_buf(),
                vec![crate::workspace::RecentEdit {
                    project_id: "proj_1".to_string(),
                    volume_id: "vol_1".to_string(),
                    chapter_id: "ch_1".to_string(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                }],
            );
        }

        assert!(!recent_path.exists());
        assert!(!workspace_path.join("app-meta/settings").exists());

        // Now flush cache to disk. It should create the missing directories.
        flush_recent_edits(workspace_path).unwrap();

        // Assert file exists
        assert!(recent_path.exists());

        // Verify contents from memory cache
        let edits = get_recent_edits(workspace_path).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].project_id, "proj_1");
        assert_eq!(edits[0].volume_id, "vol_1");
        assert_eq!(edits[0].chapter_id, "ch_1");

        // Verify that flush actually wrote the expected content to disk
        let file_content = fs::read_to_string(&recent_path).unwrap();
        let parsed_edits: Vec<serde_json::Value> = serde_json::from_str(&file_content).unwrap();
        assert_eq!(parsed_edits.len(), 1);
        assert_eq!(parsed_edits[0]["project_id"], "proj_1");
        assert_eq!(parsed_edits[0]["volume_id"], "vol_1");
        assert_eq!(parsed_edits[0]["chapter_id"], "ch_1");
        assert!(parsed_edits[0]["timestamp"].is_string());
    }

    #[test]
    fn test_global_workspace_apis() {
        let dir = tempdir().unwrap();
        let path_str = dir.path().to_string_lossy().into_owned();

        // 1. init_workspace should succeed and create default project/volume/chapter
        crate::init_workspace(path_str.clone()).unwrap();
        assert!(crate::workspace::validate_workspace(dir.path()).unwrap());

        // Check default project was created
        let projects = crate::project::list_projects(dir.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].title, "示例作品");

        // 2. load_workspace_summary should return correct values
        let summary = crate::load_workspace_summary(path_str.clone()).unwrap();
        assert_eq!(summary.path, path_str);
        assert!(summary.is_valid);
        assert_eq!(summary.projects.len(), 1);
        assert_eq!(summary.projects[0].title, "示例作品");

        // 3. open_workspace should succeed and return WriterAppService
        let service = crate::open_workspace(path_str.clone()).unwrap();
        let service_projects = service.list_projects().unwrap();
        assert_eq!(service_projects.len(), 1);

        // 4. create_project_in_workspace should succeed
        let new_proj = crate::create_project_in_workspace(path_str.clone(), "我的新作品".to_string()).unwrap();
        assert_eq!(new_proj.title, "我的新作品");

        let service_projects_after = service.list_projects().unwrap();
        assert_eq!(service_projects_after.len(), 2);

        // 5. repair_workspace should succeed
        // Let's delete a folder (like backups) and see if repair recreates it
        let backups_dir = dir.path().join("backups");
        fs::remove_dir(&backups_dir).unwrap();
        assert!(!backups_dir.exists());

        crate::repair_workspace(path_str.clone()).unwrap();
        assert!(backups_dir.exists());
    }

    #[test]
    fn test_create_chapter_in_project() {
        let dir = tempdir().unwrap();
        let path_str = dir.path().to_string_lossy().into_owned();
        crate::init_workspace(path_str.clone()).unwrap();

        let service = crate::open_workspace(path_str.clone()).unwrap();
        let projects = service.list_projects().unwrap();
        let project_id = &projects[0].id;

        let chapter = service.create_chapter_in_project(project_id.clone(), "新章：起锚".to_string()).unwrap();
        assert_eq!(chapter.title, "新章：起锚");

        // Verify it was actually created in the project's first volume
        let volumes = service.list_volumes(project_id.clone()).unwrap();
        let chapters = service.list_chapters(project_id.clone(), volumes[0].id.clone()).unwrap();
        // The first volume should now have 2 chapters (the default "第一章：新的起点" and the new "新章：起锚")
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[1].title, "新章：起锚");
    }
}
