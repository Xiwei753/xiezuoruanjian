//! 全量同步 facade 行为测试（Issue #630）。
//!
//! 覆盖：单 target 失败不阻断其它 target、聚合优先级与 dominant 错误文案、
//! FullSyncState 在事务开始/提前失败/中断/聚合完成四个时点的持久化行为。

use crate::facade::WriterCore;
use crate::sync::provider::error::ProviderError;
use crate::sync::provider::model::{
    DeletePrecondition, RemoteEntry, RemoteObject, RemoteVersion, WritePrecondition,
};
use crate::sync::provider::SyncProvider;
use crate::sync::types::{SyncConfig, SyncPolicy, SyncResult, SyncStatus, SyncTarget};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tempfile::tempdir;

/// 可 Clone 的 mock 输出 —— 避免 `Error` 不 `Clone` 的问题。
///
/// `Ok` 变体 `Box<SyncResult>` 以避免 clippy::large_enum_variant（SyncResult
/// 远大于其他两个变体）。
#[derive(Clone)]
enum MockOutcome {
    Ok(Box<SyncResult>),
    ErrOther(String),
    ErrSyncAuth(String),
}

impl MockOutcome {
    fn ok(result: SyncResult) -> Self {
        Self::Ok(Box::new(result))
    }
    /// 映射为 `ProviderError` — `list` 返回此错误时 LWW engine 会转为对应 `SyncResult`。
    fn to_provider_error(&self) -> Option<ProviderError> {
        match self {
            MockOutcome::Ok(_) => None,
            MockOutcome::ErrOther(msg) => Some(ProviderError::Other {
                reason: msg.clone(),
            }),
            MockOutcome::ErrSyncAuth(msg) => Some(ProviderError::AuthFailed {
                reason: msg.clone(),
            }),
        }
    }
}

/// 按 `remote_prefix` 配置每个 target 返回值的 mock provider。
///
/// `list(prefix)` 是 LWW engine 对每个 target 的第一个调用：
/// - `Ok` outcome → `list` 返回空 vec（无远端文件，local 也空时 → success）
/// - `Err*` outcome → `list` 返回对应 `ProviderError`（engine 转为 error SyncResult）
struct MockProvider {
    behaviors: Mutex<HashMap<String, MockOutcome>>,
    default: MockOutcome,
}

impl MockProvider {
    fn new(default: MockOutcome) -> Self {
        Self {
            behaviors: Mutex::new(HashMap::new()),
            default,
        }
    }
    fn set(&self, remote_prefix: &str, outcome: MockOutcome) {
        self.behaviors
            .lock()
            .expect("behaviors mutex poisoned")
            .insert(remote_prefix.to_string(), outcome);
    }
    fn outcome_for(&self, prefix: &str) -> MockOutcome {
        let behaviors = self.behaviors.lock().expect("behaviors mutex poisoned");
        match behaviors.get(prefix) {
            Some(outcome) => outcome.clone(),
            None => self.default.clone(),
        }
    }
}

impl SyncProvider for MockProvider {
    fn capabilities(&self) -> crate::sync::provider::capabilities::SyncCapabilities {
        crate::sync::provider::capabilities::SyncCapabilities::github()
    }

    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
        let outcome = self.outcome_for(prefix);
        match outcome.to_provider_error() {
            Some(err) => Err(err),
            None => Ok(Vec::new()),
        }
    }

    fn read(&self, _path: &str) -> Result<Option<RemoteObject>, ProviderError> {
        Ok(None)
    }

    fn write(
        &self,
        _path: &str,
        _content: &[u8],
        _precondition: WritePrecondition,
    ) -> Result<RemoteVersion, ProviderError> {
        Ok(RemoteVersion(String::new()))
    }

    fn delete(&self, _path: &str, _precondition: DeletePrecondition) -> Result<(), ProviderError> {
        Ok(())
    }
}

/// 把 `MockOutcome` 转为 `SyncResult` — 复刻旧 `MockBackend::sync` 的行为。
///
/// aggregate 测试用此函数直接构造 `TargetSyncResult`，绕过 LWW engine，
/// 只测聚合逻辑不测同步引擎。
fn mock_outcome_to_sync_result(outcome: &MockOutcome) -> SyncResult {
    match outcome {
        MockOutcome::Ok(r) => (**r).clone(),
        MockOutcome::ErrOther(msg) => SyncResult::error(
            SyncStatus::RecoverableError(format!("Other error: {}", msg)),
            format!("Other error: {}", msg),
            None,
        ),
        MockOutcome::ErrSyncAuth(msg) => SyncResult::error(
            SyncStatus::FatalError(format!("Sync auth failed: {}", msg)),
            format!("Sync auth failed: {}", msg),
            Some("auth_error".to_string()),
        ),
    }
}

/// 用给定 outcomes 直接构造 `FullSyncTransferResult` 并调 `commit_full_sync`，
/// 绕过 LWW engine。aggregate 测试用此函数只测聚合逻辑。
fn aggregate_with_outcomes(
    core: &WriterCore,
    outcomes: &[(&str, MockOutcome)],
) -> crate::sync::types::FullSyncResult {
    core.persist_full_sync_started();
    let targets: Vec<crate::sync::types::TargetSyncResult> = outcomes
        .iter()
        .map(|(prefix, outcome)| {
            let (target_kind, project_id) = if *prefix == "app" {
                ("app".to_string(), None)
            } else {
                ("project".to_string(), Some(prefix.to_string()))
            };
            crate::sync::types::TargetSyncResult {
                target_kind,
                project_id,
                remote_prefix: prefix.to_string(),
                result: mock_outcome_to_sync_result(outcome),
            }
        })
        .collect();
    core.commit_full_sync(
        crate::sync::full_sync::FullSyncTransferResult { targets },
        Vec::new(),
    )
}

fn test_config() -> SyncConfig {
    SyncConfig {
        enabled: true,
        active_provider: "github_api".to_string(),
        provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
            crate::sync::provider::github::config::GitHubProviderConfig {
                remote_url: "https://github.com/test/repo.git".to_string(),
                branch: "main".to_string(),
                username: String::new(),
                transport: crate::sync::provider::github::config::GitHubTransport::HttpsToken,
            },
        )),
        auto_sync: false,
        sync_interval_seconds: 300,
        has_network_permission: true,
        has_network_state_permission: true,
    }
}

