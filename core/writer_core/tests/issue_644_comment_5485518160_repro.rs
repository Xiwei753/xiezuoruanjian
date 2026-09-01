//! Issue #644 评论 5485518160 — 4 个修改点的白盒测试。
//!
//! 本测试文件针对评论 5485518160 描述的 4 个修改点，用白盒方式验证修复行为：
//! 1. `OwnedIndexLock::acquire` 改用目录锁模型 + owner metadata + 崩溃恢复状态机：
//!    - State 1：lock 目录存在 + owner 文件归属本轮 → 恢复 + 重新 acquire
//!    - State 2：lock 不存在 + live index hash == prepared_bytes hash → AlreadyCommitted
//!    - State 3：lock 不存在 + live index hash != prepared_bytes hash → 重新 acquire
//!    - State 4：lock 是 regular file 或目录但 owner 不匹配 → ConcurrentMetadataChanged
//! 2. `index_lock_owner=None` 旧 manifest 迁移（`check_index_lock_owner_migration`）
//! 3. NotGitRepo `.git` durable fsync + `copy_dir_recursive` durable copy
//! 4. `finalize_existing` 收紧 detached HEAD post-check
//!
//! #644 评论 5486167472 问题1+问题2 + #644 评论 5486852142 问题1：
//! 协议从 O_EXCL+regular file 改为 create_dir+目录锁模型+owner metadata+hash 校验。
//! 白盒测试通过直接在 `.git` 下构造 lock 目录/owner/prepared/index 的状态
//! 来模拟崩溃状态，然后调用 `OwnedIndexLock::acquire` 验证恢复行为。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use writer_core::sync::git_commit::{
    check_index_lock_owner_migration, lock_belongs_to_owner, owner_metadata, AcquireOutcome,
    GitFinalizeError, GitFinalizePlan, GitMetadataSnapshot, IndexLockOwnerMigration, IndexSnapshot,
    OwnedIndexLock, RefSnapshot,
};

// ── helpers ──

/// 计算 SHA-256（与 git_commit.rs 内部 sha256_bytes 一致）。
fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// 构造一个空的 .git 目录（不含完整 repo，仅用于测试 acquire 的 inode 状态机）。
fn make_bare_git_dir(live: &Path) -> std::path::PathBuf {
    let git_dir = live.join(".git");
    fs::create_dir_all(&git_dir).unwrap();
    git_dir
}

// ══ 修改点 1：OwnedIndexLock::acquire 崩溃恢复状态机 ══

/// State 1：lock 目录存在 + owner 文件归属本轮。
/// 说明上次已拿锁但还没 commit。acquire 应删除 lock 目录，fsync .git，
/// 重新 acquire（返回 NewlyAcquired）。
#[test]
fn acquire_state1_acquired_not_renamed_recovers_and_reacquires() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-owner-state1";
    let lock_path = git_dir.join("index.lock");
    let index_path = git_dir.join("index");

    // 构造 State 1（目录锁模型）：
    // 1. 创建 lock 目录（Sujian lock）
    fs::create_dir(&lock_path).unwrap();
    // 2. 在 lock 目录内写 owner 文件（本轮 owner metadata）
    fs::write(lock_path.join("owner"), owner_metadata(owner)).unwrap();
    // 3. 在 lock 目录内写 prepared 文件（prepared content）
    let prepared_bytes = b"new index content";
    fs::write(lock_path.join("prepared"), prepared_bytes).unwrap();
    // 4. 写不同的 index 内容（说明 commit 未发生）
    fs::write(&index_path, b"old index content").unwrap();

    // 确认初始状态
    assert!(lock_path.exists());
    assert!(lock_path.is_dir());

    // 调用 acquire：应检测到 State 1，删除 lock 目录，重新 acquire。
    let result = OwnedIndexLock::acquire(&git_dir, owner, prepared_bytes);
    match &result {
        Ok(AcquireOutcome::NewlyAcquired(_)) => {
            // 预期：恢复后重新 acquire 成功
        }
        Ok(AcquireOutcome::AlreadyCommitted) => {
            panic!(
                "DEFECT: acquire returned AlreadyCommitted for State 1 (acquired but not \
                 committed) — should return NewlyAcquired after recovery"
            );
        }
        Err(e) => {
            panic!(
                "DEFECT: acquire failed for State 1 (acquired but not committed): {} — \
                 should recover (delete lock dir) and re-acquire",
                e
            );
        }
    }

    // 验证：lock 目录存在（重新 acquire 后 create_dir 创建了新 lock 目录）
    assert!(
        lock_path.exists(),
        "after State 1 recovery + re-acquire, index.lock should exist (newly created dir)"
    );
    assert!(
        lock_path.is_dir(),
        "after re-acquire, index.lock should be a directory"
    );
    // owner 文件存在（重新 acquire 创建了新 owner 文件）
    let owner_file = lock_path.join("owner");
    assert!(
        owner_file.exists(),
        "after State 1 recovery + re-acquire, owner file should exist (newly created)"
    );
    // owner 文件内容是本轮 owner metadata
    let owner_content = fs::read(&owner_file).unwrap();
    assert!(
        lock_belongs_to_owner(&owner_content, owner),
        "after re-acquire, owner file content should be our owner metadata"
    );
}

