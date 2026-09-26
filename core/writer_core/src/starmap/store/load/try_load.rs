use crate::starmap::package_storage;
use crate::starmap::types::*;

use super::super::types::*;
use super::super::StarMapStore;

impl StarMapStore {
    pub(in crate::starmap::store) fn try_load_node(
        &mut self,
        node_id: &str,
    ) -> Option<StarMapNode> {
        let bucket_dir = self
            .starmap_dir()
            .join("nodes")
            .join(package_storage::bucket_for_id(node_id));
        let bucket_path = bucket_dir.join(format!("{}.json", node_id));
        if !bucket_path.exists() {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "node".to_string(),
                object_id: node_id.to_string(),
                detail: format!("node file not found: {}", bucket_path.display()),
            });
            return None;
        }
        let content = std::fs::read_to_string(&bucket_path).ok()?;
        match serde_json::from_str::<StarMapNode>(&content) {
            Ok(node) => {
                if node.id != node_id {
                    self.recovery_log.push(LoadDiagnostic {
                        kind: LoadDiagnosticKind::Corrupt,
                        object_type: "node".to_string(),
                        object_id: node_id.to_string(),
                        detail: format!(
                            "node internal id '{}' does not match filename '{}'",
                            node.id, node_id
                        ),
                    });
                    return None;
                }
                Some(node)
            }
            Err(e) => {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "node".to_string(),
                    object_id: node_id.to_string(),
                    detail: format!("parse error: {}", e),
                });
                None
            }
        }
    }

    pub(in crate::starmap::store) fn try_load_edge(
        &mut self,
        edge_id: &str,
    ) -> Option<StarMapEdge> {
        let bucket_dir = self
            .starmap_dir()
            .join("edges")
            .join(package_storage::bucket_for_id(edge_id));
        let bucket_path = bucket_dir.join(format!("{}.json", edge_id));
        if !bucket_path.exists() {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "edge".to_string(),
                object_id: edge_id.to_string(),
                detail: format!("edge file not found: {}", bucket_path.display()),
            });
            return None;
        }
        let content = std::fs::read_to_string(&bucket_path).ok()?;
        match serde_json::from_str::<StarMapEdge>(&content) {
            Ok(edge) => {
                if edge.id != edge_id {
                    self.recovery_log.push(LoadDiagnostic {
                        kind: LoadDiagnosticKind::Corrupt,
                        object_type: "edge".to_string(),
                        object_id: edge_id.to_string(),
                        detail: format!(
                            "edge internal id '{}' does not match filename '{}'",
                            edge.id, edge_id
                        ),
                    });
                    return None;
                }
                Some(edge)
            }
            Err(e) => {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "edge".to_string(),
                    object_id: edge_id.to_string(),
                    detail: format!("parse error: {}", e),
                });
                None
            }
        }
    }

    pub(in crate::starmap::store) fn try_load_embed(
        &mut self,
        instance_id: &str,
    ) -> Option<StarMapEmbed> {
        let bucket_dir = self
            .starmap_dir()
            .join("embeds")
            .join(package_storage::bucket_for_id(instance_id));
        let bucket_path = bucket_dir.join(format!("{}.json", instance_id));
        if !bucket_path.exists() {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "embed".to_string(),
                object_id: instance_id.to_string(),
                detail: format!("embed file not found: {}", bucket_path.display()),
            });
            return None;
        }
        let content = std::fs::read_to_string(&bucket_path).ok()?;
        match serde_json::from_str::<StarMapEmbed>(&content) {
            Ok(embed) => {
                if embed.instance_id != instance_id {
                    self.recovery_log.push(LoadDiagnostic {
                        kind: LoadDiagnosticKind::Corrupt,
                        object_type: "embed".to_string(),
                        object_id: instance_id.to_string(),
                        detail: format!(
                            "embed internal instance_id '{}' does not match filename '{}'",
                            embed.instance_id, instance_id
                        ),
                    });
                    return None;
                }
                Some(embed)
            }
            Err(e) => {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "embed".to_string(),
                    object_id: instance_id.to_string(),
                    detail: format!("parse error: {}", e),
                });
                None
            }
        }
    }

    pub(in crate::starmap::store) fn try_load_hyperlink(
        &mut self,
        hyperlink_id: &str,
    ) -> Option<StarMapHyperlink> {
        let bucket_dir = self
            .starmap_dir()
            .join("hyperlinks")
            .join(package_storage::bucket_for_id(hyperlink_id));
        let bucket_path = bucket_dir.join(format!("{}.json", hyperlink_id));
        if !bucket_path.exists() {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "hyperlink".to_string(),
                object_id: hyperlink_id.to_string(),
                detail: format!("hyperlink file not found: {}", bucket_path.display()),
            });
            return None;
        }
        let content = std::fs::read_to_string(&bucket_path).ok()?;
        match serde_json::from_str::<StarMapHyperlink>(&content) {
            Ok(hl) => {
                if hl.hyperlink_id != hyperlink_id {
                    self.recovery_log.push(LoadDiagnostic {
                        kind: LoadDiagnosticKind::Corrupt,
                        object_type: "hyperlink".to_string(),
                        object_id: hyperlink_id.to_string(),
                        detail: format!(
                            "hyperlink internal id '{}' does not match filename '{}'",
                            hl.hyperlink_id, hyperlink_id
                        ),
                    });
                    return None;
                }
                Some(hl)
            }
            Err(e) => {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "hyperlink".to_string(),
                    object_id: hyperlink_id.to_string(),
                    detail: format!("parse error: {}", e),
                });
                None
            }
        }
    }

    pub(in crate::starmap::store) fn try_load_layout(&self) -> Option<StarMapLayout> {
        let dir = self.starmap_dir();
        package_storage::load_layout_sharded(&dir)
    }

    pub(in crate::starmap::store) fn try_load_link(
        &mut self,
        link_id: &str,
    ) -> Option<StarMapLink> {
        let bucket_dir = self
            .starmap_dir()
            .join("links")
            .join(package_storage::bucket_for_id(link_id));
        let bucket_path = bucket_dir.join(format!("{}.json", link_id));
        if !bucket_path.exists() {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "link".to_string(),
                object_id: link_id.to_string(),
                detail: format!("link file not found: {}", bucket_path.display()),
            });
            return None;
        }
        let content = std::fs::read_to_string(&bucket_path).ok()?;
        match serde_json::from_str::<StarMapLink>(&content) {
            Ok(link) => {
                if link.link_id != link_id {
                    self.recovery_log.push(LoadDiagnostic {
                        kind: LoadDiagnosticKind::Corrupt,
                        object_type: "link".to_string(),
                        object_id: link_id.to_string(),
                        detail: format!(
                            "link internal id '{}' does not match filename '{}'",
                            link.link_id, link_id
                        ),
                    });
                    return None;
                }
                Some(link)
            }
            Err(e) => {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "link".to_string(),
                    object_id: link_id.to_string(),
                    detail: format!("parse error: {}", e),
                });
                None
            }
        }
    }

    pub(in crate::starmap::store) fn try_load_viewport(&self) -> Option<StarMapViewport> {
        package_storage::load_viewport(&self.app_data_root, &self.starmap_id)
    }
}
