//! 贡献事务恢复的对象版本事实与裁决。
//!
//! 恢复不能只看文件是否不同：章节对象有 generation（删除重建后改变）、
//! revision（只在正文真实变化时推进）和 content_sha256；星图/注释 JSON 也有
//! updated_at 等版本字段。只有 source 不新于备份、且内容/身份可证明一致时
//! 才允许用备份覆盖；同 revision 不同摘要或 generation 改变一律保留 source。

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::super::ContributionTransactionManifest;

/// 对象完整版本事实。`revision` 优先取 JSON 的 `revision`，其次
/// `updatedAt`/`updated_at`（数值）；`chapter.md` 的版本继承同目录
/// `chapter.meta.json`；摘要始终是对象自身内容的 SHA-256。
#[derive(Debug, Clone, Default)]
pub(super) struct ObjectFacts {
    pub(super) exists: bool,
    pub(super) digest: Option<String>,
    pub(super) revision: Option<u64>,
    pub(super) generation: Option<String>,
    pub(super) content_sha256: Option<String>,
}

#[derive(Debug, Default)]
struct VersionFields {
    revision: Option<u64>,
    generation: Option<String>,
    content_sha256: Option<String>,
}

pub(super) fn read_object_facts(
    path: &Path,
    meta_override: Option<&Path>,
) -> std::result::Result<ObjectFacts, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ObjectFacts::default());
        }
        Err(error) => {
            return Err(format!("read object metadata failed: {error}"));
        }
    };
    if metadata.is_dir() {
        return Err("expected object file but found directory".to_string());
    }
    let bytes = fs::read(path)
        .map_err(|error| format!("read object failed for {}: {error}", path.display()))?;
    let digest = sha256_hex(&bytes);
    let mut facts = ObjectFacts {
        exists: true,
        digest: Some(digest.clone()),
        ..ObjectFacts::default()
    };

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let fields = if file_name.ends_with(".json") {
        parse_version_fields(&bytes)
    } else {
        None
    };
    if let Some(fields) = fields {
        facts.revision = fields.revision;
        facts.generation = fields.generation;
        facts.content_sha256 = fields.content_sha256;
    }

    if file_name == "chapter.md" {
        apply_chapter_meta_facts(&mut facts, path, meta_override, &digest)?;
    }
    Ok(facts)
}

/// `chapter.md` 的 generation/revision/content_sha256 存在同目录 meta 中。
/// 单文件备份根没有同目录结构时，由 `meta_override` 指向清单中兄弟 meta 的
/// 备份路径；没有可证明的 meta 就只保留正文摘要。
fn apply_chapter_meta_facts(
    facts: &mut ObjectFacts,
    body_path: &Path,
    meta_override: Option<&Path>,
    body_digest: &str,
) -> std::result::Result<(), String> {
    let Some(meta_path) = meta_override.map(Path::to_path_buf).or_else(|| {
        body_path
            .parent()
            .map(|directory| directory.join("chapter.meta.json"))
    }) else {
        return Ok(());
    };
    if !meta_path.is_file() {
        return Ok(());
    }
    let meta_bytes = fs::read(&meta_path).map_err(|error| {
        format!(
            "read chapter meta failed for {}: {error}",
            meta_path.display()
        )
    })?;
    if let Some(fields) = parse_version_fields(&meta_bytes) {
        facts.revision = fields.revision;
        facts.generation = fields.generation;
        facts.content_sha256 = fields.content_sha256;
    }
    if facts.content_sha256.is_none() {
        facts.content_sha256 = Some(body_digest.to_string());
    }
    Ok(())
}

fn parse_version_fields(bytes: &[u8]) -> Option<VersionFields> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let revision = value
        .get("revision")
        .and_then(Value::as_u64)
        .or_else(|| value.get("updatedAt").and_then(Value::as_u64))
        .or_else(|| value.get("updated_at").and_then(Value::as_u64));
    let generation = optional_string(&value, "object_generation")
        .or_else(|| optional_string(&value, "generation"));
    let content_sha256 = optional_string(&value, "content_sha256");
    Some(VersionFields {
        revision,
        generation,
        content_sha256,
    })
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug)]
pub(super) enum Decision {
    Skip,
    KeepBaseline,
    Restore(String),
    KeepNewer(String),
    KeepCreated(String),
    KeepDeleted(String),
    Conflict(String),
}

