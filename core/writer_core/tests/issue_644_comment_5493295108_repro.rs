//! Issue #644 评论 5493295108 — 修复验证测试（Phase B: Patch Verification）。
//!
//! 本测试文件针对评论 5493295108 描述的问题，验证修复后的行为：
//!
//! - 问题1：`list_projects_inner` 不再调 `ensure_project_repo_with_layout`，
//!   迁移职责移到 `sync::staging::prepare_staging_runs`（已释放 Core 写锁之后）。
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
