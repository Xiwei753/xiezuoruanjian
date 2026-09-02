use super::*;

/// 辅助函数：获取 journal 路径（旧格式，用于测试兼容）
fn journal_path_old(git_dir: &Path) -> PathBuf {
    git_dir.join(LAYOUT_MIGRATION_JOURNAL_NAME)
}

/// 辅助函数：获取新格式 journal 路径
fn journal_path_new(git_dir: &Path, owner: &str) -> PathBuf {
    journal_path(git_dir, owner).unwrap()
}

/// 正常迁移：迁移完成后 embedded .git 不存在、private repo 存在、journal 已清理。
#[test]
fn migrate_writes_and_clears_journal() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let worktree_root = tmp.path().join("worktree");
    let private_root = tmp.path().join("private");
    std::fs::create_dir_all(&worktree_root).unwrap();
    std::fs::create_dir_all(&private_root).unwrap();

    // 在 worktree_root 下创建 embedded .git。
    git2::Repository::init(&worktree_root).unwrap();
    let embedded_git = worktree_root.join(".git");
    assert!(embedded_git.exists());

    let git_dir = private_root.join("repo.git");
    let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());

    ensure_project_repo_with_layout(&layout).unwrap();

    assert!(!embedded_git.exists(), "embedded .git should be removed");
    assert!(git_dir.exists(), "private git_dir should exist");
    // journal 应该在父目录的 .layout-migrations 下
    let migrations_dir = git_dir.parent().unwrap().join(LAYOUT_MIGRATIONS_DIR);
    if migrations_dir.exists() {
        // 如果目录存在，应该是空的
        let entries: Vec<_> = std::fs::read_dir(&migrations_dir).unwrap().collect();
        assert!(
            entries.is_empty(),
            "migrations dir should be empty after success"
        );
    }
}

/// 双仓库并存但无 journal：无法证明 ownership，保留 embedded .git，返回 Ok(())。
#[test]
fn dual_repo_no_journal_keeps_embedded() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let worktree_root = tmp.path().join("worktree");
    let private_root = tmp.path().join("private");
    std::fs::create_dir_all(&worktree_root).unwrap();
    std::fs::create_dir_all(&private_root).unwrap();

    git2::Repository::init(&worktree_root).unwrap();
    let embedded_git = worktree_root.join(".git");

    // private git_dir 是独立仓库，不是迁移来的（无 journal）。
    let git_dir = private_root.join("repo.git");
    git2::Repository::init_bare(&git_dir).unwrap();

    let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
    let result = ensure_project_repo_with_layout(&layout);

    assert!(
        result.is_ok(),
        "no journal should keep embedded .git and return Ok"
    );
    assert!(
        embedded_git.exists(),
        "embedded .git must be preserved without journal"
    );
    assert!(git_dir.exists(), "private git_dir should still exist");
}

/// #644 评论 5493295108 问题3：journal 拟留 + claimed_source 已删 + 后来新建 worktree/.git：
/// 恢复时不应删除后来新建的 .git，只清理 journal（terminal cleanup）。
#[test]
fn dual_repo_journal_claimed_source_removed_keeps_later_git() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let worktree_root = tmp.path().join("worktree");
    let private_root = tmp.path().join("private");
    std::fs::create_dir_all(&worktree_root).unwrap();
    std::fs::create_dir_all(&private_root).unwrap();

    // private git_dir 是已迁移好的仓库。
    let git_dir = private_root.join("repo.git");
    git2::Repository::init_bare(&git_dir).unwrap();

    // 后来在 worktree 下新建 .git（别人的仓库）。
    git2::Repository::init(&worktree_root).unwrap();
    let embedded_git = worktree_root.join(".git");

    // 写 journal：claimed_source 指向一个已不存在的路径（迁移时已删），
    // original_source 指向 worktree/.git（现在存在的是别人后来新建的）。
    let owner = uuid::Uuid::new_v4().to_string();
    let journal = LayoutMigrationJournal {
        owner: owner.clone(),
        worktree_canonical: canonicalize_or_lossy(&worktree_root),
        original_source: embedded_git.to_string_lossy().into_owned(),
        claimed_source: worktree_root
            .join(".git.sujian-migrate-source-dead")
            .to_string_lossy()
            .into_owned(),
        target_tmp: git_dir
            .parent()
            .unwrap()
            .join(format!(".git.sujian-migrate-{}", owner))
            .to_string_lossy()
            .into_owned(),
        target_git_dir: git_dir.to_string_lossy().into_owned(),
        phase: MigrationPhase::TargetPrepared,
    };
    write_migration_journal(&git_dir, &journal).unwrap();

    let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
    let result = ensure_project_repo_with_layout(&layout);

    assert!(
        result.is_ok(),
        "claimed_source removed should keep later-created .git and return Ok"
    );
    assert!(
        embedded_git.exists(),
        "later-created embedded .git must be preserved (not the claimed source)"
    );
    // journal 应该被清理
    assert!(
        !journal_path_new(&git_dir, &journal.owner).exists(),
        "journal should be cleaned after terminal cleanup"
    );
    assert!(git_dir.exists(), "private git_dir should still exist");
}