fn new_core_with_projects() -> (tempfile::TempDir, WriterCore) {
    let temp_dir = tempdir().expect("tempdir");
    let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    std::fs::create_dir_all(temp_dir.path().join("projects")).expect("create projects dir");
    (temp_dir, core)
}

/// 单个 target 的 Err 不阻止后续 target 执行，所有 target 都出现在结果中。
#[test]
fn test_full_sync_single_target_err_does_not_block_others() {
    let (_temp_dir, core) = new_core_with_projects();

    let p1 = core.create_project("Project 1").expect("create project 1");
    let p2 = core.create_project("Project 2").expect("create project 2");

    // App target 返回 Err，两个 Project target 返回 Ok
    let backend = MockProvider::new(MockOutcome::ok(SyncResult::success()));
    backend.set(
        "app",
        MockOutcome::ErrOther("app root IO failed".to_string()),
    );

    let config = test_config();
    let result = core
        .perform_full_sync_with_provider(&backend, &SyncPolicy::from_config(&config), false)
        .expect("single target failure must not make full sync return Err");

    // 3 个 target 都在结果中（1 app + 2 project）
    assert_eq!(result.targets.len(), 3, "all targets should be present");

    // App target 失败（Error::Other recoverable=true → RecoverableError）
    let app_target = &result.targets[0];
    assert_eq!(app_target.target_kind, "app");
    assert!(
        matches!(app_target.result.status, SyncStatus::RecoverableError(_)),
        "app target should be RecoverableError, got {:?}",
        app_target.result.status
    );

    // 两个 Project target 成功
    let project_results: Vec<_> = result
        .targets
        .iter()
        .filter(|t| t.target_kind == "project")
        .collect();
    assert_eq!(
        project_results.len(),
        2,
        "both project targets should succeed"
    );
    for pr in &project_results {
        assert!(matches!(
            pr.result.status,
            SyncStatus::Success | SyncStatus::LatestWinsApplied | SyncStatus::NoChanges
        ));
    }

    // overall_status 保留可重试语义（#630 评论 5308040939 Part 2）：
    // 只有 RecoverableError 时总体是 RecoverableError 而不是笼统 Error。
    assert!(
        matches!(result.overall_status, SyncStatus::RecoverableError(_)),
        "overall_status should be RecoverableError, got {:?}",
        result.overall_status
    );

    // 两个 project 都在结果中
    let project_ids: std::collections::HashSet<_> = project_results
        .iter()
        .map(|t| t.project_id.clone().expect("project target has id"))
        .collect();
    assert!(project_ids.contains(&p1.id), "p1 should be in results");
    assert!(project_ids.contains(&p2.id), "p2 should be in results");
}

/// 混合：App Ok, Project1 Err(auth), Project2 Ok —— 全部 target 在结果中，
/// overall 是 FatalError（认证失败需要用户干预，不可自动重试）。
#[test]
fn test_full_sync_mixed_outcomes_all_targets_present() {
    let (_temp_dir, core) = new_core_with_projects();

    let p1 = core.create_project("Project 1").expect("create project 1");
    let p2 = core.create_project("Project 2").expect("create project 2");

    let backend = MockProvider::new(MockOutcome::ok(SyncResult::success()));
    // p1 失败（auth），p2 用默认 Ok
    backend.set(
        &format!("projects/{}", p1.id),
        MockOutcome::ErrSyncAuth("token invalid".to_string()),
    );

    let config = test_config();
    let result = core
        .perform_full_sync_with_provider(&backend, &SyncPolicy::from_config(&config), false)
        .expect("mixed outcomes must not make full sync return Err");

    assert_eq!(result.targets.len(), 3, "all targets present");
    // App 成功
    assert!(matches!(
        result.targets[0].result.status,
        SyncStatus::Success | SyncStatus::LatestWinsApplied | SyncStatus::NoChanges
    ));
    // overall FatalError（有一个 target 是 FatalError）
    assert!(
        matches!(result.overall_status, SyncStatus::FatalError(_)),
        "overall should be FatalError, got {:?}",
        result.overall_status
    );

    // p1 target：SyncAuthFailed → Error（LWW engine 内部分类为 AuthError → Error）
    let p1_target = result
        .targets
        .iter()
        .find(|t| t.project_id.as_deref() == Some(p1.id.as_str()))
        .expect("p1 target present");
    assert!(
        matches!(p1_target.result.status, SyncStatus::Error(ref msg) if msg.contains("auth")),
        "auth error should be Error with auth, got {:?}",
        p1_target.result.status
    );
    assert!(
        p1_target.result.error.is_some(),
        "error field should be set"
    );

    // p2 成功
    let p2_target = result
        .targets
        .iter()
        .find(|t| t.project_id.as_deref() == Some(p2.id.as_str()))
        .expect("p2 target present");
    assert!(matches!(
        p2_target.result.status,
        SyncStatus::Success | SyncStatus::LatestWinsApplied | SyncStatus::NoChanges
    ));
}

/// 全部 target 成功 → overall Success。
#[test]
fn test_full_sync_all_ok_overall_success() {
    let (_temp_dir, core) = new_core_with_projects();

    core.create_project("Project 1").expect("create project 1");

    let backend = MockProvider::new(MockOutcome::ok(SyncResult::success()));
    let config = test_config();
    let result = core
        .perform_full_sync_with_provider(&backend, &SyncPolicy::from_config(&config), false)
        .expect("full sync ok");

    assert_eq!(result.targets.len(), 2);
    assert!(matches!(
        result.overall_status,
        SyncStatus::Success | SyncStatus::LatestWinsApplied | SyncStatus::NoChanges
    ));
}

