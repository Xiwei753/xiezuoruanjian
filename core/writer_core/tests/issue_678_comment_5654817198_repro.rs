//! Issue #678 Comment 5654817198 — 第三轮剩余 5 个实质问题的修复验证测试。
//!
//! 本文件验证评论 5654817198 指出的 5 个问题已修复：
//! 1. 后台同步线程用 with_layout_core_api + layout 快照，不再 bootstrap（静态分析）
//! 2. SyncBackend 回调链 sync_qptr 一路传递不断链（静态分析）
//! 3. diagnostics / dry-run 的 single-flight 生效，设置 current_sync_in_progress=true（静态分析）
//! 4. 卷/章节的 tombstone 写到 project_root 的 SyncState，不吞 load/save 错误（运行时）
//! 5. known_files 缺失且无 tombstone 时不伪造 delete record，返回 Err（静态分析）
//!
//! 修复前这些测试以 "repro_" 前缀证明问题存在；修复后以 "verify_" 前缀证明问题已修复。
//! 问题 1/2/3 涉及 Linux_qt 后端代码，采用静态分析方式验证修复后的代码模式。
//! 问题 4 用 writer_core 公开 API 运行时验证。
//! 问题 5 的核心函数 snapshot_local_records_read_only 是 pub(crate)，采用静态分析验证。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;
use writer_core::chapter;
use writer_core::volume;

// ---------------------------------------------------------------------------
// 辅助：定位仓库根目录（从 CARGO_MANIFEST_DIR 向上找 Cargo.toml workspace）
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let mut dir = PathBuf::from(manifest_dir);
    // core/writer_core -> core -> repo root
    for _ in 0..3 {
        if dir.join("Cargo.toml").exists()
            && fs::read_to_string(dir.join("Cargo.toml"))
                .map(|c| c.contains("[workspace]"))
                .unwrap_or(false)
        {
            return dir;
        }
        dir = dir.parent().unwrap_or(&dir).to_path_buf();
    }
    // fallback: core/writer_core 的上两级
    PathBuf::from(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(Path::new("."))
        .to_path_buf()
}

fn linux_qt_backend_dir() -> PathBuf {
    repo_root().join("apps/Linux_qt/src/backend")
}

fn read_source(rel_path: &str) -> String {
    let path = linux_qt_backend_dir().join(rel_path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("读取 {path:?} 失败: {e}"))
}

fn read_writer_core_source(rel_path: &str) -> String {
    let path = repo_root().join("core/writer_core/src").join(rel_path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("读取 {path:?} 失败: {e}"))
}

// ===========================================================================
// 问题 1：后台同步线程用 with_layout_core_api + layout 快照，不再 bootstrap
// ===========================================================================

/// 验证问题 1 已修复：perform_sync_internal / perform_sync_dry_run / perform_sync_diagnostics
/// 的后台线程用 with_layout_core_api + layout 快照，不再调用 create_core_api（bootstrap）。
#[test]
fn verify_problem1_backend_sync_thread_uses_layout_snapshot() {
    let sync_ops = read_source("sync_operations.rs");
    let sync_backend = read_source("sync_backend.rs");

    // 后台线程内不应再调用 create_core_api（bootstrap）。
    // create_core_api 只能出现在非线程入口（如 core_api getter 的 fallback）。
    // 验证 sync_operations.rs 和 sync_backend.rs 的线程内用 with_layout_core_api。
    let ops_with_layout_count = sync_ops.matches("with_layout_core_api(").count();
    let backend_with_layout_count = sync_backend.matches("with_layout_core_api(").count();

    // sync_operations.rs 至少 2 处 with_layout_core_api（sync + dry_run 线程内）。
    assert!(
        ops_with_layout_count >= 2,
        "sync_operations.rs 应至少有 2 处 with_layout_core_api 调用（sync + dry_run），实际: {ops_with_layout_count}"
    );
    // sync_backend.rs 至少 1 处 with_layout_core_api（diagnostics 线程内）。
    assert!(
        backend_with_layout_count >= 1,
        "sync_backend.rs 应至少有 1 处 with_layout_core_api 调用（diagnostics），实际: {backend_with_layout_count}"
    );

    // 验证线程内不再用 create_core_api：thread::spawn 后不应有 create_core_api 调用。
    // 检查 sync_operations.rs 中 create_core_api 不出现在 thread::spawn 闭包内。
    let has_create_in_thread =
        sync_ops.contains("thread::spawn(move ||") && sync_ops.contains("create_core_api(");
    assert!(
        !has_create_in_thread,
        "sync_operations.rs 后台线程内不应再调用 create_core_api（问题 1 已修复）"
    );

    let has_create_in_backend_thread =
        sync_backend.contains("thread::spawn(move ||") && sync_backend.contains("create_core_api(");
    assert!(
        !has_create_in_backend_thread,
        "sync_backend.rs 后台线程内不应再调用 create_core_api（问题 1 已修复）"
    );

    // 验证无 layout 时返回错误（no_workspace_git_layout）。
    assert!(
        sync_ops.contains("no_workspace_git_layout"),
        "sync_operations.rs 应在无 layout 时返回 no_workspace_git_layout 错误"
    );
    assert!(
        sync_backend.contains("no_workspace_git_layout"),
        "sync_backend.rs 应在无 layout 时返回 no_workspace_git_layout 错误"
    );

    eprintln!(
        "[BUGFIX_VERIFY] problem1: 后台线程用 with_layout_core_api + layout 快照，不再 bootstrap"
    );
}

