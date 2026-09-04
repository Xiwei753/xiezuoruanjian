//! Issue #645 评论 5504296097 — Phase 2 收口 3 个问题的复现测试。
//!
//! 本测试文件验证评论 5504296097 第 2 轮指出的 3 个实质问题仍然存在。
//! 这些测试断言"修复后正确行为"，但在当前 bug 下会 FAIL，从而证明 bug 可复现。
//!
//! 三个问题：
//! 1. `reorder_volumes_with_changes()` 漏真实落盘文件：底层 `reorder_volumes`
//!    对每个卷都重写 volume.json（updated_at 变化），但 change_set 只记录
//!    order 变化的文件。传入相同顺序时，磁盘全变但 change_set 为空。
//! 2. `delete_project()` 把 StarMap 解绑放在删除事务外：API 层先做 best-effort
//!    unbind loop，再调 delete_project_with_changes。若删除失败，starmap 已解绑
//!    （半状态：starmap.project_id = None 但 project 还在）。
//! 3. `workspace_paths` 漏了 `sync/trash/` 和 `app-meta/delete-journals/`：
//!    这两个真实运行时目录被归成 UserContent，且 is_workspace_internal_path_str
//!    返回 false、is_workspace_history_path_str 返回 true（bug）。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::WriterCoreApi;
use writer_core::storage::git_repo_layout::GitRepoLayout;
use writer_core::storage::workspace_paths::{
    classify_workspace_path_str, is_workspace_history_path_str, is_workspace_internal_path_str,
    WorkspacePathClass,
};
use writer_core::storage::{ensure_workspace_repo, git_runtime};
use writer_core::volume::{list_volumes, reorder_volumes_with_changes, Volume};

// ── helpers ──

/// 构造一个带 workspace git 的 WriterCoreApi。
/// `app_data_root = tmp`，`projects_root = tmp/projects`。
fn make_api() -> (TempDir, WriterCoreApi, GitRepoLayout) {
    git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();
    let layout = GitRepoLayout::new(app_data_root.clone());
    ensure_workspace_repo(&layout).unwrap();
    let api = WriterCoreApi::new(&app_data_root, &projects_root);
    (tmp, api, layout)
}

/// 读取 volume.json 并解析为 Volume。
fn read_volume_json(meta_path: &std::path::Path) -> Volume {
    let content = std::fs::read_to_string(meta_path).unwrap();
    serde_json::from_str::<Volume>(&content).unwrap()
}

// ══ 问题1：reorder_volumes_with_changes 漏真实落盘文件 ══

/// 复现：传入和当前顺序相同的 ordered_ids（不改变顺序）。
///
/// 修复后正确行为：因为没有 order 变化，磁盘上不应有任何 volume.json 被重写
/// （updated_at 不变），change_set 为空。两者一致。
///
/// 当前 bug：底层 `reorder_volumes` 对每个卷都无条件重写 volume.json
/// （`meta.updated_at = Utc::now().to_rfc3339()`），所以磁盘上所有 volume.json
/// 的 updated_at 都变了；但 `reorder_volumes_with_changes` 只记录 order 变化的
/// 文件，change_set 为空。磁盘事实（N 个文件变了）和 change_set（0 个）不一致。
#[test]
fn problem1_reorder_with_same_order_should_not_rewrite_any_volume_json() {
    let (tmp, _api, _layout) = make_api();
    let projects_root = tmp.path().join("projects");

    let project = writer_core::project::create_project(&projects_root, "测试作品").unwrap();
    let project_root = projects_root.join(&project.id);

    // create_project 会自带 1 个默认卷，再额外创建 2 个，共 3 个。
    writer_core::volume::create_volume(&project_root, "卷二").unwrap();
    writer_core::volume::create_volume(&project_root, "卷三").unwrap();

    let volumes = list_volumes(&project_root).unwrap();
    assert_eq!(volumes.len(), 3, "前置：项目下应有 3 个卷");

    // 记录每个 volume.json 的 updated_at。
    let mut before: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for v in &volumes {
        let meta_path = project_root.join("volumes").join(&v.id).join("volume.json");
        let meta = read_volume_json(&meta_path);
        before.insert(v.id.clone(), meta.updated_at.clone());
    }

    // 传入和当前顺序完全相同的 ordered_ids。
    let ordered_ids: Vec<String> = volumes.iter().map(|v| v.id.clone()).collect();

    // 等待 >1 秒确保 chrono rfc3339 精度可区分。
    std::thread::sleep(std::time::Duration::from_millis(1100));

    let change_set = reorder_volumes_with_changes(&project_root, &ordered_ids, tmp.path())
        .expect("reorder_volumes_with_changes 应成功");

    // 重新读取每个 volume.json 的 updated_at。
    let mut after: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for v in &volumes {
        let meta_path = project_root.join("volumes").join(&v.id).join("volume.json");
        let meta = read_volume_json(&meta_path);
        after.insert(v.id.clone(), meta.updated_at.clone());
    }

    let disk_changed = volumes
        .iter()
        .filter(|v| before[&v.id] != after[&v.id])
        .count();

    // ── 断言修复后正确行为 ──
    // 修复后：没有 order 变化时，磁盘上不应有任何 volume.json 的 updated_at 变化。
    assert_eq!(
        disk_changed, 0,
        "问题1修复：传入相同顺序时，磁盘上不应有任何 volume.json 的 updated_at 变化（实际 {} 个变了）",
        disk_changed,
    );

    // 修复后：change_set 应为空。
    assert!(
        change_set.is_empty(),
        "问题1修复：传入相同顺序时，change_set 应为空",
    );
}

