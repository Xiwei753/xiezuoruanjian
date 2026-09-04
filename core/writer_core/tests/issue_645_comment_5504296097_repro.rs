//! Issue #645 评论 5504296097 — changed-path 收口 4 个问题的回归测试。
//!
//! 本测试文件验证评论 5504296097 指出的 4 个收口问题已修复。这些测试断言
//! "修复后正确行为"，证明修复方案生效。
//!
//! 四个问题及修复后期望：
//! 1. `record_workspace_paths(paths=[])` 直接返回空结果，不触发全量扫描。
//! 2. changed paths 由底层/变更集提供，目录创建/删除不漏文件：
//!    (a) create_chapter 同时记录 chapter.md；
//!    (b) delete_project 用 DeleteTree 清理整个项目目录；
//!    (c) delete_volume 用 DeleteTree 清理整个卷目录。
//! 3. StarMap 直接持久化（create/rename/delete/bind/unbind/set_main/create_child）
//!    进入本地 Git history。
//! 4. `is_workspace_history_path` 把 sync engine state 排除，
//!    manifest.sync.json / state.local.json / config.local.json 不进历史；
//!    save_app_sync_state / save_sync_config 不再提交本地 history。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use tempfile::TempDir;
use writer_core::api::WriterCoreApi;
use writer_core::storage::git_repo_layout::GitRepoLayout;
use writer_core::storage::workspace_paths::is_workspace_history_path;
use writer_core::storage::{ensure_workspace_repo, list_workspace_history, record_workspace_paths};

// ── helpers ──

/// 构造一个带 workspace git 的 WriterCoreApi。
/// `app_data_root = tmp`，`projects_root = tmp/projects`，与生产布局一致。
fn make_api() -> (TempDir, WriterCoreApi, GitRepoLayout) {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();
    let layout = GitRepoLayout::new(app_data_root.clone());
    ensure_workspace_repo(&layout).unwrap();
    let api = WriterCoreApi::new(&app_data_root, &projects_root);
    (tmp, api, layout)
}

