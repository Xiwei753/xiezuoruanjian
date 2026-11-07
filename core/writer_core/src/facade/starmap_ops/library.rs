//! 全局/作品素材库 facade：目录管理、引用实例、复制与反向引用查询。

use super::*;

/// 复制星图时创建的对象及其新旧 ID 映射，供 API 层补全历史与贡献事件。
pub struct StarMapCopyResult {
    pub root: crate::starmap::StarMapMeta,
    pub starmaps: Vec<crate::starmap::StarMapMeta>,
    pub nodes: Vec<(String, String)>,
    pub embeds: Vec<(String, String)>,
    pub edges: Vec<(String, String)>,
    pub links: Vec<(String, String)>,
    pub starmap_map: std::collections::HashMap<String, String>,
    pub node_maps: std::collections::HashMap<
        String,
        std::collections::HashMap<String, String>,
    >,
    pub embed_maps: std::collections::HashMap<
        String,
        std::collections::HashMap<String, String>,
    >,
    pub change_paths: Vec<std::path::PathBuf>,
}

pub enum StarMapAssetCopyResult {
    Node {
        asset: crate::starmap::types::StarMapAssetRef,
        definition: crate::starmap::types::StarMapNodeDefinition,
    },
    StarMap(StarMapCopyResult),
}

impl super::super::WriterCore {
    pub fn list_starmap_library(
        &self,
        scope: &crate::starmap::types::StarMapAssetScope,
    ) -> Result<(
        Vec<crate::starmap::types::StarMapLibraryEntry>,
        Vec<crate::starmap::types::StarMapLibraryFolder>,
    )> {
        crate::starmap::library::list_library(&self.app_data_root, scope)
    }

    pub fn create_starmap_library_folder(
        &self,
        scope: &crate::starmap::types::StarMapAssetScope,
        parent_folder_id: Option<String>,
        name: &str,
    ) -> Result<crate::starmap::types::StarMapLibraryFolder> {
        crate::starmap::library::create_folder(
            &self.app_data_root,
            scope,
            parent_folder_id,
            name,
        )
    }

    pub fn move_starmap_library_entry(
        &self,
        entry_id: &str,
        scope: &crate::starmap::types::StarMapAssetScope,
        parent_folder_id: Option<String>,
    ) -> Result<crate::starmap::types::StarMapLibraryEntry> {
        crate::starmap::library::move_entry(
            &self.app_data_root,
            entry_id,
            scope,
            parent_folder_id,
        )
    }

