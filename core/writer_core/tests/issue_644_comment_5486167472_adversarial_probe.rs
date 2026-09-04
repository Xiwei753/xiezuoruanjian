//! #644 评论 5486167472 对抗式探针（Type 4 Patch Adversarial）
//!
//! 独立验证新引入的 owner metadata 解析逻辑的边界情况，
//! 以及 fsync-gitdir 实际启用（ensure_initialized 返回 Ok）。
//! 这些测试不依赖 acquire() 的状态机，直接探针 lock_belongs_to_owner / owner_metadata
//! 的纯函数行为，覆盖：空输入、非 UTF-8、错误 marker、错误 owner、round-trip。

use writer_core::storage::workspace_git::{lock_belongs_to_owner, owner_metadata, INDEX_LOCK_MARKER};

/// Probe 1: owner_metadata → lock_belongs_to_owner round-trip 必须成立。
/// 这是对新协议核心不变量的直接验证。
#[test]
fn probe_owner_metadata_round_trip() {
    for owner in [
        "owner-a",
        "test-owner-state1",
        "uuid-1234-abcd",
        "x",
        "中文owner",
    ] {
        let meta = owner_metadata(owner);
        assert!(
            lock_belongs_to_owner(&meta, owner),
            "round-trip failed for owner={:?}",
            owner
        );
    }
}

/// Probe 2: lock_belongs_to_owner 对空内容必须返回 false（外部 Git 空 lock）。
#[test]
fn probe_empty_lock_returns_false() {
    assert!(
        !lock_belongs_to_owner(b"", "any-owner"),
        "empty lock must not belong to any owner (external git empty lock)"
    );
}

/// Probe 3: lock_belongs_to_owner 对非 UTF-8 内容必须返回 false。
#[test]
fn probe_non_utf8_lock_returns_false() {
    let non_utf8 = [0xFF, 0xFE, 0xFD, 0x00];
    assert!(
        !lock_belongs_to_owner(&non_utf8, "any-owner"),
        "non-UTF-8 lock must not belong to any owner"
    );
}

/// Probe 4: lock_belongs_to_owner 对错误 marker 必须返回 false。
/// 外部 Git 的 index.lock 可能含 index 内容（以 Git index header 开头，非我们的 marker）。
#[test]
fn probe_wrong_marker_returns_false() {
    let external_git_lock = b"DIRC\x00\x00\x00\x04some index content";
    assert!(
        !lock_belongs_to_owner(external_git_lock, "any-owner"),
        "lock with wrong marker (external git index content) must not belong to any owner"
    );
    // 仅 marker 行，无 owner 行
    let marker_only = format!("{INDEX_LOCK_MARKER}\n").into_bytes();
    assert!(
        !lock_belongs_to_owner(&marker_only, "any-owner"),
        "lock with only marker line (no owner line) must not belong to any owner"
    );
}

/// Probe 5: lock_belongs_to_owner 对正确 marker 但错误 owner 必须返回 false。
/// 这是不同事务的 lock（同协议但不同 owner），绝不能误判为本轮 lock。
#[test]
fn probe_wrong_owner_returns_false() {
    let meta = owner_metadata("transaction-A");
    assert!(
        !lock_belongs_to_owner(&meta, "transaction-B"),
        "lock with correct marker but different owner must NOT belong to another owner \
         (cross-transaction isolation)"
    );
}

/// Probe 6: owner metadata 格式解析的安全属性。
/// `lock_belongs_to_owner` 用 `str::lines()` 解析（标准 Rust 语义：最后一行不需末尾换行），
/// 这是可接受的宽松解析，因为 `owner_metadata` 总是生成规范格式（带末尾换行）。
/// 关键安全属性：错误 marker、错误 owner、owner 后有空格、marker 前有空行都必须返回 false。
#[test]
fn probe_strict_format_parsing() {
    let owner = "strict-owner";
    // 正确格式（规范）
    let correct = format!("{INDEX_LOCK_MARKER}\nowner={owner}\n");
    assert!(lock_belongs_to_owner(correct.as_bytes(), owner));

    // 缺少末尾换行 — lines() 语义下仍解析为 valid（可接受：owner_metadata 总是生成规范格式）
    let no_trailing_nl = format!("{INDEX_LOCK_MARKER}\nowner={owner}");
    assert!(
        lock_belongs_to_owner(no_trailing_nl.as_bytes(), owner),
        "lines() semantics: missing trailing newline still parses as valid (owner_metadata \
         always generates canonical format with trailing newline, so this is acceptable)"
    );

    // owner 后有空格 — 必须返回 false（严格 owner 匹配，防止 owner=foo 误匹配 owner=foo )
    let with_trailing_space = format!("{INDEX_LOCK_MARKER}\nowner={owner} \n");
    assert!(
        !lock_belongs_to_owner(with_trailing_space.as_bytes(), owner),
        "trailing space after owner must not parse as valid (strict owner match)"
    );

    // 多一个空行在 marker 前 — 必须返回 false（marker 必须是第一行）
    let leading_empty = format!("\n{INDEX_LOCK_MARKER}\nowner={owner}\n");
    assert!(
        !lock_belongs_to_owner(leading_empty.as_bytes(), owner),
        "leading empty line must not parse as valid (marker must be first line)"
    );

    // owner 行前有空格 — 必须返回 false
    let spaced_owner = format!("{INDEX_LOCK_MARKER}\n owner={owner}\n");
    assert!(
        !lock_belongs_to_owner(spaced_owner.as_bytes(), owner),
        "leading space before owner= must not parse as valid"
    );
}

/// Probe 7: fsync-gitdir 实际启用 — ensure_initialized 必须返回 Ok。
/// 如果 libgit2-sys FFI 调用失败，ensure_initialized 会返回 Err。
/// 这是对问题 3 修复的运行时确认（不只是静态 grep）。
#[test]
fn probe_fsync_gitdir_actually_enabled() {
    let result = writer_core::storage::git_runtime::ensure_initialized();
    assert!(
        result.is_ok(),
        "ensure_initialized must return Ok after enabling fsync-gitdir, got: {:?}",
        result.err()
    );
    // 幂等性：多次调用必须返回同一结果
    let result2 = writer_core::storage::git_runtime::ensure_initialized();
    assert_eq!(
        result.is_ok(),
        result2.is_ok(),
        "ensure_initialized must be idempotent"
    );
}
