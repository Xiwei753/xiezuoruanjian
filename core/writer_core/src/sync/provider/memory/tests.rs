use super::*;

#[test]
fn write_and_read_roundtrip() {
    let p = MemoryProvider::new();
    let v = p
        .write("a/b.md", b"hello", WritePrecondition::CreateNew)
        .unwrap();
    assert!(!v.as_str().is_empty());
    let obj = p.read("a/b.md").unwrap().unwrap();
    assert_eq!(obj.content, b"hello");
    assert_eq!(obj.version, v);
}

#[test]
fn create_new_rejects_existing() {
    let p = MemoryProvider::new();
    p.write("x", b"1", WritePrecondition::CreateNew).unwrap();
    let err = p
        .write("x", b"2", WritePrecondition::CreateNew)
        .unwrap_err();
    assert!(matches!(err, ProviderError::PreconditionFailed { .. }));
}

#[test]
fn if_match_rejects_stale() {
    let p = MemoryProvider::new();
    let v1 = p.write("x", b"1", WritePrecondition::CreateNew).unwrap();
    let _v2 = p
        .write("x", b"2", WritePrecondition::IfMatch(v1.clone()))
        .unwrap();
    // 用旧版本写入应失败
    let err = p
        .write("x", b"3", WritePrecondition::IfMatch(v1))
        .unwrap_err();
    assert!(matches!(err, ProviderError::PreconditionFailed { .. }));
}

#[test]
fn list_strips_prefix() {
    let p = MemoryProvider::with_entries([
        ("projects/p1/a.md".to_string(), b"a".to_vec()),
        ("projects/p1/b.md".to_string(), b"b".to_vec()),
        ("projects/p2/c.md".to_string(), b"c".to_vec()),
    ]);
    let entries = p.list("projects/p1").unwrap();
    let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, vec!["a.md", "b.md"]);
}

#[test]
fn list_empty_prefix_returns_all() {
    let p = MemoryProvider::with_entries([
        ("a".to_string(), b"".to_vec()),
        ("b".to_string(), b"".to_vec()),
    ]);
    let entries = p.list("").unwrap();
    assert_eq!(entries.len(), 2);
}

#[test]
fn delete_with_precondition() {
    let p = MemoryProvider::new();
    let v = p.write("x", b"1", WritePrecondition::CreateNew).unwrap();
    p.delete("x", DeletePrecondition::IfMatch(v)).unwrap();
    assert!(p.read("x").unwrap().is_none());
}

// ===== Unconditional 契约测试 =====
//
// Unconditional 语义：Provider 自己处理存在性，直接覆盖/删除。
// 对 MemoryProvider 而言没有 SHA 约束，Unconditional 直接写入/删除即可。
// 这组测试同时作为 GitHubProvider Unconditional 契约的语义参照：
// GitHub 的 Unconditional 应表现为"存在则覆盖、不存在则创建；删除幂等"。

#[test]
fn unconditional_write_overwrites_existing() {
    let p = MemoryProvider::new();
    let v1 = p.write("x", b"old", WritePrecondition::CreateNew).unwrap();
    // Unconditional 覆盖已存在对象，不检查版本。
    let v2 = p
        .write("x", b"new", WritePrecondition::Unconditional)
        .unwrap();
    assert_ne!(v1, v2);
    let obj = p.read("x").unwrap().unwrap();
    assert_eq!(obj.content, b"new");
    assert_eq!(obj.version, v2);
}

#[test]
fn unconditional_write_creates_when_absent() {
    let p = MemoryProvider::new();
    // Unconditional 写入不存在的对象 = 创建。
    let v = p
        .write("y", b"fresh", WritePrecondition::Unconditional)
        .unwrap();
    let obj = p.read("y").unwrap().unwrap();
    assert_eq!(obj.content, b"fresh");
    assert_eq!(obj.version, v);
}

#[test]
fn unconditional_delete_existing_succeeds() {
    let p = MemoryProvider::new();
    p.write("z", b"1", WritePrecondition::CreateNew).unwrap();
    // Unconditional 删除已存在对象，不检查版本。
    p.delete("z", DeletePrecondition::Unconditional).unwrap();
    assert!(p.read("z").unwrap().is_none());
}

#[test]
fn unconditional_delete_missing_is_idempotent() {
    let p = MemoryProvider::new();
    // Unconditional 删除不存在的对象 = 幂等成功（无需删除）。
    p.delete("missing", DeletePrecondition::Unconditional)
        .unwrap();
    assert!(p.read("missing").unwrap().is_none());
}

#[test]
fn unconditional_write_does_not_reject_on_version_change() {
    // 与 IfMatch 对比：IfMatch 用旧版本会失败，Unconditional 总是成功。
    let p = MemoryProvider::new();
    let v1 = p.write("k", b"1", WritePrecondition::CreateNew).unwrap();
    let _v2 = p
        .write("k", b"2", WritePrecondition::IfMatch(v1.clone()))
        .unwrap();
    // 此时远端版本已是 _v2，用 v1 走 IfMatch 会失败，但 Unconditional 应成功。
    p.write("k", b"3", WritePrecondition::Unconditional)
        .unwrap();
    let obj = p.read("k").unwrap().unwrap();
    assert_eq!(obj.content, b"3");
}

#[test]
fn create_new_does_not_query_old_sha_and_rejects_existing() {
    // CreateNew 语义：不查旧 SHA；对象已存在直接返回 PreconditionFailed。
    let p = MemoryProvider::new();
    p.write("c", b"1", WritePrecondition::CreateNew).unwrap();
    let err = p
        .write("c", b"2", WritePrecondition::CreateNew)
        .unwrap_err();
    assert!(matches!(err, ProviderError::PreconditionFailed { .. }));
}

#[test]
fn if_match_does_not_refresh_sha_on_conflict() {
    // IfMatch 语义：严格使用调用方给的版本，冲突时返回 PreconditionFailed，
    // 绝不刷新 SHA 后继续写。这里用旧版本连续两次写入都应失败。
    let p = MemoryProvider::new();
    let v1 = p.write("m", b"1", WritePrecondition::CreateNew).unwrap();
    let _v2 = p
        .write("m", b"2", WritePrecondition::IfMatch(v1.clone()))
        .unwrap();
    let err1 = p
        .write("m", b"3", WritePrecondition::IfMatch(v1.clone()))
        .unwrap_err();
    assert!(matches!(err1, ProviderError::PreconditionFailed { .. }));
    // 再次用同一旧版本尝试，仍应失败（没有"刷新 SHA 重试"的旁路）。
    let err2 = p
        .write("m", b"4", WritePrecondition::IfMatch(v1))
        .unwrap_err();
    assert!(matches!(err2, ProviderError::PreconditionFailed { .. }));
    // 远端内容仍是 _v2 对应的 b"2"，未被旧版本写入改写。
    let obj = p.read("m").unwrap().unwrap();
    assert_eq!(obj.content, b"2");
}

#[test]
fn capabilities_are_memory() {
    let p = MemoryProvider::new();
    let cap = p.capabilities();
    assert!(cap.atomic_write);
    assert!(cap.batch);
    assert!(!cap.server_timestamp);
}
