//! 星图本体（documents）与作品绑定的 `AppBackend` 领域方法。
//!
//! 从 `starmap_backend.rs` 拆出：文档级 CRUD（列表 / 读取 / 新建 / 重命名 /
//! 删除）与作品绑定关系（绑定 / 解绑 / 主星图）是一组关注点，和节点、边、
//! 超链接的图元操作无关。所有方法都只是把 QString 参数转成字符串后转调
//! `starmap_bridge` 自由函数，真正的业务真相仍在 Core。

use super::*;
use writer_core::api::types::StarMapMetaDto;
use writer_core::api::WriterCoreApi;
use writer_core::search::extractor;
use writer_core::search::{IndexEntry, SearchIndexAction, SearchIndexUpdate, SearchScope};

impl AppBackend {
    /// 仅在增量索引查询失败时兜底：重建受影响项目；根星图则完整重建。
    pub(crate) fn reindex_starmap_after_mutation(&mut self, starmap_id: &str) {
        let project_id = self
            .core_api()
            .and_then(|core| core.get_starmap(starmap_id).ok())
            .and_then(|meta| meta.project_id);
        match project_id {
            Some(project_id) => self.search_bridge().rebuild_project(project_id),
            None => self.search_bridge().rebuild_all(),
        }
    }

    /// 成功信封返回 `data`，失败或非法 JSON 返回 `None`。
    pub(crate) fn starmap_mutation_data(raw: &str) -> Option<serde_json::Value> {
        let value = serde_json::from_str::<serde_json::Value>(raw).ok()?;
        if value.get("success").and_then(|success| success.as_bool()) != Some(true) {
            return None;
        }
        value.get("data").cloned()
    }

    pub(crate) fn starmap_mutation_succeeded(raw: &str) -> bool {
        serde_json::from_str::<serde_json::Value>(raw)
            .ok()
            .and_then(|value| value.get("success").and_then(|success| success.as_bool()))
            .unwrap_or(false)
    }

    /// `Some(None)` 表示星图存在且是根星图；`None` 表示查询失败。
    pub(crate) fn starmap_project_scope(
        core: &WriterCoreApi,
        starmap_id: &str,
    ) -> Option<Option<String>> {
        core.get_starmap(starmap_id)
            .ok()
            .map(|meta| meta.project_id)
    }

