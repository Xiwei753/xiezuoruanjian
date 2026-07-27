use crate::starmap::package_storage;
use crate::starmap::types::*;

use super::super::types::*;
use super::super::StarMapStore;

impl StarMapStore {
    pub(in crate::starmap::store) fn try_load_node(&mut self, node_id: &str) -> Option<StarMapNode> {
        let bucket_dir = self.starmap_dir().join("nodes").join(package_storage::bucket_for_id(node_id));
        let bucket_path = bucket_dir.join(format!("{}.json", node_id));
        let flat_path = self.starmap_dir().join("nodes").join(format!("{}.json", node_id));
        let (path, from_flat) = if bucket_path.exists() {
            (bucket_path.clone(), false)
        } else if flat_path.exists() {
            (flat_path.clone(), true)
        } else {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "node".to_string(),
                object_id: node_id.to_string(),
                detail: format!("node file not found: tried {} and {}", bucket_path.display(), flat_path.display()),
            });
            return None;
        };
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapNode>(&content) {
            Ok(node) => {
                if from_flat {
                    self.migrate_flat_to_bucket(&flat_path, &bucket_path);
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

    pub(in crate::starmap::store) fn try_load_edge(&mut self, edge_id: &str) -> Option<StarMapEdge> {
        let bucket_dir = self.starmap_dir().join("edges").join(package_storage::bucket_for_id(edge_id));
        let bucket_path = bucket_dir.join(format!("{}.json", edge_id));
        let flat_path = self.starmap_dir().join("edges").join(format!("{}.json", edge_id));
        let (path, from_flat) = if bucket_path.exists() {
            (bucket_path.clone(), false)
        } else if flat_path.exists() {
            (flat_path.clone(), true)
        } else {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "edge".to_string(),
                object_id: edge_id.to_string(),
                detail: format!("edge file not found: tried {} and {}", bucket_path.display(), flat_path.display()),
            });
            return None;
        };
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapEdge>(&content) {
            Ok(edge) => {
                if from_flat {
                    self.migrate_flat_to_bucket(&flat_path, &bucket_path);
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

    pub(in crate::starmap::store) fn try_load_embed(&mut self, instance_id: &str) -> Option<StarMapEmbed> {
        let bucket_dir = self.starmap_dir().join("child_starmaps").join(package_storage::bucket_for_id(instance_id));
        let bucket_path = bucket_dir.join(format!("{}.json", instance_id));
        let flat_path = self.starmap_dir().join("child_starmaps").join(format!("{}.json", instance_id));
        let (path, from_flat) = if bucket_path.exists() {
            (bucket_path.clone(), false)
        } else if flat_path.exists() {
            (flat_path.clone(), true)
        } else {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "embed".to_string(),
                object_id: instance_id.to_string(),
                detail: format!("embed file not found: tried {} and {}", bucket_path.display(), flat_path.display()),
            });
            return None;
        };
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapEmbed>(&content) {
            Ok(embed) => {
                if from_flat {
                    self.migrate_flat_to_bucket(&flat_path, &bucket_path);
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

    pub(in crate::starmap::store) fn try_load_hyperlink(&mut self, hyperlink_id: &str) -> Option<StarMapHyperlink> {
        let bucket_dir = self.starmap_dir().join("hyperlinks").join(package_storage::bucket_for_id(hyperlink_id));
        let bucket_path = bucket_dir.join(format!("{}.json", hyperlink_id));
        let flat_path = self.starmap_dir().join("hyperlinks").join(format!("{}.json", hyperlink_id));
        let (path, from_flat) = if bucket_path.exists() {
            (bucket_path.clone(), false)
        } else if flat_path.exists() {
            (flat_path.clone(), true)
        } else {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "hyperlink".to_string(),
                object_id: hyperlink_id.to_string(),
                detail: format!("hyperlink file not found: tried {} and {}", bucket_path.display(), flat_path.display()),
            });
            return None;
        };
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapHyperlink>(&content) {
            Ok(hl) => {
                if from_flat {
                    self.migrate_flat_to_bucket(&flat_path, &bucket_path);
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
        if let Some(layout) = package_storage::load_layout_sharded(&dir) {
            return Some(layout);
        }
        if let Some(layout) = package_storage::load_legacy_layout(&dir) {
            if package_storage::save_layout_sharded(&dir, &layout).is_ok() {
                let legacy_path = dir.join("layouts").join("default.json");
                let _ = std::fs::remove_file(&legacy_path);
                self.record_migration("layout_sharded", "migrated legacy default.json to sharded format");
            }
            return Some(layout);
        }
        None
    }

    pub(in crate::starmap::store) fn try_load_link(&mut self, link_id: &str) -> Option<StarMapLink> {
        let bucket_dir = self.starmap_dir().join("links").join(package_storage::bucket_for_id(link_id));
        let bucket_path = bucket_dir.join(format!("{}.json", link_id));
        let flat_path = self.starmap_dir().join("links").join(format!("{}.json", link_id));
        let (path, from_flat) = if bucket_path.exists() {
            (bucket_path.clone(), false)
        } else if flat_path.exists() {
            (flat_path.clone(), true)
        } else {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "link".to_string(),
                object_id: link_id.to_string(),
                detail: format!("link file not found: tried {} and {}", bucket_path.display(), flat_path.display()),
            });
            return None;
        };
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapLink>(&content) {
            Ok(link) => {
                if from_flat {
                    self.migrate_flat_to_bucket(&flat_path, &bucket_path);
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
        package_storage::load_viewport(&self.workspace, &self.starmap_id)
    }
}
