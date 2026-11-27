//! 内容寻址对象文件（`manifest/objects/<sha256>.json`）。
//!
//! 每个对象文件承载一个 object_key 下的全部文件记录；文件名字节必须等于
//! 文件内容（canonical JSON）的 sha256 hex。对象不可变：内容变化即换新文件，
//! 旧文件由写入计划的 `deletes` 回收。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::root::MANIFEST_SCHEMA_VERSION;
use crate::sync::types::ManifestFileRecord;

/// 一个 object_key 下的文件记录集合。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManifestObject {
    /// 存储格式版本，必须等于 [`MANIFEST_SCHEMA_VERSION`]。
    pub schema_version: u32,
    /// 本对象文件承载的 object_key。
    pub key: String,
    /// 该 object_key 下的全部记录，按 path 排序。
    pub files: Vec<ManifestFileRecord>,
}

impl ManifestObject {
    /// 构造对象并按 path 稳定排序（同一内容必须得到逐字节一致的 JSON）。
    pub(crate) fn new(key: &str, mut files: Vec<ManifestFileRecord>) -> Self {
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            key: key.to_string(),
            files,
        }
    }

    /// canonical JSON：records 按 path 排序、compact。
    pub(crate) fn canonical_bytes(&self) -> crate::Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(crate::Error::Json)
    }

    /// 校验对象结构：版本、key、每条记录的 object_key 归属。
    pub(crate) fn validate(&self, origin: &str, expected_key: &str) -> crate::Result<()> {
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(crate::Error::Other(format!(
                "manifest object {} has unsupported schemaVersion {} (expected {})",
                origin, self.schema_version, MANIFEST_SCHEMA_VERSION
            )));
        }
        if self.key != expected_key {
            return Err(crate::Error::Other(format!(
                "manifest object {} declares key {} but index references {}",
                origin, self.key, expected_key
            )));
        }
        for record in &self.files {
            let actual = object_key(&record.path);
            if actual != expected_key {
                return Err(crate::Error::Other(format!(
                    "manifest object {} contains path {} belonging to object key {}",
                    origin, record.path, actual
                )));
            }
        }
        Ok(())
    }
}

/// 路径 → object_key（两级索引的第一级分组）。
///
/// - `volumes/<v>/chapters/<c>/...` → `volumes/<v>/chapters/<c>`
/// - `volumes/<v>/...` 其它 → `volumes/<v>`
/// - `starmaps/<id>/...` → `starmaps/<id>`
/// - `annotations/<kind>/<id>...` → `annotations/<kind>/<id>`
/// - 其余取前两段；不足两段用整路径。
pub(crate) fn object_key(path: &str) -> String {
    let mut segments = path.split('/');
    let Some(first) = segments.next() else {
        return path.to_string();
    };
    if first.is_empty() {
        return path.to_string();
    }
    let second = segments.next();
    match first {
        "volumes" => {
            let Some(volume) = second else {
                return first.to_string();
            };
            let third = segments.next();
            let fourth = segments.next();
            match (third, fourth) {
                (Some("chapters"), Some(chapter)) if !chapter.is_empty() => {
                    format!("{first}/{volume}/chapters/{chapter}")
                }
                _ => format!("{first}/{volume}"),
            }
        }
        "starmaps" => match second {
            Some(id) if !id.is_empty() => format!("{first}/{id}"),
            _ => first.to_string(),
        },
        "annotations" => match (second, segments.next()) {
            (Some(kind), Some(id)) if !kind.is_empty() && !id.is_empty() => {
                format!("{first}/{kind}/{id}")
            }
            (Some(kind), _) if !kind.is_empty() => format!("{first}/{kind}"),
            _ => first.to_string(),
        },
        _ => match second {
            Some(rest) if !rest.is_empty() => format!("{first}/{rest}"),
            _ => first.to_string(),
        },
    }
}

/// 文件内容（canonical JSON 字节）的 sha256 hex。
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// 是否为 64 位小写 sha256 hex（对象/索引文件名）。
pub(crate) fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
