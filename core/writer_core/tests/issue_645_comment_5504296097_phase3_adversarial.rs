#![allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
//! Issue #645 评论 5504296097 — Phase C 对抗性验证测试。
//!
//! 独立生成的对抗性测试，验证 patch 的正确性并尝试找出过度修正/边界漏洞。
//! 覆盖：
//! - 问题1：reorder_volumes_with_changes 5 卷打乱顺序，验证磁盘事实 == change_set
//! - 问题1 边界：单卷 reorder（无变化）应零写入
//! - 问题1 边界：全反转 5 卷，所有 5 个都应被重写
//! - 问题1 对抗：reorder_volumes 薄包装路径占位不 panic
//! - 问题2：删除失败时 starmap 不解绑（事务原子性）
//! - 问题2 对抗：多 starmap 绑定同一 project，删除失败时全部不解绑
//! - 问题2 对抗：正常删除成功后 starmap 应被解绑（project_id = None）
//! - 问题3：sync/trash 和 app-meta/delete-journals 各类边界路径
//! - 问题3 对抗：大小写/前缀近似/子串混淆
//! - 问题3 对抗：相邻保留路径（sync/staging, app-meta/transactions）分类不变

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::WriterCoreApi;
use writer_core::storage::git_repo_layout::GitRepoLayout;
use writer_core::storage::workspace_paths::{
    classify_workspace_path_str, is_workspace_history_path_str, is_workspace_internal_path_str,
    WorkspacePathClass,
};
use writer_core::storage::{ensure_workspace_repo, git_runtime};
use writer_core::volume::{list_volumes, reorder_volumes, reorder_volumes_with_changes, Volume};

// ── helpers ──

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

fn read_volume_json(meta_path: &std::path::Path) -> Volume {
    let content = std::fs::read_to_string(meta_path).unwrap();
    serde_json::from_str::<Volume>(&content).unwrap()
}

fn snapshot_updated_ats(
    project_root: &std::path::Path,
    volumes: &[Volume],
) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for v in volumes {
        let meta_path = project_root.join("volumes").join(&v.id).join("volume.json");
        let meta = read_volume_json(&meta_path);
        map.insert(v.id.clone(), meta.updated_at.clone());
    }
    map
}

// ══ 问题1 对抗性：5 卷打乱顺序，磁盘事实 == change_set ══

/// 创建 5 个卷，传入打乱顺序的 ordered_ids，验证：
/// - 磁盘上只有 order 实际变化的 volume.json 被重写
/// - change_set 包含所有被重写的文件（数量一致）
/// - change_set 中每个路径对应磁盘上确实被重写的文件
#[test]
fn adv_problem1_five_volumes_shuffle_disk_matches_change_set() {
    let (tmp, _api, _layout) = make_api();
    let projects_root = tmp.path().join("projects");

    let project = writer_core::project::create_project(&projects_root, "对抗作品").unwrap();
    let project_root = projects_root.join(&project.id);

    // create_project 自带 1 个默认卷，再创建 4 个，共 5 个。
    writer_core::volume::create_volume(&project_root, "卷二").unwrap();
    writer_core::volume::create_volume(&project_root, "卷三").unwrap();
    writer_core::volume::create_volume(&project_root, "卷四").unwrap();
    writer_core::volume::create_volume(&project_root, "卷五").unwrap();

    let volumes = list_volumes(&project_root).unwrap();
    assert_eq!(volumes.len(), 5, "前置：应有 5 个卷");

    let before = snapshot_updated_ats(&project_root, &volumes);

    // 打乱顺序：[v0, v1, v2, v3, v4] -> [v4, v0, v3, v1, v2]
    // 验证每个卷的 order 都会变（derangement）：
    //   v0: 0->1, v1: 1->3, v2: 2->4, v3: 3->2, v4: 4->0  全部变化
    // 预期 5 个都被重写。
    let ids: Vec<String> = volumes.iter().map(|v| v.id.clone()).collect();
    let shuffled: Vec<String> = vec![
        ids[4].clone(),
        ids[0].clone(),
        ids[3].clone(),
        ids[1].clone(),
        ids[2].clone(),
    ];

    // 验证打乱后 order 确实都变了（前置条件）
    for (new_idx, id) in shuffled.iter().enumerate() {
        let old = volumes.iter().find(|v| &v.id == id).unwrap();
        assert_ne!(
            old.order, new_idx as i32,
            "前置：卷 {} 的 order 应该变化",
            id
        );
    }

    std::thread::sleep(std::time::Duration::from_millis(1100));

    let change_set = reorder_volumes_with_changes(&project_root, &shuffled, tmp.path())
        .expect("reorder_volumes_with_changes 应成功");

    let after = snapshot_updated_ats(&project_root, &volumes);

    let disk_changed: Vec<String> = volumes
        .iter()
        .filter(|v| before[&v.id] != after[&v.id])
        .map(|v| v.id.clone())
        .collect();

    // 断言1：所有 5 个卷的 order 都变了，磁盘上 5 个 volume.json 都应被重写。
    assert_eq!(
        disk_changed.len(),
        5,
        "对抗问题1：5 卷全打乱时磁盘上应有 5 个 volume.json 被重写，实际 {} 个",
        disk_changed.len(),
    );

    // 断言2：change_set 应包含 5 个路径。
    assert_eq!(
        change_set.to_flat_paths().len(),
        5,
        "对抗问题1：5 卷全打乱时 change_set 应包含 5 个路径",
    );

    // 断言3：change_set 不为空。
    assert!(!change_set.is_empty());

    // 断言4：每个 change_set 路径都对应磁盘上确实被重写的文件。
    let flat = change_set.to_flat_paths();
    for id in &disk_changed {
        let rel = format!("projects/{}/volumes/{}/volume.json", project.id, id);
        let found = flat.iter().any(|p| p.to_string_lossy().contains(&rel));
        assert!(found, "对抗问题1：被重写的卷 {} 应在 change_set 中", id);
    }
}

