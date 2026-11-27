//! 两级索引的 namespace 索引（`manifest/index/<ns>.json`）。
//!
//! 索引把 object_key 映射到内容寻址的 objects 文件 sha256。同一命名空间的
//! 索引内容变化才需要重写；未涉及 namespace 的索引保持逐字节不变。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::root::{MANIFEST_SCHEMA_VERSION, MAX_NAMESPACE_LEN};

/// 顶层散文件（路径不足两段）使用的 namespace。
pub(crate) const ROOT_NAMESPACE: &str = "_root";

/// 一个命名空间的 object_key → object 文件 sha256 索引。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManifestIndex {
    /// 存储格式版本，必须等于 [`MANIFEST_SCHEMA_VERSION`]。
    pub schema_version: u32,
    /// 本索引对应的 namespace（必须与文件名一致）。
    pub namespace: String,
    /// object_key → object 文件 sha256。
    #[serde(default)]
    pub objects: BTreeMap<String, String>,
}

impl ManifestIndex {
    pub(crate) fn new(namespace: &str) -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            namespace: namespace.to_string(),
            objects: BTreeMap::new(),
        }
    }

    /// canonical JSON：object_key 按字典序、compact。
    pub(crate) fn canonical_bytes(&self) -> crate::Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(crate::Error::Json)
    }

    /// 校验索引结构；`expected_namespace` 是 root 中列出的 namespace。
    pub(crate) fn validate(&self, origin: &str, expected_namespace: &str) -> crate::Result<()> {
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(crate::Error::Other(format!(
                "manifest index {} has unsupported schemaVersion {} (expected {})",
                origin, self.schema_version, MANIFEST_SCHEMA_VERSION
            )));
        }
        if self.namespace != expected_namespace {
            return Err(crate::Error::Other(format!(
                "manifest index {} declares namespace {} but root references {}",
                origin, self.namespace, expected_namespace
            )));
        }
        for (object_key, sha) in &self.objects {
            if object_key.is_empty() {
                return Err(crate::Error::Other(format!(
                    "manifest index {} contains an empty object key",
                    origin
                )));
            }
            if !super::objects::is_sha256_hex(sha) {
                return Err(crate::Error::Other(format!(
                    "manifest index {} has invalid object sha256 for key {object_key}: {sha}",
                    origin
                )));
            }
        }
        Ok(())
    }
}

/// 路径 → namespace：第一段做安全化（字母数字/`._-`，其它替换 `_`，截 64），
/// 顶层散文件用 [`ROOT_NAMESPACE`]。
pub(crate) fn namespace_for_path(path: &str) -> String {
    let top = match path.split_once('/') {
        Some((top, _)) => top,
        None => return ROOT_NAMESPACE.to_string(),
    };
    sanitize_namespace(top)
}

fn sanitize_namespace(top: &str) -> String {
    let key: String = top
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(MAX_NAMESPACE_LEN)
        .collect();
    if key.is_empty() {
        ROOT_NAMESPACE.to_string()
    } else {
        key
    }
}

/// 校验 namespace 可以作为文件名安全使用（root 是外部可写文件，不能允许路径穿越）。
pub(crate) fn validate_namespace(namespace: &str, origin: &str) -> crate::Result<()> {
    if namespace.is_empty() || namespace.len() > MAX_NAMESPACE_LEN {
        return Err(crate::Error::Other(format!(
            "manifest root {origin} contains invalid namespace {namespace:?}"
        )));
    }
    if namespace == "." || namespace == ".." {
        return Err(crate::Error::Other(format!(
            "manifest root {origin} contains invalid namespace {namespace:?}"
        )));
    }
    if !namespace
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(crate::Error::Other(format!(
            "manifest root {origin} contains invalid namespace {namespace:?}"
        )));
    }
    Ok(())
}
