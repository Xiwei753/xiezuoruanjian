use super::sync::*;
use crate::sync::{SyncConfig, SyncProtocol, BackendType};

#[test]
fn test_sync_config_dto_json_key_contract() {
    let dto = SyncConfigDto {
        enabled: true,
        backend_type: "git".to_string(),
        remote_url: "https://github.com/example/repo.git".to_string(),
        transport: "https_token".to_string(),
        branch: "main".to_string(),
        auto_sync: true,
        sync_interval_seconds: 300,
        username: "testuser".to_string(),
        has_network_permission: true,
        has_network_state_permission: true,
    };

    let json_val = serde_json::to_value(&dto).unwrap();
    // Verify snake_case without rename_all="camelCase"
    assert_eq!(json_val["backend_type"], "git");
    assert_eq!(json_val["remote_url"], "https://github.com/example/repo.git");
    assert_eq!(json_val["auto_sync"], true);
    assert_eq!(json_val["sync_interval_seconds"], 300);
}

#[test]
fn test_sync_config_dto_roundtrip() {
    let internal = SyncConfig {
        enabled: true,
        backend_type: BackendType::Git,
        remote_url: "https://github.com/example/repo.git".to_string(),
        transport: SyncProtocol::HttpsToken,
        branch: "main".to_string(),
        auto_sync: true,
        sync_interval_seconds: 300,
        username: "testuser".to_string(),
        scope: crate::sync::SyncScope::Project,
        has_network_permission: true,
        has_network_state_permission: true,
    };

    let dto: SyncConfigDto = internal.clone().into();
    let back: SyncConfig = dto.into();

    assert_eq!(internal.enabled, back.enabled);
    assert_eq!(internal.backend_type, back.backend_type);
    assert_eq!(internal.remote_url, back.remote_url);
    assert_eq!(internal.branch, back.branch);
    assert_eq!(internal.auto_sync, back.auto_sync);
}

#[test]
fn test_sync_state_dto_json_key_contract() {
    let dto = SyncStateDto {
        status: "idle".to_string(),
        remote_url: Some("ssh://git@github.com".to_string()),
        backend_type: Some("git".to_string()),
        transport: Some("ssh_deploy_key".to_string()),
        last_synced_commit: Some("abcdef123".to_string()),
        last_sync_time: Some(1234567890),
        last_error: None,
        conflicts: Some(vec![]),
    };
    let json_val = serde_json::to_value(&dto).unwrap();
    assert_eq!(json_val["remote_url"], "ssh://git@github.com");
    assert_eq!(json_val["last_synced_commit"], "abcdef123");
    assert_eq!(json_val["last_sync_time"], 1234567890);
}
