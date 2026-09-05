//! Issue #644 评论 5496728184 — migration 恢复状态机两个关键崩溃窗口缺陷的回归测试。
//!
//! 验证评论 5496728184 指出的两个缺陷已修复：
//! - 缺陷1：`resume_layout_migration` 的 `Prepared` 分支必须按三态判断，
//!   识别 claimed_source 已是 rename 成功状态并补推进，不能删 journal 遗弃历史。
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

/// 与源码 `canonicalize_or_lossy` 对齐：canonicalize 成功则用规范路径，否则 lossy。
fn canonicalize_or_lossy(path: &Path) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
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

// ══ 缺陷1修复验证：Prepared 阶段三态判断正确恢复 ══

/// 验证缺陷1已修复：构造"rename 已成功、phase 还没推进"的真实崩溃磁盘状态，
/// 调用 `ensure_project_repo_with_layout`（内部触发 `resume_layout_migration`），
/// 确认 Prepared 分支识别 claimed_source 已是 rename 成功状态，补推进 phase 到
/// SourceClaimed 并继续迁移，最终 target 含完整历史、claimed_source 被清理、
/// journal 被清理。
#[test]
fn defect1_prepared_phase_recovers_renamed_source() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();

    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // 外部布局：worktree_root 与外部 git_dir（target）分离
    let worktree_root = root.join("worktree");
    let git_dir = root.join("private").join("project.git");
    fs::create_dir_all(&worktree_root).unwrap();
    fs::create_dir_all(git_dir.parent().unwrap()).unwrap();

    // 步骤1：在 worktree_root 下 init 仓库（original_source = worktree/.git），
    //        创建一个 commit 作为历史标记（用于后续验证历史是否被迁移）。
    let repo = git2::Repository::init(&worktree_root).unwrap();
    fs::write(worktree_root.join("README.md"), "hello sujian").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("README.md")).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    let original_commit = repo
        .commit(
            Some("HEAD"),
            &sig,
            &sig,
            "init commit (历史标记)",
            &tree,
            &[],
        )
        .unwrap();
    drop(tree);
    drop(index);
    drop(repo);

    let original_source = worktree_root.join(".git");
    assert!(original_source.exists(), "setup: original_source 应存在");

    // 步骤2：模拟崩溃状态——rename + fsync 已完成，但 phase=SourceClaimed 未落盘。
    //        把 original_source rename 到 claimed_source（取得所有权）。
    let owner = "test-owner-644-c5496728184";
    let claimed_source = worktree_root.join(format!(".git.sujian-migrate-source-{}", owner));
    fs::rename(&original_source, &claimed_source).unwrap();
    // 此时：original_source 不存在，claimed_source 存在（含完整 Git 历史 + commit）

    // 步骤3：写 journal: phase=Prepared（崩在 phase=SourceClaimed 落盘之前）。
    //        journal 路径：<target_git_dir.parent()>/.layout-migrations/<owner>.json
    let target_tmp = git_dir
        .parent()
        .unwrap()
        .join(format!(".git.sujian-migrate-tmp-{}", owner));
    let migrations_dir = git_dir.parent().unwrap().join(".layout-migrations");
    fs::create_dir_all(&migrations_dir).unwrap();
    let journal_path = migrations_dir.join(format!("{}.json", owner));

    let worktree_canonical = canonicalize_or_lossy(&worktree_root);
    let journal_json = serde_json::json!({
        "owner": owner,
        "worktree_canonical": worktree_canonical,
        "original_source": original_source.to_string_lossy(),
        "claimed_source": claimed_source.to_string_lossy(),
        "target_tmp": target_tmp.to_string_lossy(),
        "target_git_dir": git_dir.to_string_lossy(),
        "phase": "prepared"
    });
    fs::write(&journal_path, serde_json::to_string(&journal_json).unwrap()).unwrap();

    // 磁盘状态确认（缺陷1触发条件）：
    assert!(
        journal_path.exists(),
        "setup: journal 应存在 (phase=Prepared)"
    );
    assert!(
        !original_source.exists(),
        "setup: original_source 应不存在 (rename 已成功)"
    );
    assert!(
        claimed_source.exists(),
        "setup: claimed_source 应存在 (含 Git 历史)"
    );
    assert!(!git_dir.exists(), "setup: target git_dir 应不存在");

    eprintln!(
        "[DEFECT1-FIXED] 触发状态: journal.phase=Prepared, original=缺失, claimed=存在(含历史), target=缺失"
    );

    // 步骤4：调用 ensure_project_repo_with_layout 触发 resume_layout_migration。
    let layout = writer_core::storage::git_repo_layout::GitRepoLayout::with_external_git_dir(
        worktree_root.clone(),
        git_dir.clone(),
    );
    let result = writer_core::storage::workspace_git::ensure_workspace_repo(&layout);

    // 步骤5：观察修复后的正确行为。
    // 修复后：Prepared 识别 (false, true, Missing) → 补推进 SourceClaimed →
    // SourceClaimed copy claimed -> target_tmp → TargetPrepared rename -> target →
    // TargetInstalled 删 claimed → SourceCleaned 删 journal。
    // 最终：journal 被清理（在 SourceCleaned 阶段，不是 Prepared 阶段）、
    //       claimed_source 被清理、target 含原 commit、ensure 返回 Ok。
    let journal_deleted = !journal_path.exists();
    let claimed_cleaned = !claimed_source.exists();

    // target 是否含原 commit（正确行为应迁移 claimed_source 内容，含原 commit）
    let target_has_original_commit = match git2::Repository::open(&git_dir) {
        Ok(r) => match r.head() {
            Ok(h) => h
                .target()
                .map(|oid| oid == original_commit)
                .unwrap_or(false),
            Err(_) => false, // UnbornBranch 或无 commit → 空仓库
        },
        Err(_) => false, // target 不可打开
    };

    eprintln!(
        "[DEFECT1-FIXED] ensure_project_repo_with_layout result: {:?}",
        result.as_ref().err()
    );
    eprintln!(
        "[DEFECT1-FIXED] journal_deleted={} claimed_cleaned={} target_has_original_commit={}",
        journal_deleted, claimed_cleaned, target_has_original_commit
    );

    // ── 缺陷1修复后运行时行为断言 ──
    // (a) ensure_project_repo_with_layout 应返回 Ok（迁移成功完成）
    assert!(
        result.is_ok(),
        "缺陷1修复: ensure_project_repo_with_layout 应返回 Ok — Prepared 识别 claimed \
         已存在并补推进 SourceClaimed，继续迁移至完成。实际: {:?}",
        result.as_ref().err()
    );
    // (b) journal 被清理 — 迁移完整走完所有阶段后 SourceCleaned 删 journal
    assert!(
        journal_deleted,
        "缺陷1修复: journal 应被清理 — 迁移走完 SourceCleaned 阶段后正常删除 journal \
         （而非 Prepared 阶段误删）"
    );
    // (c) claimed_source 被清理 — TargetInstalled 阶段删除 claimed_source
    assert!(
        claimed_cleaned,
        "缺陷1修复: claimed_source 应被清理 — TargetInstalled 阶段正常删除 \
         claimed_source，无孤儿目录残留"
    );
    // (d) target 含原 commit — 仓库历史被正确迁移
    assert!(
        target_has_original_commit,
        "缺陷1修复: target 应含原 commit — claimed_source 的 Git 历史被完整迁移到 \
         target_git_dir，历史不遗失"
    );

    // ── 缺陷1修复后源码结构断言 ──
    // 确认缺陷代码已移除：Prepared 分支不再含 "source 不存在，清理 journal" 注释。
    let src = read_src_file("src/storage/git_repo_layout/migration.rs");
    let resume_body = extract_fn_body(&src, "resume_layout_migration");
    assert!(
        !resume_body.contains("source 不存在，清理 journal"),
        "源码断言: Prepared 分支不应再含 'source 不存在，清理 journal' 注释 — \
         缺陷代码已移除"
    );
    // 确认 Prepared 分支现在按三态判断（含 claimed_exists 和 Corrupt 检查）
    // 用 `MigrationPhase::SourceClaimed =>` 作为分隔符，避免被
    // `phase: MigrationPhase::SourceClaimed`（字段赋值）误截断。
    let prepared_section = resume_body
        .split("MigrationPhase::Prepared")
        .nth(1)
        .and_then(|s| s.split("MigrationPhase::SourceClaimed =>").next())
        .unwrap_or("");
    assert!(
        prepared_section.contains("claimed_exists"),
        "源码断言: Prepared 分支应检查 claimed_exists — 三态判断已引入"
    );
    assert!(
        prepared_section.contains("RepoOpenResult::Corrupt"),
        "源码断言: Prepared 分支应检查 RepoOpenResult::Corrupt — 不再吞 Corrupt 错误"
    );
    assert!(
        prepared_section.contains("RepoOpenResult::Valid"),
        "源码断言: Prepared 分支应检查 RepoOpenResult::Valid — final 已是有效仓库时 \
         推进到 TargetInstalled"
    );
    // 确认 Prepared 分支不再调用 remove_migration_journal（只在 SourceCleaned 调用）
    assert!(
        !prepared_section.contains("remove_migration_journal"),
        "源码断言: Prepared 分支不应再调 remove_migration_journal — 不能直接删 journal \
         遗弃 claimed_source"
    );

    eprintln!(
        "[DEFECT1-FIXED] 验证成功：Prepared 阶段三态判断正确恢复。journal 清理={}, \
         claimed 清理={}, 历史迁移={}",
        journal_deleted, claimed_cleaned, target_has_original_commit
    );
}

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
        phase: ProjectDeletePhase::Completed,
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
