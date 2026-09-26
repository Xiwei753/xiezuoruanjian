use std::path::Path;

use crate::error::{Error, Result};

use super::super::meta::GraphMeta;
use super::super::relation_index::{extract_ehi_node_refs, extract_eri_node_refs};
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
pub(in crate::starmap) fn load_current_graph_meta(
    path: &Path,
    expected_starmap_id: &str,
) -> Result<Option<GraphMeta>> {
    if !path.exists() {
        return Ok(None);
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
                LoadPhase::ViewportAndLayoutIndex => {
                    self.layout = self.try_load_layout();
                    self.viewport = self.try_load_viewport();
                    self.current_load_phase = Some(LoadPhase::ViewportAndLayoutIndex);
                }
                LoadPhase::CurrentViewportObjects => {
                    self.load_viewport_objects(&mut diagnostics);
                    self.current_load_phase = Some(LoadPhase::CurrentViewportObjects);
                }
                LoadPhase::PrefetchNearbyObjects => {
                    self.prefetch_nearby_objects(&mut diagnostics);
                    self.current_load_phase = Some(LoadPhase::PrefetchNearbyObjects);
                }
                LoadPhase::BackgroundFullLoad => {
                    self.load_remaining_objects(&mut diagnostics);
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

    pub(in crate::starmap::store) fn load_viewport_objects(
        &mut self,
        diagnostics: &mut Vec<LoadDiagnostic>,
    ) {
        self.load_viewport_objects_impl(diagnostics, false);
    }

    // TODO(#597): 既有代码可读性技术债，待后续重构拆分
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    fn load_viewport_objects_impl(
        &mut self,
        diagnostics: &mut Vec<LoadDiagnostic>,
        index_already_rebuilt: bool,
    ) {
        let viewport_node_ids: std::collections::HashSet<String> =
            match (&self.layout, &self.viewport) {
                (Some(l), Some(vp)) => {
                    let vp_left = vp.offset_x;
                    let vp_top = vp.offset_y;
                    let vp_right = vp.offset_x + vp.width / vp.scale;
                    let vp_bottom = vp.offset_y + vp.height / vp.scale;
                    l.nodes
                        .iter()
                        .filter(|n| {
                            let node_left = n.x;
                            let node_top = n.y;
                            let node_right = n.x + n.width;
                            let node_bottom = n.y + n.height;
                            node_right > vp_left
                                && node_left < vp_right
                                && node_bottom > vp_top
                                && node_top < vp_bottom
                        })
                        .map(|n| n.node_id.clone())
                        .collect()
                }
                (Some(l), None) => l.nodes.iter().map(|n| n.node_id.clone()).collect(),
                _ => std::collections::HashSet::new(),
            };

        if viewport_node_ids.is_empty() {
            let _ = diagnostics;
            return;
        }

        for node_id in &viewport_node_ids {
            // 严格服从 GraphMeta 成员列表：只加载 meta.node_ids 声明的 node。
            // 不在成员列表里的（layout 引用的 stale node 或 orphan）跳过。
            // meta 为 None（空星图）时没有声明对象，全部跳过。
            let in_meta = self
                .graph_meta
                .as_ref()
                .map(|m| m.node_ids.contains(node_id))
                .unwrap_or(false);
            if !in_meta {
                continue;
            }
            if !self.nodes.contains_key(node_id) {
                if let Some(node) = self.try_load_node(node_id) {
                    self.nodes.insert(node_id.clone(), node);
                }
            }
        }

        let has_index = self
            .graph_meta
            .as_ref()
            .map(|m| !m.edge_relation_index.is_empty() || m.edge_ids.is_empty())
            .unwrap_or(false);

        if has_index {
            if let Some(meta) = self.graph_meta.as_ref() {
                let edge_relation_index = meta.edge_relation_index.clone();
                let embed_host_index = meta.embed_host_index.clone();
                // clone 成员列表到 HashSet，让 meta 借用提前释放，
                // 循环内可变借用 self.try_load_edge/try_load_embed 不冲突。
                let meta_edge_ids: std::collections::HashSet<String> =
                    meta.edge_ids.iter().cloned().collect();
                let meta_embed_ids: std::collections::HashSet<String> =
                    meta.embed_instance_ids.iter().cloned().collect();

                for eri in &edge_relation_index {
                    // 严格服从 GraphMeta 成员列表：只加载 meta.edge_ids 声明的 edge。
                    // stale index 条目（edge 已删但 index 未更新）跳过，不加载 orphan。
                    if !meta_edge_ids.contains(&eri.edge_id) {
                        continue;
                    }
                    if self.edges.contains_key(&eri.edge_id) {
                        continue;
                    }
                    let refs = extract_eri_node_refs(eri, &self.starmap_id);
                    let any_in_viewport = refs.iter().any(|id| viewport_node_ids.contains(*id));
                    if any_in_viewport {
                        if let Some(edge) = self.try_load_edge(&eri.edge_id) {
                            self.edges.insert(eri.edge_id.clone(), edge);
                        }
                    }
                }
                for ehi in &embed_host_index {
                    // 严格服从 GraphMeta 成员列表：只加载 meta.embed_instance_ids 声明的 embed。
                    // stale index 条目跳过，不加载 orphan。
                    if !meta_embed_ids.contains(&ehi.instance_id) {
                        continue;
                    }
                    if self.embeds.contains_key(&ehi.instance_id) {
                        continue;
                    }
                    let refs = extract_ehi_node_refs(ehi, &self.starmap_id);
                    let any_in_viewport = refs.iter().any(|id| viewport_node_ids.contains(*id));
                    if any_in_viewport {
                        if let Some(embed) = self.try_load_embed(&ehi.instance_id) {
                            self.embeds.insert(ehi.instance_id.clone(), embed);
                        }
                    }
                }
            }
        } else if !index_already_rebuilt {
            self.rebuild_relation_indexes();
            self.load_viewport_objects_impl(diagnostics, true);
        }

        let _ = diagnostics;
    }
}