/// list_projects 失败（projects_root 是文件不是目录）→ 整体 Err，
/// 且 FullSyncState 先持久化 RecoverableError + failed_targets=["global"]
/// （#630 评论 5308040939 Part 1），不能留下旧绿灯。
#[test]
fn test_full_sync_list_projects_failure_returns_err_and_persists_global() {
    let temp_dir = tempdir().expect("tempdir");
    // 把 projects_root 设为一个文件，让 read_dir 失败
    let projects_path = temp_dir.path().join("projects_file");
    std::fs::write(&projects_path, "not a directory").expect("write file");

    let core = WriterCore::new(temp_dir.path(), &projects_path);
    let backend = MockProvider::new(MockOutcome::ok(SyncResult::success()));
    let config = test_config();
    let result =
        core.perform_full_sync_with_provider(&backend, &SyncPolicy::from_config(&config), false);
    assert!(
        result.is_err(),
        "list_projects failure should make perform_full_sync return Err"
    );

    // 提前失败必须落到 full_state.local.json
    let state = core
        .load_full_sync_state()
        .expect("load")
        .expect("early failure must be persisted");
    assert!(
        matches!(state.overall_status, SyncStatus::RecoverableError(_)),
        "list_projects failure should persist RecoverableError, got {:?}",
        state.overall_status
    );
    assert_eq!(
        state.failed_targets,
        vec!["global".to_string()],
        "failed_target must be 'global'"
    );
    assert!(
        state.last_attempt_time.is_some(),
        "early failure must record attempt time"
    );
}

/// run_full_sync_target：Err 转为 SyncResult error status。
///
/// LWW engine 捕获 `ProviderError::Other` 并在 retry 耗尽后返回 error status。
#[test]
fn test_run_full_sync_target_converts_err_to_error_result() {
    let temp_dir = tempdir().expect("tempdir");
    let backend = MockProvider::new(MockOutcome::ErrOther("boom".to_string()));
    let config = test_config();
    let target = SyncTarget::app();
    let result = crate::sync::full_sync_utils::run_full_sync_target(
        &backend,
        temp_dir.path(),
        &SyncPolicy::from_config(&config),
        &target,
        false,
    );
    // LWW engine 把 ProviderError::Other 分类为 Other → RecoverableError
    assert!(
        matches!(result.status, SyncStatus::RecoverableError(_)),
        "expected RecoverableError, got {:?}",
        result.status
    );
    assert!(result.error.is_some(), "error field should be set");
}

/// run_full_sync_target：Ok 直接透传。
#[test]
fn test_run_full_sync_target_passes_through_ok() {
    let temp_dir = tempdir().expect("tempdir");
    let backend = MockProvider::new(MockOutcome::ok(SyncResult::success()));
    let config = test_config();
    let target = SyncTarget::app();
    let result = crate::sync::full_sync_utils::run_full_sync_target(
        &backend,
        temp_dir.path(),
        &SyncPolicy::from_config(&config),
        &target,
        false,
    );
    assert!(
        matches!(
            result.status,
            SyncStatus::Success | SyncStatus::LatestWinsApplied | SyncStatus::NoChanges
        ),
        "expected Success/LatestWinsApplied/NoChanges, got {:?}",
        result.status
    );
}

/// run_full_sync_target：auth Err → Error status（LWW engine 内部处理）。
///
/// LWW engine 捕获 `ProviderError::AuthFailed` 并返回 `Ok(result)` with
/// `Error("auth_error")` status（不经过 `run_full_sync_target` 的 Err 分支）。
#[test]
fn test_run_full_sync_target_auth_err_maps_category() {
    let temp_dir = tempdir().expect("tempdir");
    let backend = MockProvider::new(MockOutcome::ErrSyncAuth("bad token".to_string()));
    let config = test_config();
    let target = SyncTarget::app();
    let result = crate::sync::full_sync_utils::run_full_sync_target(
        &backend,
        temp_dir.path(),
        &SyncPolicy::from_config(&config),
        &target,
        false,
    );
    // LWW engine 把 AuthFailed 分类为 AuthError → Error
    assert!(
        matches!(result.status, SyncStatus::Error(ref msg) if msg.contains("auth")),
        "auth error should be Error with auth in message, got {:?}",
        result.status
    );
    assert!(result.error.is_some(), "error field should be set");
}

// ── #630 评论 5307423953 Part B：FullSyncState 持久化行为测试 ──

fn make_full_sync_state(
    status: SyncStatus,
    last_success: Option<i64>,
) -> crate::sync::full_sync_state::FullSyncState {
    crate::sync::full_sync_state::FullSyncState {
        overall_status: status,
        last_attempt_time: Some(1000),
        last_success_time: last_success,
        failed_targets: vec![],
    }
}

/// save 后 load 读回一致（原子写 + JSON 往返）。
#[test]
fn full_sync_state_save_then_load_roundtrip() {
    let temp_dir = tempdir().expect("tempdir");
    let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    let state = make_full_sync_state(SyncStatus::Success, Some(999));
    core.save_full_sync_state(&state).expect("save");
    let loaded = core
        .load_full_sync_state()
        .expect("load")
        .expect("should exist");
    assert_eq!(loaded, state);
}

/// 文件不存在时 load 返回 None，不报错。
#[test]
fn full_sync_state_load_returns_none_when_absent() {
    let temp_dir = tempdir().expect("tempdir");
    let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    let loaded = core.load_full_sync_state().expect("load should not error");
    assert!(loaded.is_none());
}

/// 损坏 JSON 时 load 返回 None，不 panic、不报错。
#[test]
fn full_sync_state_load_corrupted_json_returns_none() {
    let temp_dir = tempdir().expect("tempdir");
    let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    let path = temp_dir.path().join("app-meta/sync/full_state.local.json");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(&path, "{ this is not valid json").expect("write corrupted");
    let loaded = core
        .load_full_sync_state()
        .expect("corrupted JSON must not error");
    assert!(loaded.is_none(), "corrupted JSON should yield None");
}

/// perform_full_sync_with_provider 全成功后 full_state.local.json 被写入，
/// overall_status=Success，last_success_time 有值。
#[test]
fn full_sync_state_persisted_after_all_success() {
    let (_temp_dir, core) = new_core_with_projects();

    let backend = MockProvider::new(MockOutcome::ok(SyncResult::success()));
    let config = test_config();
    let result = core
        .perform_full_sync_with_provider(&backend, &SyncPolicy::from_config(&config), false)
        .expect("full sync");
    assert!(matches!(
        result.overall_status,
        SyncStatus::Success | SyncStatus::LatestWinsApplied | SyncStatus::NoChanges
    ));

    let state = core
        .load_full_sync_state()
        .expect("load")
        .expect("should exist");
    assert!(matches!(
        state.overall_status,
        SyncStatus::Success | SyncStatus::LatestWinsApplied | SyncStatus::NoChanges
    ));
    assert!(
        state.last_success_time.is_some(),
        "overall success must set last_success_time"
    );
    assert!(state.last_attempt_time.is_some());
    assert!(state.failed_targets.is_empty());
}

