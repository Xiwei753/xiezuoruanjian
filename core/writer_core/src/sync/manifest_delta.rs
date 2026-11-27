//! Manifest 对象级增量（评审 #1065 #7215361837 / #7218195841 第 4 项）。
//!
//! 同步与发布不再把整份清单当作“唯一可写容器”：一次合并/确认只产出
//! 受影响对象的 upsert / delete 增量，known_files 基线按增量结算，
//! 未涉及对象保留原基线。持久化走 `ManifestStore::apply_delta`：只写
//! 受影响的 object/index/root，未涉及对象保持原字节。

use crate::sync::types::{ManifestFileRecord, SyncManifest};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// 一次合并/发布相对上一份基线产生的对象级变化。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ManifestDelta {
    /// path → 最新记录（op != delete）。
    pub upserts: BTreeMap<String, ManifestFileRecord>,
    /// 已确认删除的 path。
    pub deletes: BTreeSet<String>,
}

impl ManifestDelta {
    pub(crate) fn is_empty(&self) -> bool {
        self.upserts.is_empty() && self.deletes.is_empty()
    }

    /// 由合并后的记录集合相对旧 known_files 基线计算增量。
    ///
    /// 只比较摘要/存在性：摘要相同且基线存在的对象不进入增量；清单里的
    /// delete 记录与基线中已消失的 path 进入删除集。
    pub(crate) fn from_records<'a, I>(records: I, known_files: &HashMap<String, String>) -> Self
    where
        I: IntoIterator<Item = &'a ManifestFileRecord>,
    {
        let mut delta = Self::default();
        let mut present: BTreeSet<String> = BTreeSet::new();
        for record in records {
            if record.op == "delete" {
                delta.deletes.insert(record.path.clone());
                continue;
            }
            present.insert(record.path.clone());
            let unchanged = known_files
                .get(&record.path)
                .is_some_and(|hash| hash == &record.content_hash);
            if !unchanged {
                delta.upserts.insert(record.path.clone(), record.clone());
            }
        }
        for path in known_files.keys() {
            if !present.contains(path) {
                delta.deletes.insert(path.clone());
            }
        }
        delta
    }

    /// 相对一份已确认基线清单，只计算 `affected` 范围内的对象级增量。
    ///
    /// `base` 是持久化的权威清单（未涉及记录的唯一来源），`final_files` 是
    /// 合并后的目标记录。只有 `affected` 内的路径参与比较：
    ///
    /// - 目标记录与基线不同（含新增、字段变化、delete 墓碑变化）→ upsert；
    /// - 基线有、目标记录没有 → delete；
    /// - 基线有、目标记录相同 → 不进入增量，存储对象保持原字节。
    ///
    /// `final_files` 可以只包含 affected 范围内的最终记录；未涉及的基线记录
    /// 不需要传进来，也不会被当成删除。
    pub(crate) fn between(
        base: &SyncManifest,
        final_files: &[ManifestFileRecord],
        affected: &BTreeSet<String>,
    ) -> Self {
        let mut delta = Self::default();
        let base_by_path: BTreeMap<&str, &ManifestFileRecord> = base
            .files
            .iter()
            .filter(|record| affected.contains(&record.path))
            .map(|record| (record.path.as_str(), record))
            .collect();

        let mut final_paths: BTreeSet<&str> = BTreeSet::new();
        for record in final_files {
            if !affected.contains(&record.path) {
                continue;
            }
            final_paths.insert(record.path.as_str());
            if base_by_path.get(record.path.as_str()).copied() != Some(record) {
                delta.upserts.insert(record.path.clone(), record.clone());
            }
        }

        for path in base_by_path.keys() {
            if !final_paths.contains(path) {
                delta.deletes.insert((*path).to_string());
            }
        }
        delta
    }

    /// 把增量结算进 known_files 基线（顺序：删除后写入，路径不会同时出现）。
    pub(crate) fn apply_to(
        &self,
        known_files: &mut HashMap<String, String>,
        known_files_updated_at: &mut HashMap<String, i64>,
    ) {
        for path in &self.deletes {
            known_files.remove(path);
            known_files_updated_at.remove(path);
        }
        for (path, record) in &self.upserts {
            if record.op == "delete" {
                // delete 墓碑仍要写进清单（远端/别台设备需要看到删除事实），
                // 但本地基线不能保留内容哈希。
                known_files.remove(path);
                known_files_updated_at.remove(path);
                continue;
            }
            known_files.insert(path.clone(), record.content_hash.clone());
            known_files_updated_at.insert(path.clone(), record.updated_at_ms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(path: &str, hash: &str, op: &str, updated_at_ms: i64) -> ManifestFileRecord {
        ManifestFileRecord {
            path: path.to_string(),
            content_hash: hash.to_string(),
            op: op.to_string(),
            updated_at_ms,
            deleted_at_ms: None,
            device_id: "device".to_string(),
            schema_version: 1,
        }
    }

    #[test]
    fn unchanged_records_do_not_enter_delta() {
        let known = HashMap::from([("a.md".to_string(), "h1".to_string())]);
        let records = vec![record("a.md", "h1", "upsert", 10)];
        let delta = ManifestDelta::from_records(records.iter(), &known);
        assert!(delta.is_empty());
    }

    #[test]
    fn changed_and_deleted_paths_form_delta() {
        let known = HashMap::from([
            ("a.md".to_string(), "h1".to_string()),
            ("b.md".to_string(), "h2".to_string()),
        ]);
        let records = vec![
            record("a.md", "h1-new", "upsert", 20),
            record("c.md", "h3", "upsert", 21),
        ];
        let delta = ManifestDelta::from_records(records.iter(), &known);
        assert_eq!(
            delta.upserts.keys().cloned().collect::<Vec<_>>(),
            vec!["a.md".to_string(), "c.md".to_string()]
        );
        assert_eq!(
            delta.deletes.iter().cloned().collect::<Vec<_>>(),
            vec!["b.md".to_string()]
        );

        let mut known_files = known.clone();
        let mut updated = HashMap::from([("a.md".to_string(), 10), ("b.md".to_string(), 11)]);
        delta.apply_to(&mut known_files, &mut updated);
        assert_eq!(known_files.get("a.md").map(String::as_str), Some("h1-new"));
        assert_eq!(known_files.get("c.md").map(String::as_str), Some("h3"));
        assert!(!known_files.contains_key("b.md"));
        assert_eq!(updated.get("a.md"), Some(&20));
        assert!(!updated.contains_key("b.md"));
    }

    #[test]
    fn delete_records_remove_baseline_without_upsert() {
        let known = HashMap::from([("a.md".to_string(), "h1".to_string())]);
        let records = vec![record("a.md", "h1", "delete", 30)];
        let delta = ManifestDelta::from_records(records.iter(), &known);
        assert!(delta.upserts.is_empty());
        assert!(delta.deletes.contains("a.md"));
    }
}