/// 在 worktree 下写入文件，自动创建父目录。
fn write_worktree(tmp: &TempDir, rel: &str, content: &str) {
    let path = tmp.path().join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

// ══ 问题1：空 paths 不触发全量扫描 ══

/// 修复后：`record_workspace_paths(&[], ...)` 传空 paths 时直接返回空结果，
/// 不扫描 worktree。
#[test]
fn problem1_empty_paths_does_not_trigger_full_scan() {
    let (tmp, _api, layout) = make_api();

    write_worktree(&tmp, "a.txt", "content-a");
    let r1 = record_workspace_paths(&layout, &[PathBuf::from("a.txt")], "first").unwrap();
    assert!(r1.oid.is_some(), "first commit should be created");

    // 写 b.txt，但不传给 record_workspace_paths。
    write_worktree(&tmp, "b.txt", "content-b");

    // 传空 paths —— 修复后直接返回空结果，不扫描。
    let r2 = record_workspace_paths(&layout, &[], "empty_paths").unwrap();
    assert!(
        r2.oid.is_none(),
        "问题1修复：空 paths 不触发全量扫描，直接返回 oid=None"
    );
    assert_eq!(r2.staged_count, 0);

    // history 仍只有 1 条（first）。
    let history = list_workspace_history(&layout, 10).unwrap();
    assert_eq!(history.len(), 1, "问题1修复：空 paths 不产生新 commit");
}

// ══ 问题2(a)：create_chapter 同时记录 chapter.md ══

/// 修复后：`create_chapter` 同时把 chapter.meta.json 和 chapter.md 记进 history。
/// 显式 stage chapter.md 时无变化（已在 tree）。
#[test]
fn problem2a_create_chapter_records_chapter_md() {
    let (_tmp, api, layout) = make_api();

    let project = api.create_project("测试作品").unwrap();
    let volumes = api.list_volumes(&project.id).unwrap();
    let volume_id = volumes[0].id.clone();
    let chapter = api
        .create_chapter(&project.id, &volume_id, "第一章")
        .unwrap();

    // 验证 create_chapter 已产生 commit
    let hist = list_workspace_history(&layout, 10).unwrap();
    assert!(
        hist.len() >= 2,
        "create_chapter 应产生至少 2 条 commit（create_project + create_chapter），实际 {}",
        hist.len()
    );

    let chapter_md_rel = PathBuf::from("projects")
        .join(&project.id)
        .join("volumes")
        .join(&volume_id)
        .join("chapters")
        .join(&chapter.id)
        .join("chapter.md");

    // 显式 stage chapter.md。修复后：chapter.md 已在 tree，无变化 oid=None。
    let r = record_workspace_paths(&layout, &[chapter_md_rel], "add_chapter_md").unwrap();
    assert!(
        r.oid.is_none(),
        "问题2(a)修复：create_chapter 已记录 chapter.md，显式 stage 无变化"
    );
}

// ══ 问题2(b)：delete_project 用 DeleteTree 清理整个项目目录 ══

/// 修复后：`delete_project` 用 DeleteTree(projects/{pid}) 清理整个项目目录，
/// 子文件不残留 in tree。
#[test]
fn problem2b_delete_project_cleans_child_files() {
    let (_tmp, api, layout) = make_api();

    let project = api.create_project("待删作品").unwrap();
    let volumes = api.list_volumes(&project.id).unwrap();
    let volume_id = volumes[0].id.clone();
    let chapter = api.create_chapter(&project.id, &volume_id, "章").unwrap();
    // 让 chapter.md 有内容并 tracked。
    api.save_chapter_content(&project.id, &volume_id, &chapter.id, "正文内容")
        .unwrap();

    // 全量 commit 让 volume.json / chapter.meta.json / chapter.md 都 tracked。
    writer_core::storage::record_all_workspace_changes(&layout, "snapshot_before_delete").unwrap();

    let volume_json_rel = PathBuf::from("projects")
        .join(&project.id)
        .join("volumes")
        .join(&volume_id)
        .join("volume.json");

    // 删除作品。修复后用 DeleteTree 清理整个 projects/{pid}/。
    api.delete_project(&project.id).unwrap();

    // 显式传 volume.json 路径。修复后：DeleteTree 已从 index 移除，无变化。
    let r = record_workspace_paths(&layout, &[volume_json_rel], "cleanup_volume_json").unwrap();
    assert!(
        r.oid.is_none(),
        "问题2(b)修复：delete_project 用 DeleteTree 清理了子文件，volume.json 不残留"
    );
}

// ══ 问题2(c)：delete_volume 用 DeleteTree 清理整个卷目录 ══

/// 修复后：`delete_volume` 用 DeleteTree 清理整个卷目录，卷下章节不残留。
#[test]
fn problem2c_delete_volume_cleans_chapter_files() {
    let (_tmp, api, layout) = make_api();

    let project = api.create_project("作品").unwrap();
    let volumes = api.list_volumes(&project.id).unwrap();
    let volume_id = volumes[0].id.clone();
    let chapter = api.create_chapter(&project.id, &volume_id, "章").unwrap();
    api.save_chapter_content(&project.id, &volume_id, &chapter.id, "正文")
        .unwrap();

    // 全量 commit 让章节文件 tracked。
    writer_core::storage::record_all_workspace_changes(&layout, "snapshot_before_volume_delete")
        .unwrap();

    let chapter_meta_rel = PathBuf::from("projects")
        .join(&project.id)
        .join("volumes")
        .join(&volume_id)
        .join("chapters")
        .join(&chapter.id)
        .join("chapter.meta.json");

    // 删除卷。修复后用 DeleteTree 清理整个 volumes/{vid}/。
    api.delete_volume(&project.id, &volume_id).unwrap();

    // 显式传 chapter.meta.json。修复后：DeleteTree 已从 index 移除，无变化。
    let r = record_workspace_paths(&layout, &[chapter_meta_rel], "cleanup_chapter_meta").unwrap();
    assert!(
        r.oid.is_none(),
        "问题2(c)修复：delete_volume 用 DeleteTree 清理了章节文件，chapter.meta.json 不残留"
    );
}

// ══ 问题3：StarMap 直接持久化进入本地 Git history ══

/// 修复后：`create_starmap` 写的 meta/index 文件进入本地 Git history。
#[test]
fn problem3_create_starmap_recorded_in_history() {
    let (tmp, api, layout) = make_api();

    // 先建立一个初始 commit，让 HEAD 存在。
    write_worktree(&tmp, "init.txt", "init");
    record_workspace_paths(&layout, &[PathBuf::from("init.txt")], "init").unwrap();
    let history_before = list_workspace_history(&layout, 10).unwrap();
    assert_eq!(history_before.len(), 1);

    let starmap = api.create_starmap("星图", "描述", None).unwrap();

    // 修复后：create_starmap 调用 record_workspace_change_set_history，
    // commit 数应增加。
    let history = list_workspace_history(&layout, 10).unwrap();
    assert!(
        history.len() > 1,
        "问题3修复：create_starmap 产生了本地 Git commit（直接持久化有 history）"
    );

    let starmap_meta_rel =
        PathBuf::from("starmaps").join(format!("{}.meta.json", starmap.starmap_id));
    // 显式 stage starmap meta。修复后：已在 tree，无变化。
    let r = record_workspace_paths(&layout, &[starmap_meta_rel], "add_starmap_meta").unwrap();
    assert!(
        r.oid.is_none(),
        "问题3修复：create_starmap 写的 meta 文件已在 tree，显式 stage 无变化"
    );
}

/// 修复后：`rename_starmap` / `delete_starmap` / `bind_starmap_to_project` /
/// `unbind_starmap_from_project` / `set_main_starmap_for_project` /
/// `create_child_starmap` 同样进入本地 Git history。
#[test]
fn problem3_other_direct_starmap_ops_recorded() {
    let (_tmp, api, layout) = make_api();

    let parent = api.create_starmap("父星图", "", None).unwrap();
    // rename
    api.rename_starmap(&parent.starmap_id, "新名").unwrap();
    // create_child
    let child = api
        .create_child_starmap(&parent.starmap_id, "子星图", "", None)
        .unwrap();
    // bind 到一个 project
    let project = api.create_project("绑定作品").unwrap();
    api.bind_starmap_to_project(&child.starmap_id, &project.id)
        .unwrap();
    // set main
    api.set_main_starmap_for_project(&child.starmap_id, &project.id)
        .unwrap();
    // unbind
    api.unbind_starmap_from_project(&child.starmap_id).unwrap();

    // 修复后：上述操作都调 record_workspace_change_set_history，产生 commit。
    let history = list_workspace_history(&layout, 10).unwrap();
    // create_starmap(parent) + rename + create_child + create_project +
    // bind + set_main + unbind >= 7 条 commit。
    assert!(
        history.len() >= 7,
        "问题3修复：6 个直接持久化 starmap 操作 + create_project 均产生本地 Git commit（实际 {} 条）",
        history.len()
    );
}

// ══ 问题4：sync engine state 不进本地 Git history ══

/// 修复后：`is_workspace_history_path` 把 sync engine state 排除。
#[test]
fn problem4_sync_engine_state_excluded_from_history_path() {
    // 这些都是同步引擎运行状态，不应进入本地用户版本历史。
    let sync_engine_state_paths = [
        "app-meta/sync/manifest.sync.json",
        "app-meta/sync/state.local.json",
        "app-meta/sync/full_state.local.json",
        "app-meta/sync/conflicts.json",
        "app-meta/sync/config.local.json",
    ];

    for p in &sync_engine_state_paths {
        assert!(
            !is_workspace_history_path(&PathBuf::from(p)),
            "问题4修复：sync engine state 路径 {} 被排除出 history path（归为 SyncEngineState）",
            p
        );
    }
}

/// 修复后：`save_app_sync_state` 不再提交 state.local.json 进本地 history。
#[test]
fn problem4_save_app_sync_state_does_not_commit_to_local_history() {
    let (tmp, api, layout) = make_api();

    // 先建立一个初始 commit，让 HEAD 存在。
    write_worktree(&tmp, "init.txt", "init");
    record_workspace_paths(&layout, &[PathBuf::from("init.txt")], "init").unwrap();
    let history_before = list_workspace_history(&layout, 10).unwrap();
    assert_eq!(history_before.len(), 1);

    let state = writer_core::api::SyncStateDto {
        status: "idle".to_string(),
        last_sync_time: None,
        last_error: None,
        conflicts: None,
    };
    api.save_app_sync_state(state).unwrap();

    // 修复后：save_app_sync_state 不调用 record_workspace_history，commit 数不变。
    let history = list_workspace_history(&layout, 10).unwrap();
    assert_eq!(
        history.len(),
        1,
        "问题4修复：save_app_sync_state 不再把 state.local.json 提交进本地 Git history"
    );
}

/// 修复后：`save_sync_config` 不再提交 config.local.json 进本地 history。
#[test]
fn problem4_save_sync_config_does_not_commit_to_local_history() {
    let (tmp, api, layout) = make_api();

    // 先建立一个初始 commit，让 HEAD 存在。
    write_worktree(&tmp, "init.txt", "init");
    record_workspace_paths(&layout, &[PathBuf::from("init.txt")], "init").unwrap();
    let history_before = list_workspace_history(&layout, 10).unwrap();
    assert_eq!(history_before.len(), 1);

    // 构造一个最小的 SyncConfigDto。enabled=false 即可。
    let config = writer_core::api::SyncConfigDto {
        enabled: false,
        active_provider: String::new(),
        provider_config: None,
        auto_sync: false,
        sync_interval_seconds: 0,
        has_network_permission: false,
        has_network_state_permission: false,
    };
    api.save_sync_config(config).unwrap();

    let history = list_workspace_history(&layout, 10).unwrap();
    assert_eq!(
        history.len(),
        1,
        "问题4修复：save_sync_config 不再把 config.local.json 提交进本地 Git history"
    );
    assert!(
        !history.iter().any(|h| h.message == "save_sync_config"),
        "问题4修复：history 中不应有 save_sync_config commit"
    );
}