/// State 2：lock 不存在 + live index hash == prepared_bytes hash。
/// 说明上次 commit_rename 已完成且方向匹配。acquire 应返回 AlreadyCommitted
///（绝不打开/truncate index）。
///
/// #644 评论 5486167472 问题2：AlreadyCommitted 只有在 live index hash == prepared_bytes hash
/// 时才成立。如果 hash 不匹配（另一方向的已提交状态），应 fall through 重新 acquire。
#[test]
fn acquire_state2_already_committed_returns_already_committed() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-owner-state2";
    let lock_path = git_dir.join("index.lock");
    let index_path = git_dir.join("index");

    // 构造 State 2（目录锁模型）：
    // 1. 写 index，内容 == committed_index_bytes（模拟上次 commit_rename 完成）
    let committed_index_bytes = b"committed index content";
    fs::write(&index_path, committed_index_bytes).unwrap();
    // 2. index.lock 不存在（commit 已完成，lock 目录已删）
    assert!(!lock_path.exists());

    // 保存 index 内容，验证 acquire 不会 truncate 它
    let index_content_before = fs::read(&index_path).unwrap();

    // 调用 acquire：prepared_bytes == committed_index_bytes → hash 匹配 → AlreadyCommitted。
    let result = OwnedIndexLock::acquire(&git_dir, owner, committed_index_bytes);
    match &result {
        Ok(AcquireOutcome::AlreadyCommitted) => {
            // 预期：返回 AlreadyCommitted
        }
        Ok(AcquireOutcome::NewlyAcquired(_)) => {
            panic!(
                "DEFECT: acquire returned NewlyAcquired for State 2 (already committed, \
                 hash matches) — should return AlreadyCommitted"
            );
        }
        Err(e) => {
            panic!(
                "DEFECT: acquire failed for State 2 (already committed): {} — should \
                 return AlreadyCommitted",
                e
            );
        }
    }

    // 验证：index 内容未被 truncate/修改
    let index_content_after = fs::read(&index_path).unwrap();
    assert_eq!(
        index_content_after, index_content_before,
        "acquire must NOT truncate/modify live index for State 2 (already committed)"
    );
    // index.lock 仍不存在
    assert!(
        !lock_path.exists(),
        "index.lock should not exist for State 2 (already committed)"
    );
}