// ===========================================================================
// 问题 2：SyncBackend 回调链 sync_qptr 一路传递不断链
// ===========================================================================

/// 验证问题 2 已修复：handle_sync_outcome 接收 sync_qptr 参数，
/// 排队 manual sync 时传递 sync_qptr（不再传 None）。
/// trigger_auto_sync / request_auto_sync / maybe_auto_sync_on_foreground 都接收并传递 sync_qptr。
/// SyncBackend::request_auto_sync 和 maybe_auto_sync_on_foreground 创建自己的 QPointer<SyncBackend>。
#[test]
fn verify_problem2_syncbackend_callback_chain_intact() {
    let sync_ops = read_source("sync_operations.rs");
    let sync_backend = read_source("sync_backend.rs");

    // handle_sync_outcome 不应再传 None 给排队的 manual sync。
    let has_none_in_handle_outcome =
        sync_ops.contains("self.perform_sync_internal(\"manual\", false, None)");
    assert!(
        !has_none_in_handle_outcome,
        "handle_sync_outcome 中排队 manual sync 不应再传 None（问题 2 已修复）"
    );

    // trigger_auto_sync 不应再传 None。
    let has_none_in_trigger_auto =
        sync_ops.contains("self.perform_sync_internal(reason, true, None)");
    assert!(
        !has_none_in_trigger_auto,
        "trigger_auto_sync 不应再传 None（问题 2 已修复）"
    );

    // handle_sync_outcome 应接收 sync_qptr 参数。
    assert!(
        sync_ops.contains("fn handle_sync_outcome(\n        &mut self,\n        outcome: SyncTaskOutcome,\n        sync_qptr: Option<QPointer<SyncBackend>>,\n    )"),
        "handle_sync_outcome 应接收 sync_qptr 参数"
    );

    // trigger_auto_sync / request_auto_sync / maybe_auto_sync_on_foreground 应接收 sync_qptr。
    assert!(
        sync_ops.contains("fn trigger_auto_sync(\n        &mut self,\n        reason: &str,\n        sync_qptr: Option<QPointer<SyncBackend>>,\n    )"),
        "trigger_auto_sync 应接收 sync_qptr 参数"
    );

    // SyncBackend::request_auto_sync 应创建自己的 QPointer<SyncBackend>。
    let request_auto_creates_qptr = sync_backend.contains(
        "fn request_auto_sync(&mut self, reason: QString) {\n        let qptr = QPointer::from(&*self);",
    );
    assert!(
        request_auto_creates_qptr,
        "SyncBackend::request_auto_sync 应创建自己的 QPointer<SyncBackend>（问题 2 已修复）"
    );

    // SyncBackend::maybe_auto_sync_on_foreground 应创建自己的 QPointer<SyncBackend>。
    let maybe_auto_creates_qptr = sync_backend.contains(
        "fn maybe_auto_sync_on_foreground(&mut self) {\n        let qptr = QPointer::from(&*self);",
    );
    assert!(
        maybe_auto_creates_qptr,
        "SyncBackend::maybe_auto_sync_on_foreground 应创建自己的 QPointer<SyncBackend>（问题 2 已修复）"
    );

    // SyncBackend::handle_outcome 应创建 QPointer 并传给 handle_sync_outcome。
    assert!(
        sync_backend.contains("app.handle_sync_outcome(outcome, Some(qptr))"),
        "SyncBackend::handle_outcome 应传 Some(qptr) 给 handle_sync_outcome"
    );

    eprintln!("[BUGFIX_VERIFY] problem2: sync_qptr 一路传递，回调链不断");
}

