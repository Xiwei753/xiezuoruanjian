use super::*;
use crate::sync::types::PendingDeletedTarget;
use tempfile::tempdir;

/// 验证 journal 序列化/反序列化。
#[test]
fn test_journal_serde() {
    let journal = ProjectDeleteJournal {
        token: "test_token".to_string(),
        project_id: "test_project".to_string(),
        worktree_from: "/projects/test".to_string(),
        worktree_trash: "/trash/test_token".to_string(),
        git_dir_from: Some("/private/git/test".to_string()),
        git_dir_trash: Some("/trash/test_token.git".to_string()),
        projects_root: "/projects".to_string(),
        app_data_root: "/app".to_string(),
        starmap_ids: Vec::new(),
        device_id: "test-device".to_string(),
        phase: ProjectDeletePhase::Prepared,
    };

    let json = serde_json::to_string(&journal).unwrap();
    let parsed: ProjectDeleteJournal = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.token, journal.token);
    assert_eq!(parsed.project_id, journal.project_id);
    assert_eq!(parsed.phase, ProjectDeletePhase::Prepared);
}

/// 验证完整删除流程：prepare → move_worktree → move_git → write_tombstone → complete → cleanup。
#[test]
fn test_full_delete_flow() {
    let temp_dir = tempdir().unwrap();
    let app_data_root = temp_dir.path();
    let projects_root = app_data_root.join("projects");
    let private_root = app_data_root.join("private");
    fs::create_dir_all(&projects_root).unwrap();
    fs::create_dir_all(&private_root).unwrap();

    // 创建 worktree 和 private git_dir。
    let project_id = "test-project";
    let worktree_from = projects_root.join(project_id);
    fs::create_dir_all(&worktree_from).unwrap();
    fs::write(worktree_from.join("project.json"), "{}").unwrap();

    let git_dir_from = private_root.join("sujian-git").join(project_id);
    fs::create_dir_all(&git_dir_from).unwrap();
    fs::write(git_dir_from.join("HEAD"), "ref: refs/heads/main\n").unwrap();

    // #644 评论 5495945801 问题2：传 trash root，token 在 new() 内部生成。
    let worktree_trash_root = app_data_root.join("sync/trash");
    let git_dir_trash_root = app_data_root.join("private/trash");

    let mut tx = ProjectDeleteTransaction::new(
        project_id,
        &worktree_from,
        &worktree_trash_root,
        Some(&git_dir_from),
        Some(&git_dir_trash_root),
        &projects_root,
        app_data_root,
        Vec::new(),
        "test-device",
    );

    // 获取事务实际使用的 trash 路径（token 拼出来的）。
    let worktree_trash = tx.worktree_trash_path().to_path_buf();
    let git_dir_trash = tx.git_dir_trash_path().unwrap().to_path_buf();

    // prepare。
    tx.prepare().unwrap();
    assert!(tx.journal_path.exists());
    assert_eq!(tx.journal.phase, ProjectDeletePhase::Prepared);

    // move_worktree。
    tx.move_worktree().unwrap();
    assert_eq!(tx.journal.phase, ProjectDeletePhase::WorktreeMoved);
    assert!(!worktree_from.exists());
    assert!(worktree_trash.exists());

    // move_git。
    tx.move_git().unwrap();
    assert_eq!(tx.journal.phase, ProjectDeletePhase::GitMoved);
    assert!(!git_dir_from.exists());
    assert!(git_dir_trash.exists());

    // write_tombstone。
    tx.write_tombstone().unwrap();

    // #645 评论 5504296097 问题2：unbind_starmaps（空 starmap_ids，应直接推进 phase）。
    tx.unbind_starmaps().unwrap();
    assert_eq!(tx.journal.phase, ProjectDeletePhase::StarMapsUnbound);

    // complete。
    tx.complete().unwrap();
    assert_eq!(tx.journal.phase, ProjectDeletePhase::Completed);

    // cleanup。
    let journal_path = tx.journal_path.clone();
    tx.cleanup_journal().unwrap();
    assert!(!journal_path.exists());
}