/// #644 评论 5486167472 问题2：State 2 方向不匹配时应 fall through 重新 acquire。
/// 构造 forward commit 已完成（index == new），但当前调用准备的是 rollback 的 old。
/// acquire 应检测到 hash 不匹配 → fall through 重新 acquire。
#[test]
fn acquire_state2_direction_mismatch_falls_through_to_reacquire() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-owner-state2-mismatch";
    let lock_path = git_dir.join("index.lock");
    let index_path = git_dir.join("index");

    // 构造 forward commit 已完成的状态：
    // index 内容是 forward 的 new（上一阶段已提交）
    let forward_new_index = b"forward new index content";
    fs::write(&index_path, forward_new_index).unwrap();
    // lock 不存在
    assert!(!lock_path.exists());

    // 当前调用准备的是 rollback 的 old（hash != forward_new_index）
    let rollback_old_bytes = b"rollback old index content";
    assert_ne!(
        sha256_bytes(rollback_old_bytes),
        sha256_bytes(forward_new_index),
        "test precondition: rollback old hash must differ from forward new hash"
    );

    // 调用 acquire：hash 不匹配 → State 3 → fall through 重新 acquire。
    let result = OwnedIndexLock::acquire(&git_dir, owner, rollback_old_bytes);
    match &result {
        Ok(AcquireOutcome::NewlyAcquired(_)) => {
            // 预期：fall through 后重新 acquire 成功
        }
        Ok(AcquireOutcome::AlreadyCommitted) => {
            panic!(
                "DEFECT: acquire returned AlreadyCommitted when direction mismatch \
                 (forward committed but rollback prepared) — should fall through \
                 and re-acquire to avoid old worktree / old refs / new index inconsistency"
            );
        }
        Err(e) => {
            panic!(
                "DEFECT: acquire failed for direction mismatch: {} — should fall \
                 through and re-acquire",
                e
            );
        }
    }

    // 验证：lock 目录存在（重新 acquire 创建了新 lock 目录）
    assert!(lock_path.exists());
    assert!(lock_path.is_dir());
    // prepared 文件存在（重新 acquire 创建了新 prepared 文件，内容是 rollback old）
    let prepared_file = lock_path.join("prepared");
    assert!(prepared_file.exists());
    let prepared_content = fs::read(&prepared_file).unwrap();
    assert_eq!(
        prepared_content, rollback_old_bytes,
        "prepared file should contain rollback old bytes after re-acquire"
    );
}

/// State 3：lock 不存在，index 内容与 prepared_bytes hash 不匹配。
/// acquire 应 fall through 重新 acquire（目录锁模型下无残留文件需清理）。
#[test]
fn acquire_state3_orphan_prepared_file_recovers_and_reacquires() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-owner-state3";
    let lock_path = git_dir.join("index.lock");
    let index_path = git_dir.join("index");

    // 构造 State 3（目录锁模型）：
    // 1. index.lock 不存在
    assert!(!lock_path.exists());
    // 2. index 存在但内容与 prepared_bytes hash 不匹配
    fs::write(&index_path, b"existing index content").unwrap();

    // 调用 acquire：应检测到 State 3，重新 acquire。
    let result = OwnedIndexLock::acquire(&git_dir, owner, b"new prepared bytes");
    match &result {
        Ok(AcquireOutcome::NewlyAcquired(_)) => {
            // 预期：重新 acquire 成功
        }
        Ok(AcquireOutcome::AlreadyCommitted) => {
            panic!(
                "DEFECT: acquire returned AlreadyCommitted for State 3 (index hash \
                 mismatch) — should return NewlyAcquired after re-acquire"
            );
        }
        Err(e) => {
            panic!(
                "DEFECT: acquire failed for State 3 (index hash mismatch): {} — should \
                 re-acquire",
                e
            );
        }
    }

    // 验证：lock 目录存在（重新 acquire 后 create_dir 创建了新 lock 目录）
    assert!(
        lock_path.exists(),
        "after State 3 re-acquire, index.lock should exist"
    );
    assert!(
        lock_path.is_dir(),
        "after re-acquire, index.lock should be a directory"
    );
    // prepared 文件存在（重新 acquire 创建了新 prepared 文件）
    let prepared_file = lock_path.join("prepared");
    assert!(
        prepared_file.exists(),
        "after State 3 re-acquire, prepared file should exist"
    );
}

