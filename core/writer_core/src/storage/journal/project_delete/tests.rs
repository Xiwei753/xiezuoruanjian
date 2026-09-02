use super::*;
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
    );
    tx.prepare().unwrap();

    // 获取事务实际使用的 trash 路径。
    let worktree_trash = tx.worktree_trash_path().to_path_buf();
    let git_dir_trash = tx.git_dir_trash_path().unwrap().to_path_buf();

    // 重启恢复。
    let recovered = recover_pending_delete_transactions(app_data_root).unwrap();
    assert_eq!(recovered, 1);
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
    );
    tx.prepare().unwrap();
    tx.move_worktree().unwrap();

    // 获取事务实际使用的 git_dir_trash 路径。
    let git_dir_trash = tx.git_dir_trash_path().unwrap().to_path_buf();

    // 重启恢复。
    let recovered = recover_pending_delete_transactions(app_data_root).unwrap();
    assert_eq!(recovered, 1);
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
    );
    tx.prepare().unwrap();
    tx.move_worktree().unwrap();
    tx.move_git().unwrap();

    // 重启恢复。
    let journal_path = tx.journal_path.clone();
    let recovered = recover_pending_delete_transactions(app_data_root).unwrap();
    assert_eq!(recovered, 1);
    assert!(!journal_path.exists());
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
    );

    tx.prepare().unwrap();
    tx.move_worktree().unwrap();
    tx.move_git().unwrap(); // 无 private git，应直接推进到 GitMoved。
    tx.write_tombstone().unwrap();
    tx.complete().unwrap();

    let worktree_trash = tx.worktree_trash_path().to_path_buf();
    let journal_path = tx.journal_path.clone();
    tx.cleanup_journal().unwrap();

    assert!(!worktree_from.exists());
    assert!(worktree_trash.exists());
    assert!(!journal_path.exists());
}
