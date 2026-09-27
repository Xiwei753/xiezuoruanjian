//! # 星图语义模型（Core 层）
//!
//! 定义节点内容类型（内联文本、章节引用、实体引用、外部链接）、
//! 锚点、链接、嵌入和传送门等语义结构。
//! 这些类型是星图数据模型的跨平台契约，平台端只负责渲染和交互。

use serde::{Deserialize, Serialize};

/// 节点内容类型。
///
/// `ChapterRef` 中的 `range_start`/`range_end` 为 UTF-8 byte offset（半开区间），
/// 指向章节正文中的引用范围。`None` 表示引用整个章节。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StarMapNodeContent {
    #[default]
    Empty,
    Inline {
        summary: Option<String>,
        body: Option<String>,
    },
    ChapterRef {
        project_id: String,
        volume_id: Option<String>,
        chapter_id: String,
        range_start: Option<u32>,
        range_end: Option<u32>,
    },
    EntityRef {
        entity_type: String,
        entity_id: String,
    },
    ExternalRef {
        uri: String,
        label: Option<String>,
    },
}

impl StarMapNodeContent {
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn search_text(&self) -> String {
        let mut parts = Vec::new();
        match self {
            StarMapNodeContent::Inline { summary, body } => {
                if let Some(ref s) = summary {
                    if !s.is_empty() {
                        parts.push(s.clone());
                    }
                }
                if let Some(ref b) = body {
                    if !b.is_empty() {
                        parts.push(b.clone());
                    }
                }
            }
            StarMapNodeContent::ChapterRef { chapter_id, .. } => {
                if !chapter_id.is_empty() {
                    parts.push(chapter_id.clone());
                }
            }
            StarMapNodeContent::EntityRef {
                entity_type,
                entity_id,
            } => {
                if !entity_type.is_empty() {
                    parts.push(entity_type.clone());
                }
                if !entity_id.is_empty() {
                    parts.push(entity_id.clone());
                }
            }
            StarMapNodeContent::ExternalRef { uri, label } => {
                if let Some(ref l) = label {
                    if !l.is_empty() {
                        parts.push(l.clone());
                    }
                }
                if !uri.is_empty() {
                    parts.push(uri.clone());
                }
            }
            StarMapNodeContent::Empty => {}
        }
        parts.join(" ")
    }
}

/// 锚点：节点内的可引用定位点。
///
/// 锚点允许边精确连接到节点内部的特定位置（如章节段落、实体属性），
/// 而不仅仅是节点整体。`role` 描述锚点在关系中的角色。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapAnchor {
    pub anchor_id: String,
    pub target: StarMapAnchorTarget,
    pub label: Option<String>,
    #[serde(default)]
    pub role: StarMapAnchorRole,
}

/// 锚点目标：锚点指向的具体资源。
///
/// `ChapterRange` 中的 `range_start`/`range_end` 为 UTF-8 byte offset（半开区间）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StarMapAnchorTarget {
    ChapterRange {
        project_id: Option<String>,
        volume_id: Option<String>,
        chapter_id: String,
        range_start: Option<u32>,
        range_end: Option<u32>,
    },
    Project {
        project_id: String,
    },
    Volume {
        project_id: Option<String>,
        volume_id: String,
    },
    Chapter {
        project_id: Option<String>,
        volume_id: Option<String>,
        chapter_id: String,
    },
    Character {
        entity_id: String,
    },
    Item {
        entity_id: String,
    },
    Location {
        entity_id: String,
    },
    Event {
        entity_id: String,
    },
    Starmap {
        starmap_id: String,
    },
    External {
        uri: String,
    },
    Custom {
        payload: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub enum StarMapAnchorRole {
    Source,
    Destination,
    #[default]
    Reference,
    #[serde(other)]
    Custom,
}

/// 传送门：节点进入子星图的入口。
///
/// Portal 是一个明确的跳转定义，直接持有目标星图 ID 和可选落点，
/// 不再嵌套 `StarMapTargetPath`（避免 Portal 自身持有一条可能再次包含
/// `EnterPortal` 的路径，那会与 `EnterPortal { node_id }` 路径段语义冲突）。
///
/// - `destination_starmap_id`：portal 跳转的目标星图 ID
/// - `destination_target`：可选的目标落点（目标图内的 Node/Anchor/ChapterRange 等）
///
/// 显示/交互策略（mode、preview_policy）已退出 Core，由平台端自行管理。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapPortal {
    pub destination_starmap_id: String,
    #[serde(default)]
    pub destination_target: Option<StarMapTargetDetail>,
}

/// 来源溯源（Provenance）：记录节点/嵌入的创建来源和审核状态。
///
/// ## 审计语义
///
/// - `source`：创建来源（Human/Import/Plugin/Ai/System）
/// - `review_status`：审核状态（Accepted/Draft/NeedsReview/Rejected）
/// - `generated_by`/`prompt_id`：AI 生成时的模型和提示词标识
/// - `created_from_anchor`：从锚点自动创建时的来源锚点 ID
///
/// 平台端可据此实现审核工作流和来源过滤。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapProvenance {
    #[serde(default)]
    pub source: StarMapSourceKind,
    pub source_id: Option<String>,
    pub generated_by: Option<String>,
    pub prompt_id: Option<String>,
    #[serde(default)]
    pub review_status: StarMapReviewStatus,
    pub created_from_anchor: Option<String>,
}