/// 双仓库并存 + journal worktree 不匹配：保留 embedded .git，返回 Ok(())。
#[test]
fn dual_repo_journal_worktree_mismatch_keeps_embedded() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let worktree_root = tmp.path().join("worktree");
    let private_root = tmp.path().join("private");
    std::fs::create_dir_all(&worktree_root).unwrap();
    std::fs::create_dir_all(&private_root).unwrap();

    git2::Repository::init(&worktree_root).unwrap();
    let embedded_git = worktree_root.join(".git");

    let git_dir = private_root.join("repo.git");
    git2::Repository::init_bare(&git_dir).unwrap();

    // 写 journal，worktree 不匹配（指向不同的 worktree）。
    let owner = uuid::Uuid::new_v4().to_string();
    let journal = LayoutMigrationJournal {
        owner: owner.clone(),
        worktree_canonical: "/nonexistent/worktree".to_string(),
        original_source: "/nonexistent/source.git".to_string(),
        claimed_source: "/nonexistent/claimed".to_string(),
        target_tmp: git_dir
            .parent()
            .unwrap()
            .join(format!(".git.sujian-migrate-{}", owner))
            .to_string_lossy()
            .into_owned(),
        target_git_dir: git_dir.to_string_lossy().into_owned(),
        phase: MigrationPhase::TargetPrepared,
    };
    write_migration_journal(&git_dir, &journal).unwrap();

    let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
    let result = ensure_project_repo_with_layout(&layout);

    assert!(
        result.is_ok(),
        "worktree mismatch should keep embedded .git and return Ok"
    );
    assert!(
        embedded_git.exists(),
        "embedded .git must be preserved on worktree mismatch"
    );
    // journal 应该保留（因为 worktree 不匹配，无法处理）
    assert!(
        journal_path_new(&git_dir, &journal.owner).exists(),
        "journal should be preserved on worktree mismatch"
    );
}

/// 双仓库并存 + journal 损坏（无效 JSON）：返回 Err，保留 embedded .git。
#[test]
fn dual_repo_corrupt_journal_returns_err() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let worktree_root = tmp.path().join("worktree");
    let private_root = tmp.path().join("private");
    std::fs::create_dir_all(&worktree_root).unwrap();
    std::fs::create_dir_all(&private_root).unwrap();

    git2::Repository::init(&worktree_root).unwrap();
    let embedded_git = worktree_root.join(".git");

    let git_dir = private_root.join("repo.git");
    git2::Repository::init_bare(&git_dir).unwrap();

    // 写损坏的 journal（新格式）。
    let migrations_dir = git_dir.parent().unwrap().join(LAYOUT_MIGRATIONS_DIR);
    std::fs::create_dir_all(&migrations_dir).unwrap();
    std::fs::write(migrations_dir.join("corrupt.json"), b"not valid json").unwrap();

    let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
    let result = ensure_project_repo_with_layout(&layout);

    assert!(result.is_err(), "corrupt journal should return Err");
    assert!(
        embedded_git.exists(),
        "embedded .git must be preserved on corrupt journal"
    );
}

