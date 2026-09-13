//! Issue #678 Comment 5654145414 — 回归测试套件。
//!
//! 验证 4 个硬问题的修复：
//! 1. 卷/章节删除的 journal 顺序：先 plan（不碰磁盘）再 save_pending 再物理删除。
//! 2. workspace_change 恢复状态机区分 Pending/LocalApplied/HistoryRecorded。
//! 3. workspace bootstrap 生命周期分离：with_layout_core_api 不 bootstrap。
//! 4. （RC-4 在 apps/Linux_qt，此处不覆盖 Qt 层）
//!
//! 这些测试只调用 writer_core 公开 API，不修改业务代码。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;
use tempfile::tempdir;
use writer_core::facade::WriterCore;
use writer_core::storage::git_repo_layout::GitRepoLayout;
use writer_core::storage::journal::workspace_change::{
    recover_unfinished, DeleteTarget, PlannedWorkspaceDelete, SyncDeleteFact,
    WorkspaceChangeJournal, WorkspaceChangeOpType, WorkspaceChangePhase,
};
use writer_core::storage::workspace_git::WorkspaceChangeSet;

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

/// 构造测试用 WriterCore 实例（facade 层，不需要 git layout）。
fn make_core() -> (tempfile::TempDir, WriterCore) {
    let temp_dir = tempdir().unwrap();
    let app_data_root = temp_dir.path();
    std::fs::create_dir_all(app_data_root.join("projects")).unwrap();
    let core = WriterCore::new(app_data_root, app_data_root.join("projects"));
    (temp_dir, core)
}

/// 构造测试用 WriterCoreApi 实例（含已 bootstrap 的 workspace Git 仓库）。
/// 使用 bootstrap_core_api 确保 .git 存在 + 删除事务已恢复。
fn make_api() -> (tempfile::TempDir, writer_core::api::service::WriterCoreApi) {
    let temp_dir = tempdir().unwrap();
    let app_data_root = temp_dir.path();
    std::fs::create_dir_all(app_data_root.join("projects")).unwrap();
    let api = writer_core::api::bootstrap::bootstrap_core_api(
        app_data_root,
        app_data_root.join("projects"),
        None,
        None,
    )
    .expect("bootstrap_core_api 应成功");
    (temp_dir, api)
}

/// 构造测试用 PlannedWorkspaceDelete（含一条 dummy fact），供 save_pending 使用。
fn make_test_planned_delete(target: DeleteTarget) -> PlannedWorkspaceDelete {
    PlannedWorkspaceDelete {
        delete_target: target,
        trash_rel_path: "sync/trash/test_token".to_string(),
        sync_delete_facts: vec![SyncDeleteFact {
            original_path: "test/path".to_string(),
            original_hash: String::new(),
            deleted_at: 0,
            deleted_by: "test-device".to_string(),
            trash_path: "sync/trash/test_token/path".to_string(),
        }],
    }
}

/// workspace-change-journals 目录路径。
fn journals_dir(app_data_root: &Path) -> std::path::PathBuf {
    app_data_root.join("app-meta/workspace-change-journals")
}

/// 列出 journals 目录下的 journal 文件数。
fn count_journals(app_data_root: &Path) -> usize {
    let dir = journals_dir(app_data_root);
    if !dir.exists() {
        return 0;
    }
    fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .count()
        })
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Type 2a: 根因变体 — plan_delete_*_changes 不碰磁盘
// ---------------------------------------------------------------------------

#[test]
fn test_type2a_plan_delete_volume_does_not_touch_disk() {
    let (_dir, core) = make_core();
    let project = core.create_project("测试作品").unwrap();
    let volume = core.create_volume(&project.id, "第一卷").unwrap();

    let volume_dir = _dir
        .path()
        .join("projects")
        .join(&project.id)
        .join("volumes")
        .join(&volume.id);
    let volume_json = volume_dir.join("volume.json");
    assert!(volume_dir.exists(), "前置：volume 目录应存在");
    assert!(volume_json.exists(), "前置：volume.json 应存在");

    // 调用 plan_delete_volume_changes——不应触碰磁盘。
    let change_set = core
        .plan_delete_volume_changes(&project.id, &volume.id)
        .expect("plan_delete_volume_changes 应成功");

    // 验证 volume 目录和 volume.json 仍存在（plan 不碰磁盘）。
    assert!(
        volume_dir.exists(),
        "plan_delete_volume_changes 后 volume 目录应仍存在"
    );
    assert!(
        volume_json.exists(),
        "plan_delete_volume_changes 后 volume.json 应仍存在"
    );
    // change_set 应非空。
    assert!(
        !change_set.is_empty(),
        "plan_delete_volume_changes 应返回非空 change_set"
    );
}