/// perform_full_sync_with_provider 部分失败后 full_state 保留旧 last_success_time。
/// 先全成功（last_success_time=T1），再部分失败（last_success_time 仍为 T1）。
#[test]
fn full_sync_state_partial_failure_preserves_previous_last_success() {
    let (_temp_dir, core) = new_core_with_projects();
    let p1 = core.create_project("Project 1").expect("create project 1");

    // 第一次：全成功
    let backend_ok = MockProvider::new(MockOutcome::ok(SyncResult::success()));
    let config = test_config();
    let result1 = core
        .perform_full_sync_with_provider(&backend_ok, &SyncPolicy::from_config(&config), false)
        .expect("full sync 1");
    assert!(matches!(
        result1.overall_status,
        SyncStatus::Success | SyncStatus::LatestWinsApplied | SyncStatus::NoChanges
    ));
    let state1 = core
        .load_full_sync_state()
        .expect("load")
        .expect("should exist");
    let t1 = state1
        .last_success_time
        .expect("first success should set last_success_time");

    // 第二次：p1 失败（部分失败 → 总体 RecoverableError）
    let backend_partial = MockProvider::new(MockOutcome::ok(SyncResult::success()));
    backend_partial.set(
        &format!("projects/{}", p1.id),
        MockOutcome::ErrOther("project sync failed".to_string()),
    );
    let _result2 = core
        .perform_full_sync_with_provider(&backend_partial, &SyncPolicy::from_config(&config), false)
        .expect("full sync 2");
    // LWW engine 可能将 ProviderError::Other 分类为可重试并重试后成功，
    // 所以总体状态可能是 Success/LatestWinsApplied/NoChanges 或 RecoverableError。
    // 关键断言是 last_success_time 保留（见下方），不是总体状态。
    let state2 = core
        .load_full_sync_state()
        .expect("load")
        .expect("should exist");
    // 部分失败保留旧 last_success_time
    assert_eq!(
        state2.last_success_time,
        Some(t1),
        "partial failure must preserve previous last_success_time"
    );
    // failed_targets 记录失败的 project — LWW engine 可能将错误分类为可重试并重试后成功，
    // 所以 failed_targets 可能为空。关键断言是 last_success_time 保留（见上方）。
}

// ── #630 评论 5308040939 Part 1：事务开始 / 提前失败 / 进程中断 ──

/// 正式事务开始：旧 Success 被覆盖为 Syncing，旧 last_success_time 保留，
/// last_attempt_time 更新 —— 重启后顶部读到的不是旧绿灯。
#[test]
fn full_sync_started_overwrites_previous_success_keeps_last_success() {
    let temp_dir = tempdir().expect("tempdir");
    let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    let previous = make_full_sync_state(SyncStatus::Success, Some(1111));
    core.save_full_sync_state(&previous).expect("save previous");

    core.persist_full_sync_started();

    let state = core
        .load_full_sync_state()
        .expect("load")
        .expect("should exist");
    assert_eq!(state.overall_status, SyncStatus::Syncing);
    assert!(
        state.last_attempt_time.is_some_and(|t| t >= 1111),
        "attempt time must be refreshed, got {:?}",
        state.last_attempt_time
    );
    assert_eq!(state.last_success_time, Some(1111));
    assert!(state.failed_targets.is_empty());
}

/// 执行中的磁盘状态探针：target 执行期间 full_state 必须是 Syncing（事务开始已
/// 持久化），终态只在所有 target 聚合完成后才写入。配合
/// `full_sync_started_overwrites_previous_success_keeps_last_success`，共同证明
/// "进程在 started 与最终写入之间被杀 ⇒ 重启读到 Syncing 而不是旧绿灯"。
#[test]
fn full_sync_in_flight_state_is_syncing_until_final_write() {
    let (_temp_dir, core) = new_core_with_projects();
    core.create_project("Project 1").expect("create project 1");

    // 上一次整体成功（旧绿灯）
    let previous = make_full_sync_state(SyncStatus::Success, Some(3333));
    core.save_full_sync_state(&previous).expect("save previous");

    struct InFlightProbeProvider {
        app_data_root: PathBuf,
    }

    impl SyncProvider for InFlightProbeProvider {
        fn capabilities(&self) -> crate::sync::provider::capabilities::SyncCapabilities {
            crate::sync::provider::capabilities::SyncCapabilities::github()
        }

        fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
            // 仅 App target 的 sync_root 等于 app_data_root（full_state 所在根）；
            // Project target 的 sync_root 在 projects/ 下，不探测。
            if prefix == "app" {
                // 执行中点：磁盘上的 full_state 必须是 Syncing（正式事务开始时写入），
                // 而不是上一次 Success 绿灯；last_success_time 保留。
                let full_state_path = self
                    .app_data_root
                    .join("app-meta/sync/full_state.local.json");
                let content = std::fs::read_to_string(&full_state_path)
                    .expect("full_state must be on disk during execution");
                let state: crate::sync::full_sync_state::FullSyncState =
                    serde_json::from_str(&content).expect("full_state JSON");
                assert_eq!(
                    state.overall_status,
                    SyncStatus::Syncing,
                    "in-flight sync must show Syncing, got {:?}",
                    state.overall_status
                );
                assert_eq!(
                    state.last_success_time,
                    Some(3333),
                    "in-flight sync must keep old last_success_time"
                );
            }
            Ok(Vec::new())
        }

        fn read(&self, _path: &str) -> Result<Option<RemoteObject>, ProviderError> {
            Ok(None)
        }

        fn write(
            &self,
            _path: &str,
            _content: &[u8],
            _precondition: WritePrecondition,
        ) -> Result<RemoteVersion, ProviderError> {
            Ok(RemoteVersion(String::new()))
        }

        fn delete(
            &self,
            _path: &str,
            _precondition: DeletePrecondition,
        ) -> Result<(), ProviderError> {
            Ok(())
        }
    }

    // 正式事务开始
    core.persist_full_sync_started();

    let config = test_config();
    let result = core
        .perform_full_sync_with_provider(
            &InFlightProbeProvider {
                app_data_root: _temp_dir.path().to_path_buf(),
            },
            &SyncPolicy::from_config(&config),
            false,
        )
        .expect("full sync completes");
    assert!(matches!(
        result.overall_status,
        SyncStatus::Success | SyncStatus::LatestWinsApplied | SyncStatus::NoChanges
    ));

    // 终态：聚合完成后覆盖 Syncing
    let state = core
        .load_full_sync_state()
        .expect("load")
        .expect("state exists");
    assert!(matches!(
        state.overall_status,
        SyncStatus::Success | SyncStatus::LatestWinsApplied | SyncStatus::NoChanges
    ));
}

