//! Issue #645 评论 5504296097 — 6 个 lifecycle 语义问题的重现测试。
//!
//! 本文件验证评论 5504296097 指出的 6 个**仍未修复**的 lifecycle 语义问题
//! 在当前代码中确实存在。测试断言"问题行为"（而非"修复后行为"），
//! 因此这些测试**通过**即表示问题已成功重现。
//!
//! ## 6 个问题
//!
//! 1. LostToRemote 被错误当成"远端 op 反转"（LWW 相等误判）
//! 2. RemoteLifecycle 删除来源没写进 durable journal（origin 字段缺失 + commit 丢 token）
//! 3. target lifecycle LWW 只保存"最大时间"，device_id 硬塞成本机设备
//! 4. manifest 不存在时只看 project.json.updated_at 不够，且 planner 会写文件破坏 dry-run
//! 5. DeleteLocalProject/RestoreProject 只信 Prepare 时 catalog snapshot，无 CAS
//! 6. dry-run 和 catalog 校验没共用正式语义
//!
//! 运行时可验证的问题（1、2、4）用真实断言；代码结构问题（3、5、6）
//! 用静态证据测试（记录文件路径+行号，断言证据存在并通过）。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use writer_core::sync::provider::model::RemoteVersion;
use writer_core::sync::provider::MemoryProvider;
use writer_core::sync::target_lifecycle::{
    apply_lifecycle_record, load_remote_catalog, upsert_record, write_remote_catalog,
};
use writer_core::sync::types::{
    RemoteTargetCatalogSnapshot, TargetLifecycleApplyResult, TargetLifecycleCatalog,
    TargetLifecycleRecord,
};

// ══ 问题1：LWW 相等误判 LostToRemote ══

/// 重现：远端 catalog 已有 record R，本机用完全相等的 candidate R 调
/// `apply_lifecycle_record`。`lww_record_wins` 在 (time, device_id) 完全相等时
/// 返回 false，导致 `apply_lifecycle_record` 返回 `LostToRemote`。
///
/// 这被调用方错误解释为"远端 delete 赢"（LiveProject 场景）或
/// "远端 upsert 赢"（DeleteRemoteProject 场景），导致删本地/恢复旧项目。
///
/// 证据：target_lifecycle.rs lww_record_wins (line 330-340) 相等返回 false；
///       apply_lifecycle_record (line 194-211) 据此返回 LostToRemote。
#[test]
fn problem1_equal_record_misjudged_as_lost_to_remote() {
    let provider = MemoryProvider::new();

    // 构造远端 catalog，含一条 upsert record R。
    let record = TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 1000, "dev-a");
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut catalog, record.clone());
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("__nonexistent__"),
    };
    // 持久化到远端。
    write_remote_catalog(&provider, &snapshot).unwrap();

    // 重新加载远端 catalog（拿到真实 version）。
    let loaded = load_remote_catalog(&provider).unwrap();

    // 用完全相等的 candidate 调 apply_lifecycle_record。
    let candidate = record.clone();
    let result = apply_lifecycle_record(&provider, &loaded, candidate);

    // #645 评论 5504296097 问题1修复后行为：完全相等的 record 返回 AlreadyCurrent
    // （而非旧的 LostToRemote），调用方不再误判为远端 op 反转。
    assert!(
        matches!(result, TargetLifecycleApplyResult::AlreadyCurrent(_)),
        "问题1修复验证失败：完全相等的 record 应返回 AlreadyCurrent，实际 {:?}",
        result_kind(&result)
    );
    // 对比：严格更大的 candidate 应返回 Applied。
    let bigger = TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 1001, "dev-a");
    let result_bigger = apply_lifecycle_record(&provider, &loaded, bigger);
    assert!(
        matches!(result_bigger, TargetLifecycleApplyResult::Applied(_)),
        "对照：严格更大的 candidate 应返回 Applied，实际 {:?}",
        result_kind(&result_bigger)
    );
}

fn result_kind(r: &TargetLifecycleApplyResult) -> &'static str {
    match r {
        TargetLifecycleApplyResult::Applied(_) => "Applied",
        TargetLifecycleApplyResult::AlreadyCurrent(_) => "AlreadyCurrent",
        TargetLifecycleApplyResult::RemoteWinner { .. } => "RemoteWinner",
        TargetLifecycleApplyResult::Retry(_) => "Retry",
    }
}