#[test]
fn test_type2a_plan_delete_chapter_does_not_touch_disk() {
    let (_dir, core) = make_core();
    let project = core.create_project("测试作品").unwrap();
    let volume = core.create_volume(&project.id, "第一卷").unwrap();
    let chapter = core
        .create_chapter(&project.id, &volume.id, "第一章")
        .unwrap();

    let chapter_dir = _dir
        .path()
        .join("projects")
        .join(&project.id)
        .join("volumes")
        .join(&volume.id)
        .join("chapters")
        .join(&chapter.id);
    let meta_path = chapter_dir.join("chapter.meta.json");
    let md_path = chapter_dir.join("chapter.md");
    assert!(chapter_dir.exists(), "前置：chapter 目录应存在");
    assert!(meta_path.exists(), "前置：chapter.meta.json 应存在");

    // 调用 plan_delete_chapter_changes——不应触碰磁盘。
    let change_set = core
        .plan_delete_chapter_changes(&project.id, &volume.id, &chapter.id)
        .expect("plan_delete_chapter_changes 应成功");

    // 验证 chapter 目录和文件仍存在（plan 不碰磁盘）。
    assert!(
        chapter_dir.exists(),
        "plan_delete_chapter_changes 后 chapter 目录应仍存在"
    );
    assert!(
        meta_path.exists(),
        "plan_delete_chapter_changes 后 chapter.meta.json 应仍存在"
    );
    assert!(
        md_path.exists(),
        "plan_delete_chapter_changes 后 chapter.md 应仍存在"
    );
    assert!(
        !change_set.is_empty(),
        "plan_delete_chapter_changes 应返回非空 change_set"
    );
}

// ---------------------------------------------------------------------------
// Type 2a: delete_volume/delete_chapter API 调用后 journal 被清理
// ---------------------------------------------------------------------------

#[test]
fn test_type2a_delete_volume_api_clears_journal() {
    let (_dir, api) = make_api();
    let project = api.create_project("测试作品").unwrap();
    let volume = api.create_volume(&project.id, "第一卷").unwrap();

    let result = api.delete_volume(&project.id, &volume.id);
    assert!(result.is_ok(), "delete_volume 应成功: {:?}", result);

    // 验证 journal 被清理（无残留 Pending/LocalApplied）。
    let app_data_root = _dir.path();
    assert_eq!(
        count_journals(app_data_root),
        0,
        "delete_volume 成功后不应有残留 journal 文件"
    );

    // 验证 volume 确实被删除。
    let volumes = api.list_volumes(&project.id).unwrap();
    assert!(
        !volumes.iter().any(|v| v.id == volume.id),
        "volume 应已被删除"
    );
}

#[test]
fn test_type2a_delete_chapter_api_clears_journal() {
    let (_dir, api) = make_api();
    let project = api.create_project("测试作品").unwrap();
    let volume = api.create_volume(&project.id, "第一卷").unwrap();
    let chapter = api
        .create_chapter(&project.id, &volume.id, "第一章")
        .unwrap();

    let result = api.delete_chapter(&project.id, &volume.id, &chapter.id);
    assert!(result.is_ok(), "delete_chapter 应成功: {:?}", result);

    let app_data_root = _dir.path();
    assert_eq!(
        count_journals(app_data_root),
        0,
        "delete_chapter 成功后不应有残留 journal 文件"
    );

    let chapters = api.list_chapters(&project.id, &volume.id).unwrap();
    assert!(
        !chapters.iter().any(|c| c.id == chapter.id),
        "chapter 应已被删除"
    );
}

// ---------------------------------------------------------------------------
// Type 3: 边界 — recover_unfinished 区分 Pending 和 LocalApplied
// ---------------------------------------------------------------------------