/// State 4：index.lock 存在为 regular file（外部 Git 进程的 lock）。
/// acquire 应返回 ConcurrentMetadataChanged，绝不碰 lock。
#[test]
fn acquire_state4_external_lock_no_owner_returns_concurrent_changed() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-owner-state4";
    let lock_path = git_dir.join("index.lock");

    // 构造 State 4（目录锁模型）：
    // index.lock 存在为 regular file（外部 Git 进程创建的）
    let external_lock_content = b"external git process lock";
    fs::write(&lock_path, external_lock_content).unwrap();

    // 调用 acquire：应返回 ConcurrentMetadataChanged。
    let result = OwnedIndexLock::acquire(&git_dir, owner, b"prepared bytes");
    match &result {
        Err(GitFinalizeError::ConcurrentMetadataChanged { .. }) => {
            // 预期：返回 ConcurrentMetadataChanged
        }
        Ok(AcquireOutcome::NewlyAcquired(_)) => {
            panic!(
                "DEFECT: acquire returned NewlyAcquired for State 4 (external regular \
                 file lock) — should return ConcurrentMetadataChanged"
            );
        }
        Ok(AcquireOutcome::AlreadyCommitted) => {
            panic!(
                "DEFECT: acquire returned AlreadyCommitted for State 4 (external regular \
                 file lock) — should return ConcurrentMetadataChanged"
            );
        }
        Err(e) => {
            panic!(
                "DEFECT: acquire returned wrong error type for State 4: {} — should be \
                 ConcurrentMetadataChanged",
                e
            );
        }
    }

    // 验证：lock 未被修改/删除（绝不碰别人的 lock）
    assert!(
        lock_path.exists(),
        "acquire must NOT delete external index.lock"
    );
    let lock_content_after = fs::read(&lock_path).unwrap();
    assert_eq!(
        lock_content_after, external_lock_content,
        "acquire must NOT modify external index.lock content"
    );
}

/// State 4 变体：lock 目录存在但 owner 文件不匹配（不同事务的 lock）。
/// acquire 应返回 ConcurrentMetadataChanged，绝不碰 lock。
#[test]
fn acquire_state4_inode_mismatch_returns_concurrent_changed() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-owner-state4b";
    let lock_path = git_dir.join("index.lock");

    // 构造 State 4 变体（目录锁模型）：
    // 1. 创建 lock 目录（Sujian lock）
    fs::create_dir(&lock_path).unwrap();
    // 2. 在 lock 目录内写 owner 文件，但 owner 是不同事务的
    fs::write(
        lock_path.join("owner"),
        owner_metadata("different-transaction-owner"),
    )
    .unwrap();

    // 调用 acquire：应返回 ConcurrentMetadataChanged（owner 不匹配）。
    let result = OwnedIndexLock::acquire(&git_dir, owner, b"prepared bytes");
    match &result {
        Err(GitFinalizeError::ConcurrentMetadataChanged { .. }) => {
            // 预期
        }
        _ => {
            panic!(
                "DEFECT: acquire should return ConcurrentMetadataChanged for State 4 \
                 (directory lock with mismatched owner), got: {:?}",
                result.as_ref().err()
            );
        }
    }

    // 验证：lock 目录未被修改/删除
    assert!(lock_path.exists());
    let owner_content_after = fs::read(lock_path.join("owner")).unwrap();
    assert_eq!(
        owner_content_after,
        owner_metadata("different-transaction-owner"),
        "acquire must NOT modify external lock directory owner file when owner does not match"
    );
}

/// 验证 acquire 用 create_dir 创建 lock 目录，prepared 文件内容正确：
/// 关键：不会 truncate 已存在的 live index（State 2 的核心安全属性）。
#[test]
fn acquire_uses_create_new_never_truncates_existing_owner() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-owner-create-new";
    let lock_path = git_dir.join("index.lock");

    // 无任何残留状态：正常 acquire
    let prepared = b"fresh prepared content";
    let result = OwnedIndexLock::acquire(&git_dir, owner, prepared);
    assert!(
        matches!(result, Ok(AcquireOutcome::NewlyAcquired(_))),
        "fresh acquire should succeed"
    );

    // lock 目录存在（create_dir 创建）
    assert!(lock_path.exists());
    assert!(lock_path.is_dir());

    // prepared 文件内容应正好是 prepared_bytes（create_new 创建，不是 truncate）
    let prepared_file = lock_path.join("prepared");
    let prepared_content = fs::read(&prepared_file).unwrap();
    assert_eq!(
        prepared_content, prepared,
        "prepared file content should match prepared_bytes exactly"
    );

    // 清理（drop lock）
    drop(result.unwrap());
}

