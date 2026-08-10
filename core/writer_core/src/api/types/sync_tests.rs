use super::sync::*;
use crate::sync::{BackendType, SyncConfig, SyncProtocol, SyncScope};

#[test]
fn test_sync_config_dto_serialization_snake_case() {
    let internal_config = SyncConfig {
        enabled: true,
        backend_type: BackendType::GithubApi,
        remote_url: "https://github.com/test/repo".to_string(),
        transport: SyncProtocol::HttpsToken,
        branch: "main".to_string(),
        auto_sync: false,
        sync_interval_seconds: 3600,
        username: "testuser".to_string(),
        has_network_permission: true,
        has_network_state_permission: false,
        scope: SyncScope::Project,
    };

    let dto: SyncConfigDto = internal_config.into();
    let json_str = serde_json::to_string(&dto).unwrap();

    // Verify it serializes to snake_case since it doesn't have rename_all="camelCase"
    assert!(json_str.contains("\"backend_type\":\"github_api\""));
    assert!(json_str.contains("\"remote_url\":\"https://github.com/test/repo\""));
    assert!(json_str.contains("\"sync_interval_seconds\":3600"));
}

#[test]
fn test_sync_config_dto_roundtrip() {
    let dto = SyncConfigDto {
        enabled: true,
        backend_type: "git".to_string(),
        remote_url: "git@github.com:test/repo.git".to_string(),
        transport: "ssh_deploy_key".to_string(),
        branch: "master".to_string(),
        auto_sync: true,
        sync_interval_seconds: 600,
        username: "git".to_string(),
        has_network_permission: true,
        has_network_state_permission: true,
    };

    let internal: SyncConfig = dto.clone().into();
    let dto_back: SyncConfigDto = internal.into();

    assert_eq!(dto, dto_back);
}