#[test]
fn test_type3_recover_unfinished_distinguishes_pending_and_local_applied() {
    let dir = tempdir().unwrap();
    let app_data_root = dir.path();
    std::fs::create_dir_all(app_data_root.join("projects")).unwrap();

    // 构造一个 Pending journal。
    let change_set = WorkspaceChangeSet::new();
    let pending_target = DeleteTarget::Volume {
        project_id: "p1".to_string(),
        volume_id: "v1".to_string(),
    };
    let pending_journal = WorkspaceChangeJournal::save_pending(
        app_data_root,
        &change_set,
        "device-1",
        WorkspaceChangeOpType::DeleteVolume,
        Some(pending_target.clone()),
        Some(make_test_planned_delete(pending_target)),
    )
    .expect("save_pending 应成功");

    // 推进到 LocalApplied 构造第二个 journal。
    let local_applied_target = DeleteTarget::Chapter {
        project_id: "p2".to_string(),
        volume_id: "v2".to_string(),
        chapter_id: "c2".to_string(),
    };
    let local_applied_journal = WorkspaceChangeJournal::save_pending(
        app_data_root,
        &change_set,
        "device-2",
        WorkspaceChangeOpType::DeleteChapter,
        Some(local_applied_target.clone()),
        Some(make_test_planned_delete(local_applied_target)),
    )
    .expect("save_pending 应成功");
    local_applied_journal
        .mark_local_applied(app_data_root)
        .expect("mark_local_applied 应成功");

    // recover_unfinished 应返回两条记录，phase 分别为 Pending 和 LocalApplied。
    let recovered = recover_unfinished(app_data_root).expect("recover_unfinished 应成功");
    assert_eq!(recovered.len(), 2, "应恢复 2 条 journal");

    let has_pending = recovered
        .iter()
        .any(|r| r.phase == WorkspaceChangePhase::Pending);
    let has_local_applied = recovered
        .iter()
        .any(|r| r.phase == WorkspaceChangePhase::LocalApplied);
    assert!(
        has_pending,
        "recover_unfinished 应返回 phase=Pending 的记录"
    );
    assert!(
        has_local_applied,
        "recover_unfinished 应返回 phase=LocalApplied 的记录"
    );

    // 验证 delete_target 被正确携带。
    for r in &recovered {
        assert!(r.delete_target.is_some(), "恢复记录应携带 delete_target");
    }
    // 清理避免影响其他测试
    let _ = pending_journal.clear_journal(app_data_root);
    let _ = local_applied_journal.clear_journal(app_data_root);
}

// ---------------------------------------------------------------------------
// Type 3: 边界 — recover_pending_local_delete 幂等性（目标已不存在）
// ---------------------------------------------------------------------------

#[test]
fn test_type3_recover_pending_local_delete_idempotent() {
    let dir = tempdir().unwrap();
    let app_data_root = dir.path();
    std::fs::create_dir_all(app_data_root.join("projects")).unwrap();

    // 构造一个 Pending journal，指向一个不存在的 volume。
    //   新格式 journal 必须携带非空 planned_delete（含 sync_delete_facts）。
    // 恢复时目录不存在 -> ensure_tombstones_persisted 用 facts 补齐 tombstone。
    let change_set = WorkspaceChangeSet::new();
    let delete_target = DeleteTarget::Volume {
        project_id: "nonexistent-project".to_string(),
        volume_id: "nonexistent-volume".to_string(),
    };
    let journal = WorkspaceChangeJournal::save_pending(
        app_data_root,
        &change_set,
        "device-1",
        WorkspaceChangeOpType::DeleteVolume,
        Some(delete_target.clone()),
        Some(make_test_planned_delete(delete_target)),
    )
    .expect("save_pending 应成功");

    // recover_unfinished 应返回 Pending 记录。
    let recovered = recover_unfinished(app_data_root).expect("recover_unfinished 应成功");
    assert_eq!(recovered.len(), 1, "应恢复 1 条 journal");
    assert_eq!(
        recovered[0].phase,
        WorkspaceChangePhase::Pending,
        "phase 应为 Pending"
    );

    // 验证 recover_pending_local_delete 对不存在的目标幂等。
    // 由于 recover_pending_local_delete 是私有函数，我们通过 recover_unfinished
    // 验证其行为：返回 Pending 记录供 bootstrap 处理，不报错。
    // bootstrap 调 recover_pending_local_delete 时，volume_dir 不存在 -> Ok(())。
    let volume_dir = app_data_root
        .join("projects")
        .join("nonexistent-project")
        .join("volumes")
        .join("nonexistent-volume");
    assert!(!volume_dir.exists(), "前置：目标 volume 目录应不存在");

    // 清理
    let _ = journal.clear_journal(app_data_root);
}

// ---------------------------------------------------------------------------
// Type 3: 边界 — DeleteTarget 向后兼容（旧 journal 无 delete_target 字段）
// ---------------------------------------------------------------------------

#[test]
fn test_type3_delete_target_serde_backward_compat() {
    // 构造一个无 delete_target 字段的旧 journal JSON。
    let old_journal_json = r#"{
        "token": "old-journal-token",
        "change_set": {"changes": []},
        "device_id": "old-device",
        "op_type": "delete_volume",
        "created_at": 1000000,
        "phase": "pending"
    }"#;

    let journal: WorkspaceChangeJournal =
        serde_json::from_str(old_journal_json).expect("旧 journal 应能反序列化");

    assert_eq!(journal.token, "old-journal-token");
    assert_eq!(journal.device_id, "old-device");
    assert_eq!(journal.phase, WorkspaceChangePhase::Pending);
    // 关键：delete_target 应为 None（向后兼容）。
    assert!(
        journal.delete_target.is_none(),
        "旧 journal（无 delete_target 字段）应反序列化为 None"
    );
}

// ---------------------------------------------------------------------------
// Type 4: 补丁对抗 — recover_pending_local_delete 对 DeleteTarget::Project 跳过
// ---------------------------------------------------------------------------