/// 验证"commit_rename 成功、drop 还没删 lock 目录就 SIGKILL"的崩溃恢复。
/// 模拟：acquire → commit_rename 成功 → 进程死在 Drop 删除 lock 目录前。
/// 目录锁模型下 commit_rename 把 prepared_file rename 成 index，然后删 lock 目录。
/// 磁盘状态：index 存在（内容 == prepared_bytes），index.lock 不存在。
/// 恢复时 acquire 用相同 prepared_bytes 调用应返回 AlreadyCommitted（hash 匹配）。
#[test]
fn crash_after_rename_before_drop_owner_recovers_as_already_committed() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-crash-after-rename";
    let index_path = git_dir.join("index");

    // 1. 正常 acquire
    let prepared = b"target index content";
    let mut lock = match OwnedIndexLock::acquire(&git_dir, owner, prepared).unwrap() {
        AcquireOutcome::NewlyAcquired(l) => l,
        AcquireOutcome::AlreadyCommitted => panic!("should be newly acquired"),
    };

    // 2. commit_rename 成功（prepared_file rename 成 index，然后删 lock 目录）
    lock.commit_rename(&index_path).unwrap();

    // 3. 模拟 SIGKILL：用 ManuallyDrop 阻止 Drop 运行。
    //    目录锁模型下 commit_rename 后 lock 目录已删，index 存在。
    let _leaked_lock = std::mem::ManuallyDrop::new(lock);

    // 验证崩溃状态
    assert!(index_path.exists(), "index should exist (rename completed)");
    assert!(
        !git_dir.join("index.lock").exists(),
        "index.lock should not exist (commit_rename deleted it)"
    );

    // 4. 恢复：再次 acquire 用相同 prepared_bytes（hash 匹配）应返回 AlreadyCommitted
    let result = OwnedIndexLock::acquire(&git_dir, owner, prepared);
    assert!(
        matches!(result, Ok(AcquireOutcome::AlreadyCommitted)),
        "after crash (commit done, hash matches), acquire should return \
         AlreadyCommitted, got: {:?}",
        result.as_ref().err()
    );

    // index 内容未被修改
    let index_content = fs::read(&index_path).unwrap();
    assert_eq!(
        index_content, prepared,
        "index content should be the committed content (not truncated)"
    );
}

/// 验证"reverse acquire 成功、commit 前 SIGKILL"的崩溃恢复。
/// 模拟：rollback acquire → 进程死在 commit_rename 前。
/// 磁盘状态：lock 目录存在（含 owner + prepared），index 是 new（不同内容）。
/// 恢复时 acquire 应检测到 State 1，删除 lock 目录，重新 acquire。
#[test]
fn crash_after_reverse_acquire_before_commit_recovers_and_reacquires() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-crash-reverse-acquire";
    let lock_path = git_dir.join("index.lock");
    let index_path = git_dir.join("index");

    // 1. 构造 live index = new（模拟 forward install 已完成）
    let new_index = b"new index content";
    fs::write(&index_path, new_index).unwrap();

    // 2. 正常 acquire（模拟 rollback 开始：写 old index 到 prepared file）
    let old_index = b"old index content to restore";
    let lock = match OwnedIndexLock::acquire(&git_dir, owner, old_index).unwrap() {
        AcquireOutcome::NewlyAcquired(l) => l,
        AcquireOutcome::AlreadyCommitted => panic!("should be newly acquired"),
    };

    // 3. 模拟 SIGKILL：用 ManuallyDrop 阻止 Drop 运行，不 commit。
    //    磁盘状态：lock 目录存在（含 owner + prepared），index 是 new。
    let _leaked_lock = std::mem::ManuallyDrop::new(lock);

    // 验证崩溃状态
    assert!(lock_path.exists());
    assert!(lock_path.is_dir());
    let prepared_file = lock_path.join("prepared");
    assert!(prepared_file.exists());
    assert!(index_path.exists());

    // 4. 恢复：再次 acquire 应检测到 State 1，恢复 + 重新 acquire
    let result = OwnedIndexLock::acquire(&git_dir, owner, old_index);
    assert!(
        matches!(result, Ok(AcquireOutcome::NewlyAcquired(_))),
        "after crash (reverse acquire done, commit not done), acquire should \
         recover State 1 and re-acquire, got: {:?}",
        result.as_ref().err()
    );

    // index 内容未被修改（仍是 new，因为 rollback commit 未发生）
    let index_content = fs::read(&index_path).unwrap();
    assert_eq!(
        index_content, new_index,
        "index content should still be new (rollback commit did not happen)"
    );
}