/// 验证崩溃恢复：Prepared 阶段崩溃，重启后继续完成删除。
#[test]
fn test_recovery_from_prepared() {
    let temp_dir = tempdir().unwrap();
    let app_data_root = temp_dir.path();
    let projects_root = app_data_root.join("projects");
    let private_root = app_data_root.join("private");
    fs::create_dir_all(&projects_root).unwrap();
    fs::create_dir_all(&private_root).unwrap();

    let project_id = "test-project";
    let worktree_from = projects_root.join(project_id);
    fs::create_dir_all(&worktree_from).unwrap();
    fs::write(worktree_from.join("project.json"), "{}").unwrap();

    let git_dir_from = private_root.join("sujian-git").join(project_id);
    fs::create_dir_all(&git_dir_from).unwrap();
    fs::write(git_dir_from.join("HEAD"), "ref: refs/heads/main\n").unwrap();

    // 模拟 Prepared 阶段崩溃：journal 已写，但还没移动。
    let trash_parent = app_data_root.join("sync/trash");
    let git_trash_parent = app_data_root.join("private/trash");
    let mut tx = ProjectDeleteTransaction::new(
        project_id,
        &worktree_from,
        &trash_parent,
        Some(&git_dir_from),
        Some(&git_trash_parent),
        &projects_root,
        app_data_root,
        Vec::new(),
        "test-device",
    );
    tx.prepare().unwrap();

    // 获取事务实际使用的 trash 路径。
    let worktree_trash = tx.worktree_trash_path().to_path_buf();
    let git_dir_trash = tx.git_dir_trash_path().unwrap().to_path_buf();

    // 重启恢复。
    let recovered = recover_pending_delete_transactions(app_data_root).unwrap();
    assert_eq!(recovered.len(), 1);
    assert!(!worktree_from.exists());
    assert!(worktree_trash.exists());
    assert!(!git_dir_from.exists());
    assert!(git_dir_trash.exists());
}

/// 验证崩溃恢复：WorktreeMoved 阶段崩溃，重启后继续移动 git。
#[test]
fn test_recovery_from_worktree_moved() {
    let temp_dir = tempdir().unwrap();
    let app_data_root = temp_dir.path();
    let projects_root = app_data_root.join("projects");
    let private_root = app_data_root.join("private");
    fs::create_dir_all(&projects_root).unwrap();
    fs::create_dir_all(&private_root).unwrap();

    let project_id = "test-project";
    let worktree_from = projects_root.join(project_id);
    fs::create_dir_all(&worktree_from).unwrap();
    fs::write(worktree_from.join("project.json"), "{}").unwrap();

    let git_dir_from = private_root.join("sujian-git").join(project_id);
    fs::create_dir_all(&git_dir_from).unwrap();
    fs::write(git_dir_from.join("HEAD"), "ref: refs/heads/main\n").unwrap();

    // 模拟 WorktreeMoved 阶段崩溃：worktree 已移，git 还没移。
    let trash_parent = app_data_root.join("sync/trash");
    let git_trash_parent = app_data_root.join("private/trash");
    let mut tx = ProjectDeleteTransaction::new(
        project_id,
        &worktree_from,
        &trash_parent,
        Some(&git_dir_from),
        Some(&git_trash_parent),
        &projects_root,
        app_data_root,
        Vec::new(),
        "test-device",
    );
    tx.prepare().unwrap();
    tx.move_worktree().unwrap();

    // 获取事务实际使用的 git_dir_trash 路径。
    let git_dir_trash = tx.git_dir_trash_path().unwrap().to_path_buf();

    // 重启恢复。
    let recovered = recover_pending_delete_transactions(app_data_root).unwrap();
    assert_eq!(recovered.len(), 1);
    assert!(!git_dir_from.exists());
    assert!(git_dir_trash.exists());
}

/// 验证崩溃恢复：GitMoved 阶段崩溃，重启后生成 tombstone、推进到 Completed 并清理 journal。
#[test]
fn test_recovery_from_git_moved() {
    let temp_dir = tempdir().unwrap();
    let app_data_root = temp_dir.path();
    let projects_root = app_data_root.join("projects");
    let private_root = app_data_root.join("private");
    fs::create_dir_all(&projects_root).unwrap();
    fs::create_dir_all(&private_root).unwrap();

    let project_id = "test-project";
    let worktree_from = projects_root.join(project_id);
    fs::create_dir_all(&worktree_from).unwrap();
    fs::write(worktree_from.join("project.json"), "{}").unwrap();

    let git_dir_from = private_root.join("sujian-git").join(project_id);
    fs::create_dir_all(&git_dir_from).unwrap();
    fs::write(git_dir_from.join("HEAD"), "ref: refs/heads/main\n").unwrap();

    // 模拟 GitMoved 阶段崩溃：两边都已移，但 journal 还没清。
    let trash_parent = app_data_root.join("sync/trash");
    let git_trash_parent = app_data_root.join("private/trash");
    let mut tx = ProjectDeleteTransaction::new(
        project_id,
        &worktree_from,
        &trash_parent,
        Some(&git_dir_from),
        Some(&git_trash_parent),
        &projects_root,
        app_data_root,
        Vec::new(),
        "test-device",
    );
    tx.prepare().unwrap();
    tx.move_worktree().unwrap();
    tx.move_git().unwrap();

    // 重启恢复。
    let journal_path = tx.journal_path.clone();
    let recovered = recover_pending_delete_transactions(app_data_root).unwrap();
    assert_eq!(recovered.len(), 1);
    // #645 评论 5504296097 缺口2修复：recover 不 complete/cleanup，journal 保留。
    // 调用方（bootstrap）需 ack 后才清理。这里验证 journal 仍存在。
    assert!(
        journal_path.exists(),
        "缺口2修复：recover 后 journal 应保留在 StarMapsUnbound，由调用方 ack 后清理"
    );
}