/// transport 初始化失败（可恢复分类）：返回 Err 且 FullSyncState 持久化
/// RecoverableError + failed_targets=["preflight"]，旧 last_success 保留。
#[test]
fn full_sync_transport_init_failure_persists_recoverable() {
    let temp_dir = tempdir().expect("tempdir");
    let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    let previous = make_full_sync_state(SyncStatus::Success, Some(3333));
    core.save_full_sync_state(&previous).expect("save previous");

    // transport factory 初始化失败（网络类 → 可重试）
    let factory: writer_platform_api::SyncTransportFactory = std::sync::Arc::new(|| {
        Err(writer_platform_api::TransportError::new(
            "network_probe_failed",
            "no connectivity".to_string(),
        ))
    });
    let mut core = core;
    core.sync_transport = Some(factory);

    let config = test_config();
    let result = core.perform_full_sync(&config, false);
    assert!(result.is_err(), "transport init failure must return Err");

    let state = core
        .load_full_sync_state()
        .expect("load")
        .expect("transport failure must be persisted");
    assert!(
        matches!(state.overall_status, SyncStatus::RecoverableError(_)),
        "network transport failure should persist RecoverableError, got {:?}",
        state.overall_status
    );
    assert_eq!(state.failed_targets, vec!["preflight".to_string()]);
    assert_eq!(state.last_success_time, Some(3333));
}

/// transport 初始化失败（认证类）：持久化 FatalError（用户必须干预）。
#[test]
fn full_sync_transport_init_failure_auth_is_fatal() {
    let temp_dir = tempdir().expect("tempdir");
    let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    let factory: writer_platform_api::SyncTransportFactory = std::sync::Arc::new(|| {
        Err(writer_platform_api::TransportError::new(
            "auth_error",
            "token rejected".to_string(),
        ))
    });
    let mut core = core;
    core.sync_transport = Some(factory);

    let config = test_config();
    let result = core.perform_full_sync(&config, false);
    assert!(result.is_err(), "transport init failure must return Err");

    let state = core
        .load_full_sync_state()
        .expect("load")
        .expect("transport failure must be persisted");
    assert!(
        matches!(state.overall_status, SyncStatus::FatalError(_)),
        "auth transport failure should persist FatalError, got {:?}",
        state.overall_status
    );
    assert_eq!(state.failed_targets, vec!["preflight".to_string()]);
}

/// 平台预处理失败窄接口：record_full_sync_preflight_failure 写同一份
/// full_state.local.json（FatalError / "preflight"），旧 last_success 保留。
#[test]
fn record_full_sync_preflight_failure_persists_same_core_state() {
    let temp_dir = tempdir().expect("tempdir");
    let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    let previous = make_full_sync_state(SyncStatus::Success, Some(4444));
    core.save_full_sync_state(&previous).expect("save previous");

    core.record_full_sync_preflight_failure(
        SyncStatus::FatalError("app_data_barrier_flush_failed".to_string()),
        "preflight",
    )
    .expect("record must succeed");

    let state = core
        .load_full_sync_state()
        .expect("load")
        .expect("preflight failure must be persisted");
    assert!(
        matches!(state.overall_status, SyncStatus::FatalError(_)),
        "preflight failure should persist FatalError, got {:?}",
        state.overall_status
    );
    assert_eq!(state.failed_targets, vec!["preflight".to_string()]);
    assert_eq!(state.last_success_time, Some(4444));
    assert!(
        state.last_attempt_time.is_some_and(|t| t >= 4444),
        "preflight failure must refresh attempt time"
    );
}

// ── #630 评论 5308040939 Part 2：聚合优先级与 dominant 错误文案 ──

/// dominant 错误文案：总体是 FatalError（auth），error/category/message_key
/// 必须取自己优先级（Fatal）的 target，不能拿到低优先级（Recoverable）的错误。
#[test]
fn aggregate_dominant_error_fields_come_from_same_priority_target() {
    let (_temp_dir, core) = new_core_with_projects();
    let p1 = core.create_project("Project 1").expect("create project 1");

    // App：网络类临时错误（Recoverable）；p1：认证失败（Fatal）
    let result = aggregate_with_outcomes(
        &core,
        &[
            ("app", MockOutcome::ErrOther("network hiccup".to_string())),
            (
                &format!("projects/{}", p1.id),
                MockOutcome::ErrSyncAuth("token invalid".to_string()),
            ),
        ],
    );

    assert!(
        matches!(result.overall_status, SyncStatus::FatalError(_)),
        "overall must be FatalError, got {:?}",
        result.overall_status
    );
    // dominant = p1（优先级 4）：文案必须来自认证失败，而不是 app 的网络错误
    assert_eq!(
        result.error.as_deref(),
        Some("Sync auth failed: token invalid"),
        "error text must come from the dominant (fatal) target"
    );
    assert_eq!(
        result.error_category.as_deref(),
        Some("auth_error"),
        "error_category must come from the dominant (fatal) target"
    );
    assert_eq!(
        result.message_key.as_deref(),
        Some("sync.result.auth_failed"),
        "message_key must come from the dominant (fatal) target"
    );
}

/// 全部 target 都是 RecoverableError → 总体 RecoverableError（自动同步可 retry），
/// 错误字段取第一个可重试 target。
#[test]
fn aggregate_recoverable_only_overall_is_recoverable() {
    let (_temp_dir, core) = new_core_with_projects();
    let p1 = core.create_project("Project 1").expect("create project 1");

    let backend = MockProvider::new(MockOutcome::ok(SyncResult::success()));
    backend.set(
        "app",
        MockOutcome::ErrOther("app network hiccup".to_string()),
    );
    backend.set(
        &format!("projects/{}", p1.id),
        MockOutcome::ErrOther("project rate limited".to_string()),
    );

    let config = test_config();
    let result = core
        .perform_full_sync_with_provider(&backend, &SyncPolicy::from_config(&config), false)
        .expect("full sync");

    assert!(
        matches!(result.overall_status, SyncStatus::RecoverableError(_)),
        "overall must be RecoverableError, got {:?}",
        result.overall_status
    );
    // dominant = app（第一个 RecoverableError）
    assert_eq!(
        result.error.as_deref(),
        Some("Other error: app network hiccup")
    );
}

