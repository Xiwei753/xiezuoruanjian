// =============================================================================
// starmap_view/preview_cache.rs — 子星图微缩预览快照（Linux 平台端）
// =============================================================================
//
// 层级：Linux_qt 平台层显示几何缓存。
//
// 职责：
//   为父画布中的每个子星图入口构建一份与相机无关的不可变预览快照：
//   graph 内容摘要、普通节点布局尺寸、子入口几何、边几何、包围盒。
//   同一 (starmapId, graphRevision) 的多个入口共享同一份快照；只有图内容
//   真正变化时才产生新 revision，缩放 / 平移不会重建快照。
//
// 边界：
//   - 快照在后台线程读取 Core 并计算，QML Loader 生命周期里不再同步读图、
//     实测文本或计算边。
//   - 节点尺寸是显示层的排版估算，不是业务状态；真实节点位置、标题和正文
//     仍来自 Core graph。QML 只按快照做矢量投影。
// =============================================================================

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use writer_core::api::types::StarMapGraphDto;

/// 子星图预览的不可变显示快照。字段以 camelCase 序列化给 QML。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSnapshot {
    pub starmap_id: String,
    pub graph_revision: String,
    pub nodes: Vec<PreviewNode>,
    pub embeds: Vec<PreviewEmbed>,
    pub edges: Vec<serde_json::Value>,
    pub bounds: Option<PreviewBounds>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewNode {
    pub id: String,
    pub title: String,
    pub body: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewEmbed {
    pub instance_id: String,
    pub target_starmap_id: String,
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub diameter: f32,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewBounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
    pub width: f32,
    pub height: f32,
}

const CARD_WIDTH: f32 = 220.0;
const MIN_HEIGHT: f32 = 64.0;
const PADDING: f32 = 12.0;
const TITLE_FONT_SIZE: f32 = 14.0;
const BODY_FONT_SIZE: f32 = 12.0;
const TITLE_LINE_HEIGHT: f32 = 20.0;
const BODY_LINE_HEIGHT: f32 = 17.0;
const TITLE_MAX_LINES: usize = 2;
const BODY_MAX_LINES: usize = 5;
const TITLE_BODY_GAP: f32 = 6.0;
const TEXT_WIDTH: f32 = CARD_WIDTH - PADDING * 2.0;
const EMBED_DIAMETER: f32 = 200.0;

/// 从背景线程读取到的 graph DTO 构建快照。只做显示几何，不写 Core。
pub fn build_from_dto(dto: StarMapGraphDto) -> Result<PreviewSnapshot, String> {
    let graph_json =
        serde_json::to_string(&dto).map_err(|error| format!("序列化星图失败: {error}"))?;
    let graph_revision = graph_revision(&graph_json);
    let starmap_id = dto.starmap_id.clone();

    let core_graph = writer_core::starmap::types::StarMapGraph::try_from(dto.clone())
        .map_err(|error| error.to_string())?;

    let mut nodes = Vec::new();
    let mut embeds = Vec::new();
    let mut node_scenes = Vec::new();
    let mut bounds: Option<PreviewBounds> = None;

    for node in &dto.nodes {
        let x = node.position.x as f32;
        let y = node.position.y as f32;
        if let Some(portal) = &node.portal {
            if !portal.destination_starmap_id.is_empty() {
                let label = if node.title.is_empty() {
                    "未命名".to_string()
                } else {
                    node.title.clone()
                };
                embeds.push(PreviewEmbed {
                    instance_id: format!("legacy-portal:{}", node.id),
                    target_starmap_id: portal.destination_starmap_id.clone(),
                    label,
                    x,
                    y,
                    diameter: EMBED_DIAMETER,
                });
                include_rect(&mut bounds, x, y, EMBED_DIAMETER, EMBED_DIAMETER);
                node_scenes.push(serde_json::json!({
                    "id": node.id,
                    "x": x,
                    "y": y,
                    "width": EMBED_DIAMETER,
                    "height": EMBED_DIAMETER,
                }));
                continue;
            }
        }

        let (width, height) = measure_node(node);
        let body = display_body(node);
        nodes.push(PreviewNode {
            id: node.id.clone(),
            title: if node.title.is_empty() {
                "未命名".to_string()
            } else {
                node.title.clone()
            },
            body,
            x,
            y,
            width,
            height,
        });
        include_rect(&mut bounds, x, y, width, height);
        node_scenes.push(serde_json::json!({
            "id": node.id,
            "x": x,
            "y": y,
            "width": width,
            "height": height,
        }));
    }

    for embed in &dto.embeds {
        let x = embed.position.x as f32;
        let y = embed.position.y as f32;
        embeds.push(PreviewEmbed {
            instance_id: embed.instance_id.clone(),
            target_starmap_id: embed.target_starmap_id.clone(),
            label: embed
                .label
                .clone()
                .filter(|label| !label.is_empty())
                .unwrap_or_else(|| "未命名".to_string()),
            x,
            y,
            diameter: EMBED_DIAMETER,
        });
        include_rect(&mut bounds, x, y, EMBED_DIAMETER, EMBED_DIAMETER);
    }

    let embed_scenes: Vec<serde_json::Value> = dto
        .embeds
        .iter()
        .map(|embed| {
            serde_json::json!({
                "instanceId": embed.instance_id,
                "x": embed.position.x,
                "y": embed.position.y,
                "width": EMBED_DIAMETER,
                "height": EMBED_DIAMETER,
            })
        })
        .collect();

    let nodes_json = serde_json::to_string(&node_scenes)
        .map_err(|error| format!("序列化节点几何失败: {error}"))?;
    let embeds_json = serde_json::to_string(&embed_scenes)
        .map_err(|error| format!("序列化入口几何失败: {error}"))?;
    let render_json = crate::starmap_view::bridge::compute_edge_renders_json(
        &core_graph,
        &nodes_json,
        &embeds_json,
    );
    let edges = serde_json::from_str::<serde_json::Value>(&render_json)
        .ok()
        .and_then(|value| {
            if value.get("success").and_then(|v| v.as_bool()) == Some(true) {
                value.get("data").and_then(|data| data.as_array()).cloned()
            } else {
                None
            }
        })
        .unwrap_or_default();

    Ok(PreviewSnapshot {
        starmap_id,
        graph_revision,
        nodes,
        embeds,
        edges,
        bounds,
    })
}

pub fn to_json(snapshot: &PreviewSnapshot) -> Result<String, String> {
    serde_json::to_string(snapshot).map_err(|error| format!("序列化预览快照失败: {error}"))
}

fn graph_revision(graph_json: &str) -> String {
    let mut hasher = DefaultHasher::new();
    graph_json.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn include_rect(bounds: &mut Option<PreviewBounds>, x: f32, y: f32, width: f32, height: f32) {
    let next_min_x = x;
    let next_min_y = y;
    let next_max_x = x + width;
    let next_max_y = y + height;
    *bounds = Some(match *bounds {
        Some(current) => PreviewBounds {
            min_x: current.min_x.min(next_min_x),
            min_y: current.min_y.min(next_min_y),
            max_x: current.max_x.max(next_max_x),
            max_y: current.max_y.max(next_max_y),
            width: 0.0,
            height: 0.0,
        },
        None => PreviewBounds {
            min_x: next_min_x,
            min_y: next_min_y,
            max_x: next_max_x,
            max_y: next_max_y,
            width: 0.0,
            height: 0.0,
        },
    });
    if let Some(current) = bounds.as_mut() {
        current.width = current.max_x - current.min_x;
        current.height = current.max_y - current.min_y;
    }
}

fn display_body(node: &writer_core::api::types::StarMapNodeDto) -> String {
    if let Some(body) = &node.content.body {
        return body.clone();
    }
    node.content.summary.clone().unwrap_or_default()
}

fn measure_node(node: &writer_core::api::types::StarMapNodeDto) -> (f32, f32) {
    let title = if node.title.is_empty() {
        "未命名"
    } else {
        node.title.as_str()
    };
    let title_lines = approximate_lines(title, TITLE_FONT_SIZE, TITLE_MAX_LINES);
    let title_height = title_lines as f32 * TITLE_LINE_HEIGHT;

    // 与 StarMapNodeMetrics.bodyOf 一致：只有 empty / inline 才把正文算进高度。
    let body = if node.content.kind == "empty" || node.content.kind == "inline" {
        node.content.body.as_deref().unwrap_or("")
    } else {
        ""
    };
    let body_lines = if body.is_empty() {
        0
    } else {
        approximate_lines(body, BODY_FONT_SIZE, BODY_MAX_LINES)
    };
    let body_height = body_lines as f32 * BODY_LINE_HEIGHT;

    let height = (24.0
        + title_height
        + if body_height > 0.0 {
            TITLE_BODY_GAP + body_height
        } else {
            0.0
        })
    .ceil()
    .max(MIN_HEIGHT);
    (CARD_WIDTH, height)
}

fn approximate_lines(text: &str, font_px: f32, max_lines: usize) -> usize {
    let mut lines = 1usize;
    let mut line_width = 0.0f32;
    for ch in text.chars() {
        if ch == '\n' {
            lines += 1;
            line_width = 0.0;
            if lines >= max_lines {
                return max_lines;
            }
            continue;
        }
        let advance = if ch == ' ' || ch == '\t' {
            font_px * 0.35
        } else if ch.is_ascii() {
            font_px * 0.58
        } else {
            font_px
        };
        if line_width + advance > TEXT_WIDTH && line_width > 0.0 {
            lines += 1;
            line_width = advance;
            if lines >= max_lines {
                return max_lines;
            }
        } else {
            line_width += advance;
        }
    }
    lines.min(max_lines)
}
