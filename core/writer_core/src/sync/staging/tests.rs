use super::*;
use tempfile::TempDir;

#[test]
fn staging_run_create_and_cleanup() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let run = StagingRun::create(
        tmp.path(),
        live,
        crate::sync::types::BackendType::GithubApi,
        None,
    )
    .unwrap();
    assert!(run.base_root().exists());
    assert!(run.staging_root().exists());
    run.cleanup();
    assert!(!run.run_root().exists());
}

#[test]
fn base_snapshot_hardlink_or_copy() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("a.txt"), "hello").unwrap();
    fs::create_dir_all(live.join("sub")).unwrap();
    fs::write(live.join("sub/b.txt"), "world").unwrap();

    let run = StagingRun::create(
        tmp.path(),
        live.clone(),
        crate::sync::types::BackendType::GithubApi,
        None,
    )
    .unwrap();
    run.build_base_snapshot_from_live(
        &live,
        &[
            PathBuf::from("a.txt"),
            PathBuf::from("sub/b.txt"),
            PathBuf::from("missing.txt"),
        ],
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(run.base_root().join("a.txt")).unwrap(),
        "hello"
    );
    assert_eq!(
        fs::read_to_string(run.base_root().join("sub/b.txt")).unwrap(),
        "world"
    );
    assert!(!run.base_root().join("missing.txt").exists());
}

#[test]
fn commit_plan_local_eq_base_applies_incoming() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("f.txt"), "base").unwrap();

    let run = StagingRun::create(
        tmp.path(),
        live.clone(),
        crate::sync::types::BackendType::Git,
        None,
    )
    .unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from("f.txt")])
        .unwrap();
    // local 仍是 base（没动），incoming 改了。
    fs::write(run.staging_root().join("f.txt"), "incoming").unwrap();

    let plan = run.compute_commit_plan(&live).unwrap();
    assert_eq!(plan.content_actions.len(), 1);
    assert!(plan.keep_local.is_empty());
    assert!(plan.conflict.is_empty());
}

#[test]
fn commit_plan_incoming_eq_base_keeps_local() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("f.txt"), "base").unwrap();

    let run = StagingRun::create(
        tmp.path(),
        live.clone(),
        crate::sync::types::BackendType::Git,
        None,
    )
    .unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from("f.txt")])
        .unwrap();
    // local 改了（走 atomic_write rename 替换，hard-link 的 base 保留旧 inode），
    // incoming == base。
    crate::storage::atomic_write_string(&live.join("f.txt"), "local-changed").unwrap();
    fs::write(run.staging_root().join("f.txt"), "base").unwrap();

    let plan = run.compute_commit_plan(&live).unwrap();
    assert_eq!(plan.keep_local.len(), 1);
    assert!(plan.content_actions.is_empty());
    assert!(plan.conflict.is_empty());
}

#[test]
fn seed_from_live_copies_all_files_to_base_and_staging() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(live.join("sub")).unwrap();
    fs::write(live.join("a.txt"), "hello").unwrap();
    fs::write(live.join("sub/b.txt"), "world").unwrap();
    // 内部目录应被跳过
    fs::create_dir_all(live.join(".git/objects")).unwrap();
    fs::write(live.join(".git/HEAD"), "ref: refs/heads/main").unwrap();
    fs::create_dir_all(live.join("app-meta/transactions/tx1")).unwrap();
    fs::write(live.join("app-meta/transactions/tx1/staged"), "tmp").unwrap();

    let run = StagingRun::create(
        tmp.path(),
        live.clone(),
        crate::sync::types::BackendType::GithubApi,
        None,
    )
    .unwrap();
    run.seed_from_live(&live).unwrap();

    // base 有业务文件
    assert_eq!(
        fs::read_to_string(run.base_root().join("a.txt")).unwrap(),
        "hello"
    );
    assert_eq!(
        fs::read_to_string(run.base_root().join("sub/b.txt")).unwrap(),
        "world"
    );
    // staging 也有业务文件
    assert_eq!(
        fs::read_to_string(run.staging_root().join("a.txt")).unwrap(),
        "hello"
    );
    assert_eq!(
        fs::read_to_string(run.staging_root().join("sub/b.txt")).unwrap(),
        "world"
    );
    // 内部目录被跳过
    assert!(!run.base_root().join(".git").exists());
    assert!(!run.staging_root().join(".git").exists());
    assert!(!run.base_root().join("app-meta/transactions").exists());
    assert!(!run.staging_root().join("app-meta/transactions").exists());
}

#[test]
fn commit_plan_both_changed_is_conflict() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    // #644 评论 5473789298 第3节：UserTextDocument 双方都改才冲突。
    // 用 note.md（正文类）而非 .txt（GeneratedCache 走 LWW 不冲突）。
    fs::write(live.join("note.md"), "base").unwrap();

    let run = StagingRun::create(
        tmp.path(),
        live.clone(),
        crate::sync::types::BackendType::Git,
        None,
    )
    .unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from("note.md")])
        .unwrap();
    // local 改了（atomic_write rename 替换，base 保留旧 inode）。
    crate::storage::atomic_write_string(&live.join("note.md"), "local-changed").unwrap();
    fs::write(run.staging_root().join("note.md"), "incoming-changed").unwrap();

    let plan = run.compute_commit_plan(&live).unwrap();
    assert_eq!(plan.conflict.len(), 1);
    assert!(plan.content_actions.is_empty());
    assert!(plan.keep_local.is_empty());
}

