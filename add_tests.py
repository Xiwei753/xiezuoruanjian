import re

with open("core/writer_core/src/sync_service.rs", "r") as f:
    content = f.read()

new_tests = """
    #[test]
    fn test_sync_state_does_not_leak_tokens() {
        let dir = tempdir().unwrap();
        let state = SyncState {
            remote_url: Some("https://example.com/repo.git".to_string()),
            transport: Some(SyncTransport::HttpsToken),
            last_synced_commit: None,
            last_sync_time: None,
            last_error: None,
            known_files: std::collections::HashMap::new(),
            conflicts: vec![],
        };

        SyncService::save_sync_state(dir.path(), &state).unwrap();
        let state_path = dir.path().join("app-meta/sync/sync_state.json");
        let state_content = std::fs::read_to_string(state_path).unwrap();

        assert!(state_content.contains("https://example.com/repo.git"));
        assert!(!state_content.contains("token"));
    }

    #[test]
    fn test_stage_blacklisted_files() {
        let dir = tempdir().unwrap();

        // Initialize git repo manually or use SyncService
        let repo = git2::Repository::init(dir.path()).unwrap();

        let file_path = dir.path().join("app-meta/sync/sync_secrets.local.json");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "secret_content").unwrap();

        let backend = Git2Backend;
        let paths = vec!["app-meta/sync/sync_secrets.local.json"];
        backend.stage_paths(dir.path(), &paths).unwrap();

        // Ensure it's not staged
        let mut index = repo.index().unwrap();
        assert!(index.get_path(std::path::Path::new("app-meta/sync/sync_secrets.local.json"), 0).is_none());
    }
"""

content = content.replace("""#[test]
    fn test_sync_state_does_not_leak_tokens() {
        let dir = tempdir().unwrap();
        let state = SyncState {
            remote_url: Some("https://example.com/repo.git".to_string()),
            transport: Some(SyncTransport::HttpsToken),
            last_synced_commit: None,
            last_sync_time: None,
            last_error: None,
            known_files: std::collections::HashMap::new(),
            conflicts: vec![],
        };

        SyncService::save_sync_state(dir.path(), &state).unwrap();
        let state_path = dir.path().join("app-meta/sync/sync_state.json");
        let content = std::fs::read_to_string(state_path).unwrap();

        assert!(content.contains("https://example.com/repo.git"));
    }""", new_tests)

with open("core/writer_core/src/sync_service.rs", "w") as f:
    f.write(content)