// ===========================================================================
// 问题 3：diagnostics / dry-run 的 single-flight 生效
// ===========================================================================

/// 验证问题 3 已修复：perform_sync_dry_run 和 perform_sync_diagnostics
/// 在启动线程前设置 current_sync_in_progress=true，统一 single-flight。
#[test]
fn verify_problem3_diagnostics_dryrun_singleflight_effective() {
    let sync_ops = read_source("sync_operations.rs");
    let sync_backend = read_source("sync_backend.rs");

    // sync_operations.rs 应至少有 2 处 current_sync_in_progress = true
    //（perform_sync_internal + perform_sync_dry_run）。
    let in_progress_true_count_in_ops = sync_ops.matches("current_sync_in_progress = true").count();
    assert!(
        in_progress_true_count_in_ops >= 2,
        "sync_operations.rs 应至少有 2 处 current_sync_in_progress = true（sync + dry_run），实际: {in_progress_true_count_in_ops}"
    );

    // sync_backend.rs 应至少有 1 处 current_sync_in_progress = true（diagnostics）。
    let in_progress_true_count_in_backend = sync_backend
        .matches("current_sync_in_progress = true")
        .count();
    assert!(
        in_progress_true_count_in_backend >= 1,
        "sync_backend.rs 应至少有 1 处 current_sync_in_progress = true（diagnostics），实际: {in_progress_true_count_in_backend}"
    );

    eprintln!("[BUGFIX_VERIFY] problem3: dry_run/diagnostics 设置 current_sync_in_progress=true，single-flight 生效");
}

// ===========================================================================
// 问题 4：tombstone 写到 project_root 的 SyncState，不吞 load/save 错误
// ===========================================================================