/// 核心对象级裁决：只有 source 不新于备份、且内容与版本可证明时才恢复。
pub(super) fn decide(
    source: &ObjectFacts,
    backup: &ObjectFacts,
    deletion_fact: Option<&str>,
) -> Decision {
    if !backup.exists {
        if !source.exists {
            return Decision::Skip;
        }
        return Decision::KeepCreated(
            "object absent from backup; preserving newly created object".to_string(),
        );
    }
    if !source.exists {
        return match deletion_fact {
            Some(fact) => {
                Decision::KeepDeleted(format!("source missing with durable delete fact ({fact})"))
            }
            None => Decision::Restore(
                "source missing without delete fact; restoring backup object".to_string(),
            ),
        };
    }
    if let (Some(source_generation), Some(backup_generation)) =
        (source.generation.as_deref(), backup.generation.as_deref())
    {
        if source_generation != backup_generation {
            return Decision::KeepNewer(format!(
                "object_generation changed (source={source_generation}, backup={backup_generation}); \
                 delete/recreate result preserved"
            ));
        }
    }
    match (source.revision, backup.revision) {
        (Some(source_revision), Some(backup_revision)) if source_revision > backup_revision => {
            Decision::KeepNewer(format!(
                "source revision {source_revision} is newer than backup revision {backup_revision}"
            ))
        }
        (Some(source_revision), Some(backup_revision)) if source_revision < backup_revision => {
            Decision::Restore(format!(
                "source revision {source_revision} is older than backup revision {backup_revision}"
            ))
        }
        (Some(source_revision), Some(_)) => {
            if same_content(source, backup) {
                Decision::KeepBaseline
            } else {
                Decision::Conflict(format!(
                    "same revision {source_revision} but different content \
                     (source digest={source_digest:?}, backup digest={backup_digest:?}, \
                     source content_sha256={source_hash:?}, backup content_sha256={backup_hash:?})",
                    source_digest = source.digest,
                    backup_digest = backup.digest,
                    source_hash = source.content_sha256,
                    backup_hash = backup.content_sha256,
                ))
            }
        }
        _ => {
            if source.digest == backup.digest
                && (source.content_sha256.is_none()
                    || backup.content_sha256.is_none()
                    || source.content_sha256 == backup.content_sha256)
            {
                Decision::KeepBaseline
            } else {
                Decision::Conflict(format!(
                    "no comparable revision and content differs \
                     (source generation={:?}, revision={:?}, digest={:?}; \
                     backup generation={:?}, revision={:?}, digest={:?})",
                    source.generation,
                    source.revision,
                    source.digest,
                    backup.generation,
                    backup.revision,
                    backup.digest
                ))
            }
        }
    }
}

fn same_content(source: &ObjectFacts, backup: &ObjectFacts) -> bool {
    if let (Some(source_hash), Some(backup_hash)) = (
        source.content_sha256.as_deref(),
        backup.content_sha256.as_deref(),
    ) {
        if source_hash != backup_hash {
            return false;
        }
    }
    source.digest.is_some() && source.digest == backup.digest
}

/// 按事务清单定位某个 source 路径对应的备份路径。
pub(super) fn backup_path_for(
    manifest: &ContributionTransactionManifest,
    path: &Path,
) -> Option<PathBuf> {
    for root in &manifest.roots {
        let source = Path::new(&root.source);
        if source == path {
            return root.existed.then(|| PathBuf::from(&root.backup));
        }
        if let Ok(relative) = path.strip_prefix(source) {
            if !relative.as_os_str().is_empty() && root.existed && !root.backup.is_empty() {
                return Some(Path::new(&root.backup).join(relative));
            }
        }
    }
    None
}
