use super::*;
use tempfile::TempDir;

use std::fs;

#[test]
fn test_transaction_commit_all_files() {
    let tmp = TempDir::new().unwrap();
    let ws = tmp.path();

    let mut tx = SaveTransaction::new(ws);
    tx.add_file(
        "projects/p1/volumes/v1/chapters/c1/chapter.md",
        "hello world",
    )
    .unwrap();
    tx.add_file(
        "projects/p1/volumes/v1/chapters/c1/chapter.meta.json",
        r#"{"word_count":2}"#,
    )
    .unwrap();
    tx.commit().unwrap();

    let md =
        fs::read_to_string(ws.join("projects/p1/volumes/v1/chapters/c1/chapter.md")).unwrap();
    assert_eq!(md, "hello world");

    let meta =
        fs::read_to_string(ws.join("projects/p1/volumes/v1/chapters/c1/chapter.meta.json"))
            .unwrap();
    assert_eq!(meta, r#"{"word_count":2}"#);

    let tx_base = ws.join(TRANSACTIONS_DIR);
    assert!(!tx_base.exists() || fs::read_dir(&tx_base).unwrap().count() == 0);
}

#[test]
fn test_transaction_recovery_staging_exists() {
    let tmp = TempDir::new().unwrap();
    let ws = tmp.path();

    let mut tx = SaveTransaction::new(ws);
    tx.add_file("projects/p1/volumes/v1/chapters/c1/chapter.md", "recovered")
        .unwrap();
    tx.add_file(
        "projects/p1/volumes/v1/chapters/c1/chapter.meta.json",
        r#"{"word_count":1}"#,
    )
    .unwrap();

    let tx_dir = ws.join(TRANSACTIONS_DIR).join(tx.transaction_id());
    fs::create_dir_all(&tx_dir).unwrap();

    let manifest = TransactionManifest {
        transaction_id: tx.transaction_id().to_string(),
        created_at_ms: chrono::Utc::now().timestamp_millis(),
        entries: tx.entries.clone(),
        phase: TransactionPhase::Prepared,
        backup_entries: Vec::new(),
        git_finalize: None,
    };
    let manifest_json = serde_json::to_string_pretty(&manifest).unwrap();
    fs::write(tx_dir.join(MANIFEST_FILENAME), &manifest_json).unwrap();

    for entry in &tx.entries {
        let staging_path = tx_dir.join(&entry.staging_filename);
        let target_path = ws.join(&entry.target_relative);
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(
            &staging_path,
            fs::read_to_string(target_path).unwrap_or_default(),
        )
        .unwrap();
    }

    let (recovered, pending) = recover_pending_transactions(ws);
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].recovered_files.len(), 2);
    assert!(recovered[0].missing_files.is_empty());
    assert!(pending.is_empty());
}

#[test]
fn test_transaction_recovery_staging_lost() {
    let tmp = TempDir::new().unwrap();
    let ws = tmp.path();

    let tx_dir = ws.join(TRANSACTIONS_DIR).join("test-tx-id");
    fs::create_dir_all(&tx_dir).unwrap();

    let manifest = TransactionManifest {
        transaction_id: "test-tx-id".to_string(),
        created_at_ms: chrono::Utc::now().timestamp_millis(),
        entries: vec![TransactionEntry {
            staging_filename: "file_0".to_string(),
            target_relative: "projects/p1/volumes/v1/chapters/c1/chapter.md".to_string(),
            is_delete: false,
        }],
        phase: TransactionPhase::Prepared,
        backup_entries: Vec::new(),
        git_finalize: None,
    };
    let manifest_json = serde_json::to_string_pretty(&manifest).unwrap();
    fs::write(tx_dir.join(MANIFEST_FILENAME), &manifest_json).unwrap();

    let (recovered, pending) = recover_pending_transactions(ws);
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].recovered_files.len(), 0);
    assert_eq!(recovered[0].missing_files.len(), 1);
    assert!(pending.is_empty());
}

