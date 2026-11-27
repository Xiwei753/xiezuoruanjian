//! 版本化清单根（`manifest/root.json`）。
//!
//! 根是清单存储的权威入口：`generation` 由调用方提供（不透明字符串），
//! `namespaces` 把命名空间映射到对应索引文件的 sha256。根存在即表示 v2
//! 存储权威；旧单文件 / 旧分片目录一律忽略。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 当前清单存储格式版本。
pub(crate) const MANIFEST_SCHEMA_VERSION: u32 = 2;

/// 命名空间名最大长度（与旧分片键保持一致）。
pub(crate) const MAX_NAMESPACE_LEN: usize = 64;

/// 权威清单根。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManifestRoot {
    /// 存储格式版本，必须等于 [`MANIFEST_SCHEMA_VERSION`]。
    pub schema_version: u32,
    /// 调用方给的不透明版本串；仅用于发布/GC 判断"同一代"。
    pub generation: String,
    /// namespace → index 文件 sha256。
    #[serde(default)]
    pub namespaces: BTreeMap<String, String>,
}

impl ManifestRoot {
    pub(crate) fn new(generation: &str) -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            generation: generation.to_string(),
            namespaces: BTreeMap::new(),
        }
    }

    /// canonical JSON：字段顺序固定、namespace 按字典序、compact。
    pub(crate) fn canonical_bytes(&self) -> crate::Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(crate::Error::Json)
    }

    /// 校验根的结构约束；错误消息带来源路径。
    pub(crate) fn validate(&self, origin: &str) -> crate::Result<()> {
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(crate::Error::Other(format!(
                "manifest root {} has unsupported schemaVersion {} (expected {})",
                origin, self.schema_version, MANIFEST_SCHEMA_VERSION
            )));
        }
        for (namespace, sha) in &self.namespaces {
            super::index::validate_namespace(namespace, origin)?;
            if !super::objects::is_sha256_hex(sha) {
                return Err(crate::Error::Other(format!(
                    "manifest root {} has invalid index sha256 for namespace {namespace}: {sha}",
                    origin
                )));
            }
        }
        Ok(())
    }
}