/// 复现：只交换两个卷的顺序。
///
/// 修复后正确行为：只有 order 变化的 2 个 volume.json 被重写，
/// change_set 包含 2 个路径。两者一致。
///
/// 当前 bug：底层对所有 N 个卷都重写 volume.json，但 change_set 只含 2 个路径。
#[test]
fn problem1_reorder_with_swap_should_only_rewrite_changed_volumes() {
    let (tmp, _api, _layout) = make_api();
    let projects_root = tmp.path().join("projects");

    let project = writer_core::project::create_project(&projects_root, "测试作品").unwrap();
    let project_root = projects_root.join(&project.id);

    writer_core::volume::create_volume(&project_root, "卷二").unwrap();
    writer_core::volume::create_volume(&project_root, "卷三").unwrap();

    let volumes = list_volumes(&project_root).unwrap();
    assert_eq!(volumes.len(), 3);

    let mut before: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for v in &volumes {
        let meta_path = project_root.join("volumes").join(&v.id).join("volume.json");
        let meta = read_volume_json(&meta_path);
        before.insert(v.id.clone(), meta.updated_at.clone());
    }

    // 交换第 0 个和第 2 个卷的顺序。
    let mut ordered_ids: Vec<String> = volumes.iter().map(|v| v.id.clone()).collect();
    ordered_ids.swap(0, 2);

    std::thread::sleep(std::time::Duration::from_millis(1100));

    let change_set = reorder_volumes_with_changes(&project_root, &ordered_ids, tmp.path())
        .expect("reorder_volumes_with_changes 应成功");

    let mut after: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for v in &volumes {
        let meta_path = project_root.join("volumes").join(&v.id).join("volume.json");
        let meta = read_volume_json(&meta_path);
        after.insert(v.id.clone(), meta.updated_at.clone());
    }

    let disk_changed = volumes
        .iter()
        .filter(|v| before[&v.id] != after[&v.id])
        .count();

    // ── 断言修复后正确行为 ──
    // 修复后：只有 order 变化的 2 个 volume.json 被重写。
    assert_eq!(
        disk_changed, 2,
        "问题1修复：交换两个卷时，磁盘上只有 2 个 volume.json 的 updated_at 变化（实际 {} 个变了）",
        disk_changed,
    );

    // 修复后：change_set 应包含 2 个路径。
    assert_eq!(
        change_set.to_flat_paths().len(),
        2,
        "问题1修复：交换两个卷时，change_set 应包含 2 个路径",
    );
}

// ══ 问题2：delete_project 把 StarMap 解绑放在删除事务外 ══

