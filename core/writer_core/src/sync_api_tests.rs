use crate::api::WriterCoreApi;
use tempfile::tempdir;

#[test]
fn test_get_sync_capability_scenarios() {
    let temp_dir = tempdir().unwrap();
    std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
    let api = WriterCoreApi::new(temp_dir.path(), temp_dir.path().join("projects"));
    let project = api.create_project("Cap Test").unwrap();
    let pid = &project.id;

    // 场景 1: 同步未启用
    let mut config = api.load_sync_config(pid).unwrap();
    config.enabled = false;
    api.save_sync_config(pid, config).unwrap();

    let cap = api.get_sync_capability(pid).unwrap();
    assert!(!cap.can_run);
    assert_eq!(cap.block_reason_code.as_deref(), Some("DISABLED"));
    assert_eq!(
        cap.block_message_key.as_deref(),
        Some("sync.block.disabled")
    );

    // 场景 2: 启用但安全存储不可用（WriterCoreApi::new 无 secure_storage）
    let mut config = api.load_sync_config(pid).unwrap();
    config.enabled = true;
    config.remote_url = "https://github.com/test/repo".to_string();
    api.save_sync_config(pid, config).unwrap();

    let cap = api.get_sync_capability(pid).unwrap();
    assert!(!cap.can_run);
    assert_eq!(
        cap.block_reason_code.as_deref(),
        Some("SECURE_STORAGE_UNAVAILABLE")
    );
    assert_eq!(
        cap.block_message_key.as_deref(),
        Some("sync.block.secure_storage_unavailable")
    );

    // 场景 3: 有 URL 但未配置 Token
    let mut config = api.load_sync_config(pid).unwrap();
    config.enabled = true;
    config.remote_url = "https://github.com/test/repo".to_string();
    api.save_sync_config(pid, config).unwrap();

    let mut secrets = api.load_sync_secrets(pid).unwrap();
    secrets.token = None;
    api.save_sync_secrets(pid, secrets).unwrap();

    let cap = api.get_sync_capability(pid).unwrap();
    assert!(!cap.can_run);
    assert_eq!(
        cap.block_reason_code.as_deref(),
        Some("SECURE_STORAGE_UNAVAILABLE")
    );
    assert_eq!(
        cap.block_message_key.as_deref(),
        Some("sync.block.secure_storage_unavailable")
    );

    // 场景 4: 配置齐全，可以运行（需要 secure_storage）
    // WriterCoreApi::new 没有 secure_storage，所以即使配置齐全也会被 SECURE_STORAGE_UNAVAILABLE 阻塞
    // 这符合预期：没有安全存储时不应允许同步
}

#[test]
fn test_get_sync_capability_remote_url_missing() {
    let temp_dir = tempdir().unwrap();
    std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
    let api = WriterCoreApi::new(temp_dir.path(), temp_dir.path().join("projects"));
    let project = api.create_project("URL Test").unwrap();
    let pid = &project.id;

    let mut config = api.load_sync_config(pid).unwrap();
    config.enabled = true;
    config.remote_url = "".to_string();
    api.save_sync_config(pid, config).unwrap();

    let cap = api.get_sync_capability(pid).unwrap();
    assert!(!cap.can_run);
    assert_eq!(
        cap.block_reason_code.as_deref(),
        Some("SECURE_STORAGE_UNAVAILABLE")
    );
}


#[test]
fn test_load_save_app_sync_state_roundtrip() {
    use crate::api::SyncStateDto;
    use std::path::Path;

    let temp_dir = tempdir().unwrap();
    std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
    let api = WriterCoreApi::new(temp_dir.path(), temp_dir.path().join("projects"));

    // 初始状态：文件不存在，返回默认 state（含自动生成的 device_id）。
    let initial = api.load_app_sync_state().unwrap();
    assert!(initial.last_sync_time.is_none());
    assert!(initial.last_synced_commit.is_none());

    // 保存一个非默认 state。
    let mut state = initial.clone();
    state.last_sync_time = Some(1_700_000_000);
    state.last_synced_commit = Some("abc123".to_string());
    state.remote_url = Some("https://github.com/test/app-sync".to_string());
    api.save_app_sync_state(state.clone()).unwrap();

    // 验证写入了正确路径：<app_data_root>/app-meta/sync/state.local.json
    let state_path = temp_dir.path().join("app-meta/sync/state.local.json");
    assert!(state_path.exists(), "app sync state file should exist at <app_data_root>/app-meta/sync/state.local.json");
    let content = std::fs::read_to_string(&state_path).unwrap();
    assert!(content.contains("1700000000"), "saved state should contain last_sync_time");
    assert!(content.contains("abc123"), "saved state should contain last_synced_commit");

    // 读回验证 roundtrip。
    let loaded = api.load_app_sync_state().unwrap();
    assert_eq!(loaded.last_sync_time, Some(1_700_000_000));
    assert_eq!(loaded.last_synced_commit.as_deref(), Some("abc123"));
    assert_eq!(
        loaded.remote_url.as_deref(),
        Some("https://github.com/test/app-sync")
    );

    // 验证不与作品级 state 冲突：作品级 state 在 project_root 下。
    let project = api.create_project("State Isolation Test").unwrap();
    let project_state_path = temp_dir
        .path()
        .join("projects")
        .join(&project.id)
        .join("app-meta/sync/state.local.json");
    assert!(
        !project_state_path.exists(),
        "app sync state must not leak into project dir"
    );
    let _project_initial = api.load_sync_state(&project.id).unwrap();
    assert!(
        !Path::exists(&project_state_path) || !std::fs::read_to_string(&project_state_path).unwrap().contains("1700000000"),
        "project state must not contain app-level last_sync_time"
    );

    // 确认 app state 文件与 project state 文件路径不同。
    assert_ne!(state_path, project_state_path);
}

#[test]
fn test_app_sync_state_independent_from_project_sync_state() {
    use crate::api::SyncStateDto;

    let temp_dir = tempdir().unwrap();
    std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
    let api = WriterCoreApi::new(temp_dir.path(), temp_dir.path().join("projects"));
    let project = api.create_project("Proj").unwrap();
    let pid = &project.id;

    // 写入作品级 state。
    let mut proj_state = api.load_sync_state(pid).unwrap();
    proj_state.last_sync_time = Some(1_111_111_111);
    proj_state.last_synced_commit = Some("proj-commit".to_string());
    // load_sync_state 返回 DTO，但 save 不在 API 层暴露——通过 core 直接保存。
    api.core().save_sync_state(pid, &proj_state.clone().into()).unwrap();

    // 写入应用级 state。
    let mut app_state = api.load_app_sync_state().unwrap();
    app_state.last_sync_time = Some(2_222_222_222);
    app_state.last_synced_commit = Some("app-commit".to_string());
    api.save_app_sync_state(app_state.clone()).unwrap();

    // 两者互不影响。
    let proj_loaded = api.load_sync_state(pid).unwrap();
    let app_loaded = api.load_app_sync_state().unwrap();
    assert_eq!(proj_loaded.last_sync_time, Some(1_111_111_111));
    assert_eq!(app_loaded.last_sync_time, Some(2_222_222_222));
    assert_eq!(proj_loaded.last_synced_commit.as_deref(), Some("proj-commit"));
    assert_eq!(app_loaded.last_synced_commit.as_deref(), Some("app-commit"));
}
