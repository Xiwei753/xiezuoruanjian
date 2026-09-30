//! Issue #802 评论 5900824642 — `WriterCoreApi::new()` 不偷偷初始化仓库的回归守卫。
//!
//! 评论 5900824642 在复核 service 层 `pending` 行为集成测试时，通过代码审查确认：
//!
//! > `WriterCoreApi::new()` 只建立默认 Git layout，不会偷偷初始化仓库；
//! > 未调用 `ensure_workspace_repo()` 的第二条测试确实会走 history Err，
//! > 所以这条失败路径不是假测试。
//!
//! 本文件把这条代码审查结论转化为可执行的自动化验证，防止未来回归：
//!
//! 1. `WriterCoreApi::new()` 后 `app_data_root/.git` 目录**不存在**；
//! 2. 调用 `ensure_workspace_repo()` 后 `.git` 目录**存在**；
//! 3. 没有 git 仓库时 `create_starmap` 仍返回 `Ok`（内部 best-effort 忽略 history 错误），
//!    但 `create_starmap_child_embed` 返回 `pending: true`——确认失败路径不是假测试。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{CreateStarMapChildEmbedResultDto, StarMapPointDto};
use writer_core::api::WriterCoreApi;
use writer_core::storage::git_repo_layout::GitRepoLayout;
use writer_core::storage::{ensure_workspace_repo, git_runtime};

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

fn fresh_tmp() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();
    (tmp, app_data_root, projects_root)
}

fn sample_position() -> StarMapPointDto {
    StarMapPointDto { x: 100.0, y: 60.0 }
}

// ---------------------------------------------------------------------------
// 1. `WriterCoreApi::new()` 不会创建 `.git` 目录
// ---------------------------------------------------------------------------

/// `WriterCoreApi::new()` 只构造 `GitRepoLayout`（纯路径计算），
/// 不调用 `git2::Repository::init`，因此 `app_data_root/.git` 不应存在。
///
/// 这是评论 5900824642 确认的第一条保证：`new()` 不会偷偷初始化仓库。
/// 如果未来有人改 `new()` 让它顺手 init，本测试会立即失败。
#[test]
fn writer_core_api_new_does_not_create_git_dir() {
    let (_tmp, app_data_root, projects_root) = fresh_tmp();

    let _api = WriterCoreApi::new(&app_data_root, &projects_root);

    let git_dir = app_data_root.join(".git");
    assert!(
        !git_dir.exists(),
        "WriterCoreApi::new() 不应创建 .git 目录，但发现 {}",
        git_dir.display(),
    );
}

// ---------------------------------------------------------------------------
// 2. `ensure_workspace_repo()` 后 `.git` 目录存在
// ---------------------------------------------------------------------------

/// `ensure_workspace_repo()` 是唯一初始化入口，
/// 调用后 `app_data_root/.git` 必须存在。
///
/// 与上一条配对，确认 git 初始化只发生在显式调用 `ensure_workspace_repo()` 时。
#[test]
fn ensure_workspace_repo_creates_git_dir() {
    let (_tmp, app_data_root, projects_root) = fresh_tmp();

    // new() 不创建 .git
    let _api = WriterCoreApi::new(&app_data_root, &projects_root);
    assert!(!app_data_root.join(".git").exists());

    // 显式初始化
    let layout = GitRepoLayout::new(app_data_root.clone());
    ensure_workspace_repo(&layout).unwrap();

    assert!(
        app_data_root.join(".git").exists(),
        "ensure_workspace_repo() 后 .git 目录应存在",
    );
}

// ---------------------------------------------------------------------------
// 3. 无 git 仓库时失败路径不是假测试
// ---------------------------------------------------------------------------

/// 没有 `.git` 目录时：
/// - `create_starmap` 仍返回 `Ok`（内部 best-effort 忽略 history 错误）；
/// - `create_starmap_child_embed` 返回 `Ok` 且 `pending: true`。
///
/// 这确认了评论 5900824642 的第二条保证：未调用 `ensure_workspace_repo()`
/// 时 `record_workspace_change_set_history` 确实走 Err，`pending` 为 true，
/// 不是假测试。
#[test]
fn without_git_create_starmap_succeeds_but_child_embed_is_pending() {
    let (_tmp, app_data_root, projects_root) = fresh_tmp();

    // 不调 ensure_workspace_repo，没有 .git
    let api = WriterCoreApi::new(&app_data_root, &projects_root);
    assert!(!app_data_root.join(".git").exists());

    // create_starmap 内部用 let _ = 忽略 history 错误，仍返回 Ok
    let host = api.create_starmap("宿主", "无 git 环境", None).unwrap();

    // create_starmap_child_embed: history 失败 → pending = true，但仍返回 Ok
    let result: CreateStarMapChildEmbedResultDto = api
        .create_starmap_child_embed(&host.starmap_id, "子图", sample_position())
        .unwrap();

    assert!(
        result.pending,
        "无 git 仓库时 history 必然失败，pending 应为 true \
         (starmap_id={})",
        result.starmap.starmap_id,
    );
    // 子图和 embed 仍应实际创建成功
    assert_eq!(result.starmap.title, "子图");
    assert_eq!(
        result.embed.target_starmap_id, result.starmap.starmap_id,
        "pending=true 时 embed 仍应指向新创建的子星图",
    );
}