#[test]
fn commit_plan_remote_delete_local_unchanged_produces_delete() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("f.txt"), "base").unwrap();

    let run = StagingRun::create(
        tmp.path(),
        live.clone(),
        crate::sync::types::BackendType::Git,
        None,
    )
    .unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from("f.txt")])
        .unwrap();
    // 远端删除：staging 中没有 f.txt（incoming=None），local 没改。
    // base ∪ staging = {f.txt}（来自 base），三方比较：local==base, incoming=None → Delete。

    let plan = run.compute_commit_plan(&live).unwrap();
    assert_eq!(plan.content_actions.len(), 1);
    assert!(matches!(
        plan.content_actions[0],
        CommitAction::Delete { .. }
    ));
    assert!(plan.conflict.is_empty());
    assert!(plan.keep_local.is_empty());
}

#[test]
fn commit_plan_remote_new_local_none_applies() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    // live 没有 f.txt（local=None），base 也没有（base=None）。
    // staging 有 f.txt（incoming=Some）。
    let run = StagingRun::create(
        tmp.path(),
        live.clone(),
        crate::sync::types::BackendType::Git,
        None,
    )
    .unwrap();
    // base 为空（没有 build_base_snapshot）
    fs::write(run.staging_root().join("f.txt"), "new-from-remote").unwrap();

    let plan = run.compute_commit_plan(&live).unwrap();
    // base=None, local=None, incoming=Some → local==base (both None), incoming!=base → Apply
    assert_eq!(plan.content_actions.len(), 1);
    assert!(plan.conflict.is_empty());
}

#[test]
fn commit_plan_local_changed_remote_deleted_is_conflict() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    // #644 评论 5473789298 第3节：UserTextDocument local 改了 + 远端删除 → 冲突。
    // 用 note.md（正文类）而非 .txt（GeneratedCache 走 LWW：Apply Delete 不冲突）。
    fs::write(live.join("note.md"), "base").unwrap();

    let run = StagingRun::create(
        tmp.path(),
        live.clone(),
        crate::sync::types::BackendType::Git,
        None,
    )
    .unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from("note.md")])
        .unwrap();
    // local 改了，远端删除（staging 没有 note.md）。
    crate::storage::atomic_write_string(&live.join("note.md"), "local-changed").unwrap();

    let plan = run.compute_commit_plan(&live).unwrap();
    // local!=base, incoming=None → local!=incoming → Conflict
    assert_eq!(plan.conflict.len(), 1);
    assert!(plan.content_actions.is_empty());
    assert!(plan.keep_local.is_empty());
}

#[test]
fn walk_commit_candidates_skips_git_sujian_migrate_source() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    // 创建测试目录结构
    fs::create_dir_all(root.join("normal")).unwrap();
    fs::write(root.join("normal/file.txt"), "normal").unwrap();
    // .git.sujian-migrate-source-* 目录应被跳过
    fs::create_dir_all(root.join(".git.sujian-migrate-source-abc/objects")).unwrap();
    fs::write(
        root.join(".git.sujian-migrate-source-abc/HEAD"),
        "ref: main",
    )
    .unwrap();
    fs::write(root.join(".git.sujian-migrate-source-abc/config"), "[core]").unwrap();
    // .git.sujian-tmp-* 目录也应被跳过
    fs::create_dir_all(root.join(".git.sujian-tmp-tmp123")).unwrap();
    fs::write(root.join(".git.sujian-tmp-tmp123/tmp"), "tmp").unwrap();
    // .git 目录也应被跳过
    fs::create_dir_all(root.join(".git/objects")).unwrap();
    fs::write(root.join(".git/HEAD"), "ref: main").unwrap();

    let mut out = Vec::new();
    walk_commit_candidates(root, root, &mut out).unwrap();

    // 只应看到 normal/file.txt
    assert_eq!(out.len(), 1);
    assert!(out.contains(&PathBuf::from("normal/file.txt")));
    // 不应包含任何内部 Git 工件
    assert!(!out.iter().any(|p| p.to_string_lossy().contains(".git")));
}

#[test]
fn classify_staging_commit_path_skips_git_sujian_migrate_source() {
    // .git.sujian-migrate-source-* 应被分类为 Skip
    assert_eq!(
        classify_staging_commit_path(".git.sujian-migrate-source-abc"),
        StagingCommitClass::Skip
    );
    assert_eq!(
        classify_staging_commit_path(".git.sujian-migrate-source-abc/objects/HEAD"),
        StagingCommitClass::Skip
    );
    // Windows 路径格式也应正确处理
    assert_eq!(
        classify_staging_commit_path(".git.sujian-migrate-source-abc\\objects\\HEAD"),
        StagingCommitClass::Skip
    );

    // .git.sujian-tmp-* 也应被分类为 Skip
    assert_eq!(
        classify_staging_commit_path(".git.sujian-tmp-tmp123"),
        StagingCommitClass::Skip
    );
    assert_eq!(
        classify_staging_commit_path(".git.sujian-tmp-tmp123/tmp"),
        StagingCommitClass::Skip
    );

    // .git 也应被分类为 Skip
    assert_eq!(
        classify_staging_commit_path(".git"),
        StagingCommitClass::Skip
    );
    assert_eq!(
        classify_staging_commit_path(".git/objects/HEAD"),
        StagingCommitClass::Skip
    );

    // 普通内容应被分类为 Content
    assert_eq!(
        classify_staging_commit_path("normal/file.txt"),
        StagingCommitClass::Content
    );
}