    /// 在宿主画布中创建星图引用（Embed），不改变目标星图的一级身份。
    pub fn add_starmap_reference(
        &self,
        host_starmap_id: &str,
        target_starmap_id: &str,
        position: crate::starmap::types::StarMapPoint,
        instance_id: Option<String>,
    ) -> Result<crate::starmap::types::StarMapEmbed> {
        let meta = crate::starmap::get_starmap(&self.app_data_root, target_starmap_id)?;
        crate::starmap::library::ensure_starmap_asset(&self.app_data_root, &meta)?;
        let now = crate::starmap::now_epoch();
        let embed = crate::starmap::types::StarMapEmbed {
            instance_id: instance_id
                .unwrap_or_else(crate::storage::journal::starmap_child_embed::new_embed_instance_id),
            target_starmap_id: target_starmap_id.to_string(),
            label: None,
            position,
            host_path: crate::starmap::types::StarMapTargetPath {
                starmap_id: host_starmap_id.to_string(),
                segments: Vec::new(),
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            provenance: crate::starmap::semantic::StarMapProvenance::default(),
            created_at: now,
            updated_at: now,
        };
        self.add_starmap_embed(host_starmap_id, embed)
    }

    /// 删除画布上的引用实例；素材本体与库归属不变。
    pub fn remove_starmap_instance(
        &self,
        starmap_id: &str,
        instance_id: &str,
        is_embed: bool,
    ) -> Result<()> {
        if is_embed {
            self.delete_starmap_embed(starmap_id, instance_id)
        } else {
            self.delete_starmap_node(starmap_id, instance_id)
        }
    }

    /// 查询素材在全部作品/画布中的引用位置。
    pub fn list_starmap_asset_references(
        &self,
        target: &crate::starmap::types::StarMapAssetRef,
    ) -> Result<Vec<crate::starmap::types::StarMapAssetReference>> {
        use crate::starmap::types::{StarMapAssetKind, StarMapAssetReference};
        match target.kind {
            StarMapAssetKind::Node => {
                let project_by_starmap = crate::starmap::list_starmaps(&self.app_data_root)?
                    .into_iter()
                    .map(|meta| (meta.starmap_id, meta.project_id))
                    .collect::<std::collections::HashMap<_, _>>();
                let mut refs = Vec::new();
                for instance in crate::starmap::library::instances_for_asset(
                    &self.app_data_root,
                    &target.asset_id,
                )? {
                    refs.push(StarMapAssetReference {
                        target: target.clone(),
                        project_id: instance.project_id.or_else(|| {
                            project_by_starmap
                                .get(&instance.starmap_id)
                                .cloned()
                                .flatten()
                        }),
                        starmap_id: instance.starmap_id,
                        instance_id: instance.instance_id,
                    });
                }
                Ok(refs)
            }
            StarMapAssetKind::StarMap => Ok(crate::starmap::find_starmap_references(
                &self.app_data_root,
                &target.asset_id,
            )?
            .into_iter()
            .map(|reference| StarMapAssetReference {
                target: target.clone(),
                project_id: crate::starmap::get_starmap(
                    &self.app_data_root,
                    &reference.host_starmap_id,
                )
                .ok()
                .and_then(|meta| meta.project_id),
                starmap_id: reference.host_starmap_id,
                instance_id: reference.ref_id,
            })
            .collect()),
        }
    }

    /// 删除素材本体。存在画布引用时拒绝，由平台先展示引用位置。
    pub fn delete_starmap_library_asset(
        &self,
        target: &crate::starmap::types::StarMapAssetRef,
    ) -> Result<()> {
        let refs = self.list_starmap_asset_references(target)?;
        if !refs.is_empty() {
            return Err(crate::error::Error::Other(format!(
                "asset is still referenced by {} canvas instance(s)",
                refs.len()
            )));
        }
        match target.kind {
            crate::starmap::types::StarMapAssetKind::Node => {
                crate::starmap::library::delete_node_definition(
                    &self.app_data_root,
                    &target.asset_id,
                )?;
            }
            crate::starmap::types::StarMapAssetKind::StarMap => {
                return Err(crate::error::Error::Other(
                    "delete a starmap through delete_starmap so metadata and index stay consistent"
                        .to_string(),
                ));
            }
        }
        crate::starmap::library::remove_entries_for_asset(&self.app_data_root, target)?;
        Ok(())
    }

    /// 复制素材为本体独立的新对象。节点复制内容；星图复制整张画布并重映射
    /// 节点实例、连线端点与内部嵌入。
    pub fn copy_starmap_library_asset(
        &self,
        target: &crate::starmap::types::StarMapAssetRef,
        scope: &crate::starmap::types::StarMapAssetScope,
        title: Option<String>,
        prepare_paths: &mut dyn FnMut(
            &[std::path::PathBuf],
        ) -> Result<()>,
    ) -> Result<StarMapAssetCopyResult> {
        match target.kind {
            crate::starmap::types::StarMapAssetKind::Node => {
                let definition = crate::starmap::library::load_node_definition(
                    &self.app_data_root,
                    &target.asset_id,
                )?
                .ok_or_else(|| {
                    crate::error::Error::Other(format!(
                        "node asset not found: {}",
                        target.asset_id
                    ))
                })?;
                let asset_id = format!("n_{}", uuid::Uuid::new_v4());
                let now = crate::starmap::now_epoch();
                let copied = crate::starmap::types::StarMapNodeDefinition {
                    asset_id: asset_id.clone(),
                    title: title.unwrap_or(definition.title),
                    created_at: now,
                    updated_at: now,
                    ..definition
                };
                prepare_paths(&[
                    crate::starmap::library::node_definition_abs_path(
                        &self.app_data_root,
                        &asset_id,
                    ),
                    crate::starmap::library::library_index_abs_path(&self.app_data_root),
                    self.app_data_root.join("app-meta/sync/state.local.json"),
                ])?;
                crate::starmap::library::save_node_definition(
                    &self.app_data_root,
                    &copied,
                )?;
                let mut index = crate::starmap::library::ensure_library_index(
                    &self.app_data_root,
                )?;
                crate::starmap::library::ensure_library_entry(
                    &mut index,
                    scope,
                    crate::starmap::types::StarMapAssetRef::node(asset_id.clone()),
                    None,
                    crate::starmap::now_epoch(),
                );
                crate::starmap::library::save_library_index(&self.app_data_root, &index)?;
                Ok(StarMapAssetCopyResult::Node {
                    asset: crate::starmap::types::StarMapAssetRef::node(asset_id),
                    definition: copied,
                })
            }
            crate::starmap::types::StarMapAssetKind::StarMap => {
                let copied = self.copy_starmap_canvas(
                    &target.asset_id,
                    title,
                    scope,
                    prepare_paths,
                )?;
                Ok(StarMapAssetCopyResult::StarMap(copied))
            }
        }
    }

    fn copy_starmap_canvas(
        &self,
        source_starmap_id: &str,
        title: Option<String>,
        scope: &crate::starmap::types::StarMapAssetScope,
        prepare_paths: &mut dyn FnMut(
            &[std::path::PathBuf],
        ) -> Result<()>,
    ) -> Result<StarMapCopyResult> {
        let placeholder_root =
            crate::starmap::get_starmap(&self.app_data_root, source_starmap_id)?;
        let mut result = StarMapCopyResult {
            root: placeholder_root,
            starmaps: Vec::new(),
            nodes: Vec::new(),
            embeds: Vec::new(),
            edges: Vec::new(),
            links: Vec::new(),
            starmap_map: std::collections::HashMap::new(),
            node_maps: std::collections::HashMap::new(),
            embed_maps: std::collections::HashMap::new(),
            change_paths: Vec::new(),
        };
        let copied =
            self.copy_starmap_canvas_inner(source_starmap_id, title, scope, &mut result, prepare_paths);
        if copied.is_err() {
            let created_starmaps = result
                .starmaps
                .iter()
                .map(|meta| meta.starmap_id.clone())
                .collect::<Vec<_>>();
            let created_nodes = result
                .nodes
                .iter()
                .map(|(_, node_id)| node_id.clone())
                .collect::<Vec<_>>();
            self.cleanup_failed_starmap_copy(&created_starmaps, &created_nodes);
        }
        let meta = copied?;
        result.root = meta;
        // 副本完整落盘后再汇总路径，确保 service 能一次性写入 Graph / 对象 /
        // 素材定义和索引的全部 Workspace 变更。
        result.change_paths.push(std::path::PathBuf::from("starmaps/index.json"));
        for meta in &result.starmaps {
            result.change_paths.push(
                crate::starmap::package_storage::graph_meta_rel_path(&meta.starmap_id),
            );
        }
        result
            .change_paths
            .push(crate::starmap::library::library_index_rel_path());
        result
            .change_paths
            .push(crate::starmap::library::reference_index_rel_path());
        result.change_paths.sort();
        result.change_paths.dedup();
        Ok(result)
    }

    fn copy_starmap_canvas_inner(
        &self,
        source_starmap_id: &str,
        title: Option<String>,
        scope: &crate::starmap::types::StarMapAssetScope,
        result: &mut StarMapCopyResult,
        prepare_paths: &mut dyn FnMut(
            &[std::path::PathBuf],
        ) -> Result<()>,
    ) -> Result<crate::starmap::StarMapMeta> {
        use crate::starmap::semantic::StarMapTargetDetail;
        use crate::starmap::types::StarMapTargetPath;
        use std::collections::{HashMap, VecDeque};

        let source_meta = crate::starmap::get_starmap(&self.app_data_root, source_starmap_id)?;
        let new_root_title = title.unwrap_or_else(|| format!("{} 副本", source_meta.title));

        // 先完整遍历源画布可达的全部子画布。复制必须覆盖所有内部引用，
        // 否则子图仍指向原对象，后续编辑会穿透到副本。
        let mut source_graphs = HashMap::new();
        let mut source_metas = HashMap::new();
        let mut order = Vec::new();
        let mut queue = VecDeque::from([source_starmap_id.to_string()]);
        while let Some(starmap_id) = queue.pop_front() {
            if source_graphs.contains_key(&starmap_id) {
                continue;
            }
            let meta = crate::starmap::get_starmap(&self.app_data_root, &starmap_id)?;
            let graph = self.get_starmap_graph(&starmap_id)?;
            for embed in &graph.embeds {
                if !source_graphs.contains_key(&embed.target_starmap_id) {
                    queue.push_back(embed.target_starmap_id.clone());
                }
            }
            source_metas.insert(starmap_id.clone(), meta);
            source_graphs.insert(starmap_id.clone(), graph);
            order.push(starmap_id);
        }

        let mut source_instance_assets: HashMap<(String, String), String> = HashMap::new();
        let mut asset_map: HashMap<String, String> = HashMap::new();
        for starmap_id in &order {
            let bindings = crate::starmap::library::instance_asset_bindings_for_starmap(
                &self.app_data_root,
                starmap_id,
            )?
            .into_iter()
            .collect::<HashMap<_, _>>();
            let graph = source_graphs
                .get(starmap_id)
                .expect("graph collected with starmap");
            for node in &graph.nodes {
                let asset_id = bindings
                    .get(&node.id)
                    .cloned()
                    .unwrap_or_else(|| node.id.clone());
                asset_map
                    .entry(asset_id.clone())
                    .or_insert_with(|| format!("n_{}", uuid::Uuid::new_v4()));
                source_instance_assets.insert((starmap_id.clone(), node.id.clone()), asset_id);
            }
        }
        let mut copied_definitions = Vec::with_capacity(asset_map.len());
        for (source_asset_id, new_asset_id) in &asset_map {
            let definition = crate::starmap::library::load_node_definition(
                &self.app_data_root,
                source_asset_id,
            )?
            .ok_or_else(|| {
                crate::error::Error::Other(format!(
                    "node asset not found while copying starmap: {source_asset_id}"
                ))
            })?;
            let now = crate::starmap::now_epoch();
            copied_definitions.push(crate::starmap::types::StarMapNodeDefinition {
                asset_id: new_asset_id.clone(),
                portal: None,
                created_at: now,
                updated_at: now,
                ..definition
            });
        }

        let mut starmap_map = HashMap::new();
        for starmap_id in &order {
            starmap_map.insert(
                starmap_id.clone(),
                format!("sm_{}", uuid::Uuid::new_v4()),
            );
        }

        let mut node_maps: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut embed_maps: HashMap<String, HashMap<String, String>> = HashMap::new();
        for (starmap_id, graph) in &source_graphs {
            let nodes = graph
                .nodes
                .iter()
                .map(|node| {
                    (
                        node.id.clone(),
                        format!("n_{}", uuid::Uuid::new_v4()),
                    )
                })
                .collect::<HashMap<_, _>>();
            let embeds = graph
                .embeds
                .iter()
                .map(|embed| {
                    (
                        embed.instance_id.clone(),
                        crate::storage::journal::starmap_child_embed::new_embed_instance_id(),
                    )
                })
                .collect::<HashMap<_, _>>();
            node_maps.insert(starmap_id.clone(), nodes);
            embed_maps.insert(starmap_id.clone(), embeds);
        }
        result.node_maps = node_maps.clone();
        result.embed_maps = embed_maps.clone();

        // 写入前先把本次会改动的已有索引和全新目标路径交给事务登记。
        // 新画布目录作为不存在根整体撤销，不备份任何旧画布内容。
        let starmaps_dir = self.app_data_root.join("starmaps");
        let mut prepare = vec![
            starmaps_dir.join("index.json"),
            crate::starmap::library::library_index_abs_path(&self.app_data_root),
            crate::starmap::library::reference_index_abs_path(&self.app_data_root),
            self.app_data_root.join("app-meta/sync/state.local.json"),
        ];
        for new_starmap_id in starmap_map.values() {
            prepare.push(crate::starmap::package_storage::starmap_pkg_dir(
                &self.app_data_root,
                new_starmap_id,
            ));
            prepare.push(starmaps_dir.join(format!("{new_starmap_id}.meta.json")));
        }
        for definition in &copied_definitions {
            prepare.push(crate::starmap::library::node_definition_abs_path(
                &self.app_data_root,
                &definition.asset_id,
            ));
        }
        prepare_paths(&prepare)?;

        // 共享本体先落盘；随后每个实例只引用新本体，保证副本内部仍共享，
        // 且与原图完全独立。
        for definition in &copied_definitions {
            crate::starmap::library::save_node_definition(
                &self.app_data_root,
                definition,
            )?;
        }

        for starmap_id in &order {
            let source = source_metas
                .get(starmap_id)
                .expect("source meta collected with graph");
            let new_starmap_id = starmap_map
                .get(starmap_id)
                .expect("copy map covers collected starmaps")
                .clone();
            let is_root = starmap_id == source_starmap_id;
            let new_title = if is_root {
                new_root_title.clone()
            } else {
                source.title.clone()
            };
            let mut meta = crate::starmap::create_starmap_with_id(
                &self.app_data_root,
                &new_starmap_id,
                &new_title,
                &source.description,
                Some(source.accent_color.as_str()),
            )?;
            result.change_paths.push(std::path::PathBuf::from(format!(
                "starmaps/{new_starmap_id}.meta.json"
            )));
            let project_id = scope.project_id();
            if let Some(project_id) = project_id {
                crate::starmap::bind_starmap_to_project(
                    &self.app_data_root,
                    &new_starmap_id,
                    project_id,
                )?;
                meta.project_id = Some(project_id.to_string());
            }
            crate::starmap::library::assign_entry_scope(
                &self.app_data_root,
                crate::starmap::types::StarMapAssetRef::starmap(new_starmap_id.clone()),
                scope,
                None,
            )?;
            result.starmaps.push(meta.clone());
        }

        // 节点先不带 portal 写入，确保所有画布所有实例都已存在；portal 在
        // 第二轮统一重映射，避免环形/前向 portal 因目标实例尚未创建而失败。
        for starmap_id in &order {
            let new_starmap_id = starmap_map
                .get(starmap_id)
                .expect("copy map covers collected starmaps")
                .clone();
            let graph = source_graphs
                .get(starmap_id)
                .expect("graph collected with starmap");
            let nodes = node_maps
                .get(starmap_id)
                .expect("node map built for each copied starmap");
            for node in &graph.nodes {
                let node_id = nodes
                    .get(&node.id)
                    .expect("node id assigned")
                    .clone();
                let source_asset_id = source_instance_assets
                    .get(&(starmap_id.clone(), node.id.clone()))
                    .expect("source asset collected for every node instance");
                let new_asset_id = asset_map
                    .get(source_asset_id)
                    .expect("new asset id assigned for every source asset");
                self.add_starmap_node_reference(
                    &new_starmap_id,
                    new_asset_id,
                    node.position.clone(),
                    None,
                    Some(node_id.clone()),
                )?;
                result.nodes.push((new_starmap_id.clone(), node_id.clone()));
                result.change_paths.push(
                    crate::starmap::package_storage::node_rel_path(
                        &new_starmap_id,
                        &node_id,
                    ),
                );
                result.change_paths.push(
                    crate::starmap::library::node_definition_rel_path(new_asset_id),
                );
            }
        }

        for starmap_id in &order {
            let new_starmap_id = starmap_map
                .get(starmap_id)
                .expect("copy map covers collected starmaps")
                .clone();
            let graph = source_graphs
                .get(starmap_id)
                .expect("graph collected with starmap");
            let nodes = node_maps
                .get(starmap_id)
                .expect("node map built for each copied starmap");
            for node in &graph.nodes {
                let Some(mut portal) = node.portal.clone() else {
                    continue;
                };
                let had_target = portal.destination_target.is_some();
                let mut path = StarMapTargetPath {
                    starmap_id: portal.destination_starmap_id.clone(),
                    segments: Vec::new(),
                    target: portal
                        .destination_target
                        .clone()
                        .unwrap_or(StarMapTargetDetail::Starmap),
                };
                remap_copy_path(
                    &mut path,
                    &starmap_map,
                    &node_maps,
                    &embed_maps,
                    &source_graphs,
                )?;
                portal.destination_starmap_id = path.starmap_id;
                portal.destination_target = if had_target {
                    Some(path.target)
                } else {
                    None
                };
                let patch = crate::starmap::types::StarMapNodePatch {
                    title: None,
                    kind: None,
                    payload: None,
                    tags: None,
                    content: None,
                    anchors: None,
                    portal: Some(Some(portal)),
                    position: None,
                    style: None,
                    provenance: None,
                };
                self.update_starmap_node(
                    &new_starmap_id,
                    nodes.get(&node.id).expect("node id assigned"),
                    patch,
                )?;
            }
        }

        // 嵌入实例的 ID 也必须重建，路径中的 EnterEmbed 才能指向副本内部。
        for starmap_id in &order {
            let new_starmap_id = starmap_map
                .get(starmap_id)
                .expect("copy map covers collected starmaps")
                .clone();
            let graph = source_graphs
                .get(starmap_id)
                .expect("graph collected with starmap");
            let embeds = embed_maps
                .get(starmap_id)
                .expect("embed map built for each copied starmap");
            for embed in &graph.embeds {
                let mut copied = embed.clone();
                copied.instance_id = embeds
                    .get(&embed.instance_id)
                    .expect("embed id assigned")
                    .clone();
                if let Some(mapped) = starmap_map.get(&embed.target_starmap_id) {
                    copied.target_starmap_id = mapped.clone();
                }
                remap_copy_path(
                    &mut copied.host_path,
                    &starmap_map,
                    &node_maps,
                    &embed_maps,
                    &source_graphs,
                )?;
                copied.created_at = crate::starmap::now_epoch();
                copied.updated_at = copied.created_at;
                let embed_id = copied.instance_id.clone();
                self.add_starmap_embed(&new_starmap_id, copied)?;
                result.embeds.push((new_starmap_id.clone(), embed_id.clone()));
                result.change_paths.push(
                    crate::starmap::package_storage::embed_rel_path(
                        &new_starmap_id,
                        &embed_id,
                    ),
                );
            }
        }

        for starmap_id in &order {
            let new_starmap_id = starmap_map
                .get(starmap_id)
                .expect("copy map covers collected starmaps")
                .clone();
            let graph = source_graphs
                .get(starmap_id)
                .expect("graph collected with starmap");
            for edge in &graph.edges {
                let mut copied = edge.clone();
                copied.id = format!("edge_{}", uuid::Uuid::new_v4());
                remap_copy_path(
                    &mut copied.from,
                    &starmap_map,
                    &node_maps,
                    &embed_maps,
                    &source_graphs,
                )?;
                remap_copy_path(
                    &mut copied.to,
                    &starmap_map,
                    &node_maps,
                    &embed_maps,
                    &source_graphs,
                )?;
                copied.created_at = crate::starmap::now_epoch();
                copied.updated_at = copied.created_at;
                let edge_id = copied.id.clone();
                self.add_starmap_edge(&new_starmap_id, copied)?;
                result.edges.push((new_starmap_id.clone(), edge_id.clone()));
                result.change_paths.push(
                    crate::starmap::package_storage::edge_rel_path(
                        &new_starmap_id,
                        &edge_id,
                    ),
                );
            }
        }

        for starmap_id in &order {
            let new_starmap_id = starmap_map
                .get(starmap_id)
                .expect("copy map covers collected starmaps")
                .clone();
            let graph = source_graphs
                .get(starmap_id)
                .expect("graph collected with starmap");
            for link in &graph.links {
                let mut copied = link.clone();
                copied.link_id = format!("link_{}", uuid::Uuid::new_v4());
                remap_copy_path(
                    &mut copied.source,
                    &starmap_map,
                    &node_maps,
                    &embed_maps,
                    &source_graphs,
                )?;
                remap_copy_path(
                    &mut copied.target,
                    &starmap_map,
                    &node_maps,
                    &embed_maps,
                    &source_graphs,
                )?;
                copied.created_at = crate::starmap::now_epoch();
                copied.updated_at = copied.created_at;
                let link_id = copied.link_id.clone();
                self.add_starmap_link(&new_starmap_id, copied)?;
                result
                    .links
                    .push((new_starmap_id.clone(), link_id.clone()));
                result.change_paths.push(
                    crate::starmap::package_storage::link_rel_path(&new_starmap_id, &link_id),
                );
            }
        }

        let new_root_id = starmap_map
            .get(source_starmap_id)
            .expect("root starmap copied")
            .clone();
        let meta = crate::starmap::get_starmap(&self.app_data_root, &new_root_id)?;
        result.starmap_map = starmap_map;
        crate::starmap::library::ensure_starmap_asset(&self.app_data_root, &meta)?;
        crate::starmap::library::assign_entry_scope(
            &self.app_data_root,
            crate::starmap::types::StarMapAssetRef::starmap(new_root_id.clone()),
            scope,
            None,
        )?;
        for meta in result.starmaps.iter() {
            self.flush_starmap_store(&meta.starmap_id)?;
        }
        Ok(meta)
    }

    fn cleanup_failed_starmap_copy(
        &self,
        starmap_ids: &[String],
        node_ids: &[String],
    ) {
        for node_id in node_ids {
            let _ = crate::starmap::library::delete_node_definition(
                &self.app_data_root,
                node_id,
            );
            let _ = crate::starmap::library::remove_entries_for_asset(
                &self.app_data_root,
                &crate::starmap::types::StarMapAssetRef::node(node_id.clone()),
            );
        }
        for starmap_id in starmap_ids.iter().rev() {
            let _ = self.flush_starmap_store(starmap_id);
            self.remove_starmap_store(starmap_id);
            let _ = crate::starmap::library::remove_starmap_instance_records(
                &self.app_data_root,
                starmap_id,
            );
            let _ = std::fs::remove_dir_all(
                crate::starmap::package_storage::starmap_pkg_dir(
                    &self.app_data_root,
                    starmap_id,
                ),
            );
            let _ = std::fs::remove_file(
                self.app_data_root
                    .join("starmaps")
                    .join(format!("{starmap_id}.meta.json")),
            );
            let _ = crate::starmap::library::remove_entries_for_asset(
                &self.app_data_root,
                &crate::starmap::types::StarMapAssetRef::starmap(starmap_id.clone()),
            );
        }
        if let Ok(mut index) = crate::starmap::load_index(&self.app_data_root) {
            index.starmap_ids.retain(|id| !starmap_ids.contains(id));
            index.root_starmap_ids.retain(|id| !starmap_ids.contains(id));
            index
                .main_starmap_by_project
                .retain(|_, id| !starmap_ids.contains(id));
            index.updated_at = crate::starmap::now_epoch();
            let _ = crate::starmap::save_index(&self.app_data_root, &index);
        }
    }
}

fn remap_copy_path(
    path: &mut crate::starmap::types::StarMapTargetPath,
    starmap_map: &std::collections::HashMap<String, String>,
    node_maps: &std::collections::HashMap<String, std::collections::HashMap<String, String>>,
    embed_maps: &std::collections::HashMap<String, std::collections::HashMap<String, String>>,
    source_graphs: &std::collections::HashMap<String, crate::starmap::types::StarMapGraph>,
) -> Result<()> {
    let source_start = path.starmap_id.clone();
    let Some(new_start) = starmap_map.get(&source_start) else {
        // 指向副本外部的路径整体保持不变，外部宿主仍引用原对象。
        return Ok(());
    };
    path.starmap_id = new_start.clone();
    let mut current_source = source_start;
    for segment in &mut path.segments {
        let Some(graph) = source_graphs.get(&current_source) else {
            // 已进入外部画布，后续段不属于本次复制，保持原样。
            return Ok(());
        };
        match segment {
            crate::starmap::types::StarMapPathSegment::EnterEmbed { instance_id } => {
                let new_instance = embed_maps
                    .get(&current_source)
                    .and_then(|map| map.get(instance_id))
                    .cloned()
                    .ok_or_else(|| {
                        crate::error::Error::Other(format!(
                            "copy starmap: missing embed remap {current_source}/{instance_id}"
                        ))
                    })?;
                let target = graph
                    .embeds
                    .iter()
                    .find(|embed| &embed.instance_id == instance_id)
                    .map(|embed| embed.target_starmap_id.clone())
                    .ok_or_else(|| {
                        crate::error::Error::Other(format!(
                            "copy starmap: missing embed {current_source}/{instance_id}"
                        ))
                    })?;
                *instance_id = new_instance;
                current_source = target;
            }
            crate::starmap::types::StarMapPathSegment::EnterPortal { node_id } => {
                if let Some(map) = node_maps.get(&current_source) {
                    *node_id = map.get(node_id).cloned().ok_or_else(|| {
                        crate::error::Error::Other(format!(
                            "copy starmap: missing portal node remap {current_source}/{node_id}"
                        ))
                    })?;
                }
            }
        }
    }
    remap_copy_target(&mut path.target, &current_source, node_maps)
}

fn remap_copy_target(
    target: &mut crate::starmap::semantic::StarMapTargetDetail,
    source_starmap_id: &str,
    node_maps: &std::collections::HashMap<String, std::collections::HashMap<String, String>>,
) -> Result<()> {
    use crate::starmap::semantic::StarMapTargetDetail;
    match target {
        StarMapTargetDetail::Node { node_id }
        | StarMapTargetDetail::Anchor { node_id, .. } => {
            if let Some(map) = node_maps.get(source_starmap_id) {
                *node_id = map.get(node_id).cloned().ok_or_else(|| {
                    crate::error::Error::Other(format!(
                        "copy starmap: missing target node remap {source_starmap_id}/{node_id}"
                    ))
                })?;
            }
        }
        StarMapTargetDetail::Starmap => {}
        _ => {}
    }
    Ok(())
}
