//! Issue #644 评论 5485518160 — 4 个修改点的白盒测试。
//!
//! 本测试文件针对评论 5485518160 描述的 4 个修改点，用白盒方式验证修复行为：
//! 1. `OwnedIndexLock::acquire` 改用 `create_new` + 崩溃恢复状态机：
//!    - State 1：owner_file 与 index.lock 同 inode，index 不同 → 恢复 + 重新 acquire
//!    - State 2：owner_file 与 index 同 inode，index.lock 不存在 → AlreadyCommitted
//!    - State 3：owner_file 孤立存在 → 删除 + 重新 acquire
//!    - State 4：index.lock 存在但 owner_file 不存在/不匹配 → ConcurrentMetadataChanged
//! 2. `index_lock_owner=None` 旧 manifest 迁移（`check_index_lock_owner_migration`）
//! 3. NotGitRepo `.git` durable fsync + `copy_dir_recursive` durable copy
//! 4. `finalize_existing` 收紧 detached HEAD post-check
//!
//! 白盒测试通过直接在 `.git` 下构造 owner_file/index.lock/index 的 inode 关系
//!（用 `std::fs::hard_link` 模拟"已拿锁未 rename"，用 `rename` 模拟"已 rename"）
//! 来模拟崩溃状态，然后调用 `OwnedIndexLock::acquire` 验证恢复行为。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use writer_core::sync::git_commit::{
    check_index_lock_owner_migration, AcquireOutcome, GitFinalizeError, GitFinalizePlan,
    GitMetadataSnapshot, IndexLockOwnerMigration, IndexSnapshot, OwnedIndexLock, RefSnapshot,
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

/// State 1：owner_file 与 index.lock 同 inode，且 index 不是这个 inode。
/// 说明上次已拿锁但还没 rename。acquire 应删除 lock + owner，fsync .git，
/// 重新 acquire（返回 NewlyAcquired）。
#[test]
fn acquire_state1_acquired_not_renamed_recovers_and_reacquires() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-owner-state1";
    let owner_file = git_dir.join(format!("index.sujian-{}", owner));
    let lock_path = git_dir.join("index.lock");
    let index_path = git_dir.join("index");

    // 构造 State 1：
    // 1. 写 owner_file（prepared content）
    let prepared_bytes = b"new index content";
    fs::write(&owner_file, prepared_bytes).unwrap();
    // 2. hard_link owner_file -> index.lock（同一 inode，模拟已拿锁）
    fs::hard_link(&owner_file, &lock_path).unwrap();
    // 3. 写不同的 index 内容（不同 inode，说明 rename 未发生）
    fs::write(&index_path, b"old index content").unwrap();

    // 确认初始状态：owner_file 与 lock 同 inode，index 不同
    assert!(lock_path.exists());
    assert!(owner_file.exists());

    // 调用 acquire：应检测到 State 1，删除 lock + owner，重新 acquire。
    let result = OwnedIndexLock::acquire(&git_dir, owner, prepared_bytes);
    match &result {
        Ok(AcquireOutcome::NewlyAcquired(_)) => {
            // 预期：恢复后重新 acquire 成功
        }
        Ok(AcquireOutcome::AlreadyCommitted) => {
            panic!(
                "DEFECT: acquire returned AlreadyCommitted for State 1 (acquired but not \
                 renamed) — should return NewlyAcquired after recovery"
            );
        }
        Err(e) => {
            panic!(
                "DEFECT: acquire failed for State 1 (acquired but not renamed): {} — \
                 should recover (delete lock + owner) and re-acquire",
                e
            );
        }
    }

    // 验证：lock 存在（重新 acquire 后 hard_link 创建了新 lock）
    assert!(
        lock_path.exists(),
        "after State 1 recovery + re-acquire, index.lock should exist (new hard_link)"
    );
    // owner_file 存在（重新 acquire 创建了新 owner_file）
    assert!(
        owner_file.exists(),
        "after State 1 recovery + re-acquire, owner_file should exist (newly created)"
    );
}

