//! 同步哈希语义集中模块。
//!
//! 把同步里的哈希语义集中到一个文件，区分两种哈希：
//! - **MD5 内容哈希**（32位 hex）：当前同步系统使用的内容哈希，写入 manifest 的
//!   `content_hash` 和 `SyncState.known_files`。
//! - **Git blob OID**（40位 hex）：旧版本同步系统误写入 `known_files` 的哈希，
//!   现在只在归一化时用来识别旧基线。
//!
//! ## 为什么集中
//!
//! 旧版本 `sync::utils::compute_git_hash` 在 `git2::Oid::hash_object` 失败时偷偷
//! 退成 MD5，导致 `known_files` 里混入两种哈希。三路比较里 local/remote/base
//! 不是同一种内容哈希时，空内容会被误判为 `BothChanged` 冲突（remote MD5
//! `d41d8cd98f00b204e9800998ecf8427e` vs base Git blob OID
//! `e69de29bb2d1d6434b8b29ae775ad8c2e48c5391`，都是"空正文"但哈希不同）。
//!
//! 本模块提供明确的函数名和失败语义，不再有偷偷 fallback。`git_blob_oid` 失败
//! 返回 `Err`，调用方必须显式处理。

use std::collections::HashMap;
use std::path::Path;

/// 计算内容的 MD5 hex（32位）。
///
/// 这是当前同步系统使用的内容哈希，写入 manifest 的 `content_hash` 和
/// `SyncState.known_files`。
pub(crate) fn content_md5(bytes: &[u8]) -> String {
    format!("{:x}", md5::compute(bytes))
}

/// 读取文件并计算 MD5 hex（32位）。
pub(crate) fn content_md5_file(path: &Path) -> std::io::Result<String> {
    let content = std::fs::read(path)?;
    Ok(content_md5(&content))
}

/// 计算内容的 Git blob OID（40位 hex）。
///
/// 名字明确写 blob oid，与 MD5 内容哈希区分。失败返回 `Err`，**不**偷偷退成
/// MD5（旧 `compute_git_hash` 的 fallback 行为是 bug 的根源）。
///
/// `git2` 在 `core/writer_core/Cargo.toml` 是无条件依赖
/// （`default-features = false, features = ["vendored-libgit2"]`），
/// `Oid::hash_object` 不需要 HTTPS feature，因此本函数始终编译。
pub(crate) fn git_blob_oid(bytes: &[u8]) -> crate::Result<String> {
    git2::Oid::hash_object(git2::ObjectType::Blob, bytes)
        .map(|oid| oid.to_string())
        .map_err(|e| crate::Error::Other(format!("git_blob_oid: hash_object failed: {e}")))
}

/// 判断是否为 MD5 内容哈希（32位 hex）。
///
/// 32 个 ASCII hex 字符（`0-9` / `a-f` / `A-F`）。大小写不敏感因为 MD5 hex
/// 输出通常是小写，但旧数据可能大写。
pub(crate) fn is_md5_content_hash(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 判断是否为旧 Git blob OID（40位 hex）。
///
/// 40 个 ASCII hex 字符。用于识别 `SyncState.known_files` 里误存的旧 Git blob
/// OID，归一化时转成 MD5。
pub(crate) fn is_legacy_git_blob_oid(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 归一化旧基线哈希。
///
/// 旧版本同步系统可能把 Git blob OID（40位 hex）误写入 `SyncState.known_files`，
/// 而当前同步系统使用 MD5（32位 hex）。三路比较里 local/remote/base 不是同一种
/// 内容哈希会导致空内容被误判为 `BothChanged` 冲突。
///
/// 本函数只在能够证明旧 base 与当前一侧内容相同时转换成该侧的 MD5：
///
/// 1. **优先比较远端**：`remote_tree_files[path]`（远端 blob OID）等于旧 base
///    → 远端内容没变，把 base 改成 `remote_content_hash`（MD5）。
/// 2. **远端不匹配时再比较本地**：计算当前本地正文的 Git blob OID，等于旧 base
///    → 本地内容没变，把 base 改成 `local_content_opt`（MD5）。
/// 3. **两边都无法证明时保持原值**，不猜。
///
/// # 参数
///
/// - `base_hash`：旧基线哈希（可能是 MD5 或 Git blob OID）。
/// - `remote_content_hash`：远端当前正文的 MD5（来自 `remote_records[path].content_hash`）。
/// - `local_content_opt`：本地当前正文的 MD5（来自 `local_records[path].content_hash`）。
/// - `remote_tree_files`：远端 tree 文件（path → blob OID）。
/// - `path`：要归一化的路径。
/// - `sync_root`：本地同步根（用于读本地文件算 Git blob OID）。
///
/// # feature gate
///
/// `git2` 在 `core/writer_core/Cargo.toml` 是无条件依赖，`Oid::hash_object` 不需要
/// HTTPS feature，因此本函数始终能计算本地 Git blob OID（步骤 1 + 步骤 2 都可用）。
pub(crate) fn normalize_legacy_base_hash(
    base_hash: &str,
    remote_content_hash: &str,
    local_content_opt: Option<&str>,
    remote_tree_files: &HashMap<String, String>,
    path: &str,
    sync_root: &Path,
) -> String {
    // 只处理旧 Git blob OID（40位 hex）；MD5 或其他格式直接返回。
    if !is_legacy_git_blob_oid(base_hash) {
        return base_hash.to_string();
    }

    // 1. 优先比较远端 blob OID 与旧 base。
    //    remote_tree_files[path] 是远端 tree 的 blob SHA（Git blob OID），
    //    字符串比较即可，不需要 git_blob_oid 函数。
    if let Some(remote_blob_oid) = remote_tree_files.get(path) {
        if remote_blob_oid == base_hash {
            // 远端 blob OID == 旧 base → 远端内容没变，base 改成远端 MD5。
            return remote_content_hash.to_string();
        }
    }

    // 2. 远端不匹配时再计算当前本地正文的 Git blob OID。
    normalize_via_local_blob_oid(base_hash, local_content_opt, path, sync_root)
}

/// 通过本地 Git blob OID 归一化。
///
/// 读本地文件算 Git blob OID，与旧 base 比较：相等则把 base 改成本地 MD5，
/// 否则保持原值。`git2` 无条件依赖，不需要 feature gate。
fn normalize_via_local_blob_oid(
    base_hash: &str,
    local_content_opt: Option<&str>,
    path: &str,
    sync_root: &Path,
) -> String {
    let Some(local_md5) = local_content_opt else {
        return base_hash.to_string();
    };
    // 读本地文件失败或算 blob OID 失败时无法证明，保持原值。
    // 不用 unwrap/expect 处理外部输入（AGENTS.md Rust 安全边界）。
    let local_full = sync_root.join(path);
    let Ok(local_content) = std::fs::read(&local_full) else {
        return base_hash.to_string();
    };
    let Ok(local_blob_oid) = git_blob_oid(&local_content) else {
        return base_hash.to_string();
    };
    if local_blob_oid == base_hash {
        // 本地 blob OID == 旧 base → 本地内容没变，base 改成本地 MD5。
        local_md5.to_string()
    } else {
        // 3. 两边都无法证明时保持原值，不猜。
        base_hash.to_string()
    }
}

#[cfg(test)]
mod tests {
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
}
