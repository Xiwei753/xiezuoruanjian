//! Issue #644 评论 5493295108 — 5 个问题的修复验证测试（Phase B: Patch Verification）。
//!
//! 本测试文件针对评论 5493295108 描述的 5 个问题，验证修复后的行为：
//!
//! - 问题1：`list_projects_inner` 不再调 `ensure_project_repo_with_layout`，
//!   迁移职责移到 `sync::staging::prepare_staging_runs`（已释放 Core 写锁之后）。
//! - 问题2：`seed_from_live_as_git_repo` 识别旧 `app_data_root/.git` 并迁移，
//!   不再只检查 `layout.git_dir.exists()`。
//! - 问题3：迁移改为 journal 状态机，先取所有权再复制。恢复时只删 `claimed_source`，
//!   绝不能删除后来重新出现在 `worktree/.git` 的别人的仓库。
//! - 问题4：`delete_project` 在 workspace layout 下不移动共享 git_dir，
//!   不再遗留孤儿仓库。
//!
//! 验证策略：WHITE_BOX（源码结构断言）+ 运行时行为验证。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

// ── helpers ──

/// 读取 writer_core 源文件内容。
fn read_src_file(rel: &str) -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = PathBuf::from(manifest_dir).join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

/// 提取指定函数的源码片段（从 `fn <name>` 到匹配的 `}`）。
fn extract_fn_body(src: &str, fn_name: &str) -> String {
    let needle = format!("fn {fn_name}(");
    let start = src
        .find(&needle)
        .unwrap_or_else(|| panic!("function {fn_name} not found"));
    let body_start = src[start..].find('{').unwrap() + start;
    let mut depth = 0i32;
    let mut end = body_start;
    for (i, ch) in src[body_start..].char_indices() {
        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                end = body_start + i + 1;
                break;
            }
        }
    }
    src[start..end].to_string()
}

// ══ 问题1：list_projects() 不再在 Core 写锁里搬整个 .git ══

/// 问题1验证：`list_projects_inner` 不再调 `ensure_project_repo_with_layout`，
/// 迁移职责移到 `sync::staging::prepare_staging_runs`（已释放 Core 写锁之后）。
#[test]
fn problem1_list_projects_no_longer_migrates_in_core_write_lock() {
    let project_src = read_src_file("src/project.rs");
    let list_inner_body = extract_fn_body(&project_src, "list_projects_inner");
    // 修复后：list_projects_inner 不再调 ensure_project_repo_with_layout
    assert!(
        !list_inner_body.contains("ensure_project_repo_with_layout"),
        "problem1: list_projects_inner 仍调 ensure_project_repo_with_layout — 修复未生效"
    );
    assert!(
        !list_inner_body.contains("ensure_project_repo("),
        "problem1: list_projects_inner 仍调 ensure_project_repo — 修复未生效"
    );

    // #645 评论 5504296097 第2点：workspace Git 初始化已移到 bootstrap，
    // prepare_staging_runs 不再调 ensure_project_repo_with_layout。
    let staging_src = read_src_file("src/sync/staging/run.rs");
    let prepare_body = extract_fn_body(&staging_src, "prepare_staging_runs");
    assert!(
        !prepare_body.contains("prepare_target_git_layout"),
        "problem1: prepare_staging_runs 仍调 prepare_target_git_layout — 修复未生效"
    );
    assert!(
        !prepare_body.contains("ensure_project_repo_with_layout"),
        "problem1: prepare_staging_runs 仍调 ensure_project_repo_with_layout — 应已移到 bootstrap"
    );
    // prepare_target_git_layout 函数应已删除（workspace Git 准备收成 plan 级一次）
    assert!(
        !staging_src.contains("fn prepare_target_git_layout("),
        "problem1: prepare_target_git_layout 函数应已删除 — 修复未生效"
    );
    // #645 评论 5504296097 第2点：bootstrap 应包含 workspace Git 初始化
    let bootstrap_src = read_src_file("src/api/bootstrap.rs");
    assert!(
        bootstrap_src.contains("ensure_workspace_git"),
        "problem1: bootstrap 应包含 ensure_workspace_git — workspace Git 应在 bootstrap 初始化"
    );

    println!(
        "[BUGFIX_REPRO_TRACE] problem1: list_projects_inner 已改为纯读取，\
         迁移职责移到 prepare_staging_runs（无 Core 锁）"
    );
}

