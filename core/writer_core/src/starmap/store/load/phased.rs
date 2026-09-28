use std::path::Path;

use crate::error::{Error, Result};

use super::super::meta::GraphMeta;
use super::super::types::*;
use super::super::StarMapStore;

/// 读取 graph.json 并校验 schema 版本与 starmap_id 一致性的唯一入口。
///
/// 统一执行：读 JSON → 取 schemaVersion → 非当前版本返回
/// `UnsupportedVersion` → 当前版本 deserialize GraphMeta → 校验
/// `meta.starmap_id == expected_starmap_id`（不一致返回 `Error::Other`，
/// 上层 resolve 会映射成 `CorruptStarmap`）。
/// `load_full`、`load_graph_meta_phase`、`reload_graph_meta_if_stale`
/// 全部走此函数，确保 schema 检查与 ID 一致性检查不被绕过。
///
/// 在严格版本检查前先跑旧格式迁移（schema "3" -> "4"）。
/// 迁移是幂等的：已经是新格式则跳过。
pub(in crate::starmap) fn load_current_graph_meta(
    path: &Path,
    expected_starmap_id: &str,
) -> Result<Option<GraphMeta>> {
    if !path.exists() {
        return Ok(None);
    }

    // 在严格版本检查前先跑旧格式迁移。
    // path = app_data_root/starmaps/{id}/graph.json
    // 从 path 推导出 app_data_root 和 starmap_id。
    if let Some(graph_dir) = path.parent() {
        if let Some(starmaps_dir) = graph_dir.parent() {
            if let Some(app_data_root) = starmaps_dir.parent() {
                let starmap_id = graph_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(expected_starmap_id);
                super::super::super::migration::migrate_one_starmap_graph(
                    app_data_root,
                    starmap_id,
                )?;
            }
        }
    }

    let content = std::fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&content)?;

    let schema_version_str = value
        .get("schemaVersion")
        .or_else(|| value.get("schema_version"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if schema_version_str.as_deref() != Some(super::super::meta::CURRENT_SCHEMA_VERSION) {
        return Err(Error::UnsupportedVersion {
            version: schema_version_str.unwrap_or_default(),
        });
    }

    let meta: GraphMeta = serde_json::from_str(&content)?;
    if meta.starmap_id != expected_starmap_id {
        return Err(Error::Other(format!(
            "GraphMeta starmap_id '{}' does not match expected '{}'",
            meta.starmap_id, expected_starmap_id
        )));
    }
    Ok(Some(meta))
}

impl StarMapStore {
    pub(in crate::starmap::store) fn reload_graph_meta_if_stale(&mut self) -> Result<()> {
        let graph_json_path = self.starmap_dir().join("graph.json");
        let Some(disk_meta) = load_current_graph_meta(&graph_json_path, &self.starmap_id)? else {
            return Ok(());
        };
        let mem_rev = self
            .graph_meta
            .as_ref()
            .map(|m| m.package_revision)
            .unwrap_or(0);
        if disk_meta.package_revision > mem_rev {
            self.graph_meta = Some(disk_meta);
            self.package_revision = self
                .graph_meta
                .as_ref()
                .map(|m| m.package_revision)
                .unwrap_or(0);
        }
        Ok(())
    }

    pub fn load_phased(&mut self, up_to: LoadPhase) -> Result<StarMapStoreResult> {
        self.recovery_log.clear();
        let mut diagnostics = Vec::new();

        self.load_recovery_from_disk();

        let mut current = self.current_load_phase.unwrap_or(LoadPhase::GraphMeta);

        loop {
            match current {
                LoadPhase::GraphMeta => {
                    self.load_graph_meta_phase(&mut diagnostics)?;
                    self.current_load_phase = Some(LoadPhase::GraphMeta);
                }
                LoadPhase::CurrentObjects => {
                    self.load_current_objects(&mut diagnostics)?;
                    self.current_load_phase = Some(LoadPhase::CurrentObjects);
                }
                LoadPhase::PrefetchNearbyObjects => {
                    self.prefetch_nearby_objects(&mut diagnostics)?;
                    self.current_load_phase = Some(LoadPhase::PrefetchNearbyObjects);
                }
                LoadPhase::BackgroundFullLoad => {
                    self.load_remaining_objects(&mut diagnostics)?;
                    self.detect_dangling_references(&mut diagnostics);
                    self.detect_orphan_objects(&mut diagnostics);
                    self.current_load_phase = Some(LoadPhase::BackgroundFullLoad);
                }
            }

            if current == up_to {
                break;
            }

            match current.next() {
                Some(next) => current = next,
                None => break,
            }
        }

        self.package_revision = self
            .graph_meta
            .as_ref()
            .map(|m| m.package_revision)
            .unwrap_or(0);

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

    pub(in crate::starmap::store) fn load_graph_meta_phase(
        &mut self,
        diagnostics: &mut Vec<LoadDiagnostic>,
    ) -> Result<()> {
        // fail-closed：graph.json 损坏/IO 失败直接返回 Err，不再记 Corrupt diagnostic 后继续。
        // diagnostics 参数保留以维持调用签名兼容，此处不再向其写入。
        let _ = diagnostics;
        let graph_json_path = self.starmap_dir().join("graph.json");
        match load_current_graph_meta(&graph_json_path, &self.starmap_id) {
            Ok(Some(meta)) => {
                self.graph_meta = Some(meta);
            }
            Ok(None) => {}
            Err(Error::UnsupportedVersion { version }) => {
                return Err(Error::UnsupportedVersion { version });
            }
            Err(e) => {
                return Err(e);
            }
        }
        Ok(())
    }

    #[allow(clippy::excessive_nesting)]
    pub(in crate::starmap::store) fn load_current_objects(
        &mut self,
        diagnostics: &mut Vec<LoadDiagnostic>,
    ) -> Result<()> {
        // 布局/视口已退出 Core。CurrentObjects 阶段不再基于视口筛选，
        // 直接加载所有 GraphMeta 声明的对象（与旧 BackgroundFullLoad 行为一致）。
        // fail-closed：GraphMeta 声明的对象必须全部加载成功，任意失败返回 Err。
        let _ = diagnostics;
        if let Some(meta) = self.graph_meta.clone() {
            for node_id in &meta.node_ids {
                if self.nodes.contains_key(node_id) {
                    continue;
                }
                let node = self.try_load_node(node_id)?;
                self.nodes.insert(node_id.clone(), node);
            }
            for edge_id in &meta.edge_ids {
                if self.edges.contains_key(edge_id) {
                    continue;
                }
                let edge = self.try_load_edge(edge_id)?;
                self.edges.insert(edge_id.clone(), edge);
            }
            for embed_id in &meta.embed_instance_ids {
                if self.embeds.contains_key(embed_id) {
                    continue;
                }
                let embed = self.try_load_embed(embed_id)?;
                self.embeds.insert(embed_id.clone(), embed);
            }
            for link_id in &meta.link_ids {
                if self.links.contains_key(link_id) {
                    continue;
                }
                let link = self.try_load_link(link_id)?;
                self.links.insert(link_id.clone(), link);
            }
            for hl_id in &meta.hyperlink_ids {
                if self.hyperlinks.contains_key(hl_id) {
                    continue;
                }
                let hl = self.try_load_hyperlink(hl_id)?;
                self.hyperlinks.insert(hl_id.clone(), hl);
            }
        }
        Ok(())
    }
}
