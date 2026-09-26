use crate::error::{Error, Result};

use super::super::meta::GraphMeta;
use super::super::types::*;
use super::super::StarMapStore;

/// 当前支持的星图 schema 版本。load 只接受此版本，不猜测或迁移旧格式。
const CURRENT_SCHEMA_VERSION: &str = "2";

impl StarMapStore {
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn load_full(&mut self) -> Result<StarMapStoreResult> {
        self.recovery_log.clear();
        let mut diagnostics = Vec::new();

        self.load_recovery_from_disk();

        let graph_dir = self.starmap_dir();
        let graph_json_path = graph_dir.join("graph.json");

        if graph_json_path.exists() {
            let content = std::fs::read_to_string(&graph_json_path).unwrap_or_default();
            let value: serde_json::Value = serde_json::from_str(&content)?;

            let schema_version_str = value
                .get("schemaVersion")
                .or_else(|| value.get("schema_version"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if schema_version_str.as_deref() != Some(CURRENT_SCHEMA_VERSION) {
                return Err(Error::UnsupportedVersion {
                    version: schema_version_str.unwrap_or_default(),
                });
            }

            match serde_json::from_str::<GraphMeta>(&content) {
                Ok(meta) => {
                    self.graph_meta = Some(meta);
                }
                Err(e) => {
                    diagnostics.push(LoadDiagnostic {
                        kind: LoadDiagnosticKind::Corrupt,
                        object_type: "graph".to_string(),
                        object_id: self.starmap_id.clone(),
                        detail: format!("graph.json parse failed: {}", e),
                    });
                    self.scan_objects_from_disk(&mut diagnostics);
                }
            }
        } else {
            self.scan_objects_from_disk(&mut diagnostics);
        }

        let node_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.node_ids.clone())
            .unwrap_or_default();

        for node_id in &node_ids {
            if !self.nodes.contains_key(node_id) {
                if let Some(node) = self.try_load_node(node_id) {
                    self.nodes.insert(node_id.clone(), node);
                }
            }
        }

        let edge_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.edge_ids.clone())
            .unwrap_or_default();

        for edge_id in &edge_ids {
            if !self.edges.contains_key(edge_id) {
                if let Some(edge) = self.try_load_edge(edge_id) {
                    self.edges.insert(edge_id.clone(), edge);
                }
            }
        }

        let embed_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.embed_instance_ids.clone())
            .unwrap_or_default();

        for instance_id in &embed_ids {
            if !self.embeds.contains_key(instance_id) {
                if let Some(embed) = self.try_load_embed(instance_id) {
                    self.embeds.insert(instance_id.clone(), embed);
                }
            }
        }

        let hl_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.hyperlink_ids.clone())
            .unwrap_or_default();

        for hl_id in &hl_ids {
            if !self.hyperlinks.contains_key(hl_id) {
                if let Some(hl) = self.try_load_hyperlink(hl_id) {
                    self.hyperlinks.insert(hl_id.clone(), hl);
                }
            }
        }

        let link_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.link_ids.clone())
            .unwrap_or_default();

        for link_id in &link_ids {
            if !self.links.contains_key(link_id) {
                if let Some(link_path) = self.try_load_link(link_id) {
                    self.links.insert(link_id.clone(), link_path);
                }
            }
        }

        self.layout = self.try_load_layout();
        self.viewport = self.try_load_viewport();

        self.detect_dangling_references(&mut diagnostics);
        self.detect_orphan_objects(&mut diagnostics);

        self.package_revision = self
            .graph_meta
            .as_ref()
            .map(|m| m.package_revision)
            .unwrap_or(0);

        self.current_load_phase = Some(LoadPhase::BackgroundFullLoad);

        diagnostics.append(&mut self.recovery_log);
        self.recovery_log = diagnostics.clone();

        Ok(StarMapStoreResult {
            diagnostics,
            loaded_node_count: self.nodes.len(),
            loaded_edge_count: self.edges.len(),
            loaded_embed_count: self.embeds.len(),
            loaded_link_count: self.links.len(),
            loaded_hyperlink_count: self.hyperlinks.len(),
        })
    }
}