// ══ 问题2：App target 旧 app_data_root/.git 被识别并迁移 ══

/// 问题2验证：`seed_from_live_as_git_repo` 识别旧 `app_data_root/.git` 并迁移，
/// 不再只检查 `layout.git_dir.exists()`。
#[test]
fn problem2_app_target_old_app_data_root_git_is_migrated() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();

    let tmp = TempDir::new().unwrap();
    let app_data_root = tmp.path().join("app_data_root");
    let private_git_root = tmp.path().join("sujian-git");
    let private_app_git_dir = private_git_root.join("app");
    fs::create_dir_all(&app_data_root).unwrap();

    // 在 app_data_root 下建旧版本 .git 并提交一次（模拟旧版本已有完整历史）
    let old_repo = git2::Repository::init(&app_data_root).unwrap();
    fs::write(app_data_root.join("settings.sync.json"), "{}").unwrap();
    let mut index = old_repo.index().unwrap();
    index
        .add_path(std::path::Path::new("settings.sync.json"))
        .unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = old_repo.find_tree(tree_oid).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    old_repo
        .commit(Some("HEAD"), &sig, &sig, "init app settings", &tree, &[])
        .unwrap();
    old_repo
        .remote("origin", "https://github.com/test/app-repo.git")
        .unwrap();

    // 确认旧 .git 存在且有历史
    let old_git_dir = app_data_root.join(".git");
    assert!(old_git_dir.exists(), "test setup: 旧 .git 应存在");
    assert!(
        !private_app_git_dir.exists(),
        "test setup: private_app_git_dir 不应存在（升级前）"
    );

    // 构造 layout 指向 private git_dir（新版本 App target 布局）
    let layout = writer_core::storage::git_repo_layout::GitRepoLayout::with_external_git_dir(
        app_data_root.clone(),
        private_app_git_dir.clone(),
    );

    // #644 评论 5493295108 问题2：先调 resolve_existing_repo_layout 迁移旧 .git
    let state =
        writer_core::storage::git_repo_layout::resolve_existing_repo_layout(&layout).unwrap();
    // 修复后：应返回 Ready（旧 .git 已迁移到 private git_dir）
    match state {
        writer_core::storage::git_repo_layout::ExistingRepoLayoutState::Ready(_) => {
            // 修复生效：旧 app_data_root/.git 被识别并迁移
        }
        writer_core::storage::git_repo_layout::ExistingRepoLayoutState::NotGitRepo => {
            panic!(
                "problem2: resolve_existing_repo_layout 返回 NotGitRepo — 修复未生效：\
                 旧 app_data_root/.git 仍被忽略"
            );
        }
    }
    // 旧 .git 应被迁移走，private git_dir 应存在
    assert!(!old_git_dir.exists(), "problem2: 旧 .git 应被迁移走");
    assert!(
        private_app_git_dir.exists(),
        "problem2: private git_dir 应存在（已迁移）"
    );

    println!(
        "[BUGFIX_REPRO_TRACE] problem2: App target 旧 app_data_root/.git 已被识别并迁移到 private git_dir"
    );
}

// ══ 问题3：journal 状态机 — 后来新建的 .git 不被误删 ══

