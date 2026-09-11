//! Issue #661 评论 5636409273 回归测试。
//!
//! 验证 apps/Linux_qt/src/backend/sync_backend.rs 中 prepare_sync_profile 的协议：
//! 有全局配置和已保存 Token、selected_project_id=None 时，诊断/dry-run/正式同步
//! 进入 Provider 后看到的 Token 不能为空。
//!
//! sujian-linux-qt 是 bin crate（无 lib），集成测试无法直接调用 pub(crate) 的
//! prepare_sync_profile，因此用 WriterCoreApi public API 复现其三步协议并断言
//! override 生效。
//!
//! 测试注入 DummyTransport，让 create_sync_provider_for_plan 的 transport 初始化
//! 成功，从而能走到 GitHubRuntimeConfig::from_persisted 的 token 检查
//! （否则会先失败于 "no SyncTransport configured"，无法验证 override 对 token
//! 可见性的影响）。

#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::sync::Arc;
use tempfile::tempdir;
use writer_core::api::types::{
    ProviderConfigDto, ProviderSecretsDto, SyncConfigDto, SyncSecretsDto,
};
use writer_core::api::WriterCoreApi;
use writer_platform_api::{
    HttpRequest, HttpResponse, SyncTransport, SyncTransportFactory, TransportError,
};

/// 构造一个 enabled、github_api、有 remote_url 和 branch 的同步配置。
fn make_enabled_config() -> SyncConfigDto {
    SyncConfigDto {
        enabled: true,
        active_provider: "github_api".to_string(),
        provider_config: Some(ProviderConfigDto::GitHub {
            remote_url: "https://github.com/test/test.git".to_string(),
            branch: "main".to_string(),
            username: "".to_string(),
            transport: "https_token".to_string(),
        }),
        auto_sync: false,
        sync_interval_seconds: 300,
        has_network_permission: true,
        has_network_state_permission: true,
    }
}

/// 提取 GitHub token；非 GitHub 变体或 None 返回空串。
fn github_token(secrets: &SyncSecretsDto) -> String {
    secrets
        .provider_secrets
        .as_ref()
        .map(|ps| match ps {
            ProviderSecretsDto::GitHub { token } => token.clone(),
        })
        .unwrap_or_default()
}

/// 测试用 transport —— 对所有请求返回网络错误。
///
/// 测试 2 中 token 检查在 transport.execute 之前失败，所以 execute 永远不会被
/// 调用到。测试 1 中 token 非空，会走到 discover_legacy_remote_catalog 调用
/// execute，返回此网络错误（非 token_missing）。
struct DummyTransport;

impl SyncTransport for DummyTransport {
    fn execute(&self, _request: HttpRequest) -> Result<HttpResponse, TransportError> {
        Err(TransportError::new(
            "network",
            "dummy transport: no real network in test".to_string(),
        ))
    }
}

/// 构造测试用 transport factory。
fn dummy_transport_factory() -> SyncTransportFactory {
    Arc::new(|| Ok(Box::new(DummyTransport)))
}

#[test]
fn sync_profile_override_makes_token_visible_to_provider() {
    let dir = tempdir().unwrap();
    let data_root = dir.path().join("data_root");
    let projects_root = dir.path().join("projects_root");
    std::fs::create_dir_all(&data_root).unwrap();
    std::fs::create_dir_all(&projects_root).unwrap();

    // selected_project_id 天然 None，全新空工作区。
    // 注入 DummyTransport 让 transport 初始化成功，从而能走到 token 检查之后
    // 的网络阶段（而非卡在 transport 初始化）。
    let api =
        WriterCoreApi::with_sync_transport(&data_root, &projects_root, dummy_transport_factory());

    api.save_sync_config(make_enabled_config()).unwrap();
    api.save_sync_secrets(SyncSecretsDto {
        provider_secrets: Some(ProviderSecretsDto::GitHub {
            token: "ghp_test_token_xxx".to_string(),
        }),
    })
    .unwrap();

    // 复现 prepare_sync_profile 的三步协议：
    // 1. load_sync_config
    // 2. load_sync_secrets
    // 3. set_sync_secrets_override
    let mut config = api.load_sync_config().unwrap();
    let secrets = api.load_sync_secrets().unwrap();
    api.set_sync_secrets_override(secrets).unwrap();
    assert!(config.enabled);

    // 验证 override 已设置且 token 非空。
    let secrets_after = api.load_sync_secrets().unwrap();
    let token = github_token(&secrets_after);
    assert!(
        !token.is_empty(),
        "token should be non-empty after override"
    );
    assert_eq!(token, "ghp_test_token_xxx");

    // 验证 perform_full_sync_dry_run 不出 token_missing：
    // 有 override 时 Provider 能拿到 token，不会因 token 缺失失败
    // （会因 DummyTransport 的网络错误失败，但绝不是 token_missing）。
    config.has_network_permission = true;
    config.has_network_state_permission = true;
    let result = api.perform_full_sync_dry_run(config);
    match result {
        Ok(_) => {}
        Err(e) => {
            let msg = e.to_string();
            assert!(
                !msg.contains("token is missing"),
                "dry-run should not fail with token_missing when override is set, got: {msg}"
            );
        }
    }
}

#[test]
fn without_override_provider_sees_token_missing() {
    let dir = tempdir().unwrap();
    let data_root = dir.path().join("data_root");
    let projects_root = dir.path().join("projects_root");
    std::fs::create_dir_all(&data_root).unwrap();
    std::fs::create_dir_all(&projects_root).unwrap();

    // 独立实例，override 为空。注入 DummyTransport 让 transport 初始化成功，
    // 从而 create_sync_provider_for_plan 能走到 from_persisted 的 token 检查。
    let api =
        WriterCoreApi::with_sync_transport(&data_root, &projects_root, dummy_transport_factory());

    api.save_sync_config(make_enabled_config()).unwrap();
    // 不调 save_sync_secrets、不调 set_sync_secrets_override。
    //
    // 模拟 Linux 运行时 Issue #661 的场景：设置页保存了 Token 到 AppBackend
    // 内存字段，但后台线程新建的 WriterCoreApi 实例从 secure storage/file 读
    // 不到 token（secure storage 未注入或 key 不一致）。此时
    // secrets_override_snapshot() 返回 None → unwrap_or_default() 空 secrets
    // → create_sync_provider_for_plan 走到 from_persisted(_, None)
    // → token 空 → ProviderError::AuthFailed { reason: "token is missing" }。
    //
    // 这凸显了 prepare_sync_profile 中 set_sync_secrets_override 的必要性：
    // 当持久化 secrets 不可用时，override 是让 Provider 看到 token 的唯一途径。
    let result = api.perform_full_sync_dry_run(make_enabled_config());
    match result {
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("token is missing"),
                "expected token_missing without override, got: {msg}"
            );
        }
        Ok(_) => panic!("expected token_missing error without override, got Ok"),
    }
}
