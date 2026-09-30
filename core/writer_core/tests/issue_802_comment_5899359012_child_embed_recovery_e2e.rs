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
//!
//! 第三条测试走真实生产入口 `bootstrap_workspace()`，验证启动恢复自动补记 history，
//! 并断言 recovery change-set 中的真实文件路径确实进入本地 Git commit tree。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use tempfile::TempDir;
use writer_core::api::bootstrap::bootstrap_workspace;
use writer_core::api::types::{CreateStarMapChildEmbedResultDto, StarMapPointDto};
use writer_core::api::WriterCoreApi;
use writer_core::storage::git_repo_layout::GitRepoLayout;
use writer_core::storage::journal::starmap_child_embed::{
    ack_child_embed_history, recover_pending_child_embed_transactions, CHILD_EMBED_JOURNAL_PREFIX,
};
use writer_core::storage::workspace_git::record_workspace_change_set;
use writer_core::storage::{
    ensure_workspace_repo, git_runtime, list_workspace_history, open_workspace_repo,
};

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

/// 在 `starmaps/{host}/embeds/` 下定位指定 embed 实例的真实文件，
/// 返回相对 workspace 根目录的路径（用于校验补记 commit 的 tree）。
fn find_embed_file_rel(
    app_data_root: &std::path::Path,
    host_starmap_id: &str,
    embed_instance_id: &str,
) -> PathBuf {
    let file_name = format!("{}.json", embed_instance_id);
    let embeds_dir = app_data_root
        .join("starmaps")
        .join(host_starmap_id)
        .join("embeds");
    let mut stack = vec![embeds_dir.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .map(|n| n == file_name.as_str())
                .unwrap_or(false)
            {
                return path
                    .strip_prefix(app_data_root)
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or(path);
            }
        }
    }
    panic!(
        "未找到 embed 文件 {}/{}（create 成功后应已 flush 到磁盘）",
        embeds_dir.display(),
        file_name,
    );
}

// ---------------------------------------------------------------------------
// 1. history 失败 → journal 保留 → recovery 返回 change-set → ack 清理
// ---------------------------------------------------------------------------

/// 端到端闭环：`create_starmap_child_embed` 在 history 失败时返回 `pending: true`，
/// journal 保留在 `EmbedAdded`；随后 `recover_pending_child_embed_transactions` 读到
/// 该 journal 并返回待补 history 的 change-set；模拟"下次启动 Git 已恢复"，
/// 对 change-set 真正调用 `record_workspace_change_set()` 补记 history；
/// 成功后 `ack_child_embed_history` 推进 journal 到 `Completed` 并删除 journal 文件。
///
/// 这是 Issue #802 评论 5899359012 的核心闭环验证，也是评论 5901121699 要求补齐的
/// "history 失败 → 重启补 history → ack"完整端到端测试。
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

    // 模拟"下次启动 Git 已恢复"：创建 GitRepoLayout 并初始化 workspace git 仓库。
    let layout = GitRepoLayout::new(app_data_root.to_path_buf());
    ensure_workspace_repo(&layout).unwrap();

    // 对 recovery 返回的 change-set 真正调用 record_workspace_change_set 补记 history。
    let commit_result = record_workspace_change_set(
        &layout,
        &recovered[0].changes,
        "recover_child_embed_history",
    )
    .unwrap();
    assert!(
        commit_result.oid.is_some(),
        "record_workspace_change_set 应成功产生 commit (staged_count={})",
        commit_result.staged_count,
    );

    // history 已补记成功，ack：推进 journal 到 Completed 并清理 journal 文件。
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

// ---------------------------------------------------------------------------
// 3. 生产入口闭环：bootstrap_workspace() 自动补 history 并 ack
// ---------------------------------------------------------------------------

/// 真实生产入口闭环：`bootstrap_workspace()`（`open_app_service` 与 Linux_Qt
/// `create_core_api` 共用的启动入口）在下次启动时自动执行
/// "recover child embed journal → record_workspace_change_set → ack_child_embed_history"，
/// 不需要测试手工拼调用顺序。
///
/// 与第一条测试的区别：第一条按评论 5901121699 的步骤手工调用各函数验证顺序；
/// 本测试走真实 bootstrap，防止测试步骤与生产顺序（history 成功后才 ack）漂移，
/// 并断言 recovery change-set 中的真实文件路径确实进入本地 Git commit tree——
/// 即使 change-set 路径有误、history 根本记不进去，本测试会直接失败。
#[test]
fn bootstrap_workspace_recovers_pending_child_embed_into_real_history() {
    let (tmp, api) = make_api_without_git();
    let app_data_root = tmp.path();

    // 先创建宿主星图，再在无 Git 环境创建 child：pending=true，journal 留盘。
    let host = api
        .create_starmap("宿主星图", "bootstrap 闭环宿主", None)
        .unwrap();
    let result: CreateStarMapChildEmbedResultDto = api
        .create_starmap_child_embed(&host.starmap_id, "子星图", sample_position())
        .unwrap();
    assert!(
        result.pending,
        "无 Git 环境创建 child 应返回 pending=true (embed_instance_id={})",
        result.embed.instance_id,
    );
    let journals = list_child_embed_journals(app_data_root);
    assert_eq!(
        journals.len(),
        1,
        "history 失败后应恰好保留 1 个 journal 文件，实际 {:?}",
        journals,
    );
    let journal_path = journals[0].clone();

    // 下次启动：真实生产入口。
    let layout = bootstrap_workspace(app_data_root).unwrap();

    // 生产代码应已补 history 并 ack：journal 删除，磁盘无残留。
    assert!(
        !journal_path.exists(),
        "bootstrap_workspace 应自动补 history 并 ack 清理 journal",
    );
    assert!(
        list_child_embed_journals(app_data_root).is_empty(),
        "bootstrap 后不应残留 child embed journal",
    );

    // 再次 recover 幂等为空。
    let recovered_again = recover_pending_child_embed_transactions(app_data_root).unwrap();
    assert!(
        recovered_again.is_empty(),
        "bootstrap 后再次 recover 应无待处理 journal",
    );

    // 本地 Git history 真的记下了 recovery commit。
    let history = list_workspace_history(&layout, 10).unwrap();
    assert!(
        history
            .iter()
            .any(|commit| commit.message == "recover_starmap_child_embed"),
        "history 应包含 message=recover_starmap_child_embed 的补记 commit，实际 {:?}",
        history
            .iter()
            .map(|commit| commit.message.as_str())
            .collect::<Vec<_>>(),
    );

    // change-set 中的真实文件路径真的进入 commit tree。
    let repo = open_workspace_repo(&layout).unwrap();
    let head_tree = repo.head().unwrap().peel_to_tree().unwrap();
    let expected_paths = [
        PathBuf::from("starmaps").join("index.json"),
        PathBuf::from("starmaps").join(format!("{}.meta.json", result.starmap.starmap_id)),
        PathBuf::from("starmaps")
            .join(&host.starmap_id)
            .join("graph.json"),
        find_embed_file_rel(app_data_root, &host.starmap_id, &result.embed.instance_id),
    ];
    for rel in &expected_paths {
        assert!(
            head_tree.get_path(rel).is_ok(),
            "补记 commit tree 应包含 {}",
            rel.display(),
        );
    }
}
