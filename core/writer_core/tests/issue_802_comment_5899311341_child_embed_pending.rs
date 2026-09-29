//! Issue #802 评论 5899311341 — service 层 `create_starmap_child_embed` pending 行为集成测试。
//!
//! 验证两个修复点：
//! 1. `create_starmap_child_embed()` 里 history 失败不再通过 `?` 返回 Err，
//!    而是返回 `pending: true`；history 成功时返回 `pending: false`。
//! 2. `StarMapChildEmbedJournal.embed_instance_id` 改为 `Option<String>` 带
//!    `#[serde(default)]`，旧 journal 可向后兼容；`check_embed_exists` 按
//!    `embed_instance_id + child_starmap_id` 双重确认。
//!
//! 本文件只覆盖 service 层（`WriterCoreApi::create_starmap_child_embed`）的 pending 行为，
//! journal 层的 16 个测试已在 `storage::journal::starmap_child_embed` 模块内覆盖。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{CreateStarMapChildEmbedResultDto, StarMapPointDto};
use writer_core::api::WriterCoreApi;
use writer_core::storage::git_repo_layout::GitRepoLayout;
use writer_core::storage::{ensure_workspace_repo, git_runtime};

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

/// 构造测试用 `WriterCoreApi` 实例，并在 `app_data_root` 下初始化 workspace git 仓库。
///
/// git 仓库存在时 `record_workspace_change_set_history` 能成功 commit，
/// `create_starmap_child_embed` 应返回 `pending: false`。
fn make_api_with_git() -> (TempDir, WriterCoreApi) {
    git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();
    let layout = GitRepoLayout::new(app_data_root.clone());
    ensure_workspace_repo(&layout).unwrap();
    let api = WriterCoreApi::new(&app_data_root, &projects_root);
    (tmp, api)
}

/// 构造测试用 `WriterCoreApi` 实例，但**不**初始化 workspace git 仓库。
///
/// 没有 `.git` 目录时 `record_workspace_change_set` 调 `open_repo` 会失败，
/// `record_workspace_change_set_history` 返回 Err，
/// `create_starmap_child_embed` 应返回 `pending: true` 且仍返回 `Ok`。
fn make_api_without_git() -> (TempDir, WriterCoreApi) {
    git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();
    // 故意不调 ensure_workspace_repo，app_data_root 下没有 .git 目录。
    let api = WriterCoreApi::new(&app_data_root, &projects_root);
    (tmp, api)
}

/// 构造一个有效的星图节点位置。
fn sample_position() -> StarMapPointDto {
    StarMapPointDto { x: 120.0, y: 80.0 }
}

// ---------------------------------------------------------------------------
// 1. 正常路径：history 成功 → pending = false
// ---------------------------------------------------------------------------

/// `create_starmap_child_embed` 在 workspace git 可用时，
/// `record_workspace_change_set_history` 成功 commit，
/// 返回 `Ok` 且 `pending: false`。
#[test]
fn create_starmap_child_embed_returns_pending_false_when_history_succeeds() {
    let (_tmp, api) = make_api_with_git();

    // 先创建一个宿主星图。
    let host = api
        .create_starmap("宿主星图", "用于承载子嵌入的宿主", None)
        .unwrap();

    // 在宿主上创建子星图并嵌入。
    let result: CreateStarMapChildEmbedResultDto = api
        .create_starmap_child_embed(&host.starmap_id, "子星图", sample_position())
        .unwrap();

    // history 成功，pending 必须为 false。
    assert!(
        !result.pending,
        "history 成功时 pending 应为 false，但得到 true \
         (starmap_id={}, embed_instance_id={})",
        result.starmap.starmap_id, result.embed.instance_id,
    );

    // 子星图元数据应正确返回。
    assert_eq!(
        result.starmap.title, "子星图",
        "返回的子星图标题应与传入一致"
    );
    // embed 应指向新创建的子星图。
    assert_eq!(
        result.embed.target_starmap_id, result.starmap.starmap_id,
        "embed 的 target_starmap_id 应等于新创建子星图的 starmap_id"
    );
}

// ---------------------------------------------------------------------------
// 2. history 失败路径：history 失败 → pending = true 且仍返回 Ok
// ---------------------------------------------------------------------------

/// `create_starmap_child_embed` 在 workspace git 不可用时，
/// `record_workspace_change_set_history` 失败，
/// 但方法不传播错误，仍返回 `Ok` 且 `pending: true`。
///
/// 这是 Issue #802 评论 5899311341 的核心修复点：history 失败不再让用户操作失败，
/// journal 保留在 EmbedAdded，下次启动 recover 会补记 history。
#[test]
fn create_starmap_child_embed_returns_pending_true_when_history_fails() {
    let (_tmp, api) = make_api_without_git();

    // 先创建一个宿主星图。create_starmap 内部也会调 history，但用 `let _ =` 忽略错误，
    // 所以即使没有 git 仓库，create_starmap 仍返回 Ok。
    let host = api
        .create_starmap("宿主星图", "无 git 环境下的宿主", None)
        .unwrap();

    // 在宿主上创建子星图并嵌入。history 会失败，但方法应返回 Ok 且 pending=true。
    let result: CreateStarMapChildEmbedResultDto = api
        .create_starmap_child_embed(&host.starmap_id, "子星图", sample_position())
        .unwrap();

    // history 失败，pending 必须为 true。
    assert!(
        result.pending,
        "history 失败时 pending 应为 true，但得到 false \
         (starmap_id={}, embed_instance_id={})",
        result.starmap.starmap_id, result.embed.instance_id,
    );

    // 即使 pending=true，子星图和 embed 仍应实际创建成功。
    assert_eq!(
        result.starmap.title, "子星图",
        "pending=true 时子星图仍应创建成功，标题应与传入一致"
    );
    assert_eq!(
        result.embed.target_starmap_id, result.starmap.starmap_id,
        "pending=true 时 embed 仍应指向新创建的子星图"
    );
}