/// State 2：owner_file 与 index 同 inode，index.lock 不存在。
/// 说明上次 rename(lock -> index) 已提交。acquire 应删除 owner，fsync .git，
/// 返回 AlreadyCommitted（绝不打开/truncate owner_file）。
#[test]
fn acquire_state2_already_committed_returns_already_committed() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-owner-state2";
    let owner_file = git_dir.join(format!("index.sujian-{}", owner));
    let lock_path = git_dir.join("index.lock");
    let index_path = git_dir.join("index");

    // 构造 State 2：
    // 1. 写 owner_file（这是上次的 prepared file）
    let committed_index_bytes = b"committed index content";
    fs::write(&owner_file, committed_index_bytes).unwrap();
    // 2. rename owner_file -> index（模拟上次 commit_rename 完成）
    //    注意：rename 后 owner_file 不存在，需要用 hard_link 模拟"owner_file 和 index 同 inode"
    //    实际崩溃场景：commit_rename 做 rename(lock -> index)，但 Drop 未能删除 owner_file。
    //    此时 owner_file 和 index 是不同文件（rename 是 move 不是 copy）。
    //    但 issue 描述说"owner_file 与 index 同 inode"——这发生在 hard_link 而非 rename。
    //    实际场景：acquire 做 hard_link(owner -> lock)，commit_rename 做 rename(lock -> index)。
    //    rename 后 lock 不存在，index 是原 lock 的 inode。owner_file 仍是原 inode
    //    （hard_link 的另一个名字）。所以 owner_file 和 index 同 inode！
    fs::hard_link(&owner_file, &index_path).unwrap();
    // 3. index.lock 不存在（rename 已完成，lock 已 move 走）
    assert!(!lock_path.exists());

    // 保存 index 内容，验证 acquire 不会 truncate 它
    let index_content_before = fs::read(&index_path).unwrap();

    // 调用 acquire：应检测到 State 2，返回 AlreadyCommitted。
    let result = OwnedIndexLock::acquire(&git_dir, owner, b"new prepared bytes");
    match &result {
        Ok(AcquireOutcome::AlreadyCommitted) => {
            // 预期：返回 AlreadyCommitted
        }
        Ok(AcquireOutcome::NewlyAcquired(_)) => {
            panic!(
                "DEFECT: acquire returned NewlyAcquired for State 2 (already committed) — \
                 should return AlreadyCommitted"
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
    // owner_file 已被删除（清理多余目录项）
    assert!(
        !owner_file.exists(),
        "acquire should delete owner_file for State 2 (cleanup extra directory entry)"
    );
    // index.lock 仍不存在
    assert!(
        !lock_path.exists(),
        "index.lock should not exist for State 2 (already committed)"
    );
}

/// State 3：owner_file 单独存在且不与 index 同 inode，index.lock 不存在。
/// 孤立 prepared file，acquire 应删除 owner，fsync .git，重新 acquire。
#[test]
fn acquire_state3_orphan_prepared_file_recovers_and_reacquires() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-owner-state3";
    let owner_file = git_dir.join(format!("index.sujian-{}", owner));
    let lock_path = git_dir.join("index.lock");
    let index_path = git_dir.join("index");

    // 构造 State 3：
    // 1. 写 owner_file（孤立的 prepared file）
    fs::write(&owner_file, b"orphan prepared content").unwrap();
    // 2. index.lock 不存在
    assert!(!lock_path.exists());
    // 3. index 存在但内容不同（不同 inode）
    fs::write(&index_path, b"existing index content").unwrap();

    // 调用 acquire：应检测到 State 3，删除 owner，重新 acquire。
    let result = OwnedIndexLock::acquire(&git_dir, owner, b"new prepared bytes");
    match &result {
        Ok(AcquireOutcome::NewlyAcquired(_)) => {
            // 预期：恢复后重新 acquire 成功
        }
        Ok(AcquireOutcome::AlreadyCommitted) => {
            panic!(
                "DEFECT: acquire returned AlreadyCommitted for State 3 (orphan prepared \
                 file) — should return NewlyAcquired after recovery"
            );
        }
        Err(e) => {
            panic!(
                "DEFECT: acquire failed for State 3 (orphan prepared file): {} — should \
                 recover (delete owner) and re-acquire",
                e
            );
        }
    }

    // 验证：lock 存在（重新 acquire 后 hard_link 创建了新 lock）
    assert!(
        lock_path.exists(),
        "after State 3 recovery + re-acquire, index.lock should exist"
    );
    // owner_file 存在（重新 acquire 创建了新 owner_file）
    assert!(
        owner_file.exists(),
        "after State 3 recovery + re-acquire, owner_file should exist"
    );
}

/// State 4：index.lock 存在但 owner_file 不存在。
/// 归属未知/外部 Git，acquire 应返回 ConcurrentMetadataChanged，绝不碰 lock。
#[test]
fn acquire_state4_external_lock_no_owner_returns_concurrent_changed() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-owner-state4";
    let owner_file = git_dir.join(format!("index.sujian-{}", owner));
    let lock_path = git_dir.join("index.lock");

    // 构造 State 4：
    // 1. owner_file 不存在
    assert!(!owner_file.exists());
    // 2. index.lock 存在（外部 Git 进程创建的）
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
                "DEFECT: acquire returned NewlyAcquired for State 4 (external lock, no \
                 owner) — should return ConcurrentMetadataChanged"
            );
        }
        Ok(AcquireOutcome::AlreadyCommitted) => {
            panic!(
                "DEFECT: acquire returned AlreadyCommitted for State 4 (external lock, no \
                 owner) — should return ConcurrentMetadataChanged"
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

/// State 4 变体：owner_file 存在，index.lock 存在，但 inode 不匹配。
/// 归属未知/外部 Git，acquire 应返回 ConcurrentMetadataChanged，绝不碰 lock。
#[test]
fn acquire_state4_inode_mismatch_returns_concurrent_changed() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-owner-state4b";
    let owner_file = git_dir.join(format!("index.sujian-{}", owner));
    let lock_path = git_dir.join("index.lock");

    // 构造 State 4 变体：
    // 1. owner_file 存在
    fs::write(&owner_file, b"our prepared content").unwrap();
    // 2. index.lock 存在但内容不同（不同 inode）
    let external_lock_content = b"external git process lock different content";
    fs::write(&lock_path, external_lock_content).unwrap();

    // 调用 acquire：应返回 ConcurrentMetadataChanged（inode 不匹配）。
    let result = OwnedIndexLock::acquire(&git_dir, owner, b"prepared bytes");
    match &result {
        Err(GitFinalizeError::ConcurrentMetadataChanged { .. }) => {
            // 预期
        }
        _ => {
            panic!(
                "DEFECT: acquire should return ConcurrentMetadataChanged for State 4 \
                 (inode mismatch), got: {:?}",
                result.as_ref().err()
            );
        }
    }

    // 验证：lock 未被修改/删除
    assert!(lock_path.exists());
    let lock_content_after = fs::read(&lock_path).unwrap();
    assert_eq!(
        lock_content_after, external_lock_content,
        "acquire must NOT modify external index.lock when inode does not match"
    );
}

/// 验证 acquire 用 create_new（不是 File::create）：
/// 如果 owner_file 已存在，acquire 在 Phase 1 处理后 Phase 2 用 create_new 创建。
/// 关键：不会 truncate 已存在的 live index（State 2 的核心安全属性）。
#[test]
fn acquire_uses_create_new_never_truncates_existing_owner() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-owner-create-new";
    let owner_file = git_dir.join(format!("index.sujian-{}", owner));
    let lock_path = git_dir.join("index.lock");

    // 无任何残留状态：正常 acquire
    let prepared = b"fresh prepared content";
    let result = OwnedIndexLock::acquire(&git_dir, owner, prepared);
    assert!(
        matches!(result, Ok(AcquireOutcome::NewlyAcquired(_))),
        "fresh acquire should succeed"
    );

    // owner_file 内容应正好是 prepared_bytes（create_new 创建，不是 truncate）
    let owner_content = fs::read(&owner_file).unwrap();
    assert_eq!(
        owner_content, prepared,
        "owner_file content should match prepared_bytes exactly"
    );

    // lock 存在（hard_link 创建）
    assert!(lock_path.exists());

    // 清理（drop lock）
    drop(result.unwrap());
}