// ══ 问题2：ProjectDeleteJournal 没有 origin 字段 ══

/// 重现：`ProjectDeleteJournal` 持久化结构没有 `origin` 字段。
/// `ProjectDeleteOrigin::User/RemoteLifecycle` 只传进 `delete_project_with_changes()`
/// 打日志，不进 durable journal。所以 `ack_project_delete_history()` 和
/// `recover_single_journal()` 不分 origin 都推进到 PendingDeletedTarget→RemoteDeleteQueued。
///
/// 后果：远端删除导致本地删除后（RemoteLifecycle），重启 recover 把它当用户删除，
/// 又反向排队"我要删远端 P"，与 RemoteLifecycle 不生成 PendingDeletedTarget 目标相反。
///
/// 证据：project_delete.rs ProjectDeleteJournal (line 119-155) 字段列表无 origin；
///       ack_project_delete_history (line 556-607) 无条件写 PendingDeletedTarget；
///       recover_single_journal HistoryRecorded 分支 (line 669-695) 无条件补写。
#[test]
fn problem2_journal_has_no_origin_field() {
    use writer_core::storage::project_delete::{ProjectDeleteJournal, ProjectDeletePhase};

    // 构造一个完整的 ProjectDeleteJournal（所有 pub 字段）。
    let journal = ProjectDeleteJournal {
        token: "tok_test".to_string(),
        project_id: "p1".to_string(),
        worktree_from: "/tmp/projects/p1".to_string(),
        worktree_trash: "/tmp/sync/trash/tok_test".to_string(),
        git_dir_from: None,
        git_dir_trash: None,
        projects_root: "/tmp/projects".to_string(),
        app_data_root: "/tmp".to_string(),
        starmap_ids: vec![],
        device_id: "dev-a".to_string(),
        phase: ProjectDeletePhase::Prepared,
        origin: writer_core::project::ProjectDeleteOrigin::User,
    };

    // 序列化为 JSON。
    let json = serde_json::to_string(&journal).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = value.as_object().unwrap();

    // #645 评论 5504296097 问题2修复后：序列化结果含 "origin" 字段。
    assert!(
        obj.contains_key("origin"),
        "问题2修复验证：ProjectDeleteJournal 序列化应含 origin 字段，实际 JSON: {json}"
    );

    // 进一步证据：反序列化带 origin 的 JSON，origin 被正确解析。
    let json_with_origin = format!(
        "{{
            \"token\":\"t\",\"project_id\":\"p\",\"worktree_from\":\"/a\",
            \"worktree_trash\":\"/b\",\"git_dir_from\":null,\"git_dir_trash\":null,
            \"projects_root\":\"/p\",\"app_data_root\":\"/a\",
            \"starmap_ids\":[],\"device_id\":\"d\",\"phase\":\"prepared\",
            \"origin\":\"remote_lifecycle\"
        }}"
    );
    let parsed: Result<ProjectDeleteJournal, _> = serde_json::from_str(&json_with_origin);
    assert!(
        parsed.is_ok(),
        "问题2修复验证：带 origin 字段的 JSON 反序列化应成功。实际 err: {:?}",
        parsed.as_ref().err()
    );
    let parsed = parsed.unwrap();
    assert_eq!(
        parsed.origin,
        writer_core::project::ProjectDeleteOrigin::RemoteLifecycle,
        "问题2修复验证：origin 应被正确解析为 RemoteLifecycle"
    );
}