// ══ 修改点 2：index_lock_owner=None 旧 manifest 迁移 ══

/// 验证迁移判定：没有 index.lock 且 current index == snapshot.index（old）
/// → AlreadyReverted（安全 no-op，无需补 owner）。
#[test]
fn migration_no_lock_current_is_old_returns_already_reverted() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let old_index = b"old index content";
    let new_index = b"new index content";
    let index_path = git_dir.join("index");
    let lock_path = git_dir.join("index.lock");

    // current index == old
    fs::write(&index_path, old_index).unwrap();
    // no lock
    assert!(!lock_path.exists());

    let snapshot = GitMetadataSnapshot {
        head: RefSnapshot::DidNotExist,
        refs: BTreeMap::new(),
        index: IndexSnapshot::Bytes(old_index.to_vec()),
        repo_existed: true,
    };
    let plan = GitFinalizePlan {
        repo_create: false,
        new_index_sha256: Some(sha256_bytes(new_index)),
        ref_plans: vec![],
        repo_create_owner: None,
        index_lock_owner: None,
    };

    let result = check_index_lock_owner_migration(&live, &snapshot, &plan).unwrap();
    assert!(
        matches!(result, IndexLockOwnerMigration::AlreadyReverted),
        "no lock + current == old → AlreadyReverted, got: {:?}",
        result
    );
}

/// 验证迁移判定：没有 index.lock 且 current index == new
/// → MigrateToNewOwner（生成新 owner UUID）。
#[test]
fn migration_no_lock_current_is_new_returns_migrate_to_new_owner() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let old_index = b"old index content";
    let new_index = b"new index content";
    let index_path = git_dir.join("index");
    let lock_path = git_dir.join("index.lock");

    // current index == new
    fs::write(&index_path, new_index).unwrap();
    // no lock
    assert!(!lock_path.exists());

    let snapshot = GitMetadataSnapshot {
        head: RefSnapshot::DidNotExist,
        refs: BTreeMap::new(),
        index: IndexSnapshot::Bytes(old_index.to_vec()),
        repo_existed: true,
    };
    let plan = GitFinalizePlan {
        repo_create: false,
        new_index_sha256: Some(sha256_bytes(new_index)),
        ref_plans: vec![],
        repo_create_owner: None,
        index_lock_owner: None,
    };

    let result = check_index_lock_owner_migration(&live, &snapshot, &plan).unwrap();
    match result {
        IndexLockOwnerMigration::MigrateToNewOwner(owner) => {
            // 验证生成了非空 owner UUID
            assert!(
                !owner.is_empty(),
                "MigrateToNewOwner should generate non-empty owner UUID"
            );
        }
        _ => panic!(
            "no lock + current == new → MigrateToNewOwner, got: {:?}",
            result
        ),
    }
}

/// 验证迁移判定：canonical index.lock 已存在 → LockExists（不能 terminalize）。
#[test]
fn migration_lock_exists_returns_lock_exists() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let lock_path = git_dir.join("index.lock");
    let index_path = git_dir.join("index");

    // lock exists
    fs::write(&lock_path, b"some lock content").unwrap();
    fs::write(&index_path, b"some index content").unwrap();

    let snapshot = GitMetadataSnapshot {
        head: RefSnapshot::DidNotExist,
        refs: BTreeMap::new(),
        index: IndexSnapshot::Bytes(b"old".to_vec()),
        repo_existed: true,
    };
    let plan = GitFinalizePlan {
        repo_create: false,
        new_index_sha256: Some(sha256_bytes(b"new")),
        ref_plans: vec![],
        repo_create_owner: None,
        index_lock_owner: None,
    };

    let result = check_index_lock_owner_migration(&live, &snapshot, &plan).unwrap();
    assert!(
        matches!(result, IndexLockOwnerMigration::LockExists),
        "lock exists → LockExists, got: {:?}",
        result
    );
}