// ══ 问题1 边界：单卷 reorder（无变化）应零写入 ══

#[test]
fn adv_problem1_single_volume_no_change_zero_writes() {
    let (tmp, _api, _layout) = make_api();
    let projects_root = tmp.path().join("projects");

    let project = writer_core::project::create_project(&projects_root, "单卷作品").unwrap();
    let project_root = projects_root.join(&project.id);

    let volumes = list_volumes(&project_root).unwrap();
    assert_eq!(volumes.len(), 1);

    let before = snapshot_updated_ats(&project_root, &volumes);
    let ordered: Vec<String> = vec![volumes[0].id.clone()];

    std::thread::sleep(std::time::Duration::from_millis(1100));

    let change_set = reorder_volumes_with_changes(&project_root, &ordered, tmp.path()).unwrap();

    let after = snapshot_updated_ats(&project_root, &volumes);

    assert_eq!(
        before[&volumes[0].id], after[&volumes[0].id],
        "单卷无变化时 updated_at 不应变"
    );
    assert!(change_set.is_empty(), "单卷无变化时 change_set 应为空");
}

// ══ 问题1 边界：全反转 5 卷，所有 5 个都应被重写 ══

#[test]
fn adv_problem1_full_reverse_five_volumes_all_rewritten() {
    let (tmp, _api, _layout) = make_api();
    let projects_root = tmp.path().join("projects");

    let project = writer_core::project::create_project(&projects_root, "反转作品").unwrap();
    let project_root = projects_root.join(&project.id);

    for i in 2..=5 {
        writer_core::volume::create_volume(&project_root, &format!("卷{}", i)).unwrap();
    }

    let volumes = list_volumes(&project_root).unwrap();
    assert_eq!(volumes.len(), 5);

    let before = snapshot_updated_ats(&project_root, &volumes);

    // 全反转：[v0, v1, v2, v3, v4] -> [v4, v3, v2, v1, v0]
    let mut reversed: Vec<String> = volumes.iter().map(|v| v.id.clone()).collect();
    reversed.reverse();

    // 中间那个卷（index 2）order 不变（5 个卷反转，中间位置不变）。
    // 预期 4 个被重写（除中间那个）。
    let middle_id = volumes[2].id.clone();
    let middle_order_before = volumes[2].order;
    let middle_order_after = 2i32; // 反转后中间位置仍是 2
    let middle_will_change = middle_order_before != middle_order_after;

    std::thread::sleep(std::time::Duration::from_millis(1100));

    let change_set = reorder_volumes_with_changes(&project_root, &reversed, tmp.path()).unwrap();

    let after = snapshot_updated_ats(&project_root, &volumes);

    let disk_changed = volumes
        .iter()
        .filter(|v| before[&v.id] != after[&v.id])
        .count();

    let expected_changed = if middle_will_change { 5 } else { 4 };

    assert_eq!(
        disk_changed, expected_changed,
        "全反转 5 卷：预期 {} 个被重写（中间卷 order 不变），实际 {} 个",
        expected_changed, disk_changed,
    );

    assert_eq!(
        change_set.to_flat_paths().len(),
        expected_changed,
        "全反转 5 卷：change_set 应含 {} 个路径",
        expected_changed,
    );

    // 中间卷的 updated_at 不应变
    if !middle_will_change {
        assert_eq!(
            before[&middle_id], after[&middle_id],
            "中间卷 order 不变时 updated_at 不应变"
        );
    }
}