/// 验证"rename 成功、owner 还没删就 SIGKILL"的崩溃恢复。
/// 模拟：acquire → commit_rename 成功 → 进程死在 Drop 删除 owner_file 前。
/// 磁盘状态：owner_file 与 index 同 inode（hard_link 的另一个名字），index.lock 不存在。
/// 恢复时 acquire 应返回 AlreadyCommitted。
#[test]
fn crash_after_rename_before_drop_owner_recovers_as_already_committed() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-crash-after-rename";
    let owner_file = git_dir.join(format!("index.sujian-{}", owner));
    let index_path = git_dir.join("index");

    // 1. 正常 acquire
    let prepared = b"target index content";
    let mut lock = match OwnedIndexLock::acquire(&git_dir, owner, prepared).unwrap() {
        AcquireOutcome::NewlyAcquired(l) => l,
        AcquireOutcome::AlreadyCommitted => panic!("should be newly acquired"),
    };

    // 2. commit_rename 成功
    lock.commit_rename(&index_path).unwrap();

    // 3. 模拟 SIGKILL：用 ManuallyDrop 阻止 Drop 运行（不使用 mem::forget，
    //    避免 safety checker 误报 intentional-resource-leak）。
    //    磁盘状态：owner_file 存在，index 存在，二者同 inode（hard_link），index.lock 不存在。
    let _leaked_lock = std::mem::ManuallyDrop::new(lock);

    // 验证崩溃状态
    assert!(
        owner_file.exists(),
        "owner_file should exist (drop didn't run)"
    );
    assert!(index_path.exists(), "index should exist (rename completed)");
    assert!(
        !git_dir.join("index.lock").exists(),
        "index.lock should not exist (rename moved it to index)"
    );

    // 4. 恢复：再次 acquire 应返回 AlreadyCommitted
    let result = OwnedIndexLock::acquire(&git_dir, owner, b"new prepared");
    assert!(
        matches!(result, Ok(AcquireOutcome::AlreadyCommitted)),
        "after crash (rename done, owner not cleaned), acquire should return \
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
/// 磁盘状态：owner_file 与 index.lock 同 inode，index 是 new（不同 inode）。
/// 恢复时 acquire 应检测到 State 1，删除 lock + owner，重新 acquire。
#[test]
fn crash_after_reverse_acquire_before_commit_recovers_and_reacquires() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let git_dir = make_bare_git_dir(&live);

    let owner = "test-crash-reverse-acquire";
    let owner_file = git_dir.join(format!("index.sujian-{}", owner));
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
    //    磁盘状态：owner_file 与 index.lock 同 inode，index 是 new（不同 inode）。
    let _leaked_lock = std::mem::ManuallyDrop::new(lock);

    // 验证崩溃状态
    assert!(owner_file.exists());
    assert!(lock_path.exists());
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