/// #644 评论 5493295108 问题3：迁移完全成功后 journal 已删除；
/// 无 embedded .git 时 ensure 不进入双仓库分支，幂等 Ok。
#[test]
fn no_embedded_after_migration_idempotent() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let worktree_root = tmp.path().join("worktree");
    let private_root = tmp.path().join("private");
    std::fs::create_dir_all(&worktree_root).unwrap();
    std::fs::create_dir_all(&private_root).unwrap();

    // worktree 下无 embedded .git。
    let git_dir = private_root.join("repo.git");
    git2::Repository::init_bare(&git_dir).unwrap();

    let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
    // 无 embedded .git → 不进入双仓库分支，直接 Ok(())。
    let result = ensure_project_repo_with_layout(&layout);
    assert!(result.is_ok(), "no embedded .git should be idempotent Ok");
}

/// #644 评论 5494387963 问题1：旧格式 journal 迁移到新格式。
#[test]
fn legacy_journal_migrated_to_new_format() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let worktree_root = tmp.path().join("worktree");
    let private_root = tmp.path().join("private");
    std::fs::create_dir_all(&worktree_root).unwrap();
    std::fs::create_dir_all(&private_root).unwrap();

    git2::Repository::init(&worktree_root).unwrap();
    let embedded_git = worktree_root.join(".git");

    let git_dir = private_root.join("repo.git");
    git2::Repository::init_bare(&git_dir).unwrap();

    // 模拟旧代码：phase = "copied" 意味着 source 已经被 rename 到 claimed_source
    let owner = uuid::Uuid::new_v4().to_string();
    let claimed_source = worktree_root.join(format!(".git.sujian-migrate-source-{}", owner));
    std::fs::rename(&embedded_git, &claimed_source).unwrap();

    let legacy_journal = serde_json::json!({
        "migration_uuid": owner,
        "worktree_canonical": canonicalize_or_lossy(&worktree_root),
        "original_source": embedded_git.to_string_lossy(),
        "claimed_source": claimed_source.to_string_lossy(),
        "target_git_dir": git_dir.to_string_lossy(),
        "phase": "copied"
    });
    std::fs::write(journal_path_old(&git_dir), legacy_journal.to_string()).unwrap();

    let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
    // 这会触发旧 journal 迁移并完成整个恢复流程
    let result = ensure_project_repo_with_layout(&layout);

    assert!(result.is_ok(), "legacy journal migration should succeed");
    // 旧 journal 应该被删除
    assert!(
        !journal_path_old(&git_dir).exists(),
        "old journal should be removed"
    );
    // 恢复流程会继续执行到所有阶段完成，journal 最终被清理
    // 验证最终状态：迁移完成
    assert!(git_dir.exists(), "private git_dir should exist");
    assert!(!embedded_git.exists(), "embedded .git should be removed");
    // journal 应该被清理（完整恢复后）
    assert!(
        !journal_path_new(&git_dir, &owner).exists(),
        "new journal should be cleaned after full recovery"
    );
}

/// #644 评论 5494387963 问题1：崩溃在 Prepared 阶段（journal 已写，source 未 rename）。
/// 恢复时应该继续 rename。
#[test]
fn resume_from_prepared_phase() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let worktree_root = tmp.path().join("worktree");
    let private_root = tmp.path().join("private");
    std::fs::create_dir_all(&worktree_root).unwrap();
    std::fs::create_dir_all(&private_root).unwrap();

    git2::Repository::init(&worktree_root).unwrap();
    let embedded_git = worktree_root.join(".git");

    let git_dir = private_root.join("repo.git");
    // 不创建 git_dir，模拟崩溃在 Prepared 阶段

    // 写 journal（Prepared 阶段）
    let owner = uuid::Uuid::new_v4().to_string();
    let claimed_source = worktree_root.join(format!(".git.sujian-migrate-source-{}", owner));
    let journal = LayoutMigrationJournal {
        owner: owner.clone(),
        worktree_canonical: canonicalize_or_lossy(&worktree_root),
        original_source: embedded_git.to_string_lossy().into_owned(),
        claimed_source: claimed_source.to_string_lossy().into_owned(),
        target_tmp: git_dir
            .parent()
            .unwrap()
            .join(format!(".git.sujian-migrate-{}", owner))
            .to_string_lossy()
            .into_owned(),
        target_git_dir: git_dir.to_string_lossy().into_owned(),
        phase: MigrationPhase::Prepared,
    };
    write_migration_journal(&git_dir, &journal).unwrap();

    // 恢复：应该继续 rename 并完成整个迁移
    let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
    ensure_project_repo_with_layout(&layout).unwrap();

    // 验证：embedded .git 应该被 rename 到 claimed_source，然后被删除
    assert!(!embedded_git.exists(), "embedded .git should be renamed");
    assert!(
        !claimed_source.exists(),
        "claimed_source should be removed after full recovery"
    );
    // git_dir 应该存在
    assert!(git_dir.exists(), "private git_dir should exist");
    // journal 应该被清理（完整恢复后）
    assert!(
        !journal_path_new(&git_dir, &owner).exists(),
        "new journal should be cleaned after full recovery"
    );
}

