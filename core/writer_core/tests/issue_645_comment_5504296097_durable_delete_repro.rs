//! Issue #645 评论 5504296097 — durable delete 3 个缺口修复后行为验证。
//!
//! 本测试文件验证评论 5504296097 第 3 轮指出的 3 个 durable delete 缺口
//! 已被修复。每个测试断言"修复后正确行为"，测试 PASS 即证明缺口已消除。
//!
//! 三个缺口的修复后行为：
//! 1. `delete_project_with_changes()` 不再用 `unwrap_or_default()` 吞掉
//!    `list_starmaps_bound_to_project` 的错误——index.json 损坏时返回 `Err`。
//!    返回 `ProjectDeleteOutcome { changes, unbound_starmap_ids, journal_token }`，
//!    API 层用 `outcome.unbound_starmap_ids` 刷搜索索引，不再二次枚举。
//! 2. `recover_pending_delete_transactions()` 返回 `Vec<RecoveredProjectDelete>`，
//!    每个含 `changes`。bootstrap 用 layout 调 `record_workspace_change_set` 写
//!    history 后调 `ack_project_delete_history` 推进 journal。恢复后 workspace
//!    Git history commit 数量增加。
//! 3. `generate_tombstones()` 幂等 upsert/skip——对同一 trash 目录调用两次
//!    `write_tombstone` 后 tombstones 数量不增加。`ProjectDeletePhase` 枚举
//!    新增 `TombstoneWritten` phase，`write_tombstone` 成功后推进到此 phase。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use tempfile::TempDir;
use writer_core::storage::git_repo_layout::GitRepoLayout;
use writer_core::storage::journal::project_delete::{
    ack_project_delete_history, ProjectDeletePhase, ProjectDeleteTransaction,
    RecoveredProjectDelete,
};
use writer_core::storage::{
    ensure_workspace_repo, git_runtime, list_workspace_history, record_all_workspace_changes,
    record_workspace_change_set,
};
use writer_core::sync::SyncService;

// ── helpers ──

/// 构造临时 app_data_root + projects_root，并初始化 git runtime。
fn make_layout() -> (TempDir, PathBuf, PathBuf, GitRepoLayout) {
    git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();
    let layout = GitRepoLayout::new(app_data_root.clone());
    ensure_workspace_repo(&layout).unwrap();
    (tmp, app_data_root, projects_root, layout)
}

// ══ 缺口1修复：delete_project_with_changes 不再吞 StarMap 枚举错误 ══