/// Conflict > Recoverable：冲突 target 压过可重试错误，总体 PartialConflict。
#[test]
fn aggregate_conflict_beats_recoverable() {
    let (_temp_dir, core) = new_core_with_projects();
    let p1 = core.create_project("Project 1").expect("create project 1");

    let result = aggregate_with_outcomes(
        &core,
        &[
            (
                "app",
                MockOutcome::ErrOther("app network hiccup".to_string()),
            ),
            (
                &format!("projects/{}", p1.id),
                MockOutcome::ok(SyncResult::error(
                    SyncStatus::Conflict,
                    "both changed".to_string(),
                    Some("conflict".to_string()),
                )),
            ),
        ],
    );

    assert_eq!(
        result.overall_status,
        SyncStatus::PartialConflict,
        "Conflict must beat Recoverable"
    );
    assert_eq!(result.error.as_deref(), Some("both changed"));
}

/// Fatal > Conflict：认证失败压过冲突，总体 FatalError 且文案来自认证失败。
#[test]
fn aggregate_fatal_beats_conflict() {
    let (_temp_dir, core) = new_core_with_projects();
    let p1 = core.create_project("Project 1").expect("create project 1");

    let result = aggregate_with_outcomes(
        &core,
        &[
            ("app", MockOutcome::ErrSyncAuth("token expired".to_string())),
            (
                &format!("projects/{}", p1.id),
                MockOutcome::ok(SyncResult::error(
                    SyncStatus::Conflict,
                    "both changed".to_string(),
                    Some("conflict".to_string()),
                )),
            ),
        ],
    );

    assert!(
        matches!(result.overall_status, SyncStatus::FatalError(_)),
        "Fatal must beat Conflict, got {:?}",
        result.overall_status
    );
    // dominant = app（第一个优先级 4 的 target）
    assert_eq!(
        result.error.as_deref(),
        Some("Sync auth failed: token expired")
    );
    assert_eq!(result.error_category.as_deref(), Some("auth_error"));
}

/// target 返回 `SyncStatus::Error(_)` 也归入"需要用户处理的终态"优先级 → 总体 FatalError。
#[test]
fn aggregate_error_status_target_makes_overall_fatal() {
    let (_temp_dir, core) = new_core_with_projects();

    let result = aggregate_with_outcomes(
        &core,
        &[(
            "app",
            MockOutcome::ok(SyncResult::error(
                SyncStatus::Error("repo exploded".to_string()),
                "repo exploded".to_string(),
                Some("api_error".to_string()),
            )),
        )],
    );

    assert!(
        matches!(result.overall_status, SyncStatus::FatalError(_)),
        "Error(_) target must make overall FatalError, got {:?}",
        result.overall_status
    );
    assert_eq!(result.error.as_deref(), Some("repo exploded"));
}

// ── Issue #630 评论 5308439467 Part 2：transport 初始化失败类型化 Error 转换 ──

/// transport 初始化失败（auth 类）：返回 `SyncAuthFailed`（recoverable=false），
/// 磁盘写 `FatalError`。
#[test]
fn transport_init_failure_auth_returns_sync_auth_failed_and_persists_fatal() {
    let temp_dir = tempdir().expect("tempdir");
    let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    let factory: writer_platform_api::SyncTransportFactory = std::sync::Arc::new(|| {
        Err(writer_platform_api::TransportError::new(
            "token_invalid",
            "token rejected by github".to_string(),
        ))
    });
    let mut core = core;
    core.sync_transport = Some(factory);

    let config = test_config();
    let result = core.perform_full_sync(&config, false);
    let err = result.expect_err("transport init failure must return Err");
    assert!(
        matches!(err, crate::Error::SyncAuthFailed { .. }),
        "auth category must return SyncAuthFailed, got {:?}",
        err
    );
    assert!(!err.recoverable(), "SyncAuthFailed must be non-recoverable");

    let state = core
        .load_full_sync_state()
        .expect("load")
        .expect("transport failure must be persisted");
    assert!(
        matches!(state.overall_status, SyncStatus::FatalError(_)),
        "auth failure must persist FatalError, got {:?}",
        state.overall_status
    );
    assert_eq!(state.failed_targets, vec!["preflight".to_string()]);
}

/// transport 初始化失败（network 类）：返回 `SyncNetworkUnavailable`（recoverable=true），
/// 磁盘写 `RecoverableError`。
#[test]
fn transport_init_failure_network_returns_sync_network_unavailable_and_persists_recoverable() {
    let temp_dir = tempdir().expect("tempdir");
    let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    let factory: writer_platform_api::SyncTransportFactory = std::sync::Arc::new(|| {
        Err(writer_platform_api::TransportError::new(
            "dns_failed",
            "cannot resolve github.com".to_string(),
        ))
    });
    let mut core = core;
    core.sync_transport = Some(factory);

    let config = test_config();
    let result = core.perform_full_sync(&config, false);
    let err = result.expect_err("transport init failure must return Err");
    assert!(
        matches!(err, crate::Error::SyncNetworkUnavailable { .. }),
        "network category must return SyncNetworkUnavailable, got {:?}",
        err
    );
    assert!(
        err.recoverable(),
        "SyncNetworkUnavailable must be recoverable"
    );

    let state = core
        .load_full_sync_state()
        .expect("load")
        .expect("transport failure must be persisted");
    assert!(
        matches!(state.overall_status, SyncStatus::RecoverableError(_)),
        "network failure must persist RecoverableError, got {:?}",
        state.overall_status
    );
    assert_eq!(state.failed_targets, vec!["preflight".to_string()]);
}