/// #644 评论 5494387963 问题1：崩溃在 SourceClaimed 阶段（source 已 rename，copy 未执行）。
/// 恢复时应该继续 copy。
#[test]
fn resume_from_source_claimed_phase() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let worktree_root = tmp.path().join("worktree");
    let private_root = tmp.path().join("private");
    std::fs::create_dir_all(&worktree_root).unwrap();
    std::fs::create_dir_all(&private_root).unwrap();

    git2::Repository::init(&worktree_root).unwrap();
    let embedded_git = worktree_root.join(".git");

    let git_dir = private_root.join("repo.git");

    // 先 rename source
    let owner = uuid::Uuid::new_v4().to_string();
    let claimed_source = worktree_root.join(format!(".git.sujian-migrate-source-{}", owner));
    std::fs::rename(&embedded_git, &claimed_source).unwrap();

    // 写 journal（SourceClaimed 阶段）
    let journal = LayoutMigrationJournal {
        owner: owner.clone(),
        worktree_canonical: canonicalize_or_lossy(&worktree_root),
        original_source: embedded_git.to_string_lossy().into_owned(),
        claimed_source: claimed_source.to_string_lossy().into_owned(),
        target_tmp: git_dir
            .parent()
            .unwrap()
            .join(format!(".git.sujian-migrate-{}", owner))
            .to_string_lossy()
            .into_owned(),
        target_git_dir: git_dir.to_string_lossy().into_owned(),
        phase: MigrationPhase::SourceClaimed,
    };
    write_migration_journal(&git_dir, &journal).unwrap();

    // 恢复：应该继续 copy
    let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
    ensure_project_repo_with_layout(&layout).unwrap();

    // 验证：git_dir 应该存在
    assert!(git_dir.exists(), "private git_dir should exist");
    // claimed_source 应该被删除
    assert!(!claimed_source.exists(), "claimed_source should be removed");
}

/// #644 评论 5494387963 问题1：崩溃在 TargetInstalled 阶段（target 已安装，claimed_source 未删）。
/// 恢复时应该继续删除 claimed_source。
#[test]
fn resume_from_target_installed_phase() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let worktree_root = tmp.path().join("worktree");
    let private_root = tmp.path().join("private");
    std::fs::create_dir_all(&worktree_root).unwrap();
    std::fs::create_dir_all(&private_root).unwrap();

    git2::Repository::init(&worktree_root).unwrap();
    let embedded_git = worktree_root.join(".git");

    let git_dir = private_root.join("repo.git");

    // 完成 rename source
    let owner = uuid::Uuid::new_v4().to_string();
    let claimed_source = worktree_root.join(format!(".git.sujian-migrate-source-{}", owner));
    std::fs::rename(&embedded_git, &claimed_source).unwrap();

    // copy 到 target
    migrate_copy_dir_recursive(&claimed_source, &git_dir).unwrap();
    // 设置 workdir
    let repo = git2::Repository::init_bare(&git_dir).unwrap();
    repo.set_workdir(&worktree_root, false).unwrap();

    // 写 journal（TargetInstalled 阶段）
    let journal = LayoutMigrationJournal {
        owner: owner.clone(),
        worktree_canonical: canonicalize_or_lossy(&worktree_root),
        original_source: embedded_git.to_string_lossy().into_owned(),
        claimed_source: claimed_source.to_string_lossy().into_owned(),
        target_tmp: git_dir
            .parent()
            .unwrap()
            .join(format!(".git.sujian-migrate-{}", owner))
            .to_string_lossy()
            .into_owned(),
        target_git_dir: git_dir.to_string_lossy().into_owned(),
        phase: MigrationPhase::TargetInstalled,
    };
    write_migration_journal(&git_dir, &journal).unwrap();

    // 恢复：应该继续删除 claimed_source
    let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
    ensure_project_repo_with_layout(&layout).unwrap();

    // 验证：claimed_source 应该被删除
    assert!(!claimed_source.exists(), "claimed_source should be removed");
    // journal 应该被清理
    assert!(
        !journal_path_new(&git_dir, &owner).exists(),
        "journal should be cleaned"
    );
}