impl Default for StarMapProvenance {
    fn default() -> Self {
        Self {
            source: StarMapSourceKind::Human,
            source_id: None,
            generated_by: None,
            prompt_id: None,
            review_status: StarMapReviewStatus::Accepted,
            created_from_anchor: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapSourceKind {
    #[default]
    Human,
    Import,
    Plugin,
    Ai,
    System,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapReviewStatus {
    #[default]
    Accepted,
    Draft,
    NeedsReview,
    Rejected,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StarMapTargetDetail {
    Starmap,
    Node {
        node_id: String,
    },
    Anchor {
        node_id: String,
        anchor_id: String,
    },
    /// 章节范围引用。`range_start`/`range_end` 为 UTF-8 byte offset（半开区间）。
    ChapterRange {
        project_id: Option<String>,
        volume_id: Option<String>,
        chapter_id: String,
        range_start: Option<u32>,
        range_end: Option<u32>,
    },
    Entity {
        entity_type: String,
        entity_id: String,
    },
    External {
        uri: String,
    },
}

/// 目标路径解析状态。
///
/// - `Resolved`：路径完整可达
/// - `MissingStarmap/Node/Anchor`：引用的目标不存在
/// - `MissingEmbed/MissingPortal`：路径段引用的嵌入/传送门不存在
/// - `TooDeep`：路径超过 32 层深度限制
/// - `CycleDetected`：路径中存在循环引用
/// - `InvalidRange`：章节范围 range_start > range_end
/// - `UnsupportedVersion`：目标星图 graph.json schema 版本不被支持
/// - `CorruptStarmap`：目标星图对象文件损坏（JSON 解析失败等）
/// - `ReadFailed`：目标星图对象文件读取失败（IO 错误等）
///
/// 后三个变体由 resolver 的只读 provider 返回，用于区分"对象真不存在"
/// 与"读取失败/版本不兼容"，避免把磁盘错误吞成 MissingNode/MissingEmbed。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapTargetResolveStatus {
    #[default]
    Unresolved,
    Resolved,
    MissingStarmap,
    MissingNode,
    MissingAnchor,
    MissingEmbed,
    MissingPortal,
    TooDeep,
    CycleDetected,
    InvalidRange,
    UnsupportedVersion,
    CorruptStarmap,
    ReadFailed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::starmap::types::reference::{StarMapPathSegment, StarMapTargetPath};

    // -----------------------------------------------------------------------
    // StarMapPathSegment 有 EnterEmbed 和 EnterPortal
    // -----------------------------------------------------------------------
    #[test]
    fn test_path_segment_enter_embed_roundtrip() {
        let segment = StarMapPathSegment::EnterEmbed {
            instance_id: "embed_1".to_string(),
        };

        let json = serde_json::to_string(&segment).unwrap();
        let deserialized: StarMapPathSegment = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, segment);
    }

    #[test]
    fn test_path_segment_enter_portal_roundtrip() {
        let segment = StarMapPathSegment::EnterPortal {
            node_id: "n1".to_string(),
        };

        let json = serde_json::to_string(&segment).unwrap();
        let deserialized: StarMapPathSegment = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, segment);
    }

    #[test]
    fn test_path_segment_rejects_enter_child_json() {
        // 旧格式的 enterChild JSON 不应该被反序列化为有效的 StarMapPathSegment
        let old_enter_child_json = r#"{"type": "enterChild", "starmapId": "sm1"}"#;
        let result: Result<StarMapPathSegment, _> = serde_json::from_str(old_enter_child_json);
        assert!(
            result.is_err(),
            "enterChild should not deserialize as a valid StarMapPathSegment"
        );
    }

    // -----------------------------------------------------------------------
    // 多层路径 -> node 合法
    // -----------------------------------------------------------------------
    #[test]
    fn test_target_path_multi_layer_to_node() {
        let path = StarMapTargetPath {
            starmap_id: "sm_tools".to_string(),
            segments: vec![
                StarMapPathSegment::EnterEmbed {
                    instance_id: "emb_ai_tools".to_string(),
                },
                StarMapPathSegment::EnterPortal {
                    node_id: "portal_llm".to_string(),
                },
            ],
            target: StarMapTargetDetail::Node {
                node_id: "gpt_node".to_string(),
            },
        };

        assert_eq!(path.segments.len(), 2);

        let json = serde_json::to_string(&path).unwrap();
        let deserialized: StarMapTargetPath = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, path);
    }

    #[test]
    fn test_target_path_empty_segments_to_node() {
        let path = StarMapTargetPath {
            starmap_id: "sm_1".to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        };

        assert!(path.segments.is_empty());
        let json = serde_json::to_string(&path).unwrap();
        let deserialized: StarMapTargetPath = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, path);
    }

    #[test]
    fn test_target_path_to_anchor() {
        let path = StarMapTargetPath {
            starmap_id: "sm_root".to_string(),
            segments: vec![StarMapPathSegment::EnterEmbed {
                instance_id: "emb_child".to_string(),
            }],
            target: StarMapTargetDetail::Anchor {
                node_id: "n1".to_string(),
                anchor_id: "a1".to_string(),
            },
        };

        let json = serde_json::to_string(&path).unwrap();
        let deserialized: StarMapTargetPath = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, path);
    }
}