/// 问题3验证：迁移改为 journal 状态机，先取所有权再复制。
/// 恢复时只删 `claimed_source`，绝不能删除后来重新出现在 `worktree/.git` 的别人的仓库。
#[test]
fn problem3_journal_state_machine_preserves_later_created_git() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();

    let tmp = TempDir::new().unwrap();
    let projects_root = tmp.path().join("projects");
    let private_git_root = tmp.path().join("sujian-git");
    let project_id = "test-proj-3";
    let worktree_root = projects_root.join(project_id);
    let private_git_dir = private_git_root.join(project_id);
    let embedded_git_dir = worktree_root.join(".git");

    fs::create_dir_all(&worktree_root).unwrap();
    fs::create_dir_all(&private_git_root).unwrap();

    // 步骤1：在 worktree_root 下建旧 embedded .git
    let old_repo = git2::Repository::init(&worktree_root).unwrap();
    fs::write(worktree_root.join("project.json"), "{}").unwrap();
    let mut index = old_repo.index().unwrap();
    index
        .add_path(std::path::Path::new("project.json"))
        .unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = old_repo.find_tree(tree_oid).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    old_repo
        .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();
    old_repo
        .remote("origin", "https://github.com/test/proj3.git")
        .unwrap();

    // 步骤2：调 ensure_project_repo_with_layout 触发迁移
    let layout = writer_core::storage::git_repo_layout::GitRepoLayout::with_external_git_dir(
        worktree_root.clone(),
        private_git_dir.clone(),
    );
    writer_core::storage::workspace_git::ensure_workspace_repo(&layout).unwrap();

    // 迁移完成后：embedded .git 应被删除，private git_dir 应存在
    assert!(
        !embedded_git_dir.exists(),
        "迁移完成后 embedded .git 应被删除"
    );
    assert!(
        private_git_dir.exists(),
        "迁移完成后 private git_dir 应存在"
    );

    // 步骤3：模拟崩溃序列——手动写一个 journal，claimed_source 指向已不存在的路径
    let journal_path = private_git_dir.join(".sujian-layout-migration");
    let journal_content = format!(
        r#"{{"migration_uuid":"crash-recovery-test","worktree_canonical":"{}","original_source":"{}","claimed_source":"{}","target_git_dir":"{}","phase":"copied"}}"#,
        canonicalize_or_lossy_for_test(&worktree_root),
        embedded_git_dir.to_string_lossy(),
        worktree_root
            .join(".git.sujian-migrate-source-dead")
            .to_string_lossy(),
        private_git_dir.to_string_lossy()
    );
    fs::write(&journal_path, journal_content).unwrap();

    // 步骤4：模拟"以后某个外部程序又在同一路径新建 worktree/.git"
    let later_repo = git2::Repository::init(&worktree_root).unwrap();
    fs::write(worktree_root.join("later-file.txt"), "later content").unwrap();
    let mut later_index = later_repo.index().unwrap();
    later_index
        .add_path(std::path::Path::new("later-file.txt"))
        .unwrap();
    later_index.write().unwrap();
    let later_tree_oid = later_index.write_tree().unwrap();
    let later_tree = later_repo.find_tree(later_tree_oid).unwrap();
    let later_sig = git2::Signature::now("later", "later@example.com").unwrap();
    later_repo
        .commit(
            Some("HEAD"),
            &later_sig,
            &later_sig,
            "later commit",
            &later_tree,
            &[],
        )
        .unwrap();
    assert!(
        embedded_git_dir.exists(),
        "test setup: 后来新建的 .git 应存在"
    );

    // 步骤5：再次调 ensure_project_repo_with_layout（模拟下次启动恢复）
    let result = writer_core::storage::workspace_git::ensure_workspace_repo(&layout);
    assert!(result.is_ok(), "problem3: 恢复应返回 Ok");

    // 修复后：后来新建的 .git 不应被误删
    assert!(
        embedded_git_dir.exists(),
        "problem3: 后来新建的 .git 不应被误删 — 修复未生效"
    );
    // journal 应被清理（terminal cleanup）
    assert!(
        !journal_path.exists(),
        "problem3: journal 应被清理（terminal cleanup）"
    );

    println!(
        "[BUGFIX_REPRO_TRACE] problem3: journal 状态机恢复时保留后来新建的 .git，\
         只清理 journal（terminal cleanup）"
    );
}

/// 测试用 canonicalize 或 lossy。
fn canonicalize_or_lossy_for_test(path: &std::path::Path) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

// ══ 问题4：delete_project 在 workspace layout 下不遗留孤儿仓库 ══

