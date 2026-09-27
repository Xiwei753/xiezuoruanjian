//! Issue #779 评论 5854912227 回归测试 — generation GC maintenance 创建 provider
//! 失败时，**不应**把"用户同步成功终态"重新写成失败。
//!
//! ## Bug 路径（已修复）
//! 1. 正文同步成功，`perform_full_sync()` 已经形成并持久化成功终态
//!    （`FullSyncState.overall_status = Success`）；
//! 2. Linux_Qt 清掉用户侧 `syncing`，启动独立 GC maintenance；
//! 3. maintenance 调 `create_sync_provider_for_maintenance()` 创建 provider；
//! 4. `create_sync_provider_for_maintenance()` 内部调 `init_sync_transport()`，transport
//!    初始化失败（平台未注入 SyncTransport factory）；
//! 5. 修复前：`create_sync_provider_for_plan()` 复用了 `inspect_err` 副作用，Core 调
//!    `persist_full_sync_early_failure(..., "preflight")`，磁盘上的 `FullSyncState`
//!    被 GC maintenance 重新改成 `RecoverableError`，污染了用户同步的成功终态；
//! 6. 修复后：`create_sync_provider_for_maintenance()` 是无副作用入口，transport
//!    初始化失败只返回 Err，不调 `persist_full_sync_early_failure`，磁盘
//!    `FullSyncState` 保持用户同步的成功终态不变。
//!
//! 这符合 #779 的规则：**generation GC 失败只能作为维护错误，不能把正文同步
//! 重新变成失败。**
//!
//! ## 本测试
//! 直接在磁盘上写一份 `overall_status = Success` 的 `full_state.local.json`，
//! 模拟用户同步已成功终态。然后用一个**没有注入 SyncTransport** 的
//! `WriterCoreApi` 实例调用 `perform_generation_gc_maintenance`，让 transport
//! 初始化失败。断言 maintenance 返回 Err 后，磁盘上的 `FullSyncState` **保持
//! 原成功终态不变**（不被污染）。
//!
//! 需要在 `github-api` feature 下运行（provider 创建路径只在 `github-api` feature
//! 下的 `github_api` 分支存在）：
//! ```sh
//! cargo test -p writer_core --features github-api --test issue_779_comment_5854912227_repro
//! ```

#![cfg(feature = "github-api")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;

use tempfile::TempDir;
use writer_core::api::types::{FullSyncStateDto, SyncConfigDto};
use writer_core::api::WriterCoreApi;

// ── 辅助 ──

/// 创建临时 app_data_root + projects_root。
fn make_dirs() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    fs::create_dir_all(&projects_root).unwrap();
    (tmp, app_data_root, projects_root)
}

