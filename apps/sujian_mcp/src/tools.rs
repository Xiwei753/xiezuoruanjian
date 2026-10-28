use std::time::{SystemTime, UNIX_EPOCH};

use rmcp::{
    handler::server::wrapper::{Json, Parameters},
    model::ErrorData,
    schemars::JsonSchema,
    tool, tool_router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use writer_core::api::types::{
    AnnotationObjectRefDto, AnnotationTargetDto, StarMapAnchorDto, StarMapEdgeDto,
    StarMapEdgeKindDto, StarMapEdgePatchDto, StarMapEmbedDto, StarMapEmbedPatchDto,
    StarMapHyperlinkDto, StarMapHyperlinkPatchDto, StarMapLinkDto, StarMapLinkPatchDto,
    StarMapNodeContentDto, StarMapNodeDto, StarMapNodeKindDto, StarMapNodePatchDto,
    StarMapNodeStyleDto, StarMapPointDto, StarMapProvenanceDto, StarMapReviewStatusDto,
    StarMapSourceKindDto, StarMapTargetPathDto,
};

use crate::context::{invalid_params, to_json};
use crate::server::SujianMcpServer;

fn parse_input<T: serde::de::DeserializeOwned>(value: Value, field: &str) -> Result<T, ErrorData> {
    serde_json::from_value(value)
        .map_err(|error| invalid_params(format!("invalid {field}: {error}")))
}

fn parse_enum<T: serde::de::DeserializeOwned>(value: &str, field: &str) -> Result<T, ErrorData> {
    serde_json::from_value(Value::String(value.to_string()))
        .map_err(|error| invalid_params(format!("invalid {field}: {error}")))
}

fn generated_by_mcp() -> StarMapProvenanceDto {
    StarMapProvenanceDto {
        source: StarMapSourceKindDto::Ai,
        source_id: None,
        generated_by: Some("sujian_mcp".to_string()),
        prompt_id: None,
        review_status: StarMapReviewStatusDto::Unknown,
        created_from_anchor: None,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

macro_rules! empty_params {
    ($name:ident) => {
        #[derive(Debug, Deserialize, JsonSchema)]
        struct $name {}
    };
}

empty_params!(EmptyParams);

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ListChaptersParams {
    /// Optional project id; when omitted, chapters from every project are listed.
    project_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ReadChapterParams {
    project_id: String,
    chapter_id: String,
    volume_id: Option<String>,
    /// Character offset into the chapter body, not a byte offset.
    offset_chars: Option<usize>,
    /// Maximum number of characters to return (1..=50000, default 8000).
    max_chars: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SearchParams {
    query: String,
    /// chapterBody/chapterTitle/chapterNote/projectTitle/volumeTitle/starmapTitle/
    /// starmapNode/starmapEdgeLabel/starmapHyperlink/starmapLink/starmapEmbed/
    /// setting/annotationTag/annotationEntry/annotationLink
    scope: Option<String>,
    limit: Option<u32>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct StarmapIdParams {
    starmap_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ProjectIdParams {
    project_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ChapterIdParams {
    project_id: String,
    chapter_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ContributionStatsParams {
    /// Inclusive local date, YYYY-MM-DD.
    start_date: String,
    /// Inclusive local date, YYYY-MM-DD.
    end_date: String,
    project_id: Option<String>,
    /// human/ai/unknown/system.
    actor: Option<String>,
    /// starmap/starmap_node/tag/tag_binding/text_anchor/object_link/...
    object_type: Option<String>,
    device_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateStarmapParams {
    title: String,
    description: Option<String>,
    template_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct RenameStarmapParams {
    starmap_id: String,
    new_title: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateNodeParams {
    starmap_id: String,
    /// Optional caller-supplied id; a UUID is generated when omitted.
    id: Option<String>,
    title: String,
    /// PascalCase node kind: Character/Event/Location/Item/Concept/Theme/Note/
    /// Organization/Timeline/Plot/Foreshadowing/Chapter/Custom. Default Note.
    kind: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    payload: Option<String>,
    /// Inline summary; used only when `content` is omitted.
    summary: Option<String>,
    /// Inline body; used only when `content` is omitted.
    body: Option<String>,
    /// Full StarMapNodeContentDto object. Takes precedence over summary/body.
    content: Option<Value>,
    #[serde(default)]
    anchors: Vec<Value>,
    portal: Option<Value>,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
    style: Option<Value>,
    provenance: Option<Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateNodeParams {
    starmap_id: String,
    node_id: String,
    /// Revision from the last read; omit to write unconditionally.
    expected_revision: Option<u64>,
    /// StarMapNodePatchDto JSON. Use null for an inner field to clear it.
    patch: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct NodeIdParams {
    starmap_id: String,
    node_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateEdgeParams {
    starmap_id: String,
    id: Option<String>,
    from: Value,
    to: Value,
    /// PascalCase edge kind, e.g. RelatedTo. Default RelatedTo.
    kind: Option<String>,
    label: Option<String>,
    payload: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateEdgeParams {
    starmap_id: String,
    edge_id: String,
    /// Revision from the last read; omit to write unconditionally.
    expected_revision: Option<u64>,
    /// StarMapEdgePatchDto JSON.
    patch: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct EdgeIdParams {
    starmap_id: String,
    edge_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateEmbedParams {
    starmap_id: String,
    target_starmap_id: String,
    /// Parent path in the host graph. Required.
    host_path: Value,
    label: Option<String>,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
    provenance: Option<Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateChildEmbedParams {
    host_starmap_id: String,
    title: String,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateEmbedParams {
    starmap_id: String,
    instance_id: String,
    /// Revision from the last read; omit to write unconditionally.
    expected_revision: Option<u64>,
    /// StarMapEmbedPatchDto JSON.
    patch: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct EmbedIdParams {
    starmap_id: String,
    instance_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateLinkParams {
    starmap_id: String,
    link_id: Option<String>,
    source: Value,
    target: Value,
    label: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateLinkParams {
    starmap_id: String,
    link_id: String,
    /// Revision from the last read; omit to write unconditionally.
    expected_revision: Option<u64>,
    /// StarMapLinkPatchDto JSON.
    patch: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct LinkIdParams {
    starmap_id: String,
    link_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateHyperlinkParams {
    starmap_id: String,
    hyperlink_id: Option<String>,
    source: Value,
    target_uri: String,
    label: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateHyperlinkParams {
    starmap_id: String,
    hyperlink_id: String,
    /// Revision from the last read; omit to write unconditionally.
    expected_revision: Option<u64>,
    /// StarMapHyperlinkPatchDto JSON.
    patch: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct HyperlinkIdParams {
    starmap_id: String,
    hyperlink_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct BindStarmapParams {
    starmap_id: String,
    project_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateTagParams {
    project_id: String,
    name: String,
    color: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct RenameTagParams {
    project_id: String,
    tag_id: String,
    name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct TagIdParams {
    project_id: String,
    tag_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MergeTagsParams {
    project_id: String,
    source_tag_id: String,
    target_tag_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct BindingsParams {
    project_id: String,
    target: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SetBindingsParams {
    project_id: String,
    target: Value,
    tag_ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateObjectLinkParams {
    source: Value,
    target: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateObjectLinkParams {
    project_id: String,
    link_id: String,
    source: Value,
    target: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ObjectLinkIdParams {
    project_id: String,
    link_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct LinkSourceParams {
    source: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct BacklinksParams {
    target: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateTextAnchorParams {
    project_id: String,
    chapter_id: String,
    start_byte: u32,
    end_byte: u32,
    /// `read_chapter` 返回的 hash；提供时正文已变化会拒绝创建。
    expected_hash: Option<String>,
}

#[tool_router(vis = "pub(crate)")]
impl SujianMcpServer {
    // ---------------------------------------------------------------- reading

    #[tool(
        name = "list_projects",
        description = "List all Sujian writing projects. Chapter body content is read-only."
    )]
    fn list_projects(
        &self,
        Parameters(_): Parameters<EmptyParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let api = self.ctx().api();
        let projects = crate::context::read(|| api.list_projects())?;
        to_json(&projects)
    }

    #[tool(name = "list_volumes", description = "List volumes of a project.")]
    fn list_volumes(
        &self,
        Parameters(params): Parameters<ProjectIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let api = self.ctx().api();
        let volumes = crate::context::read(|| api.list_volumes(&params.project_id))?;
        to_json(&volumes)
    }

    #[tool(
        name = "list_chapters",
        description = "List chapter metadata for one project or all projects. Does not return body text."
    )]
    fn list_chapters(
        &self,
        Parameters(params): Parameters<ListChaptersParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let api = self.ctx().api();
        let chapters =
            crate::context::read(|| api.mcp_list_chapters(params.project_id.as_deref()))?;
        to_json(&chapters)
    }

    #[tool(
        name = "read_chapter",
        description = "Read a chapter body by character range. Body is read-only; use offsetChars/nextOffsetChars for long chapters."
    )]
    fn read_chapter(
        &self,
        Parameters(params): Parameters<ReadChapterParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let api = self.ctx().api();
        let chapter = crate::context::read(|| {
            api.mcp_read_chapter(
                &params.project_id,
                &params.chapter_id,
                params.volume_id.as_deref(),
                params.offset_chars,
                params.max_chars,
            )
        })?;
        to_json(&chapter)
    }

    #[tool(
        name = "search",
        description = "Search Sujian chapter titles/bodies, starmaps, settings and annotations."
    )]
    fn search(
        &self,
        Parameters(params): Parameters<SearchParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let api = self.ctx().api();
        let scope = params.scope.as_deref().unwrap_or("All");
        let limit = params.limit.unwrap_or(50);
        let raw = crate::context::read(|| {
            api.global_search_json(&params.query, scope, limit, params.cursor.as_deref())
        })?;
        let value: Value = serde_json::from_str(&raw)
            .map_err(|error| invalid_params(format!("invalid search response: {error}")))?;
        Ok(Json(value))
    }

    #[tool(
        name = "list_starmaps",
        description = "List starmaps. Provide projectId to list only that project's maps."
    )]
    fn list_starmaps(
        &self,
        Parameters(params): Parameters<ListChaptersParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let api = self.ctx().api();
        let maps = crate::context::read(|| match params.project_id.as_deref() {
            Some(project_id) => api.list_starmaps_for_project(project_id),
            None => api.list_starmaps(),
        })?;
        to_json(&maps)
    }

    #[tool(
        name = "get_starmap_graph",
        description = "Read a full starmap graph: nodes, edges, embeds, links and hyperlinks."
    )]
    fn get_starmap_graph(
        &self,
        Parameters(params): Parameters<StarmapIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let api = self.ctx().api();
        let graph = crate::context::read(|| api.get_starmap_graph(&params.starmap_id))?;
        to_json(&graph)
    }

    #[tool(name = "list_tags", description = "List annotation tags of a project.")]
    fn list_tags(
        &self,
        Parameters(params): Parameters<ProjectIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let api = self.ctx().api();
        let tags = crate::context::read(|| api.list_tags(params.project_id))?;
        to_json(&tags)
    }

    #[tool(
        name = "list_bindings_for_target",
        description = "List tag bindings for an AnnotationObjectRefDto target."
    )]
    fn list_bindings_for_target(
        &self,
        Parameters(params): Parameters<BindingsParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let target: AnnotationObjectRefDto = parse_input(params.target, "target")?;
        let api = self.ctx().api();
        let bindings =
            crate::context::read(|| api.list_bindings_for_target(params.project_id, target))?;
        to_json(&bindings)
    }

    #[tool(
        name = "list_links_for_source",
        description = "List object links whose source is an AnnotationObjectRefDto."
    )]
    fn list_links_for_source(
        &self,
        Parameters(params): Parameters<LinkSourceParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let source: AnnotationObjectRefDto = parse_input(params.source, "source")?;
        let api = self.ctx().api();
        let links = crate::context::read(|| api.list_links_for_source(source))?;
        to_json(&links)
    }

    #[tool(
        name = "find_backlinks",
        description = "Find object links that point at an AnnotationTargetDto."
    )]
    fn find_backlinks(
        &self,
        Parameters(params): Parameters<BacklinksParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let target: AnnotationTargetDto = parse_input(params.target, "target")?;
        let api = self.ctx().api();
        let links = crate::context::read(|| api.find_backlinks(target))?;
        to_json(&links)
    }

    #[tool(
        name = "list_text_anchors",
        description = "List annotation text anchors for a chapter. Read-only metadata."
    )]
    fn list_text_anchors(
        &self,
        Parameters(params): Parameters<ChapterIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let api = self.ctx().api();
        let anchors = crate::context::read(|| {
            api.list_text_anchors_for_chapter(params.project_id, params.chapter_id)
        })?;
        to_json(&anchors)
    }

    #[tool(
        name = "get_contribution_stats",
        description = "Read contribution statistics: historical human/AI/unknown operation volume and retained-content source distribution."
    )]
    fn get_contribution_stats(
        &self,
        Parameters(params): Parameters<ContributionStatsParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let api = self.ctx().api();
        let stats = crate::context::read(|| {
            api.get_contribution_stats(
                &params.start_date,
                &params.end_date,
                params.project_id.as_deref(),
                params.actor.as_deref(),
                params.object_type.as_deref(),
                params.device_id.as_deref(),
            )
        })?;
        to_json(&stats)
    }

    // --------------------------------------------------------- starmap writes

    #[tool(name = "create_starmap", description = "Create an empty starmap.")]
    fn create_starmap(
        &self,
        Parameters(params): Parameters<CreateStarmapParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let description = params.description.unwrap_or_default();
        let result = self.ctx().write(|api| {
            api.create_starmap(&params.title, &description, params.template_id.as_deref())
        })?;
        to_json(&result)
    }

    #[tool(name = "rename_starmap", description = "Rename a starmap.")]
    fn rename_starmap(
        &self,
        Parameters(params): Parameters<RenameStarmapParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let result = self
            .ctx()
            .write(|api| api.rename_starmap(&params.starmap_id, &params.new_title))?;
        to_json(&result)
    }

    #[tool(
        name = "delete_starmap",
        description = "Delete an empty or explicitly confirmed starmap."
    )]
    fn delete_starmap(
        &self,
        Parameters(params): Parameters<StarmapIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let result = self
            .ctx()
            .write(|api| api.delete_starmap(&params.starmap_id))?;
        to_json(&json!({ "deleted": result }))
    }

    #[tool(
        name = "create_starmap_node",
        description = "Create a starmap node. Inline summary/body or a full content object can be supplied."
    )]
    fn create_starmap_node(
        &self,
        Parameters(params): Parameters<CreateNodeParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let node_id = params
            .id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let kind = match params.kind.as_deref() {
            Some(value) => parse_enum(value, "kind")?,
            None => StarMapNodeKindDto::Note,
        };
        let content: StarMapNodeContentDto = match params.content {
            Some(value) => parse_input(value, "content")?,
            None => {
                if params.summary.is_some() || params.body.is_some() {
                    StarMapNodeContentDto {
                        kind: "inline".to_string(),
                        summary: params.summary,
                        body: params.body,
                        ..StarMapNodeContentDto::default()
                    }
                } else {
                    StarMapNodeContentDto::default()
                }
            }
        };
        let anchors = params
            .anchors
            .into_iter()
            .map(|value| parse_input::<StarMapAnchorDto>(value, "anchors"))
            .collect::<Result<Vec<_>, _>>()?;
        let portal = params
            .portal
            .map(|value| parse_input(value, "portal"))
            .transpose()?;
        let style = params
            .style
            .map(|value| parse_input::<StarMapNodeStyleDto>(value, "style"))
            .transpose()?
            .unwrap_or_default();
        let provenance = params
            .provenance
            .map(|value| parse_input(value, "provenance"))
            .transpose()?
            .unwrap_or_else(generated_by_mcp);
        let node = StarMapNodeDto {
            id: node_id,
            title: params.title,
            kind,
            payload: params.payload,
            tags: params.tags,
            content,
            anchors,
            portal,
            position: StarMapPointDto {
                x: params.x,
                y: params.y,
            },
            style,
            provenance,
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let result = self
            .ctx()
            .write(|api| api.add_starmap_node(&params.starmap_id, node, params.x, params.y))?;
        to_json(&result)
    }

    #[tool(
        name = "update_starmap_node",
        description = "Update a starmap node with a StarMapNodePatchDto JSON patch."
    )]
    fn update_starmap_node(
        &self,
        Parameters(params): Parameters<UpdateNodeParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let patch: StarMapNodePatchDto = parse_input(params.patch, "patch")?;
        let result = self
            .ctx()
            .write(|api| api.update_starmap_node_expected(
                &params.starmap_id,
                &params.node_id,
                patch,
                params.expected_revision,
            ))?;
        to_json(&result)
    }

    #[tool(name = "delete_starmap_node", description = "Delete a starmap node.")]
    fn delete_starmap_node(
        &self,
        Parameters(params): Parameters<NodeIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let result = self
            .ctx()
            .write(|api| api.delete_starmap_node(&params.starmap_id, &params.node_id))?;
        to_json(&json!({ "deleted": result }))
    }

    #[tool(
        name = "create_starmap_edge",
        description = "Create an edge between two StarMapTargetPathDto endpoints."
    )]
    fn create_starmap_edge(
        &self,
        Parameters(params): Parameters<CreateEdgeParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let from: StarMapTargetPathDto = parse_input(params.from, "from")?;
        let to: StarMapTargetPathDto = parse_input(params.to, "to")?;
        let kind: StarMapEdgeKindDto = match params.kind.as_deref() {
            Some(value) => parse_enum(value, "kind")?,
            None => StarMapEdgeKindDto::RelatedTo,
        };
        let edge = StarMapEdgeDto {
            id: params
                .id
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            from,
            to,
            kind,
            label: params.label,
            payload: params.payload,
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let result = self
            .ctx()
            .write(|api| api.add_starmap_edge(&params.starmap_id, edge))?;
        to_json(&result)
    }

    #[tool(
        name = "update_starmap_edge",
        description = "Update a starmap edge with a StarMapEdgePatchDto JSON patch."
    )]
    fn update_starmap_edge(
        &self,
        Parameters(params): Parameters<UpdateEdgeParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let patch: StarMapEdgePatchDto = parse_input(params.patch, "patch")?;
        let result = self
            .ctx()
            .write(|api| api.update_starmap_edge_expected(
                &params.starmap_id,
                &params.edge_id,
                patch,
                params.expected_revision,
            ))?;
        to_json(&result)
    }

    #[tool(name = "delete_starmap_edge", description = "Delete a starmap edge.")]
    fn delete_starmap_edge(
        &self,
        Parameters(params): Parameters<EdgeIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let result = self
            .ctx()
            .write(|api| api.delete_starmap_edge(&params.starmap_id, &params.edge_id))?;
        to_json(&json!({ "deleted": result }))
    }

    #[tool(
        name = "create_starmap_embed",
        description = "Embed an existing starmap inside a host starmap graph."
    )]
    fn create_starmap_embed(
        &self,
        Parameters(params): Parameters<CreateEmbedParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let host_path: StarMapTargetPathDto = parse_input(params.host_path, "hostPath")?;
        let provenance = params
            .provenance
            .map(|value| parse_input(value, "provenance"))
            .transpose()?
            .unwrap_or_else(generated_by_mcp);
        let embed = StarMapEmbedDto {
            instance_id: uuid::Uuid::new_v4().to_string(),
            target_starmap_id: params.target_starmap_id,
            label: params.label,
            position: StarMapPointDto {
                x: params.x,
                y: params.y,
            },
            host_path,
            provenance,
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let result = self
            .ctx()
            .write(|api| api.add_starmap_embed(&params.starmap_id, embed))?;
        to_json(&result)
    }

    #[tool(
        name = "create_starmap_child_embed",
        description = "Atomically create a child starmap and embed it in the host graph."
    )]
    fn create_starmap_child_embed(
        &self,
        Parameters(params): Parameters<CreateChildEmbedParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let position = StarMapPointDto {
            x: params.x,
            y: params.y,
        };
        let result = self.ctx().write(|api| {
            api.create_starmap_child_embed(&params.host_starmap_id, &params.title, position)
        })?;
        to_json(&result)
    }

    #[tool(
        name = "update_starmap_embed",
        description = "Update an embed with a StarMapEmbedPatchDto JSON patch."
    )]
    fn update_starmap_embed(
        &self,
        Parameters(params): Parameters<UpdateEmbedParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let patch: StarMapEmbedPatchDto = parse_input(params.patch, "patch")?;
        let result = self.ctx().write(|api| {
            api.update_starmap_embed_expected(
                &params.starmap_id,
                &params.instance_id,
                patch,
                params.expected_revision,
            )
        })?;
        to_json(&result)
    }

    #[tool(
        name = "delete_starmap_embed",
        description = "Remove an embed from a starmap."
    )]
    fn delete_starmap_embed(
        &self,
        Parameters(params): Parameters<EmbedIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let result = self
            .ctx()
            .write(|api| api.delete_starmap_embed(&params.starmap_id, &params.instance_id))?;
        to_json(&json!({ "deleted": result }))
    }

    #[tool(
        name = "create_starmap_link",
        description = "Create a link between two target paths inside a starmap."
    )]
    fn create_starmap_link(
        &self,
        Parameters(params): Parameters<CreateLinkParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let source: StarMapTargetPathDto = parse_input(params.source, "source")?;
        let target: StarMapTargetPathDto = parse_input(params.target, "target")?;
        let link = StarMapLinkDto {
            link_id: params
                .link_id
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            source,
            target,
            label: params.label,
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let result = self
            .ctx()
            .write(|api| api.add_starmap_link(&params.starmap_id, link))?;
        to_json(&result)
    }

    #[tool(
        name = "update_starmap_link",
        description = "Update a starmap link with a StarMapLinkPatchDto JSON patch."
    )]
    fn update_starmap_link(
        &self,
        Parameters(params): Parameters<UpdateLinkParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let patch: StarMapLinkPatchDto = parse_input(params.patch, "patch")?;
        let result = self
            .ctx()
            .write(|api| api.update_starmap_link_expected(
                &params.starmap_id,
                &params.link_id,
                patch,
                params.expected_revision,
            ))?;
        to_json(&result)
    }

    #[tool(name = "delete_starmap_link", description = "Delete a starmap link.")]
    fn delete_starmap_link(
        &self,
        Parameters(params): Parameters<LinkIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let result = self
            .ctx()
            .write(|api| api.delete_starmap_link(&params.starmap_id, &params.link_id))?;
        to_json(&json!({ "deleted": result }))
    }

    #[tool(
        name = "create_starmap_hyperlink",
        description = "Create an external hyperlink from a starmap target path."
    )]
    fn create_starmap_hyperlink(
        &self,
        Parameters(params): Parameters<CreateHyperlinkParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let source: StarMapTargetPathDto = parse_input(params.source, "source")?;
        let hyperlink = StarMapHyperlinkDto {
            hyperlink_id: params
                .hyperlink_id
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            source,
            target_uri: params.target_uri,
            label: params.label,
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let result = self
            .ctx()
            .write(|api| api.add_starmap_hyperlink(&params.starmap_id, hyperlink))?;
        to_json(&result)
    }

    #[tool(
        name = "update_starmap_hyperlink",
        description = "Update a starmap hyperlink with a StarMapHyperlinkPatchDto JSON patch."
    )]
    fn update_starmap_hyperlink(
        &self,
        Parameters(params): Parameters<UpdateHyperlinkParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let patch: StarMapHyperlinkPatchDto = parse_input(params.patch, "patch")?;
        let result = self.ctx().write(|api| {
            api.update_starmap_hyperlink_expected(
                &params.starmap_id,
                &params.hyperlink_id,
                patch,
                params.expected_revision,
            )
        })?;
        to_json(&result)
    }

    #[tool(
        name = "delete_starmap_hyperlink",
        description = "Delete a starmap hyperlink."
    )]
    fn delete_starmap_hyperlink(
        &self,
        Parameters(params): Parameters<HyperlinkIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let result = self
            .ctx()
            .write(|api| api.delete_starmap_hyperlink(&params.starmap_id, &params.hyperlink_id))?;
        to_json(&json!({ "deleted": result }))
    }

    #[tool(
        name = "bind_starmap_to_project",
        description = "Bind a starmap to a project."
    )]
    fn bind_starmap_to_project(
        &self,
        Parameters(params): Parameters<BindStarmapParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let result = self
            .ctx()
            .write(|api| api.bind_starmap_to_project(&params.starmap_id, &params.project_id))?;
        to_json(&json!({ "bound": result }))
    }

    #[tool(
        name = "unbind_starmap_from_project",
        description = "Remove a starmap from its project without deleting it."
    )]
    fn unbind_starmap_from_project(
        &self,
        Parameters(params): Parameters<StarmapIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let result = self
            .ctx()
            .write(|api| api.unbind_starmap_from_project(&params.starmap_id))?;
        to_json(&json!({ "unbound": result }))
    }

    // ------------------------------------------------------------- tag writes

    #[tool(
        name = "create_tag",
        description = "Create an annotation tag in a project."
    )]
    fn create_tag(
        &self,
        Parameters(params): Parameters<CreateTagParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let result = self
            .ctx()
            .write(|api| api.create_tag(params.project_id, params.name, params.color))?;
        to_json(&result)
    }

    #[tool(name = "rename_tag", description = "Rename an annotation tag.")]
    fn rename_tag(
        &self,
        Parameters(params): Parameters<RenameTagParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let result = self
            .ctx()
            .write(|api| api.rename_tag(params.project_id, params.tag_id, params.name))?;
        to_json(&result)
    }

    #[tool(
        name = "delete_tag",
        description = "Delete an annotation tag and its bindings."
    )]
    fn delete_tag(
        &self,
        Parameters(params): Parameters<TagIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let result = self
            .ctx()
            .write(|api| api.delete_tag(params.project_id, params.tag_id))?;
        to_json(&json!({ "deleted": result }))
    }

    #[tool(
        name = "merge_tags",
        description = "Merge the source tag into the target tag."
    )]
    fn merge_tags(
        &self,
        Parameters(params): Parameters<MergeTagsParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let result = self.ctx().write(|api| {
            api.merge_tags(
                params.project_id,
                params.source_tag_id,
                params.target_tag_id,
            )
        })?;
        to_json(&json!({ "merged": result }))
    }

    #[tool(
        name = "set_tag_bindings",
        description = "Replace all tag bindings on an AnnotationObjectRefDto target."
    )]
    fn set_tag_bindings(
        &self,
        Parameters(params): Parameters<SetBindingsParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let target: AnnotationObjectRefDto = parse_input(params.target, "target")?;
        let result = self
            .ctx()
            .write(|api| api.set_tag_bindings(params.project_id, target, params.tag_ids))?;
        to_json(&result)
    }

    // ---------------------------------------------------- object-link writes

    #[tool(
        name = "create_object_link",
        description = "Create a cross-object link from an AnnotationObjectRefDto to an AnnotationTargetDto."
    )]
    fn create_object_link(
        &self,
        Parameters(params): Parameters<CreateObjectLinkParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let source: AnnotationObjectRefDto = parse_input(params.source, "source")?;
        let target: AnnotationTargetDto = parse_input(params.target, "target")?;
        let result = self.ctx().write(|api| api.create_link(source, target))?;
        to_json(&result)
    }

    #[tool(
        name = "update_object_link",
        description = "Update an object link's source and target."
    )]
    fn update_object_link(
        &self,
        Parameters(params): Parameters<UpdateObjectLinkParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let source: AnnotationObjectRefDto = parse_input(params.source, "source")?;
        let target: AnnotationTargetDto = parse_input(params.target, "target")?;
        let result = self
            .ctx()
            .write(|api| api.update_link(params.project_id, params.link_id, source, target))?;
        to_json(&result)
    }

    #[tool(name = "delete_object_link", description = "Delete an object link.")]
    fn delete_object_link(
        &self,
        Parameters(params): Parameters<ObjectLinkIdParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let result = self
            .ctx()
            .write(|api| api.delete_link(params.project_id, params.link_id))?;
        to_json(&json!({ "deleted": result }))
    }

    #[tool(
        name = "create_chapter_text_anchor",
        description = "Create annotation metadata anchoring a chapter text byte range. The chapter body is never modified."
    )]
    fn create_chapter_text_anchor(
        &self,
        Parameters(params): Parameters<CreateTextAnchorParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let result = self.ctx().write(|api| {
            api.mcp_create_chapter_text_anchor(
                &params.project_id,
                &params.chapter_id,
                params.start_byte,
                params.end_byte,
                params.expected_hash.as_deref(),
            )
        })?;
        to_json(&result)
    }
}
