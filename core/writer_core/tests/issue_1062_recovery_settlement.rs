//! Issue #1062 评论 7236446138 第 3 项（P1）：同步前恢复未结算必须进入本轮
//! 同步最终状态，不能只写日志，也不能被“无变化”快路径显示成没有变化。
//!
//! 这里从公开同步入口 `WriterCoreApi::perform_full_sync` 验证：
//! 1. dirty index 中可恢复 obligation 重放失败时，本轮总体状态是
//!    `recoverable_error`，消息带真实 kind/root/transaction_id；
//! 2. 本轮恢复 settled 时，同一个公开入口仍按原有 no-op 成功返回。
//!
//! 生产 `perform_full_sync` 的 checkpoint 快路径需要真实 provider 先探测远端
//! HEAD；没有可注入 provider 时无法从集成测试驱动。快路径“未 settled 不得返回
//! NoChanges”的收口在 `api::sync_api::recovery_settlement_tests` 内直接调用
//! `finalize_fast_path_no_change` 验证。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use writer_core::api::{SyncConfigDto, WriterCoreApi};

fn projects_root(temp: &tempfile::TempDir) -> PathBuf {
    temp.path().join("projects")
}

fn api(temp: &tempfile::TempDir) -> WriterCoreApi {
    fs::create_dir_all(projects_root(temp)).unwrap();
    WriterCoreApi::new(temp.path(), projects_root(temp))
}

/// 与生产 checkpoint 前置条件一致：没有开启同步时入口仍先执行恢复，
/// 但不创建 provider，也不会产生网络 IO。
fn disabled_config() -> SyncConfigDto {
    SyncConfigDto {
        enabled: false,
        active_provider: String::new(),
        provider_config: None,
        auto_sync: false,
        sync_interval_seconds: 0,
        has_network_permission: false,
        has_network_state_permission: false,
    }
}

/// 在 app data 根写入一份“当前 schema + 已校准”的 dirty index，登记一条
/// `mutation_ack` 义务；root 指向普通文件，使 ack journal 读取失败。
fn write_broken_ack_obligation(app_data_root: &Path, broken_root: &Path, transaction_id: &str) {
    let mut obligations = serde_json::Map::new();
    obligations.insert(
        format!(
            "mutation_ack\u{1f}{}\u{1f}{}",
            broken_root.display(),
            transaction_id
        ),
        serde_json::json!({
            "kind": "mutation_ack",
            "root": broken_root.display().to_string(),
            "transaction_id": transaction_id,
            "detail": "app:global",
        }),
    );
    let index = serde_json::json!({
        "schema_version": 2,
        "targets": {},
        "obligations": obligations,
        "recovery_calibrated": true,
        "recovery_generation": 1,
        "recovery_calibrated_at_ms": 1,
    });
    let path = app_data_root.join("app-meta/sync/dirty-index.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_string(&index).unwrap()).unwrap();
}

#[test]
fn unsettled_recovery_makes_public_sync_round_recoverable() {
    let temp = tempfile::tempdir().unwrap();
    let app_data_root = temp.path();
    let api = api(&temp);
    let broken_root = app_data_root.join("broken-ack-root");
    fs::write(&broken_root, b"not a directory").unwrap();
    let transaction_id = "round-broken\u{1f}app:global";
    write_broken_ack_obligation(app_data_root, &broken_root, transaction_id);

    let result = api
        .perform_full_sync(disabled_config(), true, None, None, None)
        .expect("public sync entry must return the settled result, not a transport error");

    assert_ne!(
        result.overall_status, "no_changes",
        "recovery failure must not be reported as no changes"
    );
    assert_eq!(
        result.overall_status, "recoverable_error",
        "unsettled recovery must produce a recoverable error: {result:?}"
    );
    let message = result
        .error
        .expect("recoverable result must carry the recovery identity");
    assert!(
        message.contains("kind=mutation_ack"),
        "message must name the obligation kind: {message}"
    );
    assert!(
        message.contains(&broken_root.display().to_string()),
        "message must name the real sync root: {message}"
    );
    assert!(
        message.contains("round-broken"),
        "message must name the transaction id: {message}"
    );
    assert!(
        message.contains("state=deferred"),
        "still-pending obligation must be marked deferred: {message}"
    );
    assert_eq!(result.error_category.as_deref(), Some("recovery_unsettled"));
    assert_eq!(result.failure_stage.as_deref(), Some("recovery"));
}

#[test]
fn settled_recovery_keeps_public_sync_noop_success() {
    let temp = tempfile::tempdir().unwrap();
    let api = api(&temp);

    let result = api
        .perform_full_sync(disabled_config(), true, None, None, None)
        .expect("settled recovery must not fail the public entry");

    assert_eq!(
        result.overall_status, "success",
        "no unsettled recovery work: existing no-op contract is preserved"
    );
    assert!(result.error.is_none());
    assert_ne!(result.overall_status, "recoverable_error");
}
