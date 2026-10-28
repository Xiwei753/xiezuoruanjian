//! 对象贡献事件的业务层记录器。
//!
//! 所有星图/注释写操作在 Core 持久化成功后调用这里。调用点不自行拼统计
//! 数字，也不允许平台端上报 actor；actor/entrypoint 来自当前业务入口的
//! [`MutationContext`]。事件先写入 pending journal，再追加到正式分区；
//! 记录失败会作为业务 API 错误返回，不再静默丢失来源。业务写入失败时不会
//! 留下任何事件。

use std::cell::RefCell;

use crate::api::service::WriterCoreApi;
use crate::starmap::semantic::StarMapNodeContent;
use crate::starmap::types::StarMapGraph;
use crate::starmap::StarMapMeta;
use crate::writing_stats::contribution::{
    current_mutation_context, ContributionEvent, ContributionObjectSnapshot,
    ContributionObjectType, ContributionOperation, CurrentTextSnapshot, MutationContext,
};

thread_local! {
    static ACTIVE_BATCH: RefCell<Option<Vec<ContributionEvent>>> = const { RefCell::new(None) };
}

/// 一次业务事务内的来源事件批次。成功提交时整批写入 pending journal；
/// 业务返回错误时 [`Drop`] 丢弃，绝不留下假事件。
pub(crate) struct ContributionBatch {
    owns_batch: bool,
}