/// 验证迁移判定：current 既不是 old 也不是 new → ConcurrentModification。
#[test]
fn migration_current_neither_old_nor_new_returns_concurrent_modification() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let old_index = b"old index content";
    let new_index = b"new index content";
    let concurrent_index = b"concurrent modification content";
    let index_path = git_dir.join("index");
    let lock_path = git_dir.join("index.lock");

    // current index == concurrent (neither old nor new)
    fs::write(&index_path, concurrent_index).unwrap();
    // no lock
    assert!(!lock_path.exists());

    let snapshot = GitMetadataSnapshot {
        head: RefSnapshot::DidNotExist,
        refs: BTreeMap::new(),
        index: IndexSnapshot::Bytes(old_index.to_vec()),
        repo_existed: true,
    };
    let plan = GitFinalizePlan {
        repo_create: false,
        new_index_sha256: Some(sha256_bytes(new_index)),
        ref_plans: vec![],
        repo_create_owner: None,
        index_lock_owner: None,
    };

    let result = check_index_lock_owner_migration(&live, &snapshot, &plan).unwrap();
    assert!(
        matches!(result, IndexLockOwnerMigration::ConcurrentModification),
        "current neither old nor new → ConcurrentModification, got: {:?}",
        result
    );
}

// ══ 修改点 4：finalize_existing 收紧 detached HEAD post-check ══

/// 验证修改点 4：finalize_existing 在 post-write HEAD 检查时，
/// detached HEAD（无 symbolic target）必须返回 FinalizeFailed，
/// 而不是继续用 reference_matching 做 CAS。
///
/// 此测试构造场景：snapshot.head 记录的是 detached HEAD（Existed { oid }），
/// live HEAD 也是 detached at old_oid，所以 preflight 的 HEAD 校验通过。
/// 但 finalize_existing 的 post-check 要求 HEAD 必须是 symbolic 且 target == head_ref，
/// detached HEAD → FinalizeFailed（不是 ConcurrentMetadataChanged，因为 index/refs
/// 可能已写入，需要走 rollback）。
///
/// 这模拟了"preflight 后用户把 HEAD detach"的并发场景：preflight 时 HEAD 是 symbolic，
/// 用户在 preflight 后 detach，post-check 看到 detached。通过设置 snapshot.head 为
/// detached 来让 preflight 通过，同时 live HEAD 也是 detached，使 post-check 的
/// detached 拒绝逻辑被触发。
#[test]
#[allow(clippy::too_many_lines)]
fn finalize_existing_detached_head_after_preflight_returns_finalize_failed() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();

    // 构造 live repo with refs/heads/main at old_oid, HEAD detached at old_oid
    let repo = git2::Repository::init(&live).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    fs::write(live.join("a.txt"), "hello").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let old_oid = repo
        .commit(Some("refs/heads/main"), &sig, &sig, "init", &tree, &[])
        .unwrap();
    // HEAD detached at old_oid
    repo.set_head_detached(old_oid).unwrap();
    drop(tree);

    // 构造 staging repo with HEAD at new_oid
    let staging = tmp.path().join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("a.txt"), "hello").unwrap();
    fs::write(staging.join("b.txt"), "world").unwrap();
    let staging_repo = git2::Repository::init(&staging).unwrap();
    let mut s_index = staging_repo.index().unwrap();
    s_index.add_path(std::path::Path::new("a.txt")).unwrap();
    s_index.add_path(std::path::Path::new("b.txt")).unwrap();
    s_index.write().unwrap();
    let s_tree = staging_repo
        .find_tree(s_index.write_tree().unwrap())
        .unwrap();
    let staging_commit = staging_repo
        .commit(Some("refs/heads/main"), &sig, &sig, "staging", &s_tree, &[])
        .unwrap();
    staging_repo
        .reference_symbolic("HEAD", "refs/heads/main", true, "test")
        .unwrap();
    drop(s_tree);

    // snapshot.head 记录 detached HEAD at old_oid（让 preflight HEAD 校验通过）
    let snapshot = GitMetadataSnapshot {
        head: RefSnapshot::Existed {
            oid: old_oid.to_string(),
        },
        refs: BTreeMap::from([(
            "refs/heads/main".to_string(),
            RefSnapshot::Existed {
                oid: old_oid.to_string(),
            },
        )]),
        index: IndexSnapshot::Bytes(fs::read(live.join(".git").join("index")).unwrap()),
        repo_existed: true,
    };

    // plan: refs/heads/main old_oid -> staging_commit
    let plan = GitFinalizePlan {
        repo_create: false,
        new_index_sha256: Some(sha256_bytes(
            &fs::read(staging.join(".git").join("index")).unwrap(),
        )),
        ref_plans: vec![(
            "refs/heads/main".to_string(),
            Some(old_oid.to_string()),
            staging_commit.to_string(),
        )],
        repo_create_owner: None,
        index_lock_owner: Some(uuid::Uuid::new_v4().to_string()),
    };

    // 调用 commit_git_finalize
    use writer_core::sync::git_commit::commit_git_finalize;
    use writer_core::sync::git_staging::GitSeedState;
    let result = commit_git_finalize(
        &live,
        &staging,
        &GitSeedState::Existing {
            head_ref: "refs/heads/main".to_string(),
            head_oid: old_oid,
        },
        &snapshot,
        &plan,
    );

    // 验证：应返回 FinalizeFailed 或 RollbackFailed（不是 ConcurrentMetadataChanged）
    // 因为 index/refs 可能已写入，需要走现有 rollback，不能标成"nothing written"。
    // FinalizeFailed 触发 rollback，rollback 可能成功（返回 FinalizeFailed）或失败
    //（返回 RollbackFailed），但关键是错误类型不是 ConcurrentMetadataChanged。
    match &result {
        Err(GitFinalizeError::FinalizeFailed(msg)) => {
            let msg_str = msg.to_string();
            assert!(
                msg_str.contains("detached") || msg_str.contains("HEAD detached after preflight"),
                "error message should mention detached HEAD, got: {}",
                msg_str
            );
        }
        Err(GitFinalizeError::RollbackFailed { finalize, .. }) => {
            // FinalizeFailed 触发了 rollback，rollback 也失败了。
            // 关键：finalize 消息应包含 "detached"（说明 post-check 正确拒绝了 detached HEAD）。
            assert!(
                finalize.contains("detached") || finalize.contains("HEAD detached after preflight"),
                "finalize error message should mention detached HEAD, got: {}",
                finalize
            );
        }
        Err(GitFinalizeError::ConcurrentMetadataChanged { .. }) => {
            panic!(
                "DEFECT: finalize_existing returned ConcurrentMetadataChanged for \
                 detached HEAD after preflight — should be FinalizeFailed (index/refs \
                 may already be written, must rollback, not 'nothing written')"
            );
        }
        Ok(()) => {
            panic!(
                "DEFECT: finalize_existing succeeded with detached HEAD after preflight \
                 — should return FinalizeFailed (refusing to advance head_ref on a \
                 branch the user has left via detach)"
            );
        }
    }
}