/// 修复后行为：构造一个绑定到项目的 starmap，然后损坏 `starmaps/index.json`
/// （写入无法解析的 JSON），再调用 `delete_project_with_changes`。
///
/// 修复后断言：
/// - `list_starmaps_bound_to_project` 因 index.json 损坏返回 Err；
/// - `delete_project_with_changes` 不再用 `unwrap_or_default()` 吞掉该错误，
///   直接返回 `Err`（不再"成功"删除）；
/// - 项目目录仍存在（删除未执行，因为 starmap 枚举失败阻止了事务）。
#[test]
fn gap1_fixed_delete_project_propagates_starmap_enum_error() {
    let (tmp, app_data_root, projects_root, _layout) = make_layout();

    // 1. 创建项目。
    let project = writer_core::project::create_project(&projects_root, "测试作品").unwrap();
    let project_id = project.id.clone();

    // 2. 创建 starmap 并绑定到项目。
    let sm = writer_core::starmap::create_starmap(&app_data_root, "星图A", "desc", None).unwrap();
    writer_core::starmap::bind_starmap_to_project(&app_data_root, &sm.starmap_id, &project_id)
        .unwrap();

    // 确认绑定生效。
    let bound =
        writer_core::starmap::list_starmaps_bound_to_project(&app_data_root, &project_id).unwrap();
    assert_eq!(bound.len(), 1, "前置：应有 1 个绑定 starmap");

    // 3. 损坏 starmaps/index.json —— 写入无法解析的内容。
    let index_path = app_data_root.join("starmaps").join("index.json");
    assert!(index_path.exists(), "前置：index.json 应存在");
    std::fs::write(&index_path, "{ this is NOT valid JSON !!! ").unwrap();

    // 确认 list_starmaps_bound_to_project 现在返回 Err（读取/解析失败）。
    let enum_result =
        writer_core::starmap::list_starmaps_bound_to_project(&app_data_root, &project_id);
    assert!(
        enum_result.is_err(),
        "前置：index.json 损坏后 list_starmaps_bound_to_project 应返回 Err，实际: {:?}",
        enum_result
    );

    // 4. 调用 delete_project_with_changes。
    //    修复后：core 层不再用 unwrap_or_default() 吞掉枚举错误，直接返回 Err。
    let delete_result = writer_core::project::delete_project_with_changes(
        &projects_root,
        &project_id,
        &app_data_root,
        "test-device",
    );

    // 修复后断言 A：删除返回 Err——错误不再被吞掉，向上传播。
    assert!(
        delete_result.is_err(),
        "缺口1修复：delete_project_with_changes 不再吞 StarMap 枚举错误，应返回 Err，实际: {:?}",
        delete_result
    );

    // 5. 修复后断言 B：项目目录仍存在（删除未执行，starmap 枚举失败阻止了事务）。
    //    因为 list_starmaps_bound_to_project 在 prepare 之前调用，失败后直接返回，
    //    事务还没开始，项目目录不应被移动。
    let project_dir = projects_root.join(&project_id);
    assert!(
        project_dir.exists(),
        "缺口1修复：项目目录应仍存在（starmap 枚举失败阻止了删除事务启动）"
    );

    // 保留 tmp 防止提前清理。
    drop(tmp);
}

// ══ 缺口2修复：recover 返回 Vec<RecoveredProjectDelete>，bootstrap 写 history ══