/// #644 评论 5494387963 问题1：git_dir 存在但损坏（不是 Missing）。
/// 应该返回 Err，不能继续 init 或迁移覆盖。
#[test]
fn corrupt_git_dir_returns_err() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let worktree_root = tmp.path().join("worktree");
    let private_root = tmp.path().join("private");
    std::fs::create_dir_all(&worktree_root).unwrap();
    std::fs::create_dir_all(&private_root).unwrap();

    // 创建一个"看起来像 git_dir"的目录，但里面没有有效仓库
    let git_dir = private_root.join("repo.git");
    std::fs::create_dir_all(&git_dir).unwrap();

    let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
    let result = ensure_project_repo_with_layout(&layout);

    assert!(result.is_err(), "corrupt git_dir should return Err");
}

/// #644 评论 5494387963 问题1：embedded .git 存在但损坏（不是 Missing）。
/// 应该返回 Err，不能迁移。
#[test]
fn corrupt_embedded_git_returns_err() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let worktree_root = tmp.path().join("worktree");
    let private_root = tmp.path().join("private");
    std::fs::create_dir_all(&worktree_root).unwrap();
    std::fs::create_dir_all(&private_root).unwrap();

    // 创建一个"看起来像 embedded .git"的目录，但里面没有有效仓库
    let embedded_git = worktree_root.join(".git");
    std::fs::create_dir_all(&embedded_git).unwrap();

    let git_dir = private_root.join("repo.git");
    let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
    let result = ensure_project_repo_with_layout(&layout);

    assert!(result.is_err(), "corrupt embedded .git should return Err");
}

/// #644 评论 5494387963 问题2：resolve_existing_repo_layout 语义测试。
#[test]
fn resolve_existing_repo_layout_private_ready() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let worktree_root = tmp.path().join("worktree");
    let private_root = tmp.path().join("private");
    std::fs::create_dir_all(&worktree_root).unwrap();
    std::fs::create_dir_all(&private_root).unwrap();

    // private git_dir 已有 repo → Ready。
    let git_dir = private_root.join("repo.git");
    git2::Repository::init_bare(&git_dir).unwrap();

    let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
    let state = resolve_existing_repo_layout(&layout).unwrap();
    assert!(matches!(state, ExistingRepoLayoutState::Ready(_)));
}

/// #644 评论 5494387963 问题2：private 没有 + worktree/.git 有 → 迁移后 Ready。
#[test]
fn resolve_existing_repo_layout_migrates_embedded() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let worktree_root = tmp.path().join("worktree");
    let private_root = tmp.path().join("private");
    std::fs::create_dir_all(&worktree_root).unwrap();
    std::fs::create_dir_all(&private_root).unwrap();

    // worktree 下有 embedded .git，private 没有。
    git2::Repository::init(&worktree_root).unwrap();
    let embedded_git = worktree_root.join(".git");
    assert!(embedded_git.exists());

    let git_dir = private_root.join("repo.git");
    let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
    let state = resolve_existing_repo_layout(&layout).unwrap();
    assert!(matches!(state, ExistingRepoLayoutState::Ready(_)));
    assert!(!embedded_git.exists(), "embedded should be migrated away");
    assert!(git_dir.exists(), "private git_dir should exist");
}

/// #644 评论 5494387963 问题2：两边都没有 → NotGitRepo。
#[test]
fn resolve_existing_repo_layout_neither_is_not_git_repo() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let worktree_root = tmp.path().join("worktree");
    let private_root = tmp.path().join("private");
    std::fs::create_dir_all(&worktree_root).unwrap();
    std::fs::create_dir_all(&private_root).unwrap();

    let git_dir = private_root.join("repo.git");
    let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
    let state = resolve_existing_repo_layout(&layout).unwrap();
    assert!(matches!(state, ExistingRepoLayoutState::NotGitRepo));
    // 不应 init 新仓库。
    assert!(!git_dir.exists(), "resolve should not init new repo");
}