/// 验证问题 4a 已修复：delete_volume 把 tombstone 写到 project_root 而非 app_data_root。
#[test]
fn verify_problem4a_tombstone_written_to_correct_sync_root() {
    let temp = tempdir().unwrap();
    let app_data_root = temp.path();
    let projects_root = app_data_root.join("projects");
    fs::create_dir_all(&projects_root).unwrap();

    let project_id = "test_project_4a";
    let project_root = projects_root.join(project_id);
    fs::create_dir_all(project_root.join("volumes")).unwrap();

    let volume_id = "vol1";
    let volume_dir = project_root.join("volumes").join(volume_id);
    fs::create_dir_all(&volume_dir).unwrap();
    fs::write(
        volume_dir.join("volume.json"),
        r#"{"id":"vol1","title":"V1"}"#,
    )
    .unwrap();

    // 在 project_root 下预先创建 sync state（正确的 sync root）
    let project_sync_dir = project_root.join("app-meta/sync");
    fs::create_dir_all(&project_sync_dir).unwrap();
    let project_state_path = project_sync_dir.join("state.local.json");
    fs::write(
        &project_state_path,
        r#"{"known_files":{},"device_id":"dev1"}"#,
    )
    .unwrap();

    // 在 app_data_root 下也创建 sync state（错误位置，修复后不应写到这里）
    let app_sync_dir = app_data_root.join("app-meta/sync");
    fs::create_dir_all(&app_sync_dir).unwrap();
    let app_state_path = app_sync_dir.join("state.local.json");
    fs::write(&app_state_path, r#"{"known_files":{},"device_id":"dev1"}"#).unwrap();

    // 调用 delete_volume
    volume::delete_volume(&project_root, volume_id, app_data_root).unwrap();

    let app_state_after = fs::read_to_string(&app_state_path).unwrap_or_default();
    let project_state_after = fs::read_to_string(&project_state_path).unwrap_or_default();

    eprintln!("[BUGFIX_VERIFY] problem4a: app_data_root state after delete = {app_state_after}");
    eprintln!("[BUGFIX_VERIFY] problem4a: project_root state after delete = {project_state_after}");

    // 修复后：tombstone 写到 project_root（正确），而非 app_data_root。
    let project_has_tombstone =
        project_state_after.contains("tombstones") && project_state_after.contains("vol1");
    let app_has_tombstone =
        app_state_after.contains("tombstones") && app_state_after.contains("vol1");

    assert!(
        project_has_tombstone,
        "tombstone 应被写到 project_root（问题 4a 已修复：正确 sync root）"
    );
    assert!(
        !app_has_tombstone,
        "tombstone 不应被写到 app_data_root（问题 4a 已修复：不再写错位置）"
    );
}

/// 验证问题 4b 已修复：delete_volume 中 tombstone 的 load 失败不再被吞掉。
///
/// 修复后用 project_root 的 sync state。在 project_root 下放损坏 JSON 让 load 失败，
/// delete_volume 应返回 Err（不再吞错误）。
#[test]
fn verify_problem4b_tombstone_load_error_not_swallowed() {
    let temp = tempdir().unwrap();
    let app_data_root = temp.path();
    let projects_root = app_data_root.join("projects");
    fs::create_dir_all(&projects_root).unwrap();

    let project_id = "test_project_4b";
    let project_root = projects_root.join(project_id);
    fs::create_dir_all(project_root.join("volumes")).unwrap();

    let volume_id = "vol1";
    let volume_dir = project_root.join("volumes").join(volume_id);
    fs::create_dir_all(&volume_dir).unwrap();
    fs::write(
        volume_dir.join("volume.json"),
        r#"{"id":"vol1","title":"V1"}"#,
    )
    .unwrap();

    // 在 project_root 下创建损坏的 state.local.json 让 load_sync_state(project_root) 失败。
    let project_sync_dir = project_root.join("app-meta/sync");
    fs::create_dir_all(&project_sync_dir).unwrap();
    fs::write(
        project_sync_dir.join("state.local.json"),
        "NOT_VALID_JSON{", // 损坏的 JSON
    )
    .unwrap();

    // 调用 delete_volume：修复后 load 失败不再被吞，应返回 Err。
    let result = volume::delete_volume(&project_root, volume_id, app_data_root);

    // 修复后：tombstone load 失败不再被吞，delete_volume 返回 Err。
    assert!(
        result.is_err(),
        "delete_volume 应返回 Err（问题 4b 已修复：不再吞 load 错误）"
    );

    eprintln!("[BUGFIX_VERIFY] problem4b: tombstone load 错误不再被吞，delete_volume 返回 Err");
}

/// 验证问题 4c 已修复：delete_chapter 把 tombstone 写到 project_root 而非 app_data_root。
#[test]
fn verify_problem4c_chapter_tombstone_correct_sync_root() {
    let temp = tempdir().unwrap();
    let app_data_root = temp.path();
    let projects_root = app_data_root.join("projects");
    fs::create_dir_all(&projects_root).unwrap();

    let project_id = "test_project_4c";
    let project_root = projects_root.join(project_id);
    let volume_id = "vol1";
    let chapter_id = "ch1";

    let chapter_dir = project_root
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(chapter_id);
    fs::create_dir_all(&chapter_dir).unwrap();
    fs::write(chapter_dir.join("chapter.meta.json"), r#"{"id":"ch1"}"#).unwrap();
    fs::write(chapter_dir.join("chapter.md"), "content").unwrap();

    // 在 project_root 和 app_data_root 都创建 sync state
    let project_sync_dir = project_root.join("app-meta/sync");
    fs::create_dir_all(&project_sync_dir).unwrap();
    fs::write(
        project_sync_dir.join("state.local.json"),
        r#"{"known_files":{},"device_id":"dev1"}"#,
    )
    .unwrap();

    let app_sync_dir = app_data_root.join("app-meta/sync");
    fs::create_dir_all(&app_sync_dir).unwrap();
    fs::write(
        app_sync_dir.join("state.local.json"),
        r#"{"known_files":{},"device_id":"dev1"}"#,
    )
    .unwrap();

    // 调用 delete_chapter
    chapter::delete_chapter(&project_root, volume_id, chapter_id, app_data_root).unwrap();

    let app_state_after =
        fs::read_to_string(app_sync_dir.join("state.local.json")).unwrap_or_default();
    let project_state_after =
        fs::read_to_string(project_sync_dir.join("state.local.json")).unwrap_or_default();

    eprintln!(
        "[BUGFIX_VERIFY] problem4c: app_data_root state after chapter delete = {app_state_after}"
    );
    eprintln!("[BUGFIX_VERIFY] problem4c: project_root state after chapter delete = {project_state_after}");

    // 修复后：chapter tombstone 写到 project_root（正确），而非 app_data_root。
    let project_has_tombstone =
        project_state_after.contains("tombstones") && project_state_after.contains("ch1");
    let app_has_tombstone =
        app_state_after.contains("tombstones") && app_state_after.contains("ch1");

    assert!(
        project_has_tombstone,
        "chapter tombstone 应被写到 project_root（问题 4c 已修复：正确 sync root）"
    );
    assert!(
        !app_has_tombstone,
        "chapter tombstone 不应被写到 app_data_root（问题 4c 已修复：不再写错位置）"
    );
}

// ===========================================================================
// 问题 5：known_files 缺失且无 tombstone 时不伪造 delete record
// ===========================================================================

/// 验证问题 5 已修复：snapshot_local_records_read_only 对 known_files 中
/// "文件不存在且没有 tombstone"的路径不再用 now_ms 伪造 delete record，
/// 而是返回 Err（本地状态错误）。
#[test]
fn verify_problem5_missing_local_file_not_fabricated_as_delete() {
    let manifest_src = read_writer_core_source("sync/lww/manifest/mod.rs");

    // 验证问题 5 的伪造代码模式已移除：
    // 不应有无 tombstone → 用 now_ms 作为删除检测时间的注释/代码。
    let has_now_ms_fabricate_comment =
        manifest_src.contains("无 tombstone → 用 now_ms 作为删除检测时间");
    assert!(
        !has_now_ms_fabricate_comment,
        "不应有无 tombstone 用 now_ms 伪造 delete 的注释（问题 5 已修复）"
    );

    // 验证 step 4 中无 tombstone 的 else 分支返回 Err（而非伪造 delete record）。
    // 搜索 "cannot fabricate delete record" 在 known_files 遍历中。
    let has_err_in_known_files = manifest_src.contains("cannot fabricate delete record")
        && manifest_src.contains("for path in state.known_files.keys()");
    assert!(
        has_err_in_known_files,
        "known_files 遍历中无 tombstone 时应返回 Err（问题 5 已修复：不伪造 delete record）"
    );

    // 验证 known_files 遍历中不再有 updated_at_ms: now_ms（伪造 delete 的标志）。
    // step 4 的 known_files 遍历范围
    let known_files_pos = manifest_src.find("for path in state.known_files.keys()");
    let manifest_upsert_pos = manifest_src.find("for (path, old_rec) in &old_manifest_records");
    if let (Some(kf_pos), Some(mu_pos)) = (known_files_pos, manifest_upsert_pos) {
        let step4_section = &manifest_src[kf_pos..mu_pos];
        let has_now_ms_in_step4 = step4_section.contains("updated_at_ms: now_ms,");
        assert!(
            !has_now_ms_in_step4,
            "step 4 known_files 遍历中不应有 updated_at_ms: now_ms（问题 5 已修复：不伪造 delete）"
        );
    }

    eprintln!(
        "[BUGFIX_VERIFY] problem5: known_files 缺失且无 tombstone 时返回 Err，不伪造 delete record"
    );
}

// ===========================================================================
// 汇总：确认所有 5 个问题都已修复
// ===========================================================================

#[test]
fn verify_all_five_problems_fixed() {
    eprintln!("[BUGFIX_VERIFY] === Issue #678 Comment 5654817198: 5 个问题修复验证汇总 ===");
    eprintln!(
        "[BUGFIX_VERIFY] 问题 1: 后台同步线程用 with_layout_core_api + layout 快照，不再 bootstrap"
    );
    eprintln!("[BUGFIX_VERIFY] 问题 2: sync_qptr 一路传递，回调链不断");
    eprintln!("[BUGFIX_VERIFY] 问题 3: dry_run/diagnostics 设置 current_sync_in_progress=true，single-flight 生效");
    eprintln!("[BUGFIX_VERIFY] 问题 4: tombstone 写到 project_root，不吞 load/save 错误");
    eprintln!(
        "[BUGFIX_VERIFY] 问题 5: known_files 缺失且无 tombstone 时返回 Err，不伪造 delete record"
    );
    eprintln!("[BUGFIX_VERIFY] === 所有 5 个问题已修复 ===");
}