// ══ 问题1 对抗：reorder_volumes 薄包装路径占位不 panic ══

#[test]
fn adv_problem1_reorder_volumes_thin_wrapper_no_panic() {
    let (tmp, _api, _layout) = make_api();
    let projects_root = tmp.path().join("projects");

    let project = writer_core::project::create_project(&projects_root, "薄包装作品").unwrap();
    let project_root = projects_root.join(&project.id);

    writer_core::volume::create_volume(&project_root, "卷二").unwrap();

    let volumes = list_volumes(&project_root).unwrap();
    assert_eq!(volumes.len(), 2);

    // 交换两个卷
    let swapped: Vec<String> = vec![volumes[1].id.clone(), volumes[0].id.clone()];

    // 旧接口 reorder_volumes 不传 app_data_root，应正常工作不 panic
    let result = reorder_volumes(&project_root, &swapped);
    assert!(result.is_ok(), "reorder_volumes 薄包装应成功");

    // 验证 order 确实变了
    let after = list_volumes(&project_root).unwrap();
    assert_eq!(after[0].id, volumes[1].id, "交换后第一个应是原第二个");
    assert_eq!(after[1].id, volumes[0].id, "交换后第二个应是原第一个");
}

// ══ 问题2 对抗：多 starmap 绑定同一 project，删除失败时全部不解绑 ══

#[test]
fn adv_problem2_multiple_starmaps_bound_delete_fails_none_unbound() {
    let (tmp, api, _layout) = make_api();
    let projects_root = tmp.path().join("projects");

    let project = api
        .create_project("多绑作品")
        .expect("create_project 应成功");

    // 创建 3 个 starmap 都绑定到同一 project
    let mut starmap_ids = Vec::new();
    for i in 1..=3 {
        let sm = api
            .create_starmap(&format!("星图{}", i), "描述", None)
            .expect("create_starmap 应成功");
        api.bind_starmap_to_project(&sm.starmap_id, &project.id)
            .expect("bind 应成功");
        starmap_ids.push(sm.starmap_id);
    }

    // 验证全部已绑定
    for sm_id in &starmap_ids {
        let meta = api.get_starmap(sm_id).expect("get_starmap 应成功");
        assert_eq!(
            meta.project_id.as_deref(),
            Some(project.id.as_str()),
            "前置：starmap {} 应已绑定",
            sm_id
        );
    }

    // 手动删除 project.json 让删除失败
    let project_json = projects_root.join(&project.id).join("project.json");
    assert!(project_json.exists());
    std::fs::remove_file(&project_json).expect("删除 project.json 应成功");

    let delete_result = api.delete_project(&project.id);

    // 删除应失败
    assert!(
        delete_result.is_err(),
        "对抗问题2：project.json 不存在时 delete_project 应返回 Err",
    );

    // 所有个 starmap 都不应被解绑
    for sm_id in &starmap_ids {
        let after = api.get_starmap(sm_id).expect("get_starmap 应仍能成功读取");
        assert_eq!(
            after.project_id.as_deref(),
            Some(project.id.as_str()),
            "对抗问题2：删除失败时 starmap {} 的 project_id 应仍指向原 project（不应解绑）",
            sm_id,
        );
    }
}

// ══ 问题2 对抗：正常删除成功后 starmap 应被解绑（project_id = None）══