impl Drop for ContributionBatch {
    fn drop(&mut self) {
        if self.owns_batch {
            ACTIVE_BATCH.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }
}

impl WriterCoreApi {
    pub(crate) fn begin_contribution_batch(&self) -> ContributionBatch {
        let owns_batch = ACTIVE_BATCH.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_some() {
                false
            } else {
                *slot = Some(Vec::new());
                true
            }
        });
        ContributionBatch { owns_batch }
    }

    pub(crate) fn commit_contribution_batch(
        &self,
        batch: &mut ContributionBatch,
    ) -> super::ApiResult<()> {
        if !batch.owns_batch {
            return Ok(());
        }
        batch.owns_batch = false;
        let events = ACTIVE_BATCH
            .with(|slot| slot.borrow_mut().take())
            .unwrap_or_default();
        self.record_contribution_events(events)
    }

    pub(crate) fn record_contribution_event(
        &self,
        project_id: &str,
        object_type: ContributionObjectType,
        object_id: &str,
        operation: ContributionOperation,
        container_id: Option<String>,
        before: Option<ContributionObjectSnapshot>,
        after: Option<ContributionObjectSnapshot>,
    ) -> super::ApiResult<()> {
        let context = current_mutation_context();
        self.record_contribution_event_with_context(
            &context,
            project_id,
            object_type,
            object_id,
            operation,
            container_id,
            before,
            after,
        )
    }

    /// 同一业务事务里的多个对象变更必须共享事务 ID。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_contribution_event_with_context(
        &self,
        context: &MutationContext,
        project_id: &str,
        object_type: ContributionObjectType,
        object_id: &str,
        operation: ContributionOperation,
        container_id: Option<String>,
        before: Option<ContributionObjectSnapshot>,
        after: Option<ContributionObjectSnapshot>,
    ) -> super::ApiResult<()> {
        let event = ContributionEvent::new(
            context,
            project_id,
            object_type,
            object_id,
            operation,
            container_id,
            before,
            after,
        );
        let batched = ACTIVE_BATCH.with(|slot| slot.borrow().is_some());
        if batched {
            ACTIVE_BATCH.with(|slot| {
                if let Some(events) = slot.borrow_mut().as_mut() {
                    events.push(event);
                }
            });
            return Ok(());
        }
        self.record_contribution_events(vec![event])
    }

    pub(crate) fn record_contribution_events(
        &self,
        events: Vec<ContributionEvent>,
    ) -> super::ApiResult<()> {
        if events.is_empty() {
            return Ok(());
        }
        if crate::writing_stats::contribution_transaction::queue_active_events(events.clone()) {
            return Ok(());
        }
        self.core_write()
            .get_stats_api()
            .record_contribution_events(events)
            .map(|_| ())
            .map_err(Into::into)
    }

    pub(crate) fn begin_starmap_contribution_transaction(
        &self,
        starmap_id: &str,
    ) -> super::ApiResult<crate::writing_stats::contribution_transaction::ContributionTransaction>
    {
        let _ = starmap_id;
        crate::writing_stats::contribution_transaction::ContributionTransaction::begin_deferred(
            &self.app_data_root,
        )
        .map_err(Into::into)
    }

    pub(crate) fn begin_starmaps_contribution_transaction(
        &self,
    ) -> super::ApiResult<crate::writing_stats::contribution_transaction::ContributionTransaction>
    {
        crate::writing_stats::contribution_transaction::ContributionTransaction::begin_deferred(
            &self.app_data_root,
        )
        .map_err(Into::into)
    }

    pub(crate) fn begin_starmap_child_embed_contribution_transaction(
        &self,
        _host_starmap_id: &str,
        child_starmap_id: &str,
    ) -> super::ApiResult<crate::writing_stats::contribution_transaction::ContributionTransaction>
    {
        let starmaps = self.app_data_root.join("starmaps");
        crate::writing_stats::contribution_transaction::ContributionTransaction::begin(
            &self.app_data_root,
            vec![
                starmaps.join(child_starmap_id),
                starmaps.join(format!("{child_starmap_id}.meta.json")),
                starmaps.join("index.json"),
                self.app_data_root.join("app-meta/sync/state.local.json"),
            ],
        )
        .map_err(Into::into)
    }

    pub(crate) fn begin_starmap_creation_contribution_transaction(
        &self,
        starmap_id: &str,
    ) -> super::ApiResult<crate::writing_stats::contribution_transaction::ContributionTransaction>
    {
        let starmaps = self.app_data_root.join("starmaps");
        crate::writing_stats::contribution_transaction::ContributionTransaction::begin(
            &self.app_data_root,
            vec![
                starmaps.join(starmap_id),
                starmaps.join(format!("{starmap_id}.meta.json")),
                starmaps.join("index.json"),
                self.app_data_root.join("app-meta/sync/state.local.json"),
            ],
        )
        .map_err(Into::into)
    }

    /// 删除星图会在事务提交前物理移除 graph 目录，因此必须在删除前把
    /// 将被删除的路径登记到预备事务里，不能依赖 flush 时的 dirty 文件列表。
    pub(crate) fn begin_starmap_delete_contribution_transaction(
        &self,
        starmap_id: &str,
    ) -> super::ApiResult<crate::writing_stats::contribution_transaction::ContributionTransaction>
    {
        let starmaps = self.app_data_root.join("starmaps");
        crate::writing_stats::contribution_transaction::ContributionTransaction::begin(
            &self.app_data_root,
            vec![
                starmaps.join(starmap_id),
                starmaps.join(format!("{starmap_id}.meta.json")),
                starmaps.join("index.json"),
                self.app_data_root.join("app-meta/sync/state.local.json"),
            ],
        )
        .map_err(Into::into)
    }

    pub(crate) fn begin_annotation_contribution_transaction(
        &self,
        project_id: &str,
    ) -> super::ApiResult<crate::writing_stats::contribution_transaction::ContributionTransaction>
    {
        crate::writing_stats::contribution_transaction::ContributionTransaction::begin(
            &self.app_data_root,
            vec![self.projects_root.join(project_id).join("annotations")],
        )
        .map_err(Into::into)
    }

    /// 从星图当前快照生成对象来源快照；不存在返回 `None`（删除后）。
    pub(crate) fn starmap_object_snapshot(
        graph: &StarMapGraph,
        object_type: ContributionObjectType,
        object_id: &str,
    ) -> Option<ContributionObjectSnapshot> {
        match object_type {
            ContributionObjectType::StarMapNode => graph
                .nodes
                .iter()
                .find(|node| node.id == object_id)
                .map(|node| {
                    ContributionObjectSnapshot::text(
                        node.updated_at.to_string(),
                        node.title.clone(),
                        Self::node_contributing_text(&node.title, &node.content),
                    )
                }),
            ContributionObjectType::StarMapEdge => graph
                .edges
                .iter()
                .find(|edge| edge.id == object_id)
                .map(|edge| {
                    ContributionObjectSnapshot::object(
                        edge.updated_at.to_string(),
                        edge.label.clone().unwrap_or_else(|| edge.id.clone()),
                    )
                }),
            ContributionObjectType::StarMapEmbed => graph
                .embeds
                .iter()
                .find(|embed| embed.instance_id == object_id)
                .map(|embed| {
                    ContributionObjectSnapshot::object(
                        embed.updated_at.to_string(),
                        embed
                            .label
                            .clone()
                            .unwrap_or_else(|| embed.instance_id.clone()),
                    )
                }),
            ContributionObjectType::StarMapLink => graph
                .links
                .iter()
                .find(|link| link.link_id == object_id)
                .map(|link| {
                    ContributionObjectSnapshot::object(
                        link.updated_at.to_string(),
                        link.label.clone().unwrap_or_else(|| link.link_id.clone()),
                    )
                }),
            ContributionObjectType::StarMapHyperlink => graph
                .hyperlinks
                .iter()
                .find(|hyperlink| hyperlink.hyperlink_id == object_id)
                .map(|hyperlink| {
                    ContributionObjectSnapshot::object(
                        hyperlink.updated_at.to_string(),
                        hyperlink
                            .label
                            .clone()
                            .unwrap_or_else(|| hyperlink.target_uri.clone()),
                    )
                }),
            _ => None,
        }
    }

    pub(crate) fn starmap_meta_snapshot(meta: &StarMapMeta) -> ContributionObjectSnapshot {
        ContributionObjectSnapshot::object(meta.updated_at.to_string(), meta.title.clone())
    }

    /// 记录一次星图对象写入。`before`/`after` 是同一对象的图快照，
    /// 节点文本差异由 [`ContributionEvent::new`] 计算。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_starmap_object_with_context(
        &self,
        context: &MutationContext,
        project_id: Option<&str>,
        starmap_id: &str,
        object_type: ContributionObjectType,
        object_id: &str,
        operation: ContributionOperation,
        before_graph: Option<&StarMapGraph>,
        after_graph: Option<&StarMapGraph>,
    ) -> super::ApiResult<()> {
        let before = before_graph
            .and_then(|graph| Self::starmap_object_snapshot(graph, object_type, object_id));
        let after = after_graph
            .and_then(|graph| Self::starmap_object_snapshot(graph, object_type, object_id));
        let revision = after
            .as_ref()
            .and_then(|snapshot| snapshot.version.parse::<u64>().ok())
            .unwrap_or(0);
        let result = self.record_contribution_event_with_context(
            context,
            project_id.unwrap_or_default(),
            object_type,
            object_id,
            operation,
            Some(starmap_id.to_string()),
            before,
            after,
        );
        if result.is_ok() {
            if let Err(error) = crate::storage::workspace_events::append_event(
                &self.app_data_root,
                "starmap",
                starmap_id,
                object_type.as_str(),
                object_id,
                revision,
            ) {
                log::warn!("failed to append workspace change event: {error}");
            }
        }
        result
    }

    pub(crate) fn record_starmap_object(
        &self,
        project_id: Option<&str>,
        starmap_id: &str,
        object_type: ContributionObjectType,
        object_id: &str,
        operation: ContributionOperation,
        before_graph: Option<&StarMapGraph>,
        after_graph: Option<&StarMapGraph>,
    ) -> super::ApiResult<()> {
        let context = current_mutation_context();
        self.record_starmap_object_with_context(
            &context,
            project_id,
            starmap_id,
            object_type,
            object_id,
            operation,
            before_graph,
            after_graph,
        )
    }

    pub(crate) fn get_starmap_graph_snapshot(&self, starmap_id: &str) -> Option<StarMapGraph> {
        self.core_write().get_starmap_graph(starmap_id).ok()
    }

    /// 收集当前仍存在的星图节点文字快照。
    ///
    /// 快照包含节点标题和正文，带 project/container/version；统计投影用它
    /// 与事件历史对账，删除或外部改盘的内容不会被错误计入当前保留来源。
    pub(crate) fn collect_current_starmap_texts(
        &self,
        project_id: Option<&str>,
    ) -> super::ApiResult<Vec<CurrentTextSnapshot>> {
        let starmaps = self.list_starmaps()?;
        let mut snapshots = Vec::new();
        for starmap in starmaps {
            let starmap_project = starmap.project_id.clone().unwrap_or_default();
            if project_id.is_some_and(|value| value != starmap_project) {
                continue;
            }
            let Ok(graph) = self.get_starmap_graph(&starmap.starmap_id) else {
                continue;
            };
            for node in graph.nodes {
                let text = dto_node_contributing_text(&node.title, &node.content);
                if text.is_empty() {
                    continue;
                }
                snapshots.push(CurrentTextSnapshot {
                    project_id: starmap_project.clone(),
                    object_type: ContributionObjectType::StarMapNode,
                    object_id: node.id,
                    container_id: Some(starmap.starmap_id.clone()),
                    version: node.updated_at.to_string(),
                    text,
                });
            }
        }
        Ok(snapshots)
    }

    pub(crate) fn node_contributing_text(title: &str, content: &StarMapNodeContent) -> String {
        let mut text = String::new();
        text.push_str(title);
        if let StarMapNodeContent::Inline { summary, body } = content {
            if let Some(summary) = summary {
                text.push('\n');
                text.push_str(summary);
            }
            if let Some(body) = body {
                text.push('\n');
                text.push_str(body);
            }
        }
        text
    }

    /// 注释对象（标签、绑定、锚点、链接）事件。
    pub(crate) fn record_annotation_object(
        &self,
        project_id: &str,
        object_type: ContributionObjectType,
        object_id: &str,
        operation: ContributionOperation,
        before: Option<ContributionObjectSnapshot>,
        after: Option<ContributionObjectSnapshot>,
    ) -> super::ApiResult<()> {
        let context = current_mutation_context();
        self.record_annotation_object_with_context(
            &context,
            project_id,
            object_type,
            object_id,
            operation,
            before,
            after,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_annotation_object_with_context(
        &self,
        context: &MutationContext,
        project_id: &str,
        object_type: ContributionObjectType,
        object_id: &str,
        operation: ContributionOperation,
        before: Option<ContributionObjectSnapshot>,
        after: Option<ContributionObjectSnapshot>,
    ) -> super::ApiResult<()> {
        self.record_contribution_event_with_context(
            context,
            project_id,
            object_type,
            object_id,
            operation,
            None,
            before,
            after,
        )
    }

    pub(crate) fn annotation_snapshot(
        version: impl Into<String>,
        summary: impl Into<String>,
    ) -> ContributionObjectSnapshot {
        ContributionObjectSnapshot::object(version, summary)
    }
}

fn dto_node_contributing_text(
    title: &str,
    content: &crate::api::types::StarMapNodeContentDto,
) -> String {
    let mut text = title.to_string();
    if content.kind != "inline" {
        return text;
    }
    if let Some(summary) = &content.summary {
        text.push('\n');
        text.push_str(summary);
    }
    if let Some(body) = &content.body {
        text.push('\n');
        text.push_str(body);
    }
    text
}