/// 验证无 private git 时的删除流程。
#[test]
fn test_delete_without_private_git() {
    let temp_dir = tempdir().unwrap();
    let app_data_root = temp_dir.path();
    let projects_root = app_data_root.join("projects");
    fs::create_dir_all(&projects_root).unwrap();

    let project_id = "test-project";
    let worktree_from = projects_root.join(project_id);
    fs::create_dir_all(&worktree_from).unwrap();
    fs::write(worktree_from.join("project.json"), "{}").unwrap();

    let trash_parent = app_data_root.join("sync/trash");
    let mut tx = ProjectDeleteTransaction::new(
        project_id,
        &worktree_from,
        &trash_parent,
        None,
        None,
        &projects_root,
        app_data_root,
        Vec::new(),
        "test-device",
    );

    tx.prepare().unwrap();
    tx.move_worktree().unwrap();
    tx.move_git().unwrap(); // 无 private git，应直接推进到 GitMoved。
    tx.write_tombstone().unwrap();
    // #645 评论 5504296097 问题2：unbind_starmaps（空 starmap_ids）。
    tx.unbind_starmaps().unwrap();
    tx.complete().unwrap();

    let worktree_trash = tx.worktree_trash_path().to_path_buf();
    let journal_path = tx.journal_path.clone();
    tx.cleanup_journal().unwrap();

    assert!(!worktree_from.exists());
    assert!(worktree_trash.exists());
    assert!(!journal_path.exists());
}

/// #645 评论 5504296097 问题1：ack_project_delete_history 推进到
/// HistoryRecorded → RemoteDeleteQueued → Completed，并写 PendingDeletedTarget。
#[test]
fn test_ack_writes_pending_deleted_target_and_completes() {
    let temp_dir = tempdir().unwrap();
    let app_data_root = temp_dir.path();
    let projects_root = app_data_root.join("projects");
    fs::create_dir_all(&projects_root).unwrap();

    let project_id = "test-project";
    let worktree_from = projects_root.join(project_id);
    fs::create_dir_all(&worktree_from).unwrap();
    fs::write(worktree_from.join("project.json"), "{}").unwrap();

    let trash_parent = app_data_root.join("sync/trash");
    let mut tx = ProjectDeleteTransaction::new(
        project_id,
        &worktree_from,
        &trash_parent,
        None,
        None,
        &projects_root,
        app_data_root,
        Vec::new(),
        "test-device",
    );

    // 推进到 StarMapsUnbound（模拟 delete_project_with_changes 的状态）。
    tx.prepare().unwrap();
    tx.move_worktree().unwrap();
    tx.move_git().unwrap();
    tx.write_tombstone().unwrap();
    tx.unbind_starmaps().unwrap();
    let token = tx.token().to_string();
    // tx 在此处 drop，journal 保留在 StarMapsUnbound。

    // ack：推进到 HistoryRecorded → RemoteDeleteQueued → Completed。
    ack_project_delete_history(app_data_root, &token).unwrap();

    // journal 已清理。
    let journal_path = app_data_root
        .join(DELETE_JOURNALS_DIR)
        .join(format!("{}{}", DELETE_JOURNAL_PREFIX, token));
    assert!(!journal_path.exists(), "ack 后 journal 应已清理");

    // PendingDeletedTarget 已落盘。
    let pending =
        crate::sync::pending_deleted::load_pending_deleted_targets(app_data_root).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].journal_token, token);
    assert_eq!(
        pending[0].target.remote_prefix,
        format!("projects/{}", project_id)
    );
}