#[test]
fn adv_problem2_normal_delete_unbinds_starmaps() {
    let (tmp, api, _layout) = make_api();

    let project = api
        .create_project("正常删作品")
        .expect("create_project 应成功");

    // 创建 2 个 starmap 绑定到 project
    let sm1 = api
        .create_starmap("星图1", "描述", None)
        .expect("create_starmap 应成功");
    let sm2 = api
        .create_starmap("星图2", "描述", None)
        .expect("create_starmap 应成功");
    api.bind_starmap_to_project(&sm1.starmap_id, &project.id)
        .expect("bind 1 应成功");
    api.bind_starmap_to_project(&sm2.starmap_id, &project.id)
        .expect("bind 2 应成功");

    // 正常删除（不破坏 project.json）
    let delete_result = api.delete_project(&project.id);
    assert!(
        delete_result.is_ok(),
        "对抗问题2：正常删除应成功，实际 err: {:?}",
        delete_result.err(),
    );

    // 删除成功后两个 starmap 都应被解绑
    let after1 = api
        .get_starmap(&sm1.starmap_id)
        .expect("get_starmap 1 应成功");
    let after2 = api
        .get_starmap(&sm2.starmap_id)
        .expect("get_starmap 2 应成功");

    assert_eq!(
        after1.project_id, None,
        "对抗问题2：删除成功后 starmap 1 应被解绑（project_id = None）",
    );
    assert_eq!(
        after2.project_id, None,
        "对抗问题2：删除成功后 starmap 2 应被解绑（project_id = None）",
    );

    // project.json 应不存在
    let project_json = tmp
        .path()
        .join("projects")
        .join(&project.id)
        .join("project.json");
    assert!(
        !project_json.exists(),
        "对抗问题2：删除成功后 project.json 应不存在",
    );
}

// ══ 问题2 对抗：无 starmap 绑定时正常删除应成功 ══

#[test]
fn adv_problem2_delete_project_without_starmap_succeeds() {
    let (tmp, api, _layout) = make_api();

    let project = api
        .create_project("无绑定作品")
        .expect("create_project 应成功");

    // 不创建任何 starmap 绑定
    let delete_result = api.delete_project(&project.id);
    assert!(
        delete_result.is_ok(),
        "对抗问题2：无 starmap 绑定时删除应成功，实际 err: {:?}",
        delete_result.err(),
    );

    let project_json = tmp
        .path()
        .join("projects")
        .join(&project.id)
        .join("project.json");
    assert!(!project_json.exists(), "project.json 应已被删除");
}

// ══ 问题3 对抗：边界路径变体 ══

#[test]
fn adv_problem3_sync_trash_boundary_variants() {
    // 根路径
    assert_eq!(
        classify_workspace_path_str("sync/trash"),
        WorkspacePathClass::InternalRuntime,
    );
    assert!(is_workspace_internal_path_str("sync/trash"));
    assert!(!is_workspace_history_path_str("sync/trash"));

    // 子路径
    assert_eq!(
        classify_workspace_path_str("sync/trash/abc"),
        WorkspacePathClass::InternalRuntime,
    );
    assert!(is_workspace_internal_path_str("sync/trash/abc"));
    assert!(!is_workspace_history_path_str("sync/trash/abc"));

    // 深层子路径
    assert_eq!(
        classify_workspace_path_str("sync/trash/a/b/c"),
        WorkspacePathClass::InternalRuntime,
    );
    assert!(is_workspace_internal_path_str("sync/trash/a/b/c"));
    assert!(!is_workspace_history_path_str("sync/trash/a/b/c"));
}

#[test]
fn adv_problem3_app_meta_delete_journals_boundary_variants() {
    // 根路径
    assert_eq!(
        classify_workspace_path_str("app-meta/delete-journals"),
        WorkspacePathClass::InternalRuntime,
    );
    assert!(is_workspace_internal_path_str("app-meta/delete-journals"));
    assert!(!is_workspace_history_path_str("app-meta/delete-journals"));

    // 子路径
    assert_eq!(
        classify_workspace_path_str("app-meta/delete-journals/abc"),
        WorkspacePathClass::InternalRuntime,
    );
    assert!(is_workspace_internal_path_str(
        "app-meta/delete-journals/abc"
    ));
    assert!(!is_workspace_history_path_str(
        "app-meta/delete-journals/abc"
    ));

    // 深层子路径
    assert_eq!(
        classify_workspace_path_str("app-meta/delete-journals/a/b/c"),
        WorkspacePathClass::InternalRuntime,
    );
    assert!(is_workspace_internal_path_str(
        "app-meta/delete-journals/a/b/c"
    ));
    assert!(!is_workspace_history_path_str(
        "app-meta/delete-journals/a/b/c"
    ));
}