#[test]
fn test_transaction_empty_commit() {
    let tmp = TempDir::new().unwrap();
    let ws = tmp.path();

    let mut tx = SaveTransaction::new(ws);
    tx.commit().unwrap();

    assert!(tx.entries.is_empty());
}

#[test]
fn test_transaction_commit_with_delete() {
    let tmp = TempDir::new().unwrap();
    let ws = tmp.path();

    // 先创建一个文件
    fs::create_dir_all(ws.join("sub")).unwrap();
    fs::write(ws.join("sub/to_delete.txt"), "will be deleted").unwrap();
    fs::write(ws.join("sub/to_keep.txt"), "will be kept").unwrap();

    let mut tx = SaveTransaction::new(ws);
    tx.add_file("sub/new_file.txt", "new content").unwrap();
    tx.add_delete("sub/to_delete.txt");
    tx.commit().unwrap();

    // new_file.txt 应该存在
    assert_eq!(
        fs::read_to_string(ws.join("sub/new_file.txt")).unwrap(),
        "new content"
    );
    // to_delete.txt 应该被删除
    assert!(!ws.join("sub/to_delete.txt").exists());
    // to_keep.txt 应该保留
    assert_eq!(
        fs::read_to_string(ws.join("sub/to_keep.txt")).unwrap(),
        "will be kept"
    );
}

/// #644 评论 5483239422 问题1：`SaveTransaction::finish()` 吞掉
/// `write_manifest_phase(Finished)` 的错误，随后仍调用 `cleanup()` 删除 tx_dir。
///
/// 复现策略：构造 backup_mode 事务 commit 成功（phase=FilesCommittedPendingGit），
/// 然后使 manifest 读取失败（把 manifest 文件替换为同名目录，使
/// `fs::read_to_string` 返回 Err）。此时调用 `finish()`：
/// - 当前行为：`let _ = write_manifest_phase(...)` 吞错，`finished=true`，
///   `cleanup()` 执行 `remove_dir_all(tx_dir)`，tx_dir 被删，恢复证据丢失。
///   调用方（sync_ops）完全不知道 Finished 没写成功，仍会删 owner marker。
/// - 预期行为：`finish()` 应返回 `Err`，不调用 `cleanup()`，tx_dir 保留，
///   manifest 仍停在 FilesCommittedPendingGit，下次恢复可重试。
///
/// 此测试断言预期行为（tx_dir 应保留），当前代码下断言失败。
#[test]
fn finish_should_preserve_tx_dir_when_manifest_write_fails() {
    let tmp = TempDir::new().unwrap();
    let ws = tmp.path();

    let mut tx = SaveTransaction::new(ws);
    tx.enable_backup_mode();
    tx.add_file("a.txt", "hello").unwrap();
    tx.commit().unwrap();
    // 此时 phase = FilesCommittedPendingGit，tx_dir 存在，manifest 存在。

    // 使 manifest 读取失败：删除 manifest 文件，创建同名目录。
    // fs::read_to_string(目录) 会返回 Err，write_manifest_phase 返回 Err。
    let manifest_path = tx.tx_dir.join(MANIFEST_FILENAME);
    fs::remove_file(&manifest_path).unwrap();
    fs::create_dir(&manifest_path).unwrap();

    // 当前：finish() 返回 ()，吞错，cleanup() 删 tx_dir。
    // 预期：finish() 应返回 Err，不 cleanup，tx_dir 保留。
    let finish_result = tx.finish();
    assert!(
        finish_result.is_err(),
        "finish() should return Err when write_manifest_phase(Finished) fails; \
         current code swallows the error via `let _ = ...` and returns ()"
    );

    // 预期：tx_dir 应保留（manifest 写失败，不应 cleanup）。
    assert!(
        tx.tx_dir.exists(),
        "finish() should NOT cleanup tx_dir when write_manifest_phase(Finished) fails; \
         current code swallows the error via `let _ = ...` and deletes tx_dir, \
         losing recovery evidence while sync_ops still removes owner marker"
    );
}