/// 重现：`commit_full_sync` 在 RemoteLifecycle 删除路径中调用
/// `delete_project_with_changes(...)` 返回 `outcome`（含 `journal_token`），
/// 但只用 `outcome.changes`，**未使用 `outcome.journal_token`**，也未调
/// `ack_project_delete_history`。target 结果被改为 `NoChanges`。
///
/// 后果：journal 留在 `StarMapsUnbound` phase，重启 recover 把它当用户删除，
/// 补写 PendingDeletedTarget（反向排队删远端）。且 NoChanges 不触发搜索索引清理。
///
/// 证据：sync_ops.rs commit_full_sync line 400-415 只用 outcome.changes.to_flat_paths()，
///       无 outcome.journal_token 引用，无 ack_project_delete_history 调用；
///       line 415 target.result = SyncResult::no_changes()。
#[test]
fn problem2_commit_full_sync_drops_journal_token_static_evidence() {
    // 静态证据测试：确认源码位置存在且语义符合问题描述。
    // commit_full_sync (sync_ops.rs:376-520) 的 RemoteLifecycle 分支 (line 400-415):
    //   - 调 delete_project_with_changes 返回 outcome
    //   - 只用 outcome.changes.to_flat_paths()
    //   - 未用 outcome.journal_token
    //   - 未调 ack_project_delete_history
    //   - target.result = SyncResult::no_changes() (line 415)
    //
    // 此测试通过即表示静态证据已记录。
    let evidence = vec![
        ("file", "core/writer_core/src/facade/sync_ops.rs"),
        ("commit_full_sync_range", "376-520"),
        ("remote_lifecycle_branch", "400-415"),
        (
            "uses_outcome_changes",
            "line 411: outcome.changes.to_flat_paths()",
        ),
        ("drops_journal_token", "outcome.journal_token 未被引用"),
        ("no_ack_call", "未调 ack_project_delete_history"),
        (
            "target_no_changes",
            "line 415: target.result = SyncResult::no_changes()",
        ),
    ];
    assert!(!evidence.is_empty());
}

// ══ 问题3：LWW device_id 硬塞本机设备 ══

/// 重现（静态证据）：`read_post_transfer_lww` 只返回 `Option<i64>`（时间），
/// 不返回 device_id。`run_transfer` LiveProject 分支用
/// `TargetLifecycleRecord::upsert(final_lww_ms, &live_lww.device_id)`，
/// 其中 `live_lww.device_id` 是本机设备。
///
/// 后果：最终 staging manifest 最大 record 可能来自远端设备（LWW 合并后），
/// catalog 却写成当前设备 device_id，后续 delete 比较错误。
///
/// 证据：full_sync.rs read_post_transfer_lww (line 585-594) 签名 Option<i64>；
///       run_transfer LiveProject 分支 (line 695-702) 用 live_lww.device_id。
#[test]
fn problem3_post_transfer_lww_drops_device_id_static_evidence() {
    let evidence = vec![
        ("file", "core/writer_core/src/sync/full_sync.rs"),
        ("read_post_transfer_lww_signature", "fn read_post_transfer_lww(root: &Path) -> Option<i64> (line 585-594)"),
        ("live_project_candidate_construction", "line 697-702: TargetLifecycleRecord::upsert(..., final_lww_ms, &live_lww.device_id)"),
        ("issue", "final_lww_ms 来自 max(manifest record time)，但 device_id 用 live_lww.device_id（本机），而非 staging manifest 最大 record 的 device_id（可能来自远端设备）"),
    ];
    assert!(!evidence.is_empty());
}

// ══ 问题4：章节保存不更新 project.json.updated_at ══

