//! Issue #644 评论 5496728184 — migration 恢复状态机缺陷2的回归测试。
//!
//! 验证评论 5496728184 指出的缺陷2已修复：
//! - 缺陷2：`recover_single_journal` 的 `Completed` 分支必须用 durable cleanup
//!   范式（`remove_file?` + `sync_dir?`），删除失败时返回 Err，不能吞错误。
//!
//! 验证策略：运行时行为验证 + 源码结构断言。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

// ── helpers ──

/// 读取 writer_core 源文件内容（用于源码结构断言）。
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

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

// ══ 缺陷2修复验证：Completed 阶段 durable cleanup ══

/// 验证缺陷2已修复：构造 phase=Completed 的 journal 文件，使 `fs::remove_file` 失败
/// （父目录只读，无写权限），调用 `recover_pending_delete_transactions`，
/// 确认 Completed 分支用 `fs::remove_file(journal_path)?` + `sync_dir(parent)?` 范式，
/// 删除失败时返回 Err（而非 Ok(1) 吞掉错误）。
#[test]
fn defect2_completed_phase_durable_cleanup_returns_err() {
    use writer_core::storage::journal::project_delete::{
        recover_pending_delete_transactions, ProjectDeleteJournal, ProjectDeletePhase,
    };

    let tmp = TempDir::new().unwrap();
    let app_data_root = tmp.path().join("appdata");
    let journals_dir = app_data_root.join("app-meta").join("delete-journals");
    fs::create_dir_all(&journals_dir).unwrap();

    // 步骤1：写一个 phase=Completed 的 journal 文件。
    let token = "test_token_644_c5496728184";
    let journal_path = journals_dir.join(format!(".sujian-delete-journal-{}", token));
    let journal = ProjectDeleteJournal {
        token: token.to_string(),
        project_id: "test_proj_644".to_string(),
        worktree_from: "/tmp/nonexistent_sujian/worktree".to_string(),
        worktree_trash: "/tmp/nonexistent_sujian/trash".to_string(),
        git_dir_from: None,
        git_dir_trash: None,
        projects_root: "/tmp/nonexistent_sujian/projects".to_string(),
        app_data_root: app_data_root.to_string_lossy().into_owned(),
        starmap_ids: Vec::new(),
        device_id: "test-device".to_string(),
        phase: ProjectDeletePhase::Completed,
        origin: writer_core::project::ProjectDeleteOrigin::User,
    };
    fs::write(&journal_path, serde_json::to_string(&journal).unwrap()).unwrap();
    assert!(
        journal_path.exists(),
        "setup: journal 应存在 (phase=Completed)"
    );

    // 步骤2：检测是否以 root 运行（root 绕过权限检查，权限方法不生效）。
    let probe = tmp.path().join("probe_root");
    fs::create_dir_all(&probe).unwrap();
    fs::write(probe.join("f"), "x").unwrap();
    set_mode(&probe, 0o555);
    let is_root = fs::remove_file(probe.join("f")).is_ok();
    set_mode(&probe, 0o755);

    let mut runtime_evidence = false;

    if is_root {
        eprintln!(
            "[DEFECT2-FIXED] 以 root 运行，权限限制不生效，退化为源码断言验证修复 \
             （运行时行为无法触发）"
        );
    } else {
        // 步骤3：chmod journals_dir 只读（0o555 = r-xr-xr-x）：可读可遍历不可写。
        //        read_dir 需 r+x（OK），read journal 需文件 r（OK），
        //        remove_file 需父目录 w+x（缺 w → EACCES 失败）。
        set_mode(&journals_dir, 0o555);

        // 步骤4：调用 recover_pending_delete_transactions。
        let result = recover_pending_delete_transactions(&app_data_root);

        // 恢复权限以便后续清理。
        set_mode(&journals_dir, 0o755);

        eprintln!(
            "[DEFECT2-FIXED] recover_pending_delete_transactions result: {:?}",
            result
        );

        // ── 缺陷2修复后运行时行为断言 ──
        // (a) 返回 Ok(0) — 删除失败时 recover_single_journal 返回 Err，
        //     上层 recover_pending_delete_transactions catch 并 log error，
        //     不计入 recovered（原缺陷行为是 Ok(1)，把删除失败误声称成已完成）。
        assert!(
            result.is_ok(),
            "缺陷2修复: recover 应返回 Ok — 上层 recover_pending_delete_transactions \
             不因单个 journal 恢复失败而整体失败。实际: {:?}",
            result
        );
        let recovered = result.unwrap();
        assert_eq!(
            recovered.len(),
            0,
            "缺陷2修复: 应报告 recovered=0 — 删除失败（父目录只读）时 \
             recover_single_journal 返回 Err，不被计入 recovered。 \
             原缺陷行为是 Ok(1)（吞错误误声称完成），修复后正确不计入。"
        );

        // (b) journal 文件仍存在 — 删除失败，journal 保留供下次重启继续
        assert!(
            journal_path.exists(),
            "缺陷2修复: journal 应仍存在 — 删除失败（父目录只读），journal 保留供 \
             下次重启继续恢复，不再被声称'已完成'"
        );

        runtime_evidence = true;
        eprintln!(
            "[DEFECT2-FIXED] 运行时验证成功：删除失败返回 Ok(0)（未计入 recovered）， \
             journal 保留。正确行为：recover_single_journal 内部用 remove_file? + \
             sync_dir? 范式返回 Err，上层不计入 recovered"
        );
    }

    // ── 缺陷2修复后源码结构断言（无论是否 root 都验证）──
    let src = read_src_file("src/storage/journal/project_delete.rs");
    let recover_body = extract_fn_body(&src, "recover_single_journal");
    // 确认 Completed 分支不再用 `let _ =` 吞掉删除错误
    assert!(
        !recover_body.contains("let _ = fs::remove_file(journal_path)"),
        "源码断言: Completed 分支不应再含 'let _ = fs::remove_file(journal_path)' — \
         缺陷代码已移除"
    );
    // 确认 Completed 分支现在含 `fs::remove_file(journal_path)?` + sync_dir
    let completed_section = recover_body
        .split("ProjectDeletePhase::Completed")
        .nth(1)
        .unwrap_or("");
    assert!(
        completed_section.contains("fs::remove_file(journal_path)?"),
        "源码断言: Completed 分支应含 'fs::remove_file(journal_path)?' — 删除失败 \
         正确传播错误"
    );
    assert!(
        completed_section.contains("sync_dir"),
        "源码断言: Completed 分支应含 sync_dir(parent) — durable cleanup 范式已引入"
    );
    // 对比正确范式：cleanup_journal 含 remove_file? + sync_dir（仍应成立）
    let cleanup_body = extract_fn_body(&src, "cleanup_journal");
    assert!(
        cleanup_body.contains("fs::remove_file(&self.journal_path)?")
            && cleanup_body.contains("sync_dir"),
        "源码断言: cleanup_journal 含正确的 'remove_file? + sync_dir' durable cleanup 范式 — \
         Completed 分支已复用此范式"
    );

    eprintln!(
        "[DEFECT2-FIXED] 验证成功（运行时证据={}）：Completed 阶段 durable cleanup。 \
         源码断言确认 'let _ =' 已移除，'remove_file? + sync_dir' 范式已引入",
        runtime_evidence
    );
}
