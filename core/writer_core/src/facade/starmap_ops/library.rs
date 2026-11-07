//! 全局/作品素材库 facade：目录管理、引用实例、复制与反向引用查询。

use super::*;

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
    ) -> Result<crate::starmap::types::StarMapEmbed> {
        if host_starmap_id == target_starmap_id {
            return Err(crate::error::Error::Other(
                "cannot reference a starmap from itself".to_string(),
            ));
        }
        let meta = crate::starmap::get_starmap(&self.app_data_root, target_starmap_id)?;
        crate::starmap::library::ensure_starmap_asset(&self.app_data_root, &meta)?;
        let now = crate::starmap::now_epoch();
        let embed = crate::starmap::types::StarMapEmbed {
            instance_id: crate::storage::journal::starmap_child_embed::new_embed_instance_id(),
            target_starmap_id: target_starmap_id.to_string(),
            label: Some(meta.title),
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
    ) -> Result<crate::starmap::types::StarMapAssetRef> {
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
                let copied = crate::starmap::types::StarMapNodeDefinition {
                    asset_id: asset_id.clone(),
                    title: title.unwrap_or(definition.title),
                    ..definition
                };
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
                Ok(crate::starmap::types::StarMapAssetRef::node(asset_id))
            }
            crate::starmap::types::StarMapAssetKind::StarMap => {
                let copied =
                    self.copy_starmap_canvas(&target.asset_id, title, scope)?;
                Ok(crate::starmap::types::StarMapAssetRef::starmap(
                    copied.starmap_id,
                ))
            }
        }
    }

    fn copy_starmap_canvas(
        &self,
        source_starmap_id: &str,
        title: Option<String>,
        scope: &crate::starmap::types::StarMapAssetScope,
    ) -> Result<crate::starmap::StarMapMeta> {
        use crate::starmap::semantic::StarMapTargetDetail;

        let source_meta = crate::starmap::get_starmap(&self.app_data_root, source_starmap_id)?;
        let graph = self.get_starmap_graph(source_starmap_id)?;
        let new_starmap_id = format!("sm_{}", uuid::Uuid::new_v4());
        let new_title = title.unwrap_or_else(|| format!("{} 副本", source_meta.title));
        let mut meta = crate::starmap::create_starmap_with_id(
            &self.app_data_root,
            &new_starmap_id,
            &new_title,
            &source_meta.description,
            Some(source_meta.accent_color.as_str()),
        )?;
        if let Some(project_id) = scope.project_id() {
            crate::starmap::bind_starmap_to_project(
                &self.app_data_root,
                &new_starmap_id,
                project_id,
            )?;
            meta.project_id = Some(project_id.to_string());
        }

        let mut node_map = std::collections::HashMap::new();
        for node in &graph.nodes {
            let new_instance_id = format!("n_{}", uuid::Uuid::new_v4());
            let mut copied = node.clone();
            copied.id = new_instance_id.clone();
            copied.created_at = crate::starmap::now_epoch();
            copied.updated_at = copied.created_at;
            node_map.insert(node.id.clone(), new_instance_id.clone());
            self.add_starmap_node(
                &new_starmap_id,
                copied,
                node.position.x,
                node.position.y,
            )?;
        }

        let remap_path = |path: &mut crate::starmap::types::StarMapTargetPath| {
            if path.starmap_id == source_starmap_id {
                path.starmap_id = new_starmap_id.clone();
            }
            // 内部嵌入的实例 ID 在复制时整体重建；指向旧实例的路径在复制
            // 画布中不再可达，保留旧 ID 会制造悬空引用，因此清空该段。
            path.segments.retain(|segment| {
                !matches!(
                    segment,
                    crate::starmap::types::StarMapPathSegment::EnterEmbed { .. }
                )
            });
            match &mut path.target {
                StarMapTargetDetail::Node { node_id } => {
                    if let Some(mapped) = node_map.get(node_id) {
                        *node_id = mapped.clone();
                    }
                }
                StarMapTargetDetail::Anchor { node_id, .. } => {
                    if let Some(mapped) = node_map.get(node_id) {
                        *node_id = mapped.clone();
                    }
                }
                _ => {}
            }
        };

        for edge in graph.edges {
            let mut copied = edge.clone();
            copied.id = format!("edge_{}", uuid::Uuid::new_v4());
            copied.created_at = crate::starmap::now_epoch();
            copied.updated_at = copied.created_at;
            remap_path(&mut copied.from);
            remap_path(&mut copied.to);
            if let Err(error) = self.add_starmap_edge(&new_starmap_id, copied) {
                log::warn!("copy_starmap: skip edge {}: {error}", edge.id);
            }
        }

        for link in graph.links {
            let mut copied = link.clone();
            copied.link_id = format!("link_{}", uuid::Uuid::new_v4());
            copied.created_at = crate::starmap::now_epoch();
            copied.updated_at = copied.created_at;
            remap_path(&mut copied.source);
            remap_path(&mut copied.target);
            if let Err(error) = self.add_starmap_link(&new_starmap_id, copied) {
                log::warn!("copy_starmap: skip link {}: {error}", link.link_id);
            }
        }

        for embed in graph.embeds {
            let mut copied = embed.clone();
            copied.instance_id =
                crate::storage::journal::starmap_child_embed::new_embed_instance_id();
            copied.created_at = crate::starmap::now_epoch();
            copied.updated_at = copied.created_at;
            remap_path(&mut copied.host_path);
            if let Err(error) = self.add_starmap_embed(&new_starmap_id, copied) {
                log::warn!("copy_starmap: skip embed {}: {error}", embed.instance_id);
            }
        }

        crate::starmap::library::ensure_starmap_asset(&self.app_data_root, &meta)?;
        crate::starmap::library::assign_entry_scope(
            &self.app_data_root,
            crate::starmap::types::StarMapAssetRef::starmap(new_starmap_id.clone()),
            scope,
            None,
        )?;
        self.flush_starmap_store(&new_starmap_id)?;
        Ok(meta)
    }
}
