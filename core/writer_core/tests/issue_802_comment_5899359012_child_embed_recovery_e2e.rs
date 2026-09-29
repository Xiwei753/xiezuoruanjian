//! Issue #802 评论 5899359012 — 子嵌入 journal recovery 端到端闭环集成测试。
//!
//! 验证两项修复的端到端交互闭环：
//! 1. **history 失败处理**：`create_starmap_child_embed()` 在 history 失败时不返回 Err，
//!    返回 `pending: true`，journal 保留在 `EmbedAdded` phase。
//! 2. **recovery 闭环**：启动 `recover_pending_child_embed_transactions()` 能读到保留的
//!    journal 并返回待补 history 的 change-set；`ack_child_embed_history()` 推进 journal
//!    到 `Completed` 并清理 journal 文件，闭环完成。
//!
//! 本测试覆盖的是"history 失败 → journal 留盘 → 重启 recovery 补记 → ack 清理"的
//! 完整生命周期，现有单元测试只覆盖各阶段独立行为，service 层集成测试
//! (`issue_802_comment_5899311341_child_embed_pending`) 只验证 `create` 返回值，
//! 均未验证后续 recovery 闭环。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{CreateStarMapChildEmbedResultDto, StarMapPointDto};
use writer_core::api::WriterCoreApi;
use writer_core::storage::git_repo_layout::GitRepoLayout;
use writer_core::storage::journal::starmap_child_embed::{
    ack_child_embed_history, recover_pending_child_embed_transactions, CHILD_EMBED_JOURNAL_PREFIX,
};
use writer_core::storage::{ensure_workspace_repo, git_runtime};

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

/// 构造测试用 `WriterCoreApi` 实例，但**不**初始化 workspace git 仓库。
///
/// 没有 `.git` 目录时 `record_workspace_change_set_history` 返回 Err，
/// `create_starmap_child_embed` 应返回 `pending: true` 且 journal 保留在磁盘。
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

/// 构造测试用 `WriterCoreApi` 实例，并在 `app_data_root` 下初始化 workspace git 仓库。
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

/// 构造一个有效的星图节点位置。
fn sample_position() -> StarMapPointDto {
    StarMapPointDto { x: 100.0, y: 60.0 }
}

/// 列出 `app_data_root` 下所有 child-embed journal 文件路径。
fn list_child_embed_journals(app_data_root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let dir = app_data_root.join("app-meta/child-embed-journals");
    if !dir.exists() {
        return Vec::new();
    }
    std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let path = e.unwrap().path();
            let name = path.file_name()?.to_string_lossy();
            if name.starts_with(CHILD_EMBED_JOURNAL_PREFIX) {
                Some(path)
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 1. history 失败 → journal 保留 → recovery 返回 change-set → ack 清理
// ---------------------------------------------------------------------------

/// 端到端闭环：`create_starmap_child_embed` 在 history 失败时返回 `pending: true`，
/// journal 保留在 `EmbedAdded`；随后 `recover_pending_child_embed_transactions` 读到
/// 该 journal 并返回待补 history 的 change-set；`ack_child_embed_history` 推进 journal
/// 到 `Completed` 并删除 journal 文件。
///
/// 这是 Issue #802 评论 5899359012 的核心闭环验证。
#[test]
fn history_failure_retains_journal_then_recovery_and_ack_complete_cycle() {
    let (tmp, api) = make_api_without_git();
    let app_data_root = tmp.path();

    // 先创建宿主星图（无 git 时 create_starmap 仍返回 Ok）。
    let host = api
        .create_starmap("宿主星图", "无 git 环境宿主", None)
        .unwrap();

    // 创建子嵌入：history 失败，应返回 pending=true 且不返回 Err。
    let result: CreateStarMapChildEmbedResultDto = api
        .create_starmap_child_embed(&host.starmap_id, "子星图", sample_position())
        .unwrap();
    assert!(
        result.pending,
        "history 失败时 pending 应为 true \
         (starmap_id={}, embed_instance_id={})",
        result.starmap.starmap_id, result.embed.instance_id,
    );

    // journal 应保留在磁盘（EmbedAdded phase，待 recovery 补记 history）。
    let journals = list_child_embed_journals(app_data_root);
    assert_eq!(
        journals.len(),
        1,
        "history 失败后应恰好保留 1 个 journal 文件，实际 {:?}",
        journals,
    );
    let journal_path = journals[0].clone();

    // recovery：读到保留的 journal，返回待补 history 的 change-set。
    let recovered = recover_pending_child_embed_transactions(app_data_root).unwrap();
    assert_eq!(
        recovered.len(),
        1,
        "recover 应返回 1 个待补 history 的 change-set",
    );
    let journal_token = recovered[0].journal_token.clone();
    assert!(
        journal_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(&journal_token),
        "recover 返回的 journal_token 应对应保留的 journal 文件 \
         (journal_path={}, token={})",
        journal_path.display(),
        journal_token,
    );

    // ack：推进 journal 到 Completed 并清理 journal 文件。
    ack_child_embed_history(app_data_root, &journal_token).unwrap();
    assert!(
        !journal_path.exists(),
        "ack 后 journal 文件应已删除，闭环完成",
    );

    // 再次 recover：应无待处理 journal（幂等）。
    let recovered_again = recover_pending_child_embed_transactions(app_data_root).unwrap();
    assert!(
        recovered_again.is_empty(),
        "ack 后再次 recover 应无待处理 journal",
    );
}

// ---------------------------------------------------------------------------
// 2. 正常路径闭环：history 成功 → journal 不残留 → recover 无待处理
// ---------------------------------------------------------------------------

/// 对照测试：`create_starmap_child_embed` 在 history 成功时返回 `pending: false`，
/// journal 已被 ack 清理，`recover_pending_child_embed_transactions` 无待处理项。
#[test]
fn history_success_completes_journal_and_recovery_is_idle() {
    let (tmp, api) = make_api_with_git();
    let app_data_root = tmp.path();

    let host = api
        .create_starmap("宿主星图", "有 git 环境宿主", None)
        .unwrap();

    let result: CreateStarMapChildEmbedResultDto = api
        .create_starmap_child_embed(&host.starmap_id, "子星图", sample_position())
        .unwrap();
    assert!(!result.pending, "history 成功时 pending 应为 false",);

    // history 成功后 journal 应已被 ack 清理，磁盘无残留。
    let journals = list_child_embed_journals(app_data_root);
    assert!(
        journals.is_empty(),
        "history 成功后不应残留 journal 文件，实际 {:?}",
        journals,
    );

    // recover 无待处理项。
    let recovered = recover_pending_child_embed_transactions(app_data_root).unwrap();
    assert!(
        recovered.is_empty(),
        "正常路径完成后 recover 应无待处理 journal",
    );
}