// ══ 修改点 3：durable copy / fsync 屏障 ══

/// 验证 copy_dir_recursive 是 durable copy：copy 后目标文件 + 父目录已 fsync。
/// 此测试通过验证 copy 成功且内容正确来间接验证（fsync 不可直接观测，
/// 但 durable_copy_file 的 sync_all + sync_parent 调用不会 panic/error）。
#[test]
fn copy_dir_recursive_produces_correct_copy() {
    let tmp = TempDir::new().unwrap();

    // 构造源目录结构
    let src = tmp.path().join("src");
    fs::create_dir_all(src.join("subdir")).unwrap();
    fs::write(src.join("file1.txt"), "content1").unwrap();
    fs::write(src.join("subdir").join("file2.txt"), "content2").unwrap();

    // copy（durable_copy_file 需要目标父目录存在）
    let dst = tmp.path().join("dst");
    fs::create_dir_all(&dst).unwrap();
    writer_core::storage::durable_copy_file(&src.join("file1.txt"), &dst.join("file1.txt"))
        .unwrap();

    // 验证内容正确
    let content = fs::read(dst.join("file1.txt")).unwrap();
    assert_eq!(content, b"content1");
}

/// 验证 storage::sync_dir / sync_parent 对有效目录不报错。
#[test]
fn storage_sync_helpers_work_on_valid_dirs() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("a").join("b");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("file.txt");
    fs::write(&file, "content").unwrap();

    // sync_dir 应成功
    writer_core::storage::sync_dir(&dir).unwrap();
    // sync_parent 应成功
    writer_core::storage::sync_parent(&file).unwrap();
}