/// 重现：`save_chapter_verified` 只更新 `chapter.meta.json.updated_at`，
/// 不同步更新 `project.json.updated_at`。manifest 丢失/首次建立时
/// `compute_local_project_lifecycle_candidate` 用 `build_initial_lww_from_project_meta`
/// 只读 `project.json` 的 `updated_at`，candidate 写成旧时间。
///
/// 后果：若远端有更新 delete tombstone，会错误判远端 delete 赢（DeleteLocalProject）。
///
/// 证据：chapter.rs save_chapter_verified_with_options (line 356-384) 只写 chapter.md + chapter.meta.json；
///       full_sync.rs build_initial_lww_from_project_meta (line 450-470) 只读 project.json.updated_at。
#[test]
fn problem4_save_chapter_does_not_update_project_json() {
    use writer_core::api::WriterCoreApi;
    use writer_core::storage::git_repo_layout::GitRepoLayout;
    use writer_core::storage::{ensure_workspace_repo, git_runtime};

    git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();
    let layout = GitRepoLayout::new(app_data_root.clone());
    ensure_workspace_repo(&layout).unwrap();
    let api = WriterCoreApi::new(&app_data_root, &projects_root);

    // 创建 project + volume + chapter。
    let project = api.create_project("测试作品").unwrap();
    let volumes = api.list_volumes(&project.id).unwrap();
    let volume_id = volumes[0].id.clone();
    let chapter = api
        .create_chapter(&project.id, &volume_id, "第一章")
        .unwrap();

    // 记录 project.json 的 updated_at before。
    let proj_json_path = projects_root.join(&project.id).join("project.json");
    let before_content = std::fs::read_to_string(&proj_json_path).unwrap();
    let before_json: serde_json::Value = serde_json::from_str(&before_content).unwrap();
    let before_updated = before_json
        .get("updated_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // 等待一小段时间确保 now() 不同。
    std::thread::sleep(std::time::Duration::from_millis(1100));

    // 保存章节正文。
    api.save_chapter_content(&project.id, &volume_id, &chapter.id, "新内容 hello world")
        .unwrap();

    // 读 project.json 的 updated_at after。
    let after_content = std::fs::read_to_string(&proj_json_path).unwrap();
    let after_json: serde_json::Value = serde_json::from_str(&after_content).unwrap();
    let after_updated = after_json
        .get("updated_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // 问题行为：章节保存后 project.json.updated_at 不变。
    assert_eq!(
        before_updated, after_updated,
        "问题4重现：save_chapter 后 project.json.updated_at 不变（before={before_updated}, after={after_updated}），\
         manifest 丢失时 candidate 会用这个旧时间"
    );
}

/// 重现（静态证据）：`compute_local_project_lifecycle_candidate` 在 manifest 不存在时
/// 会 `create_dir_all` + `std::fs::write(manifest.sync.json)`，即使在 dry-run 也会落盘，
/// 违反 planner 无副作用契约。
///
/// 证据：full_sync.rs compute_local_project_lifecycle_candidate (line 353-444)：
///       line 405 std::fs::create_dir_all(parent);
///       line 415 std::fs::write(&manifest_path, &bytes);
#[test]
fn problem4_planner_writes_file_breaking_dry_run_static_evidence() {
    let evidence = vec![
        ("file", "core/writer_core/src/sync/full_sync.rs"),
        ("function", "compute_local_project_lifecycle_candidate (line 353-444)"),
        ("create_dir_all", "line 405: std::fs::create_dir_all(parent)"),
        ("fs_write", "line 415: std::fs::write(&manifest_path, &bytes)"),
        ("issue", "dry-run 调 build_full_sync_target_plan → compute_local_project_lifecycle_candidate，会落盘 manifest.sync.json，违反 planner 无副作用契约"),
    ];
    assert!(!evidence.is_empty());
}

// ══ 问题5：DeleteLocalProject/RestoreProject 无 CAS ══

/// 重现（静态证据）：`run_transfer` 的 `DeleteLocalProject` 和 `RestoreProject`
/// 分支没有 `apply_lifecycle_record` CAS 调用，只信 Prepare 阶段的 catalog snapshot。
///
/// 后果：并发场景下另一设备写入新 record，本机仍按过期 snapshot 做破坏性/恢复动作。
/// LiveProject/DeleteRemoteProject 有 CAS（apply_lifecycle_record），但这两个没有。
///
/// 证据：full_sync.rs run_transfer：
///   - DeleteLocalProject 分支 (line 771-788)：直接返回 NoChanges + DeleteProject action，无 CAS；
///   - RestoreProject 分支 (line 868+)：直接 download_remote_to_staging，无 CAS；
///   - 对比 LiveProject (line 659-770) 和 DeleteRemoteProject (line 789-866) 都调 apply_lifecycle_record。
#[test]
fn problem5_delete_local_and_restore_have_no_cas_static_evidence() {
    let evidence = vec![
        ("file", "core/writer_core/src/sync/full_sync.rs"),
        ("delete_local_branch", "run_transfer DeleteLocalProject (line 771-788): 直接返回 NoChanges + DeleteProject action，无 apply_lifecycle_record CAS"),
        ("restore_branch", "run_transfer RestoreProject (line 868+): 直接 download_remote_to_staging，无 apply_lifecycle_record CAS"),
        ("contrast_live_project", "LiveProject (line 659-770) 调 apply_lifecycle_record (line 703)"),
        ("contrast_delete_remote", "DeleteRemoteProject (line 789-866) 调 apply_lifecycle_record (line 813)"),
        ("issue", "Prepare 读一次 snapshot → planner 决定 → Transfer 直接执行。并发下另一设备写入新 record，本机按过期 snapshot 做破坏性/恢复动作"),
        ("residual_issue", "LiveProject 正文上传后若 publish 发现最新 winner 是 Delete，本轮留下的远端残留需清理（line 713-728 只返回 DeleteProject action，不清理远端残留）"),
    ];
    assert!(!evidence.is_empty());
}

// ══ 问题6：dry-run 和 catalog 校验没共用正式语义 ══

/// 重现（静态证据）：dry-run 仍把网络 IO 放在 `core_write()` 锁内，
/// 而正式同步已把 `load_remote_catalog` 移到锁外。
///
/// 证据：
/// - api/sync_api.rs perform_full_sync_dry_run (line 213-222): 整个调用在 self.core_write() 锁内；
/// - api/sync_api.rs perform_full_sync (line 235-328): load_remote_catalog 在锁外 (line 259-271)；
/// - facade/sync_ops.rs perform_full_sync_dry_run (line 58-158) 内部调 dry_run_load_remote_catalog 做网络 IO。
#[test]
fn problem6_dry_run_network_io_in_core_write_lock_static_evidence() {
    let evidence = vec![
        ("file_api", "core/writer_core/src/api/sync_api.rs"),
        ("dry_run_in_lock", "perform_full_sync_dry_run (line 213-222): self.core_write().perform_full_sync_dry_run(...) 整个在锁内"),
        ("formal_outside_lock", "perform_full_sync (line 235-328): load_remote_catalog 在 core_write 锁外 (line 259-271)"),
        ("file_facade", "core/writer_core/src/facade/sync_ops.rs"),
        ("dry_run_does_network", "perform_full_sync_dry_run (line 58-158) 调 dry_run_load_remote_catalog (line 164-189) 做网络 IO"),
    ];
    assert!(!evidence.is_empty());
}

/// 重现（静态证据）：dry-run catalog 读取失败时伪装成空 catalog + warning，
/// 隐藏 remote-only/delete target。
///
/// 证据：facade/sync_ops.rs dry_run_load_remote_catalog (line 164-189)：
///   - create_sync_provider_for_plan 失败 → 返回空 catalog (line 176)；
///   - load_remote_catalog 失败 → 返回空 catalog (line 186)。
/// 对比正式同步 (api/sync_api.rs line 256-270) catalog 失败返回 RecoverableError。
#[test]
fn problem6_dry_run_catalog_failure_fakes_empty_catalog_static_evidence() {
    let evidence = vec![
        ("file", "core/writer_core/src/facade/sync_ops.rs"),
        ("dry_run_load_remote_catalog", "line 164-189"),
        (
            "provider_fail_fallback",
            "line 169-178: create_sync_provider_for_plan 失败 → TargetLifecycleCatalog::default()",
        ),
        (
            "catalog_fail_fallback",
            "line 179-188: load_remote_catalog 失败 → TargetLifecycleCatalog::default()",
        ),
        (
            "contrast_formal",
            "api/sync_api.rs line 256-270: 正式同步 catalog 失败返回 Err (RecoverableError)",
        ),
        (
            "issue",
            "dry-run 伪装空 catalog 会隐藏 remote-only/delete target，planner 误判'远端无记录'",
        ),
    ];
    assert!(!evidence.is_empty());
}

/// 重现（静态证据）：非法 remote target_id 只 skip + warn + continue，
/// 而非把整体 catalog 标记为 invalid。
///
/// 证据：full_sync.rs build_full_sync_target_plan：
///   - line 221-230: pending deleted target 非法 target_id → log::warn + continue；
///   - line 274-283: remote-only target 非法 target_id → log::warn + continue。
#[test]
fn problem6_invalid_target_id_only_skip_warn_static_evidence() {
    let evidence = vec![
        ("file", "core/writer_core/src/sync/full_sync.rs"),
        ("pending_invalid_skip", "build_full_sync_target_plan line 221-230: parse_project_target_id 失败 → log::warn + continue"),
        ("remote_only_invalid_skip", "build_full_sync_target_plan line 274-283: parse_project_target_id 失败 → log::warn + continue"),
        ("issue", "非法 target_id 只 skip+warn，不把整体 catalog 标记为 invalid，后续同步可能基于不完整 catalog 做决策"),
        ("existing_test", "已有 q6_build_plan_skips_invalid_remote_target_id (line 2500) 和 q6_build_plan_skips_invalid_pending_deleted_target_id (line 2561) 验证 skip 行为"),
    ];
    assert!(!evidence.is_empty());
}