/// #644 评论 5483239422 问题4：`recover_pending_transactions()` 在 manifest
/// 读/解析失败时直接 `remove_dir_all(tx_dir)`，销毁崩溃恢复材料。
///
/// 复现策略：构造 tx_dir 含损坏 manifest（无效 JSON）+ backup 恢复材料
/// （backup_entries + staging 文件），调用 `recover_pending_transactions`。
/// - 当前行为：manifest 解析失败，`remove_dir_all(tx_dir)`，恢复证据被销毁。
/// - 预期行为：记录错误并保留 tx_dir，不继续改 live，不删除 backup，
///   等下次启动重试或显式修复入口。
///
/// 此测试断言预期行为（tx_dir 应保留），当前代码下断言失败。
#[test]
fn recover_should_preserve_tx_dir_when_manifest_corrupted() {
    let tmp = TempDir::new().unwrap();
    let ws = tmp.path();

    let tx_dir = ws.join(TRANSACTIONS_DIR).join("corrupted-tx-644");
    fs::create_dir_all(&tx_dir).unwrap();

    // 写入损坏的 manifest（无效 JSON）。
    fs::write(tx_dir.join(MANIFEST_FILENAME), "{ this is not valid json").unwrap();

    // 放入 full-sync 崩溃恢复材料：backup + staging。
    let backup_dir = tx_dir.join("backup");
    fs::create_dir_all(&backup_dir).unwrap();
    fs::write(backup_dir.join("backup_0"), "old file content").unwrap();
    fs::write(tx_dir.join("file_0"), "staged content").unwrap();

    // 当前：manifest 解析失败 → remove_dir_all(tx_dir) 销毁恢复证据。
    // 预期：保留 tx_dir，等下次启动重试。
    let _ = recover_pending_transactions(ws);

    assert!(
        tx_dir.exists(),
        "recover must NOT delete tx_dir when manifest is corrupted; \
         tx_dir contains backup_entries + GitMetadataSnapshot + GitFinalizePlan \
         recovery material; current code remove_dir_all(tx_dir), destroying \
         last recovery evidence"
    );
}

// ── atomic_write tests ──

#[test]
fn test_atomic_write_success() {
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test.txt");

    atomic_write_string(&file_path, "hello world").unwrap();
    let read_content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(read_content, "hello world");
}

#[test]
fn test_atomic_write_continuous_overwrite() {
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test.txt");

    for i in 0..10 {
        let content = format!("content {}", i);
        atomic_write_string(&file_path, &content).unwrap();
        let read_content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(read_content, content);
    }
}

#[test]
#[cfg(unix)]
fn test_atomic_write_failure_keeps_old_file() {
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test.json");

    // 1. First write: success
    let original_content = r#"{"status": "ok", "version": 1}"#;
    atomic_write_string(&file_path, original_content).unwrap();

    // Check it exists and is correct
    let read_content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(read_content, original_content);

    // 2. Make the directory read-only (0o500)
    let dir_permissions = Permissions::from_mode(0o500);
    std::fs::set_permissions(temp_dir.path(), dir_permissions).unwrap();

    // 3. Attempt to overwrite: should fail because we can't create the tmp file in the read-only dir
    let new_content = r#"{"status": "updated", "version": 2}"#;
    let result = atomic_write_string(&file_path, new_content);
    assert!(result.is_err());

    // Restore directory permissions so we can clean up and read
    let restore_permissions = Permissions::from_mode(0o700);
    std::fs::set_permissions(temp_dir.path(), restore_permissions).unwrap();

    // 4. Verify the old file is still intact and readable (no half-written JSON)
    let read_content_after = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(read_content_after, original_content);

    // 5. Verify no tmp files are left in the directory
    let mut entries = std::fs::read_dir(temp_dir.path()).unwrap();
    let mut file_names: Vec<String> = Vec::new();
    while let Some(Ok(entry)) = entries.next() {
        file_names.push(entry.file_name().to_string_lossy().into_owned());
    }
    // Should only contain "test.json", no tmp files
    assert_eq!(file_names, vec!["test.json".to_string()]);
}