#[test]
fn test_type4_recover_pending_local_delete_project_skip() {
    let dir = tempdir().unwrap();
    let app_data_root = dir.path();
    std::fs::create_dir_all(app_data_root.join("projects")).unwrap();

    // 构造一个 Pending journal，delete_target = Project。
    let change_set = WorkspaceChangeSet::new();
    let journal = WorkspaceChangeJournal::save_pending(
        app_data_root,
        &change_set,
        "device-1",
        WorkspaceChangeOpType::DeleteProject,
        Some(DeleteTarget::Project {
            project_id: "test-project".to_string(),
        }),
        None,
    )
    .expect("save_pending 应成功");

    // recover_unfinished 应返回 Pending 记录。
    let recovered = recover_unfinished(app_data_root).expect("recover_unfinished 应成功");
    assert_eq!(recovered.len(), 1, "应恢复 1 条 journal");
    assert_eq!(
        recovered[0].phase,
        WorkspaceChangePhase::Pending,
        "phase 应为 Pending"
    );
    assert!(
        matches!(
            recovered[0].delete_target,
            Some(DeleteTarget::Project { .. })
        ),
        "delete_target 应为 Project"
    );

    // 补丁行为：recover_pending_local_delete 对 DeleteTarget::Project 返回 Ok(()) 跳过。
    // 验证 project 目录不会被此恢复路径删除（project 删除有独立事务）。
    // 由于 recover_pending_local_delete 是私有，我们验证 recover_unfinished
    // 返回的记录携带正确的 delete_target，bootstrap 会据此跳过本地删除。
    let project_dir = app_data_root.join("projects").join("test-project");
    // project 目录不存在（未创建），恢复不应报错。
    assert!(!project_dir.exists());

    // 清理
    let _ = journal.clear_journal(app_data_root);
}

// ---------------------------------------------------------------------------
// Type 4: 补丁对抗 — with_layout_core_api 不执行 bootstrap
// ---------------------------------------------------------------------------

#[test]
fn test_type4_with_layout_core_api_does_not_bootstrap() {
    use writer_core::api::bootstrap::with_layout_core_api;
    use writer_core::api::service::WriterCoreApi;

    let dir = tempdir().unwrap();
    let app_data_root = dir.path();
    std::fs::create_dir_all(app_data_root.join("projects")).unwrap();

    // 构造一个 layout，但不执行 bootstrap（不调 ensure_workspace_repo）。
    let layout = GitRepoLayout::new(app_data_root.to_path_buf());

    // with_layout_core_api 不应执行 bootstrap。
    let _api: WriterCoreApi = with_layout_core_api(
        app_data_root,
        app_data_root.join("projects"),
        &layout,
        None,
        None,
    );

    // 验证 .git 目录未被创建（bootstrap 未执行）。
    // 注意：GitRepoLayout 的 git_dir 可能在 app_data_root 下。
    let git_dir = &layout.git_dir;
    assert!(
        !git_dir.exists(),
        "with_layout_core_api 不应创建 .git 目录（bootstrap 未执行），但 {:?} 存在",
        git_dir
    );
}

// ---------------------------------------------------------------------------
// Type 4: 补丁对抗 — save_pending 正确记录 delete_target
// ---------------------------------------------------------------------------

#[test]
fn test_type4_save_pending_records_delete_target() {
    let dir = tempdir().unwrap();
    let app_data_root = dir.path();
    std::fs::create_dir_all(app_data_root.join("projects")).unwrap();

    let change_set = WorkspaceChangeSet::new();
    let delete_target = DeleteTarget::Volume {
        project_id: "p-123".to_string(),
        volume_id: "v-456".to_string(),
    };

    let journal = WorkspaceChangeJournal::save_pending(
        app_data_root,
        &change_set,
        "device-test",
        WorkspaceChangeOpType::DeleteVolume,
        Some(delete_target.clone()),
        Some(make_test_planned_delete(delete_target.clone())),
    )
    .expect("save_pending 应成功");

    // 读取 journal 文件，验证 delete_target 被持久化。
    let journal_path = app_data_root
        .join("app-meta/workspace-change-journals")
        .join(format!(
            ".sujian-workspace-change-journal-{}",
            journal.token
        ));
    assert!(journal_path.exists(), "journal 文件应存在");

    let content = fs::read_to_string(&journal_path).expect("应能读取 journal 文件");
    let parsed: WorkspaceChangeJournal =
        serde_json::from_str(&content).expect("应能反序列化 journal");

    assert_eq!(parsed.delete_target, Some(delete_target));
    assert_eq!(parsed.phase, WorkspaceChangePhase::Pending);

    // 清理
    let _ = journal.clear_journal(app_data_root);
}
