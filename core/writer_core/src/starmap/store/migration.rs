use std::collections::HashMap;
use std::path::Path;

use crate::error::Result;

use super::meta::{DeletedSinceLastSync, GraphMeta, LegacyGraphMeta};
use super::relation_index::*;
use super::types::*;
use super::StarMapStore;

impl StarMapStore {
    pub(super) fn migrate_flat_to_bucket(&mut self, flat_path: &Path, bucket_path: &Path) {
        if !flat_path.exists() || bucket_path.exists() {
            return;
        }
        if let Some(parent) = bucket_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "migration".to_string(),
                    object_id: flat_path.to_string_lossy().to_string(),
                    detail: format!("flat_to_bucket: create_dir_all failed: {}", e),
                });
                return;
            }
        }
        let content = match std::fs::read(flat_path) {
            Ok(c) => c,
            Err(e) => {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "migration".to_string(),
                    object_id: flat_path.to_string_lossy().to_string(),
                    detail: format!("flat_to_bucket: read failed: {}", e),
                });
                return;
            }
        };
        let tmp_path = bucket_path.with_extension("json.tmp");
        if let Err(e) = std::fs::write(&tmp_path, &content) {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Corrupt,
                object_type: "migration".to_string(),
                object_id: flat_path.to_string_lossy().to_string(),
                detail: format!("flat_to_bucket: write tmp failed: {}", e),
            });
            return;
        }
        if let Err(e) = std::fs::rename(&tmp_path, bucket_path) {
            let _ = std::fs::remove_file(&tmp_path);
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Corrupt,
                object_type: "migration".to_string(),
                object_id: flat_path.to_string_lossy().to_string(),
                detail: format!("flat_to_bucket: rename failed: {}", e),
            });
            return;
        }
        if let Err(e) = std::fs::remove_file(flat_path) {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Corrupt,
                object_type: "migration".to_string(),
                object_id: flat_path.to_string_lossy().to_string(),
                detail: format!("flat_to_bucket: remove old file failed: {}", e),
            });
        }
        let file_name = flat_path.file_name().unwrap_or_default().to_string_lossy();
        self.record_migration(
            "flat_to_bucket",
            &format!("migrated {} from flat to bucket", file_name),
        );
    }

    pub(super) fn record_migration(&self, kind: &str, detail: &str) {
        let dir = self.metadata_dir();
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = dir.join("migration.json");
        let mut entries: Vec<MigrationEntry> = if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|c| serde_json::from_str(&c).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        entries.push(MigrationEntry {
            kind: kind.to_string(),
            detail: detail.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });
        if let Ok(json) = serde_json::to_string_pretty(&entries) {
            let _ = crate::storage::atomic_write_string(&path, &json);
        }
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub(super) fn load_graph_meta_from_file(&self, path: &Path) -> Result<GraphMeta> {
        let content = std::fs::read_to_string(path)?;
        let value: serde_json::Value = serde_json::from_str(&content)?;

        if let Some(schema_version) = value
            .get("schemaVersion")
            .or_else(|| value.get("schema_version"))
        {
            if let Some(sv_str) = schema_version.as_str() {
                if sv_str == "2" {
                    let meta: GraphMeta = serde_json::from_str(&content)?;
                    return Ok(meta);
                }
            }
            if let Some(sv_num) = schema_version.as_u64() {
                if sv_num == 1 {
                    if value.get("nodes").is_some()
                        && value.get("nodes").and_then(|v| v.as_array()).is_some()
                    {
                        let graph: crate::starmap::types::StarMapGraph =
                            serde_json::from_str(&content)?;
                        return Ok(GraphMeta {
                            schema_version: "2".to_string(),
                            starmap_id: graph.starmap_id.clone(),
                            title: graph.title.clone(),
                            node_ids: graph.nodes.iter().map(|n| n.id.clone()).collect(),
                            edge_ids: graph.edges.iter().map(|e| e.id.clone()).collect(),
                            embed_instance_ids: graph
                                .embeds
                                .iter()
                                .map(|e| e.instance_id.clone())
                                .collect(),
                            link_ids: graph.links.iter().map(|l| l.link_id.clone()).collect(),
                            hyperlink_ids: vec![],
                            edge_relation_index: graph
                                .edges
                                .iter()
                                .map(|e| EdgeRelationIndex {
                                    edge_id: e.id.clone(),
                                    from: e.from.clone().unwrap_or_default(),
                                    to: e.to.clone().unwrap_or_default(),
                                    from_endpoint: e.from_endpoint.clone(),
                                    to_endpoint: e.to_endpoint.clone(),
                                    from_endpoint_path: e.from_endpoint_path.clone(),
                                    to_endpoint_path: e.to_endpoint_path.clone(),
                                })
                                .collect(),
                            embed_host_index: graph
                                .embeds
                                .iter()
                                .map(|e| EmbedHostIndex {
                                    instance_id: e.instance_id.clone(),
                                    host_node_id: e.source_node_id.clone().unwrap_or_default(),
                                    host_endpoint: e.host_endpoint.clone(),
                                })
                                .collect(),
                            link_relation_index: graph
                                .links
                                .iter()
                                .map(|l| LinkRelationIndex {
                                    link_id: l.link_id.clone(),
                                    source_node_id: endpoint_node_id(&l.source)
                                        .unwrap_or_default()
                                        .to_string(),
                                })
                                .collect(),
                            hyperlink_relation_index: vec![],
                            node_kind_counts: {
                                let mut counts = HashMap::new();
                                for node in &graph.nodes {
                                    *counts.entry(format!("{:?}", node.kind)).or_insert(0u32) += 1;
                                }
                                counts
                            },
                            package_revision: 0,
                            updated_at: graph.updated_at,
                            deleted_since_last_sync: DeletedSinceLastSync::default(),
                        });
                    }
                    let meta: LegacyGraphMeta = serde_json::from_str(&content)?;
                    return Ok(GraphMeta {
                        schema_version: "2".to_string(),
                        starmap_id: meta.starmap_id.clone(),
                        title: meta.title.clone(),
                        node_ids: vec![],
                        edge_ids: vec![],
                        embed_instance_ids: vec![],
                        link_ids: vec![],
                        hyperlink_ids: vec![],
                        edge_relation_index: vec![],
                        embed_host_index: vec![],
                        link_relation_index: vec![],
                        hyperlink_relation_index: vec![],
                        node_kind_counts: HashMap::new(),
                        package_revision: 0,
                        updated_at: meta.updated_at,
                        deleted_since_last_sync: DeletedSinceLastSync::default(),
                    });
                }
            }
        }

        let meta: GraphMeta = serde_json::from_str(&content)?;
        Ok(meta)
    }
}