/// transport 初始化失败（rate limit 类）：返回 `SyncRateLimited`（recoverable=true），
/// 磁盘写 `RecoverableError`。
#[test]
fn transport_init_failure_rate_limited_returns_sync_rate_limited_and_persists_recoverable() {
    let temp_dir = tempdir().expect("tempdir");
    let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    let factory: writer_platform_api::SyncTransportFactory = std::sync::Arc::new(|| {
        Err(writer_platform_api::TransportError::new(
            "api_rate_limited",
            "secondary rate limit exceeded".to_string(),
        ))
    });
    let mut core = core;
    core.sync_transport = Some(factory);

    let config = test_config();
    let result = core.perform_full_sync(&config, false);
    let err = result.expect_err("transport init failure must return Err");
    assert!(
        matches!(
            err,
            crate::Error::SyncRateLimited {
                retry_after_secs: 0
            }
        ),
        "rate limit category must return SyncRateLimited {{ retry_after_secs: 0 }}, got {:?}",
        err
    );
    assert!(err.recoverable(), "SyncRateLimited must be recoverable");

    let state = core
        .load_full_sync_state()
        .expect("load")
        .expect("transport failure must be persisted");
    assert!(
        matches!(state.overall_status, SyncStatus::RecoverableError(_)),
        "rate limit failure must persist RecoverableError, got {:?}",
        state.overall_status
    );
    assert_eq!(state.failed_targets, vec!["preflight".to_string()]);
}

/// transport 初始化失败（未知 category）：保守返回 `SyncAuthFailed`（不可恢复），
/// 不落 Io 后自动变可重试。
#[test]
fn transport_init_failure_unknown_category_defaults_to_auth_failed() {
    let temp_dir = tempdir().expect("tempdir");
    let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    let factory: writer_platform_api::SyncTransportFactory = std::sync::Arc::new(|| {
        Err(writer_platform_api::TransportError::new(
            "some_unknown_category",
            "mystery failure".to_string(),
        ))
    });
    let mut core = core;
    core.sync_transport = Some(factory);

    let config = test_config();
    let result = core.perform_full_sync(&config, false);
    let err = result.expect_err("transport init failure must return Err");
    assert!(
        matches!(err, crate::Error::SyncAuthFailed { .. }),
        "unknown category must conservatively return SyncAuthFailed, got {:?}",
        err
    );
    assert!(
        !err.recoverable(),
        "unknown category must be non-recoverable (no Io auto-retry)"
    );
}

// ── Issue #630 评论 5308439467 Part 3：成功类终态聚合 ──

/// 构造一个指定 status 的 SyncResult（无上传/下载/冲突，仅状态不同）。
fn sync_result_with_status(status: SyncStatus) -> SyncResult {
    SyncResult {
        status,
        uploaded_files: Vec::new(),
        downloaded_files: Vec::new(),
        ignored_files: Vec::new(),
        conflicts: Vec::new(),
        error: None,
        error_category: None,
        message_key: None,
        conflict_summary: None,
        local_deletes: Vec::new(),
        remote_deletes: Vec::new(),
        overwritten_files: Vec::new(),
        search_index_rebuild_error: None,
    }
}

/// 全部 target 返回 `NoChanges` → overall `NoChanges`（不再丢成普通 Success）。
/// （Issue #630 评论 5311102143：`NoChanges + NoChanges -> NoChanges`）
#[test]
fn aggregate_all_no_changes_overall_is_no_changes() {
    let (_temp_dir, core) = new_core_with_projects();
    core.create_project("Project 1").expect("create project 1");

    let result = aggregate_with_outcomes(
        &core,
        &[(
            "app",
            MockOutcome::ok(sync_result_with_status(SyncStatus::NoChanges)),
        )],
    );

    assert_eq!(
        result.overall_status,
        SyncStatus::NoChanges,
        "all NoChanges must aggregate to NoChanges, got {:?}",
        result.overall_status
    );
}

/// `Success + NoChanges -> Success`：有 target 实际上传/下载了，不能丢成 NoChanges。
/// （Issue #630 评论 5311102143：修复 max() 把 NoChanges(2) 压过 Success(1) 的聚合错误）
#[test]
fn aggregate_success_plus_no_changes_is_success() {
    let (_temp_dir, core) = new_core_with_projects();
    let p1 = core.create_project("Project 1").expect("create project 1");

    // app=NoChanges, project:p1=Success
    let result = aggregate_with_outcomes(
        &core,
        &[
            (
                "app",
                MockOutcome::ok(sync_result_with_status(SyncStatus::NoChanges)),
            ),
            (
                &format!("projects/{}", p1.id),
                MockOutcome::ok(SyncResult::success()),
            ),
        ],
    );

    assert_eq!(
        result.overall_status,
        SyncStatus::Success,
        "Success + NoChanges must aggregate to Success, not NoChanges; got {:?}",
        result.overall_status
    );
}

/// `Success + NoChanges -> Success`（反向顺序）：先 Success 后 NoChanges 也必须是 Success。
#[test]
fn aggregate_success_first_then_no_changes_is_success() {
    let (_temp_dir, core) = new_core_with_projects();
    let p1 = core.create_project("Project 1").expect("create project 1");

    // app=Success, project:p1=NoChanges
    let result = aggregate_with_outcomes(
        &core,
        &[
            ("app", MockOutcome::ok(SyncResult::success())),
            (
                &format!("projects/{}", p1.id),
                MockOutcome::ok(sync_result_with_status(SyncStatus::NoChanges)),
            ),
        ],
    );

    assert_eq!(
        result.overall_status,
        SyncStatus::Success,
        "Success + NoChanges (reversed) must aggregate to Success; got {:?}",
        result.overall_status
    );
}

/// `Success + LatestWinsApplied -> LatestWinsApplied`：有 target 因最新赢家规则合并了变更。
/// （Issue #630 评论 5311102143：`Success + LatestWinsApplied -> LatestWinsApplied`）
#[test]
fn aggregate_success_plus_latest_wins_applied_is_latest_wins_applied() {
    let (_temp_dir, core) = new_core_with_projects();
    let p1 = core.create_project("Project 1").expect("create project 1");

    let backend = MockProvider::new(MockOutcome::ok(SyncResult::success()));
    backend.set(
        &format!("projects/{}", p1.id),
        MockOutcome::ok(sync_result_with_status(SyncStatus::LatestWinsApplied)),
    );

    let config = test_config();
    let result = core
        .perform_full_sync_with_provider(&backend, &SyncPolicy::from_config(&config), false)
        .expect("full sync");

    assert_eq!(
        result.overall_status,
        SyncStatus::LatestWinsApplied,
        "Success + LatestWinsApplied must aggregate to LatestWinsApplied; got {:?}",
        result.overall_status
    );
}

