use crate::error::{Error, Result};
use crate::starmap::package_storage;
use crate::starmap::types::*;

use super::super::types::*;
use super::super::StarMapStore;

impl StarMapStore {
    /// 严格加载单个 node：文件缺失 / IO 失败 / JSON 解析失败 / 内部 ID 不一致
    /// 都返回 `Err`（仍 push diagnostic 到 recovery_log 便于调试）。
    /// 不再返回 `Option<T>` 吞掉错误——GraphMeta 声明的对象必须加载成功。
    pub(in crate::starmap::store) fn try_load_node(
        &mut self,
        node_id: &str,
    ) -> Result<StarMapNode> {
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
            return Err(Error::Other(format!(
                "node '{}' file not found: {}",
                node_id,
                bucket_path.display()
            )));
        }
        let content = std::fs::read_to_string(&bucket_path)?;
        let node: StarMapNode = serde_json::from_str(&content).map_err(|e| {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Corrupt,
                object_type: "node".to_string(),
                object_id: node_id.to_string(),
                detail: format!("parse error: {}", e),
            });
            Error::Other(format!("node '{}' parse error: {}", node_id, e))
        })?;
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
            return Err(Error::Other(format!(
                "node '{}' internal id '{}' does not match filename",
                node_id, node.id
            )));
        }
        Ok(node)
    }

    /// 严格加载单个 edge：同 `try_load_node` 的 fail-closed 语义。
    pub(in crate::starmap::store) fn try_load_edge(
        &mut self,
        edge_id: &str,
    ) -> Result<StarMapEdge> {
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
            return Err(Error::Other(format!(
                "edge '{}' file not found: {}",
                edge_id,
                bucket_path.display()
            )));
        }
        let content = std::fs::read_to_string(&bucket_path)?;
        let edge: StarMapEdge = serde_json::from_str(&content).map_err(|e| {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Corrupt,
                object_type: "edge".to_string(),
                object_id: edge_id.to_string(),
                detail: format!("parse error: {}", e),
            });
            Error::Other(format!("edge '{}' parse error: {}", edge_id, e))
        })?;
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
            return Err(Error::Other(format!(
                "edge '{}' internal id '{}' does not match filename",
                edge_id, edge.id
            )));
        }
        Ok(edge)
    }

    /// 严格加载单个 embed：同 `try_load_node` 的 fail-closed 语义。
    pub(in crate::starmap::store) fn try_load_embed(
        &mut self,
        instance_id: &str,
    ) -> Result<StarMapEmbed> {
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
            return Err(Error::Other(format!(
                "embed '{}' file not found: {}",
                instance_id,
                bucket_path.display()
            )));
        }
        let content = std::fs::read_to_string(&bucket_path)?;
        let embed: StarMapEmbed = serde_json::from_str(&content).map_err(|e| {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Corrupt,
                object_type: "embed".to_string(),
                object_id: instance_id.to_string(),
                detail: format!("parse error: {}", e),
            });
            Error::Other(format!("embed '{}' parse error: {}", instance_id, e))
        })?;
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
            return Err(Error::Other(format!(
                "embed '{}' internal instance_id '{}' does not match filename",
                instance_id, embed.instance_id
            )));
        }
        Ok(embed)
    }

    /// 严格加载单个 hyperlink：同 `try_load_node` 的 fail-closed 语义。
    pub(in crate::starmap::store) fn try_load_hyperlink(
        &mut self,
        hyperlink_id: &str,
    ) -> Result<StarMapHyperlink> {
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
            return Err(Error::Other(format!(
                "hyperlink '{}' file not found: {}",
                hyperlink_id,
                bucket_path.display()
            )));
        }
        let content = std::fs::read_to_string(&bucket_path)?;
        let hl: StarMapHyperlink = serde_json::from_str(&content).map_err(|e| {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Corrupt,
                object_type: "hyperlink".to_string(),
                object_id: hyperlink_id.to_string(),
                detail: format!("parse error: {}", e),
            });
            Error::Other(format!("hyperlink '{}' parse error: {}", hyperlink_id, e))
        })?;
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
            return Err(Error::Other(format!(
                "hyperlink '{}' internal id '{}' does not match filename",
                hyperlink_id, hl.hyperlink_id
            )));
        }
        Ok(hl)
    }

    /// 严格加载单个 link：同 `try_load_node` 的 fail-closed 语义。
    pub(in crate::starmap::store) fn try_load_link(
        &mut self,
        link_id: &str,
    ) -> Result<StarMapLink> {
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
            return Err(Error::Other(format!(
                "link '{}' file not found: {}",
                link_id,
                bucket_path.display()
            )));
        }
        let content = std::fs::read_to_string(&bucket_path)?;
        let link: StarMapLink = serde_json::from_str(&content).map_err(|e| {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Corrupt,
                object_type: "link".to_string(),
                object_id: link_id.to_string(),
                detail: format!("parse error: {}", e),
            });
            Error::Other(format!("link '{}' parse error: {}", link_id, e))
        })?;
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
            return Err(Error::Other(format!(
                "link '{}' internal id '{}' does not match filename",
                link_id, link.link_id
            )));
        }
        Ok(link)
    }
}
