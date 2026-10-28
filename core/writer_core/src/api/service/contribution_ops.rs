//! 对象贡献事件的业务层记录器。
//!
//! 所有星图/注释写操作在 Core 持久化成功后调用这里。调用点不自行拼统计
//! 数字，也不允许平台端上报 actor；actor/entrypoint 来自当前业务入口的
//! [`MutationContext`]。记录失败只写日志，不回滚已经成功的业务写入，也绝不
//! 在业务失败时留下假事件。

use crate::api::service::WriterCoreApi;
use crate::starmap::semantic::StarMapNodeContent;
use crate::starmap::types::StarMapGraph;
use crate::starmap::StarMapMeta;
use crate::writing_stats::contribution::{
    current_mutation_context, ContributionEvent, ContributionObjectSnapshot,
    ContributionObjectType, ContributionOperation, MutationContext,
};

impl WriterCoreApi {
    pub(crate) fn record_contribution_event(
        &self,
        project_id: &str,
        object_type: ContributionObjectType,
        object_id: &str,
        operation: ContributionOperation,
        container_id: Option<String>,
        before: Option<ContributionObjectSnapshot>,
        after: Option<ContributionObjectSnapshot>,
    ) {
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
        );
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
    ) {
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
        self.record_contribution_events(vec![event]);
    }

    pub(crate) fn record_contribution_events(&self, events: Vec<ContributionEvent>) {
        if events.is_empty() {
            return;
        }
        let result = self
            .core_write()
            .get_stats_api()
            .record_contribution_events(events);
        if let Err(error) = result {
            log::warn!("record contribution events failed: {error}");
        }
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
                        Self::node_contributing_text(&node.content),
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
    ) {
        let before = before_graph
            .and_then(|graph| Self::starmap_object_snapshot(graph, object_type, object_id));
        let after = after_graph
            .and_then(|graph| Self::starmap_object_snapshot(graph, object_type, object_id));
        self.record_contribution_event_with_context(
            context,
            project_id.unwrap_or_default(),
            object_type,
            object_id,
            operation,
            Some(starmap_id.to_string()),
            before,
            after,
        );
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
    ) {
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
        );
    }

    pub(crate) fn get_starmap_graph_snapshot(&self, starmap_id: &str) -> Option<StarMapGraph> {
        self.core_write().get_starmap_graph(starmap_id).ok()
    }

    /// 节点正文用于保留内容来源；不把引用类内容伪造成作者文字。
    /// 收集当前仍存在、但没有贡献事件的星图节点文字，供统计按 Unknown 兜底。
    pub(crate) fn collect_legacy_starmap_texts(
        &self,
        project_id: Option<&str>,
    ) -> Vec<(String, ContributionObjectType, String, usize)> {
        let Ok(starmaps) = self.list_starmaps() else {
            return Vec::new();
        };
        let mut legacy = Vec::new();
        for starmap in starmaps {
            let starmap_project = starmap.project_id.clone().unwrap_or_default();
            if project_id.is_some_and(|value| value != starmap_project) {
                continue;
            }
            let Ok(graph) = self.get_starmap_graph(&starmap.starmap_id) else {
                continue;
            };
            for node in graph.nodes {
                let text = dto_node_contributing_text(&node.content);
                let chars = text.chars().count();
                if chars > 0 {
                    legacy.push((
                        starmap_project.clone(),
                        ContributionObjectType::StarMapNode,
                        node.id,
                        chars,
                    ));
                }
            }
        }
        legacy
    }

    pub(crate) fn node_contributing_text(content: &StarMapNodeContent) -> String {
        match content {
            StarMapNodeContent::Inline { summary, body } => {
                let mut text = String::new();
                if let Some(summary) = summary {
                    text.push_str(summary);
                }
                if let Some(body) = body {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(body);
                }
                text
            }
            _ => String::new(),
        }
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
    ) {
        let context = current_mutation_context();
        self.record_annotation_object_with_context(
            &context,
            project_id,
            object_type,
            object_id,
            operation,
            before,
            after,
        );
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
    ) {
        self.record_contribution_event_with_context(
            context,
            project_id,
            object_type,
            object_id,
            operation,
            None,
            before,
            after,
        );
    }

    pub(crate) fn annotation_snapshot(
        version: impl Into<String>,
        summary: impl Into<String>,
    ) -> ContributionObjectSnapshot {
        ContributionObjectSnapshot::object(version, summary)
    }
}

fn dto_node_contributing_text(content: &crate::api::types::StarMapNodeContentDto) -> String {
    if content.kind != "inline" {
        return String::new();
    }
    let mut text = String::new();
    if let Some(summary) = &content.summary {
        text.push_str(summary);
    }
    if let Some(body) = &content.body {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(body);
    }
    text
}