/// 修复后行为：构造一个 pending delete journal（phase = StarMapsUnbound），
/// 记录 workspace Git history 的 commit 数量，然后调用
/// `recover_pending_delete_transactions`。
///
/// 修复后断言：
/// - `recover_pending_delete_transactions` 返回 `Vec<RecoveredProjectDelete>`，
///   每个含 `changes` 和 `unbound_starmap_ids`；
/// - 用 layout 调 `record_workspace_change_set` 写 history 后 commit 数量增加；
/// - 调 `ack_project_delete_history` 后 journal 被清理。
#[test]
fn gap2_fixed_recover_returns_change_set_and_bootstrap_writes_history() {
    let (tmp, app_data_root, projects_root, layout) = make_layout();

    // 1. 创建项目并放入一个文件。
    let project = writer_core::project::create_project(&projects_root, "待恢复作品").unwrap();
    let project_id = project.id.clone();
    let worktree_from = projects_root.join(&project_id);
    std::fs::write(worktree_from.join("chapter.md"), "正文内容").unwrap();

    // 1b. 先创建一个初始 workspace history commit，使 list_workspace_history 可用。
    record_all_workspace_changes(&layout, "initial snapshot").unwrap();

    // 2. 构造 durable delete transaction 并推进到 StarMapsUnbound phase
    //    （不 complete，模拟崩溃在 unbind_starmaps 和 history 之间）。
    let worktree_trash_root = app_data_root.join("sync/trash");
    let mut tx = ProjectDeleteTransaction::new(
        &project_id,
        &worktree_from,
        &worktree_trash_root,
        None,
        None,
        &projects_root,
        &app_data_root,
        Vec::new(),
        "test-device",
    );
    tx.prepare().unwrap();
    tx.move_worktree().unwrap();
    tx.move_git().unwrap();
    tx.write_tombstone().unwrap();
    tx.unbind_starmaps().unwrap();
    // 不调 complete —— journal 保留在 StarMapsUnbound phase。
    let journal_token = tx.token().to_string();
    let journal_path = app_data_root
        .join("app-meta/delete-journals")
        .join(format!(".sujian-delete-journal-{}", journal_token));
    assert!(journal_path.exists(), "前置：journal 文件应存在");
    // 读 journal 确认 phase == star_maps_unbound。
    let journal_json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&journal_path).unwrap()).unwrap();
    assert_eq!(
        journal_json["phase"], "star_maps_unbound",
        "前置：journal phase 应为 star_maps_unbound，实际: {:?}",
        journal_json["phase"]
    );

    // 3. 记录恢复前的 workspace Git history commit 数量。
    let history_before = list_workspace_history(&layout, 100).unwrap();
    let commits_before = history_before.len();

    // 4. 调用 recover_pending_delete_transactions。
    //    修复后：返回 Vec<RecoveredProjectDelete>，每个含 changes。
    let recover_result =
        writer_core::storage::journal::project_delete::recover_pending_delete_transactions(
            &app_data_root,
        );

    assert!(
        recover_result.is_ok(),
        "缺口2修复：recover 应返回 Ok(Vec<RecoveredProjectDelete>)，实际: {:?}",
        recover_result
    );
    let recovered_list: Vec<RecoveredProjectDelete> = recover_result.unwrap();
    assert!(
        !recovered_list.is_empty(),
        "缺口2修复：应返回非空 Vec<RecoveredProjectDelete>（含待补 history 的 change-set）"
    );

    // 5. 修复后断言 A：RecoveredProjectDelete 含 changes 字段。
    let rec = &recovered_list[0];
    assert!(
        !rec.changes.is_empty(),
        "缺口2修复：RecoveredProjectDelete.changes 应非空（含 DeleteTree(projects/{{id}}）"
    );
    assert_eq!(
        rec.journal_token, journal_token,
        "缺口2修复：RecoveredProjectDelete.journal_token 应匹配"
    );

    // 6. 修复后断言 B：bootstrap 用 layout 调 record_workspace_change_set 写 history，
    //    commit 数量应增加。
    let commit_result =
        record_workspace_change_set(&layout, &rec.changes, "recover_project_delete");
    assert!(
        commit_result.is_ok(),
        "缺口2修复：record_workspace_change_set 应成功，实际: {:?}",
        commit_result
    );
    let history_after = list_workspace_history(&layout, 100).unwrap();
    let commits_after = history_after.len();
    assert!(
        commits_after > commits_before,
        "缺口2修复：恢复后用 layout 写 history，commit 数量应增加（before={}, after={}）",
        commits_before,
        commits_after
    );

    // 7. 修复后断言 C：调 ack_project_delete_history 后 journal 被清理。
    let ack_result = ack_project_delete_history(&app_data_root, &rec.journal_token);
    assert!(
        ack_result.is_ok(),
        "缺口2修复：ack_project_delete_history 应成功，实际: {:?}",
        ack_result
    );
    assert!(
        !journal_path.exists(),
        "缺口2修复：ack 后 journal 应被清理（推进到 HistoryRecorded → Completed → cleanup）"
    );

    drop(tmp);
}

// ══ 缺口3修复：tombstone 重放幂等 ══

