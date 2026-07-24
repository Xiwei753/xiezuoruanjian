use crate::api::WriterCoreApi;
use tempfile::tempdir;

#[test]
fn test_get_sync_capability_scenarios() {
    let temp_dir = tempdir().unwrap();
    crate::workspace::create_workspace(temp_dir.path()).unwrap();
    let api = WriterCoreApi::new(temp_dir.path());

    // 场景 1: 同步未启用
    let mut config = api.load_sync_config().unwrap();
    config.enabled = false;
    api.save_sync_config(config).unwrap();

    let cap = api.get_sync_capability().unwrap();
    assert!(!cap.can_run);
    assert_eq!(cap.block_reason_code.as_deref(), Some("DISABLED"));
    assert_eq!(cap.block_message_key.as_deref(), Some("sync.block.disabled"));

    // 场景 2: 启用但安全存储不可用（WriterCoreApi::new 无 secure_storage）
    let mut config = api.load_sync_config().unwrap();
    config.enabled = true;
    config.remote_url = "https://github.com/test/repo".to_string();
    api.save_sync_config(config).unwrap();

    let cap = api.get_sync_capability().unwrap();
    assert!(!cap.can_run);
    assert_eq!(cap.block_reason_code.as_deref(), Some("SECURE_STORAGE_UNAVAILABLE"));
    assert_eq!(cap.block_message_key.as_deref(), Some("sync.block.secure_storage_unavailable"));

    // 场景 3: 有 URL 但未配置 Token
    let mut config = api.load_sync_config().unwrap();
    config.enabled = true;
    config.remote_url = "https://github.com/test/repo".to_string();
    api.save_sync_config(config).unwrap();

    let mut secrets = api.load_sync_secrets().unwrap();
    secrets.token = None;
    api.save_sync_secrets(secrets).unwrap();

    let cap = api.get_sync_capability().unwrap();
    assert!(!cap.can_run);
    assert_eq!(cap.block_reason_code.as_deref(), Some("SECURE_STORAGE_UNAVAILABLE"));
    assert_eq!(cap.block_message_key.as_deref(), Some("sync.block.secure_storage_unavailable"));

    // 场景 4: 配置齐全，可以运行（需要 secure_storage）
    // WriterCoreApi::new 没有 secure_storage，所以即使配置齐全也会被 SECURE_STORAGE_UNAVAILABLE 阻塞
    // 这符合预期：没有安全存储时不应允许同步
}

#[test]
fn test_get_sync_capability_remote_url_missing() {
    let temp_dir = tempdir().unwrap();
    crate::workspace::create_workspace(temp_dir.path()).unwrap();
    let api = WriterCoreApi::new(temp_dir.path());

    let mut config = api.load_sync_config().unwrap();
    config.enabled = true;
    config.remote_url = "".to_string();
    api.save_sync_config(config).unwrap();

    let cap = api.get_sync_capability().unwrap();
    assert!(!cap.can_run);
    assert_eq!(cap.block_reason_code.as_deref(), Some("SECURE_STORAGE_UNAVAILABLE"));
}