/// #645 评论 5504296097 问题1：HistoryRecorded phase 崩溃恢复时，
/// recover 先补写 PendingDeletedTarget，再推进到 Completed 并清 journal。
#[test]
fn test_recovery_from_history_recorded_writes_pending_target() {
    let temp_dir = tempdir().unwrap();
    let app_data_root = temp_dir.path();
    let projects_root = app_data_root.join("projects");
    fs::create_dir_all(&projects_root).unwrap();

    let project_id = "test-project";
    let worktree_from = projects_root.join(project_id);
    fs::create_dir_all(&worktree_from).unwrap();
    fs::write(worktree_from.join("project.json"), "{}").unwrap();

    let trash_parent = app_data_root.join("sync/trash");
    let mut tx = ProjectDeleteTransaction::new(
        project_id,
        &worktree_from,
        &trash_parent,
        None,
        None,
        &projects_root,
        app_data_root,
        Vec::new(),
        "test-device",
    );

    // 推进到 HistoryRecorded（模拟 ack 在写 pending target 前崩溃）。
    tx.prepare().unwrap();
    tx.move_worktree().unwrap();
    tx.move_git().unwrap();
    tx.write_tombstone().unwrap();
    tx.unbind_starmaps().unwrap();
    tx.advance_phase(ProjectDeletePhase::HistoryRecorded)
        .unwrap();
    let token = tx.token().to_string();
    let journal_path = tx.journal_path.clone();

    // PendingDeletedTarget 还没写。
    let pending_before =
        crate::sync::pending_deleted::load_pending_deleted_targets(app_data_root).unwrap();
    assert!(pending_before.is_empty());

    // 重启恢复：recover 遇 HistoryRecorded 先补写 PendingDeletedTarget，
    // 再推进到 Completed 并清 journal。
    let recovered = recover_pending_delete_transactions(app_data_root).unwrap();
    assert!(
        recovered.is_empty(),
        "HistoryRecorded 已记 history，recover 应直接 complete，不返回 recovered"
    );
    assert!(!journal_path.exists(), "recover 后 journal 应已清理");

    // PendingDeletedTarget 已补写。
    let pending_after =
        crate::sync::pending_deleted::load_pending_deleted_targets(app_data_root).unwrap();
    assert_eq!(pending_after.len(), 1);
    assert_eq!(pending_after[0].journal_token, token);
}

/// #645 评论 5504296097 问题1：RemoteDeleteQueued phase 崩溃恢复时，
/// recover 直接推进到 Completed 并清 journal（pending target 已落盘）。
#[test]
fn test_recovery_from_remote_delete_queued() {
    let temp_dir = tempdir().unwrap();
    let app_data_root = temp_dir.path();
    let projects_root = app_data_root.join("projects");
    fs::create_dir_all(&projects_root).unwrap();

    let project_id = "test-project";
    let worktree_from = projects_root.join(project_id);
    fs::create_dir_all(&worktree_from).unwrap();
    fs::write(worktree_from.join("project.json"), "{}").unwrap();

    let trash_parent = app_data_root.join("sync/trash");
    let mut tx = ProjectDeleteTransaction::new(
        project_id,
        &worktree_from,
        &trash_parent,
        None,
        None,
        &projects_root,
        app_data_root,
        Vec::new(),
        "test-device",
    );

    // 推进到 RemoteDeleteQueued（模拟 ack 在 complete 前崩溃）。
    tx.prepare().unwrap();
    tx.move_worktree().unwrap();
    tx.move_git().unwrap();
    tx.write_tombstone().unwrap();
    tx.unbind_starmaps().unwrap();
    tx.advance_phase(ProjectDeletePhase::HistoryRecorded)
        .unwrap();
    tx.advance_phase(ProjectDeletePhase::RemoteDeleteQueued)
        .unwrap();
    let journal_path = tx.journal_path.clone();

    // 重启恢复：recover 遇 RemoteDeleteQueued 直接 complete 并清 journal。
    let recovered = recover_pending_delete_transactions(app_data_root).unwrap();
    assert!(recovered.is_empty());
    assert!(!journal_path.exists(), "recover 后 journal 应已清理");
}

/// #645 评论 5504296097 问题1：record_pending_deleted_target 不吞文件损坏错误。
/// 文件损坏时返回 Err，不覆盖丢失其他 pending target。
#[test]
fn test_record_pending_deleted_target_fails_on_corrupted_file() {
    let temp_dir = tempdir().unwrap();
    let app_data_root = temp_dir.path();

    // 写一个损坏的 pending_deleted_targets.json。
    let pending_path = app_data_root
        .join("app-meta")
        .join("sync")
        .join("pending_deleted_targets.json");
    fs::create_dir_all(pending_path.parent().unwrap()).unwrap();
    fs::write(&pending_path, "{ corrupted json").unwrap();

    // record 应返回 Err，不吞错误。
    let target = PendingDeletedTarget::for_project("p1", 1000, "token_a", "dev-1");
    let result = crate::sync::pending_deleted::record_pending_deleted_target(app_data_root, target);
    assert!(
        result.is_err(),
        "record_pending_deleted_target 应在文件损坏时返回 Err，不吞错误"
    );

    // 损坏文件未被覆盖。
    let content = fs::read_to_string(&pending_path).unwrap();
    assert_eq!(content, "{ corrupted json");
}