/// 问题4验证：#645 workspace layout 模型下，git_dir 是所有 target 共享的
/// （`root/workspace/`），删除单个作品只移 worktree 进 trash，
/// 不移动共享 workspace git_dir，因此不会遗留孤儿仓库。
#[test]
fn problem4_delete_project_cleans_private_git_dir() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();

    let tmp = TempDir::new().unwrap();
    let app_data_root = tmp.path().join("app_data_root");
    let projects_root = app_data_root.join("projects");
    let private_git_root = tmp.path().join("sujian-git");
    fs::create_dir_all(&projects_root).unwrap();
    fs::create_dir_all(&private_git_root).unwrap();

    let api = writer_core::api::service::WriterCoreApi::with_platform_services(
        &app_data_root,
        &projects_root,
        None,
        None,
    );

    let project_id = "test-proj-4-manual";
    let shared_worktree = projects_root.join(project_id);
    // #645 workspace layout：git_dir 在 root/workspace/，所有 target 共享。
    let workspace_git_dir = private_git_root.join("workspace");
    fs::create_dir_all(&shared_worktree).unwrap();
    fs::create_dir_all(&workspace_git_dir).unwrap();

    let now = chrono::Utc::now().to_rfc3339();
    let project_json = format!(
        r#"{{"id":"{}","title":"Test Project 4","created_at":"{}","updated_at":"{}","order":0}}"#,
        project_id, now, now
    );
    fs::write(shared_worktree.join("project.json"), project_json).unwrap();
    git2::Repository::init_bare(&workspace_git_dir).unwrap();

    assert!(
        workspace_git_dir.exists(),
        "test setup: workspace git_dir 应存在"
    );
    assert!(shared_worktree.exists(), "test setup: 共享 worktree 应存在");

    // 调 delete_project（通过 pub API）
    api.delete_project(project_id).unwrap();

    // 共享 worktree 应被移进 trash
    assert!(
        !shared_worktree.exists(),
        "delete_project 后共享 worktree 应被移进 trash"
    );

    // #645 workspace layout：共享 git_dir 不应被删除（其他作品仍需使用）
    assert!(
        workspace_git_dir.exists(),
        "problem4: workspace git_dir 应保留（共享，不因删除单个作品而移除）"
    );

    println!(
        "[BUGFIX_REPRO_TRACE] problem4: delete_project 在 workspace layout 下 \
         不遗留孤儿仓库（共享 git_dir 保留）"
    );
}

// ══ 综合修复验证汇总 ══

/// 综合测试：确认 4 个问题的修复都已生效。
#[test]
fn problem_all_four_issues_fixed_on_current_branch() {
    let project_src = read_src_file("src/project.rs");
    let staging_src = read_src_file("src/sync/staging/run.rs");
    let git_repo_layout_src = format!(
        "{}\n{}",
        read_src_file("src/storage/git_repo_layout/mod.rs"),
        read_src_file("src/storage/git_repo_layout/migration.rs")
    );
    let project_ops_src = read_src_file("src/facade/project_ops.rs");

    // 问题1：list_projects_inner 不再调 ensure_project_repo_with_layout
    let list_inner_body = extract_fn_body(&project_src, "list_projects_inner");
    assert!(!list_inner_body.contains("ensure_project_repo_with_layout"));
    // #645 评论 5504296097 第2点：workspace Git 初始化已移到 bootstrap，
    // prepare_staging_runs 不再调 ensure_project_repo_with_layout。
    assert!(!staging_src.contains("fn prepare_target_git_layout("));
    let bootstrap_src = read_src_file("src/api/bootstrap.rs");
    assert!(bootstrap_src.contains("ensure_workspace_git"));

    // 问题2：resolve_existing_repo_layout 已新增
    assert!(git_repo_layout_src.contains("resolve_existing_repo_layout"));
    assert!(git_repo_layout_src.contains("ExistingRepoLayoutState"));

    // 问题3：journal 状态机
    assert!(git_repo_layout_src.contains("LayoutMigrationJournal"));
    assert!(git_repo_layout_src.contains("claimed_source"));
    assert!(git_repo_layout_src.contains("complete_migration_with_journal"));

    // 问题4：delete_project 已合并（不再有 delete_project_with_layout）。
    // #645 评论 5504296097 问题2：facade `delete_project` 和 core 简化入口
    // `project::delete_project`（直接 ack 不记 history）已删除，
    // 所有写操作统一走 `delete_project_with_changes → record_workspace_change_set → ack`。
    assert!(!project_src.contains("fn delete_project_with_layout("));
    assert!(project_src.contains("fn delete_project_with_changes("));
    assert!(!project_ops_src.contains("pub fn delete_project("));

    println!("[BUGFIX_REPRO_TRACE] problem_all: 4 个问题的修复全部生效于当前分支 HEAD");
}