/// 直接在磁盘上写一份"用户同步成功终态"的 `full_state.local.json`。
///
/// 路径：`<app_data_root>/app-meta/sync/full_state.local.json`
/// 格式与 `FullSyncState` 的 serde 序列化对齐（snake_case）。
fn write_success_full_sync_state(app_data_root: &Path, last_success_time: i64) {
    let dir = app_data_root.join("app-meta").join("sync");
    fs::create_dir_all(&dir).unwrap();
    // FullSyncState { overall_status: Success, last_attempt_time, last_success_time, failed_targets: [] }
    // SyncStatus::Success 序列化为 "success"（snake_case）。
    let json = serde_json::json!({
        "overall_status": "success",
        "last_attempt_time": last_success_time,
        "last_success_time": last_success_time,
        "failed_targets": [],
    });
    let path = dir.join("full_state.local.json");
    fs::write(&path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
}

/// 构造一个 `enabled = true, active_provider = "github_api"` 的 SyncConfigDto。
fn make_gc_sync_config() -> SyncConfigDto {
    SyncConfigDto {
        enabled: true,
        active_provider: "github_api".to_string(),
        provider_config: None,
        auto_sync: false,
        sync_interval_seconds: 0,
        has_network_permission: true,
        has_network_state_permission: true,
    }
}

// ── 回归测试（证明 bug 已修复）──

/// **核心回归**：用户同步已成功终态 → GC maintenance transport 初始化失败 →
/// 磁盘 FullSyncState **保持 Success 不变**（不被污染）。
///
/// 修复前本测试 FAIL（状态被污染成 recoverable_error）。
/// 修复后本测试 PASS（maintenance 用无副作用 provider factory，不触碰 FullSyncState）。
#[test]
fn gc_maintenance_transport_failure_does_not_pollute_user_success_state() {
    let (_tmp, app_data_root, _projects_root) = make_dirs();

    // 1. 模拟用户同步已成功终态：写 Success 的 full_state.local.json。
    let user_success_time = 1_700_000_000;
    write_success_full_sync_state(&app_data_root, user_success_time);

    // 2. 创建一个没有注入 SyncTransport 的 API 实例。
    //    init_sync_transport() 会返回 Err(SyncNetworkUnavailable { "no SyncTransport configured" })。
    let api = WriterCoreApi::new(&app_data_root, &_projects_root);

    // 确认初始状态是 Success。
    let initial_state = api.load_full_sync_state().expect("load 应成功");
    let initial_state: FullSyncStateDto = initial_state.expect("初始状态应存在");
    assert_eq!(
        initial_state.overall_status, "success",
        "前置条件：用户同步已成功终态"
    );
    assert_eq!(
        initial_state.failed_targets,
        Vec::<String>::new(),
        "前置条件：成功终态无 failed_targets"
    );

    // 3. 调用 perform_generation_gc_maintenance。
    //    内部调 create_sync_provider_for_maintenance → init_sync_transport 失败 →
    //    只返回 Err，不调 persist_full_sync_early_failure，不触碰磁盘状态。
    //    maintenance 本身返回 Err（? 传播），但无副作用。
    let config = make_gc_sync_config();
    let maintenance_result = api.perform_generation_gc_maintenance(config, None);

    // maintenance 应返回 Err（transport 初始化失败）。
    assert!(
        maintenance_result.is_err(),
        "maintenance 应返回 Err（transport 初始化失败），got {:?}",
        maintenance_result
    );

    // 4. 读取磁盘状态 — 修复后的关键证据。
    let after_state = api.load_full_sync_state().expect("load 应成功");
    let after_state: FullSyncStateDto = after_state.expect("状态应仍存在");

    // ── 修复后证据 ──
    // overall_status 应仍是 "success"（GC maintenance 不应触碰 FullSyncState）。
    assert_eq!(
        after_state.overall_status, "success",
        "修复后：GC maintenance transport 失败不应污染用户成功终态，got {:?}",
        after_state.overall_status
    );
    assert_eq!(
        after_state.failed_targets,
        Vec::<String>::new(),
        "修复后：failed_targets 应仍为空（无 preflight 副作用），got {:?}",
        after_state.failed_targets
    );

    // last_success_time 应被保留。
    assert_eq!(
        after_state.last_success_time,
        Some(user_success_time),
        "last_success_time 应保留旧值"
    );
}

/// **对照回归**：NoChanges 终态同样不应被污染。
///
/// `FullSyncState::is_overall_success` 把 `NoChanges` 也视为整体成功类，
/// 用户同步无变更时终态是 NoChanges，GC maintenance 失败同样不应污染它。
#[test]
fn gc_maintenance_transport_failure_does_not_pollute_no_changes_state() {
    let (_tmp, app_data_root, _projects_root) = make_dirs();

    // 写一份 NoChanges 终态。
    let dir = app_data_root.join("app-meta").join("sync");
    fs::create_dir_all(&dir).unwrap();
    let json = serde_json::json!({
        "overall_status": "no_changes",
        "last_attempt_time": 1_700_000_100,
        "last_success_time": 1_700_000_100,
        "failed_targets": [],
    });
    fs::write(
        dir.join("full_state.local.json"),
        serde_json::to_string_pretty(&json).unwrap(),
    )
    .unwrap();

    let api = WriterCoreApi::new(&app_data_root, &_projects_root);
    let config = make_gc_sync_config();
    let result = api.perform_generation_gc_maintenance(config, None);
    assert!(result.is_err(), "maintenance 应返回 Err");

    let after_state: FullSyncStateDto = api
        .load_full_sync_state()
        .expect("load 应成功")
        .expect("状态应存在");
    assert_eq!(
        after_state.overall_status, "no_changes",
        "修复后：NoChanges 终态不应被污染，got {:?}",
        after_state.overall_status
    );
    assert_eq!(
        after_state.failed_targets,
        Vec::<String>::new(),
        "修复后：failed_targets 应仍为空，got {:?}",
        after_state.failed_targets
    );
}

/// **对照测试**：sync disabled 时 maintenance 直接返回 Ok(())，不触碰状态。
///
/// 这证明 maintenance 入口在 disabled 时无条件返回，不触碰 FullSyncState。
#[test]
fn gc_maintenance_disabled_sync_does_not_pollute_state() {
    let (_tmp, app_data_root, _projects_root) = make_dirs();
    write_success_full_sync_state(&app_data_root, 1_700_000_000);

    let api = WriterCoreApi::new(&app_data_root, &_projects_root);
    let config = SyncConfigDto {
        enabled: false,
        ..make_gc_sync_config()
    };
    let result = api.perform_generation_gc_maintenance(config, None);
    assert!(result.is_ok(), "disabled sync 应返回 Ok(())");

    let after_state: FullSyncStateDto = api
        .load_full_sync_state()
        .expect("load 应成功")
        .expect("状态应存在");
    assert_eq!(
        after_state.overall_status, "success",
        "disabled sync 不应触碰状态"
    );
}