/// 复现：API 层 `delete_project` 先做 best-effort unbind loop，再调
/// `delete_project_with_changes`。若删除失败，starmap 已解绑（半状态）。
///
/// 修复后正确行为：解绑和删除应是原子的——若删除失败，starmap 的 project_id
/// 应仍然指向原 project（不应被解绑）。
///
/// 当前 bug：解绑在删除事务外，删除失败时 starmap 已被解绑。
///
/// 本测试通过手动删除 project.json 让 delete_project_with_changes 失败，
/// 然后验证 starmap 的 project_id 是否仍指向原 project。
#[test]
fn problem2_delete_project_failure_should_not_unbind_starmap() {
    let (tmp, api, _layout) = make_api();
    let projects_root = tmp.path().join("projects");

    // 创建 project + starmap + bind。
    let project = api
        .create_project("待删作品")
        .expect("create_project 应成功");
    let starmap = api
        .create_starmap("星图", "描述", None)
        .expect("create_starmap 应成功");
    api.bind_starmap_to_project(&starmap.starmap_id, &project.id)
        .expect("bind_starmap_to_project 应成功");

    // 验证绑定已建立。
    let bound = api
        .get_starmap(&starmap.starmap_id)
        .expect("get_starmap 应成功");
    assert_eq!(
        bound.project_id.as_deref(),
        Some(project.id.as_str()),
        "前置：starmap 应已绑定到 project",
    );

    // 手动删除 project.json，让 delete_project_with_changes 中的
    // validate_delete_target 失败（marker file not found）。
    let project_json = projects_root.join(&project.id).join("project.json");
    assert!(project_json.exists(), "前置：project.json 应存在",);
    std::fs::remove_file(&project_json).expect("删除 project.json 应成功");

    // 调用 delete_project，期望它返回 Err（因为 project.json 不存在）。
    let delete_result = api.delete_project(&project.id);

    // ── 断言修复后正确行为 ──
    // 修复后：delete_project 应返回 Err（因为删除失败）。
    assert!(
        delete_result.is_err(),
        "问题2修复：project.json 不存在时 delete_project 应返回 Err（实际 Ok）",
    );

    // 修复后：starmap 的 project_id 应仍指向原 project（解绑和删除应原子）。
    // 当前 bug：unbind 已在删除事务外执行，starmap.project_id 已变成 None。
    let after = api
        .get_starmap(&starmap.starmap_id)
        .expect("get_starmap 应仍能成功读取 starmap meta");
    assert_eq!(
        after.project_id.as_deref(),
        Some(project.id.as_str()),
        "问题2修复：删除失败时 starmap 的 project_id 应仍指向原 project（半状态 bug：实际已解绑为 None）",
    );
}

// ══ 问题3：workspace_paths 漏了 sync/trash/ 和 app-meta/delete-journals/ ══

/// 复现：`classify_workspace_path_str` 和 `is_workspace_internal_path_str`
/// 不包含 `sync/trash` 和 `app-meta/delete-journals` 的处理。
///
/// 修复后正确行为：
/// - `classify_workspace_path_str("sync/trash")` 返回 `InternalRuntime`
/// - `classify_workspace_path_str("sync/trash/xxx")` 返回 `InternalRuntime`
/// - `is_workspace_internal_path_str("sync/trash/xxx")` 返回 `true`
/// - `is_workspace_history_path_str("sync/trash/xxx")` 返回 `false`
///
/// 当前 bug：这些路径被归成 `UserContent`，`is_workspace_internal_path_str`
/// 返回 `false`，`is_workspace_history_path_str` 返回 `true`。
#[test]
fn problem3_sync_trash_should_be_internal_runtime() {
    // ── 断言修复后正确行为 ──
    assert_eq!(
        classify_workspace_path_str("sync/trash"),
        WorkspacePathClass::InternalRuntime,
        "问题3修复：classify(sync/trash) 应为 InternalRuntime",
    );
    assert_eq!(
        classify_workspace_path_str("sync/trash/xxx"),
        WorkspacePathClass::InternalRuntime,
        "问题3修复：classify(sync/trash/xxx) 应为 InternalRuntime",
    );

    assert!(
        is_workspace_internal_path_str("sync/trash/xxx"),
        "问题3修复：is_internal(sync/trash/xxx) 应为 true",
    );

    assert!(
        !is_workspace_history_path_str("sync/trash/xxx"),
        "问题3修复：is_history(sync/trash/xxx) 应为 false",
    );
}

#[test]
fn problem3_app_meta_delete_journals_should_be_internal_runtime() {
    // ── 断言修复后正确行为 ──
    assert_eq!(
        classify_workspace_path_str("app-meta/delete-journals"),
        WorkspacePathClass::InternalRuntime,
        "问题3修复：classify(app-meta/delete-journals) 应为 InternalRuntime",
    );
    assert_eq!(
        classify_workspace_path_str("app-meta/delete-journals/xxx"),
        WorkspacePathClass::InternalRuntime,
        "问题3修复：classify(app-meta/delete-journals/xxx) 应为 InternalRuntime",
    );

    assert!(
        is_workspace_internal_path_str("app-meta/delete-journals/xxx"),
        "问题3修复：is_internal(app-meta/delete-journals/xxx) 应为 true",
    );

    assert!(
        !is_workspace_history_path_str("app-meta/delete-journals/xxx"),
        "问题3修复：is_history(app-meta/delete-journals/xxx) 应为 false",
    );
}
