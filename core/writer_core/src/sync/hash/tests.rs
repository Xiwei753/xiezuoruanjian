//! hash.rs 的单元测试。
//!
//! 按仓库结构门禁（`tools/check_source_structure.py` 的
//! production-test-bloat 规则）从生产文件拆出，测试逻辑保持不变。
use super::*;

#[test]
fn test_content_md5_empty() {
    // 空内容的 MD5：d41d8cd98f00b204e9800998ecf8427e
    assert_eq!(content_md5(b""), "d41d8cd98f00b204e9800998ecf8427e");
}

#[test]
fn test_content_md5_hello_world() {
    assert_eq!(
        content_md5(b"hello world"),
        "5eb63bbbe01eeed093cb22bb8f5acdc3"
    );
}

#[test]
fn test_is_md5_content_hash() {
    assert!(is_md5_content_hash("d41d8cd98f00b204e9800998ecf8427e"));
    assert!(!is_md5_content_hash(
        "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
    ));
    assert!(!is_md5_content_hash(""));
    assert!(!is_md5_content_hash("short"));
    assert!(!is_md5_content_hash("g41d8cd98f00b204e9800998ecf8427e")); // 非 hex
}

#[test]
fn test_is_legacy_git_blob_oid() {
    assert!(is_legacy_git_blob_oid(
        "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
    ));
    assert!(!is_legacy_git_blob_oid("d41d8cd98f00b204e9800998ecf8427e"));
    assert!(!is_legacy_git_blob_oid(""));
    assert!(!is_legacy_git_blob_oid("short"));
}

#[test]
fn test_normalize_legacy_base_hash_md5_base_unchanged() {
    // base 已经是 MD5 → 直接返回，不做归一化。
    let remote_tree = HashMap::new();
    let result = normalize_legacy_base_hash(
        "d41d8cd98f00b204e9800998ecf8427e",
        "abc123",
        None,
        &remote_tree,
        "foo.md",
        Path::new("/tmp"),
    );
    assert_eq!(result, "d41d8cd98f00b204e9800998ecf8427e");
}

#[test]
fn test_normalize_legacy_base_hash_remote_matches() {
    // 远端 blob OID == 旧 base → 改成远端 MD5。
    let mut remote_tree = HashMap::new();
    remote_tree.insert(
        "foo.md".to_string(),
        "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391".to_string(),
    );
    let result = normalize_legacy_base_hash(
        "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
        "d41d8cd98f00b204e9800998ecf8427e",
        None,
        &remote_tree,
        "foo.md",
        Path::new("/tmp"),
    );
    assert_eq!(result, "d41d8cd98f00b204e9800998ecf8427e");
}

#[test]
fn test_normalize_legacy_base_hash_remote_mismatch_keep_original() {
    // 远端 blob OID != 旧 base，且无 local_content_opt → 保持原值。
    let mut remote_tree = HashMap::new();
    remote_tree.insert(
        "foo.md".to_string(),
        "0000000000000000000000000000000000000000".to_string(),
    );
    let result = normalize_legacy_base_hash(
        "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
        "d41d8cd98f00b204e9800998ecf8427e",
        None,
        &remote_tree,
        "foo.md",
        Path::new("/tmp"),
    );
    assert_eq!(result, "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
}

#[test]
fn test_git_blob_oid_empty() {
    // 空内容的 Git blob OID：e69de29bb2d1d6434b8b29ae775ad8c2e48c5391
    assert_eq!(
        git_blob_oid(b"").unwrap(),
        "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
    );
}

#[test]
fn test_git_blob_oid_hello_world() {
    assert_eq!(
        git_blob_oid(b"hello world").unwrap(),
        "95d09f2b10159347eece71399a7e2e907ea3df4f"
    );
}