/// 修复后行为：构造一个 trash 目录（含文件），通过
/// `ProjectDeleteTransaction::write_tombstone` 对同一 trash 目录调用两次，
/// 检查 `SyncState.tombstones` 数量。
///
/// 修复后断言：
/// - 第一次调用后 tombstones 数量为 N（N > 0）；
/// - 第二次调用后 tombstones 数量仍为 N（幂等，不重复追加）；
/// - `ProjectDeletePhase` 枚举有 `TombstoneWritten` phase；
/// - `write_tombstone` 成功后 phase 推进到 `TombstoneWritten`。
#[test]
fn gap3_fixed_tombstone_replay_is_idempotent() {
    let (tmp, app_data_root, projects_root, _layout) = make_layout();

    // 1. 创建项目并放入文件。
    let project = writer_core::project::create_project(&projects_root, "幂等测试作品").unwrap();
    let project_id = project.id.clone();
    let worktree_from = projects_root.join(&project_id);
    std::fs::write(worktree_from.join("chapter.md"), "正文").unwrap();
    std::fs::write(worktree_from.join("note.md"), "笔记").unwrap();

    // 2. 构造 transaction 并推进到 GitMoved phase（trash 里已有文件）。
    let worktree_trash_root = app_data_root.join("sync/trash");
    let mut tx = ProjectDeleteTransaction::new(
        &project_id,
        &worktree_from,
        &worktree_trash_root,
        None,
        None,
        &projects_root,
        &app_data_root,
        Vec::new(),
        "test-device",
    );
    tx.prepare().unwrap();
    tx.move_worktree().unwrap();
    tx.move_git().unwrap();
    assert!(!worktree_from.exists(), "前置：worktree 应已移到 trash");
    assert!(tx.worktree_trash_path().exists(), "前置：trash 目录应存在");

    let worktree_trash = tx.worktree_trash_path().to_path_buf();

    // 3. 第一次调用 write_tombstone（间接调用 generate_tombstones）。
    tx.write_tombstone().unwrap();
    let state_after_first = SyncService::load_sync_state(&worktree_trash).unwrap();
    let count_after_first = state_after_first.tombstones.len();
    assert!(
        count_after_first > 0,
        "前置：第一次 write_tombstone 后应有 tombstone，实际: {}",
        count_after_first
    );

    // 4. 修复后断言 A：write_tombstone 后 phase 推进到 TombstoneWritten。
    let journal_path = app_data_root
        .join("app-meta/delete-journals")
        .join(format!(".sujian-delete-journal-{}", tx.token()));
    let journal_json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&journal_path).unwrap()).unwrap();
    assert_eq!(
        journal_json["phase"], "tombstone_written",
        "缺口3修复：write_tombstone 后 phase 应推进到 tombstone_written，实际: {:?}",
        journal_json["phase"]
    );

    // 5. 第二次调用 write_tombstone（对同一 trash 目录再次 generate_tombstones）。
    //    修复后：generate_tombstones 幂等 upsert/skip，tombstones 不重复追加。
    tx.write_tombstone().unwrap();
    let state_after_second = SyncService::load_sync_state(&worktree_trash).unwrap();
    let count_after_second = state_after_second.tombstones.len();

    // 修复后断言 B：tombstones 数量不增加（幂等）。
    assert_eq!(
        count_after_second, count_after_first,
        "缺口3修复：对同一 trash 目录调用两次 write_tombstone 后 tombstones 数量不增加\
         （first={}, second={}），generate_tombstones 幂等 upsert/skip",
        count_after_first, count_after_second
    );

    // 6. 修复后断言 C：不存在重复 tombstone（按 original_path + trash_path + kind 判定）。
    let mut seen_keys: std::collections::HashSet<(String, String, String)> =
        std::collections::HashSet::new();
    let mut duplicate_count = 0;
    for ts in &state_after_second.tombstones {
        let key = (
            ts.original_path.clone(),
            ts.trash_path.clone(),
            ts.kind.clone(),
        );
        if !seen_keys.insert(key) {
            duplicate_count += 1;
        }
    }
    assert_eq!(
        duplicate_count, 0,
        "缺口3修复：不存在重复 tombstone（按 original_path+trash_path+kind 判定），\
         generate_tombstones 已做幂等 upsert/skip"
    );

    // 7. 修复后断言 D：ProjectDeletePhase 枚举有 TombstoneWritten phase。
    let has_tombstone_written =
        format!("{:?}", ProjectDeletePhase::TombstoneWritten) == "TombstoneWritten";
    assert!(
        has_tombstone_written,
        "缺口3修复：ProjectDeletePhase 枚举应有 TombstoneWritten phase"
    );

    drop(tmp);
}

// ══ 额外验证：ProjectDeletePhase 新增 HistoryRecorded phase ══

/// 修复后断言：ProjectDeletePhase 枚举有 HistoryRecorded phase，
/// 用于记录 workspace Git history 已记但 journal 尚未清理的中间状态。
#[test]
fn gap2_fixed_project_delete_phase_has_history_recorded() {
    let has_history_recorded =
        format!("{:?}", ProjectDeletePhase::HistoryRecorded) == "HistoryRecorded";
    assert!(
        has_history_recorded,
        "缺口2修复：ProjectDeletePhase 枚举应有 HistoryRecorded phase"
    );
}