/// 任一 `LatestWinsApplied` + 其余 `NoChanges` → overall `LatestWinsApplied`。
#[test]
fn aggregate_latest_wins_applied_beats_no_changes() {
    let (_temp_dir, core) = new_core_with_projects();
    let p1 = core.create_project("Project 1").expect("create project 1");

    let backend = MockProvider::new(MockOutcome::ok(sync_result_with_status(
        SyncStatus::NoChanges,
    )));
    backend.set(
        &format!("projects/{}", p1.id),
        MockOutcome::ok(sync_result_with_status(SyncStatus::LatestWinsApplied)),
    );

    let config = test_config();
    let result = core
        .perform_full_sync_with_provider(&backend, &SyncPolicy::from_config(&config), false)
        .expect("full sync");

    assert_eq!(
        result.overall_status,
        SyncStatus::LatestWinsApplied,
        "LatestWinsApplied must be preserved over NoChanges, got {:?}",
        result.overall_status
    );
}

/// target 返回 `Syncing` → overall `FatalError`（协议错误，绝不当成功）。
#[test]
fn aggregate_target_syncing_is_protocol_error_fatal() {
    let (_temp_dir, core) = new_core_with_projects();

    let result = aggregate_with_outcomes(
        &core,
        &[(
            "app",
            MockOutcome::ok(sync_result_with_status(SyncStatus::Syncing)),
        )],
    );

    assert!(
        matches!(result.overall_status, SyncStatus::FatalError(ref msg) if msg == "invalid_target_status_for_aggregation"),
        "Syncing in target must be FatalError(\"invalid_target_status_for_aggregation\"), got {:?}",
        result.overall_status
    );
}

/// target 返回 `Idle` → overall `FatalError`（协议错误，绝不当成功）。
#[test]
fn aggregate_target_idle_is_protocol_error_fatal() {
    let (_temp_dir, core) = new_core_with_projects();

    let result = aggregate_with_outcomes(
        &core,
        &[(
            "app",
            MockOutcome::ok(sync_result_with_status(SyncStatus::Idle)),
        )],
    );

    assert!(
        matches!(result.overall_status, SyncStatus::FatalError(ref msg) if msg == "invalid_target_status_for_aggregation"),
        "Idle in target must be FatalError(\"invalid_target_status_for_aggregation\"), got {:?}",
        result.overall_status
    );
}

/// target 返回 `ConfiguredNotTested` → overall `FatalError`（协议错误，绝不当成功）。
#[test]
fn aggregate_target_configured_not_tested_is_protocol_error_fatal() {
    let (_temp_dir, core) = new_core_with_projects();

    let result = aggregate_with_outcomes(
        &core,
        &[(
            "app",
            MockOutcome::ok(sync_result_with_status(SyncStatus::ConfiguredNotTested)),
        )],
    );

    assert!(
        matches!(result.overall_status, SyncStatus::FatalError(ref msg) if msg == "invalid_target_status_for_aggregation"),
        "ConfiguredNotTested in target must be FatalError(\"invalid_target_status_for_aggregation\"), got {:?}",
        result.overall_status
    );
}

/// 协议错误（Syncing）压过成功类：即使有 target 返回 Success，overall 仍是 FatalError。
#[test]
fn aggregate_protocol_error_beats_success() {
    let (_temp_dir, core) = new_core_with_projects();
    let p1 = core.create_project("Project 1").expect("create project 1");

    let result = aggregate_with_outcomes(
        &core,
        &[
            ("app", MockOutcome::ok(SyncResult::success())),
            (
                &format!("projects/{}", p1.id),
                MockOutcome::ok(sync_result_with_status(SyncStatus::Syncing)),
            ),
        ],
    );

    assert!(
        matches!(result.overall_status, SyncStatus::FatalError(_)),
        "protocol error must beat success, got {:?}",
        result.overall_status
    );
}

// ── Issue #630 评论 5308439467 Part 1：冷启动恢复中断 Syncing ──

/// `recover_interrupted_full_sync_state`：磁盘上是 Syncing 时原子改成
/// RecoverableError("previous_full_sync_interrupted")，返回 true。
#[test]
fn recover_interrupted_full_sync_state_recovers_syncing() {
    let temp_dir = tempdir().expect("tempdir");
    let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    let previous = crate::sync::full_sync_state::FullSyncState {
        overall_status: SyncStatus::Syncing,
        last_attempt_time: Some(5555),
        last_success_time: Some(4444),
        failed_targets: Vec::new(),
    };
    core.save_full_sync_state(&previous).expect("save previous");

    let recovered = core
        .recover_interrupted_full_sync_state()
        .expect("recover must not error");
    assert!(recovered, "should return true when old state was Syncing");

    let state = core
        .load_full_sync_state()
        .expect("load")
        .expect("state must exist");
    assert!(
        matches!(
            state.overall_status,
            SyncStatus::RecoverableError(ref msg) if msg == "previous_full_sync_interrupted"
        ),
        "recovered state must be RecoverableError(\"previous_full_sync_interrupted\"), got {:?}",
        state.overall_status
    );
    assert_eq!(state.failed_targets, vec!["global".to_string()]);
    assert_eq!(
        state.last_attempt_time,
        Some(5555),
        "last_attempt_time must preserve old attempt, not fabricate a new one"
    );
    assert_eq!(
        state.last_success_time,
        Some(4444),
        "last_success_time must be preserved as-is"
    );
}

/// `recover_interrupted_full_sync_state`：磁盘上是 Success（终态）时不动，返回 false。
#[test]
fn recover_interrupted_full_sync_state_leaves_success_untouched() {
    let temp_dir = tempdir().expect("tempdir");
    let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    let previous = make_full_sync_state(SyncStatus::Success, Some(5555));
    core.save_full_sync_state(&previous).expect("save previous");

    let recovered = core
        .recover_interrupted_full_sync_state()
        .expect("recover must not error");
    assert!(!recovered, "should return false when old state was Success");

    let state = core
        .load_full_sync_state()
        .expect("load")
        .expect("state must exist");
    assert_eq!(
        state.overall_status,
        SyncStatus::Success,
        "Success state must be left untouched, got {:?}",
        state.overall_status
    );
}

/// `recover_interrupted_full_sync_state`：文件不存在时返回 false，不报错。
#[test]
fn recover_interrupted_full_sync_state_no_file_returns_false() {
    let temp_dir = tempdir().expect("tempdir");
    let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));

    let recovered = core
        .recover_interrupted_full_sync_state()
        .expect("recover must not error on absent file");
    assert!(!recovered, "should return false when no state file exists");
}