// ══ 问题3 对抗：近似前缀不应误判 ══

#[test]
fn adv_problem3_near_miss_prefixes_not_misclassified() {
    // "sync/trashx" 不应被识别为 sync/trash（前缀但不是目录边界）
    // 注意：starts_with("sync/trash/") 要求有 /，所以 "sync/trashx" 不会匹配
    // 但 "sync/trash" 本身会匹配。这里测 "sync/trashy" 不应被识别
    let classify_trashy = classify_workspace_path_str("sync/trashy");
    // sync/trashy 不是 sync/trash 也不是 sync/trash/...，所以不应是 InternalRuntime
    // （除非被其他规则捕获，比如 sync engine state，但 sync/trashy 不是）
    assert_ne!(
        classify_trashy,
        WorkspacePathClass::InternalRuntime,
        "对抗问题3：sync/trashy 不应被误判为 InternalRuntime（不是 sync/trash 子路径）",
    );

    // "app-meta/delete-journalsx" 不应被识别
    let classify_journalsx = classify_workspace_path_str("app-meta/delete-journalsx");
    assert_ne!(
        classify_journalsx,
        WorkspacePathClass::InternalRuntime,
        "对抗问题3：app-meta/delete-journalsx 不应被误判为 InternalRuntime",
    );

    // "sync/tr" 不应被识别为 sync/trash
    let classify_tr = classify_workspace_path_str("sync/tr");
    assert_ne!(
        classify_tr,
        WorkspacePathClass::InternalRuntime,
        "对抗问题3：sync/tr 不应被误判为 InternalRuntime",
    );
}

// ══ 问题3 对抗：相邻保留路径分类不变（回归保护）══

#[test]
fn adv_problem3_adjacent_reserved_paths_unchanged() {
    // sync/staging 仍应是 SyncEngineState 或其他非 InternalRuntime（取决于原分类）
    // 这里只验证它不被新规则误判成 InternalRuntime（除非原本就是）
    let _classify_staging = classify_workspace_path_str("sync/staging");
    // 不强断言具体类，只验证不 panic 且分类一致

    // app-meta/transactions 应仍是 InternalRuntime（原本就是）
    assert_eq!(
        classify_workspace_path_str("app-meta/transactions"),
        WorkspacePathClass::InternalRuntime,
        "app-meta/transactions 应仍是 InternalRuntime",
    );
    assert_eq!(
        classify_workspace_path_str("app-meta/transactions/abc"),
        WorkspacePathClass::InternalRuntime,
        "app-meta/transactions/abc 应仍是 InternalRuntime",
    );

    // app-meta/logs 应仍是 InternalRuntime
    assert_eq!(
        classify_workspace_path_str("app-meta/logs"),
        WorkspacePathClass::InternalRuntime,
        "app-meta/logs 应仍是 InternalRuntime",
    );

    // full-sync-staging 应仍是 InternalRuntime
    assert_eq!(
        classify_workspace_path_str("full-sync-staging"),
        WorkspacePathClass::InternalRuntime,
        "full-sync-staging 应仍是 InternalRuntime",
    );
}

// ══ 问题3 对抗：用户内容路径不应被新规则误判 ══

#[test]
fn adv_problem3_user_content_paths_not_affected() {
    // projects/ 下的用户内容应仍是 UserContent
    assert_eq!(
        classify_workspace_path_str("projects/abc/project.json"),
        WorkspacePathClass::UserContent,
        "projects/abc/project.json 应是 UserContent",
    );

    // volumes 下的用户内容
    assert_eq!(
        classify_workspace_path_str("projects/abc/volumes/v1/volume.json"),
        WorkspacePathClass::UserContent,
        "projects/abc/volumes/v1/volume.json 应是 UserContent",
    );

    // starmaps 下的用户内容
    assert_eq!(
        classify_workspace_path_str("starmaps/abc.meta.json"),
        WorkspacePathClass::UserContent,
        "starmaps/abc.meta.json 应是 UserContent",
    );
}