    pub(crate) fn search_upsert_from_entry(entry: IndexEntry) -> SearchIndexUpdate {
        SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: entry.object_id,
            scope: entry.scope,
            title: entry.title,
            body: entry.body,
            target: Some(entry.target),
        }
    }

    pub(crate) fn search_delete_update(object_id: String) -> SearchIndexUpdate {
        SearchIndexUpdate {
            action: SearchIndexAction::Delete,
            object_id,
            scope: SearchScope::All,
            title: String::new(),
            body: String::new(),
            target: None,
        }
    }

    /// 星图 mutation 提交增量索引的唯一出口。
    pub(crate) fn submit_search_updates(&mut self, updates: Vec<SearchIndexUpdate>) {
        self.search_bridge().apply_updates(updates);
    }

    /// 增量更新已构造时直接提交；查询失败时记录并退回项目/全量重建。
    pub(crate) fn finish_starmap_search_update(
        &mut self,
        starmap_id: &str,
        updates: Option<Vec<SearchIndexUpdate>>,
    ) {
        match updates {
            Some(updates) => self.submit_search_updates(updates),
            None => {
                log::warn!(
                    "starmap search index lookup failed after mutation (starmap={}); falling back to rebuild",
                    starmap_id
                );
                self.reindex_starmap_after_mutation(starmap_id);
            }
        }
    }

    /// 删除星图前抓取图元 ID，成功删除后逐条移除索引条目。
    fn starmap_delete_updates(
        core: &WriterCoreApi,
        starmap_id: &str,
    ) -> Option<Vec<SearchIndexUpdate>> {
        let graph = core.get_starmap_graph(starmap_id).ok()?;
        let mut updates = vec![Self::search_delete_update(format!(
            "starmap:{}",
            starmap_id
        ))];
        for node in &graph.nodes {
            updates.push(Self::search_delete_update(format!(
                "starmap_node:{}:{}",
                starmap_id, node.id
            )));
        }
        for edge in &graph.edges {
            updates.push(Self::search_delete_update(format!(
                "starmap_edge:{}:{}",
                starmap_id, edge.id
            )));
        }
        for link in &graph.links {
            updates.push(Self::search_delete_update(format!(
                "starmap_link:{}:{}",
                starmap_id, link.link_id
            )));
        }
        for hyperlink in &graph.hyperlinks {
            updates.push(Self::search_delete_update(format!(
                "starmap_hyperlink:{}:{}",
                starmap_id, hyperlink.hyperlink_id
            )));
        }
        for embed in &graph.embeds {
            updates.push(Self::search_delete_update(format!(
                "starmap_embed:{}:{}",
                starmap_id, embed.instance_id
            )));
        }
        Some(updates)
    }

    // AppBackend::list_starmaps_json
    pub(crate) fn list_starmaps_json(&self) -> QString {
        if let Some(core) = self.core_api() {
            starmap_bridge::list_starmaps(&core).into()
        } else {
            "[]".into()
        }
    }

    // AppBackend::list_starmaps
    pub(crate) fn list_starmaps(&self) -> QJsonArray {
        if let Some(core) = self.core_api() {
            qjson_array_data_from_json(&starmap_bridge::list_starmaps(&core))
        } else {
            QJsonArray::default()
        }
    }

    // AppBackend::list_root_starmaps_json
    /// 一级星图页：只列根星图（未被嵌入且非 legacy child）。
    pub(crate) fn list_root_starmaps_json(&self) -> QString {
        if let Some(core) = self.core_api() {
            starmap_bridge::list_root_starmaps_json(&core).into()
        } else {
            "[]".into()
        }
    }

    // AppBackend::list_root_starmaps
    pub(crate) fn list_root_starmaps(&self) -> QJsonArray {
        if let Some(core) = self.core_api() {
            qjson_array_data_from_json(&starmap_bridge::list_root_starmaps_json(&core))
        } else {
            QJsonArray::default()
        }
    }

    // AppBackend::list_starmaps_for_project_json
    pub(crate) fn list_starmaps_for_project_json(&self, project_id: QString) -> QString {
        let pid = project_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::list_starmaps_for_project(&core, &pid).into()
        } else {
            "[]".into()
        }
    }

    // AppBackend::get_starmap_json
    pub(crate) fn get_starmap_json(&self, starmap_id: QString) -> QString {
        let sid = starmap_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::get_starmap(&core, &sid).into()
        } else {
            "{}".into()
        }
    }

    // AppBackend::create_starmap_json
    pub(crate) fn create_starmap_json(
        &mut self,
        title: QString,
        description: QString,
        accent_color: QString,
    ) -> QString {
        let t = title.to_string();
        let d = description.to_string();
        let ac = accent_color.to_string();
        let color_ref = if ac.is_empty() {
            None
        } else {
            Some(ac.as_str())
        };
        let core = self.core_api();
        let raw = match core.as_ref() {
            Some(core) => starmap_bridge::create_starmap(core, &t, &d, color_ref),
            None => crate::backend::json_utils::envelope_error_json(
                writer_core::api::WriterError::Other("core api not available".to_string()),
            ),
        };
        if Self::starmap_mutation_succeeded(&raw) {
            let meta = Self::starmap_mutation_data(&raw)
                .and_then(|data| serde_json::from_value::<StarMapMetaDto>(data).ok());
            match meta {
                Some(meta) => {
                    let entry = extractor::extract_starmap_title_entry(
                        &meta.starmap_id,
                        meta.project_id.as_deref(),
                        &meta.title,
                    );
                    self.submit_search_updates(vec![Self::search_upsert_from_entry(entry)]);
                }
                None => {
                    log::warn!(
                        "create_starmap succeeded but response data is not StarMapMetaDto; falling back to full search rebuild"
                    );
                    self.search_bridge().rebuild_all();
                }
            }
        }
        raw.into()
    }

    // AppBackend::create_starmap
    pub(crate) fn create_starmap(
        &mut self,
        title: QString,
        description: QString,
        accent_color: QString,
    ) -> QJsonObject {
        let raw = self
            .create_starmap_json(title, description, accent_color)
            .to_string();
        qjson_object_from_json(&raw)
    }

    // AppBackend::rename_starmap_json
    pub(crate) fn rename_starmap_json(
        &mut self,
        starmap_id: QString,
        new_title: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let t = new_title.to_string();
        let core = self.core_api();
        let project_scope = core
            .as_ref()
            .and_then(|core| Self::starmap_project_scope(core, &sid));
        let raw = match core.as_ref() {
            Some(core) => starmap_bridge::rename_starmap(core, &sid, &t),
            None => crate::backend::json_utils::envelope_error_json(
                writer_core::api::WriterError::Other("core api not available".to_string()),
            ),
        };
        if Self::starmap_mutation_succeeded(&raw) {
            let updates = project_scope.map(|project_id| {
                let entry = extractor::extract_starmap_title_entry(&sid, project_id.as_deref(), &t);
                vec![Self::search_upsert_from_entry(entry)]
            });
            self.finish_starmap_search_update(&sid, updates);
        }
        raw.into()
    }

    // AppBackend::delete_starmap_json
    pub(crate) fn delete_starmap_json(&mut self, starmap_id: QString) -> QString {
        let sid = starmap_id.to_string();
        let core = self.core_api();
        let deletes = core
            .as_ref()
            .and_then(|core| Self::starmap_delete_updates(core, &sid));
        let raw = match core.as_ref() {
            Some(core) => starmap_bridge::delete_starmap(core, &sid),
            None => crate::backend::json_utils::envelope_error_json(
                writer_core::api::WriterError::Other("core api not available".to_string()),
            ),
        };
        if Self::starmap_mutation_succeeded(&raw) {
            self.finish_starmap_search_update(&sid, deletes);
        }
        raw.into()
    }

    // AppBackend::get_starmap_graph_json
    pub(crate) fn get_starmap_graph_json(&self, starmap_id: QString) -> QString {
        let sid = starmap_id.to_string();
        if let Some(core) = self.core_api() {
            match core.get_starmap_graph(&sid) {
                Ok(g) => {
                    // 真正有用的图快照日志：记录各类图元数量，便于排查"图空了""embed 丢失"等问题。
                    // StarMapGraphDto 同时持有 `links`（普通连线）和 `hyperlinks`（超链接）两个字段，
                    // 分别记录两者，避免把 hyperlinks 误当成 links 输出到日志。
                    log::debug!(
                        "starmap graph snapshot: id={} nodes={} edges={} embeds={} links={} hyperlinks={}",
                        sid,
                        g.nodes.len(),
                        g.edges.len(),
                        g.embeds.len(),
                        g.links.len(),
                        g.hyperlinks.len()
                    );
                    writer_core::api::ResultEnvelope::success(serde_json::json!({
                        "graph": g
                    }))
                    .to_json_string()
                    .into()
                }
                Err(e) => crate::backend::json_utils::envelope_error_json(
                    writer_core::api::WriterError::Other(e.to_string()),
                )
                .into(),
            }
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::get_starmap_graph
    pub(crate) fn get_starmap_graph(&self, starmap_id: QString) -> QJsonObject {
        let raw = self.get_starmap_graph_json(starmap_id).to_string();
        qjson_object_from_json(&raw)
    }

    // AppBackend::bind_starmap_to_project_json
    pub(crate) fn bind_starmap_to_project_json(
        &mut self,
        starmap_id: QString,
        project_id: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let pid = project_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::bind_starmap_to_project(&core, &sid, &pid).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::set_main_starmap_json
    pub(crate) fn set_main_starmap_json(
        &mut self,
        starmap_id: QString,
        project_id: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let pid = project_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::set_main_starmap(&core, &sid, &pid).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::get_main_starmap_json
    pub(crate) fn get_main_starmap_json(&self, project_id: QString) -> QString {
        let pid = project_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::get_main_starmap(&core, &pid).into()
        } else {
            "{}".into()
        }
    }

    // AppBackend::unbind_starmap_json
    pub(crate) fn unbind_starmap_json(&mut self, starmap_id: QString) -> QString {
        let sid = starmap_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::unbind_starmap(&core, &sid).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }
}
