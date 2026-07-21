//! # 星图语义模型（Core 层）
//!
//! 定义节点内容类型（内联文本、章节引用、实体引用、外部链接）、
//! 锚点、链接、嵌入和深目标（DeepTarget）等语义结构。
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
/// - `EnterChild`：点击后进入子星图编辑空间
/// - `PreviewInline`：在当前星图内内联预览子星图
/// - `ReferenceOnly`：仅作为引用标记，不提供交互入口
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapPortal {
    pub target_starmap_id: String,
    #[serde(default)]
    pub deep_target: Option<StarMapDeepTarget>,
    #[serde(default)]
    pub mode: StarMapPortalMode,
    #[serde(default)]
    pub preview_policy: StarMapPortalPreviewPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub enum StarMapPortalMode {
    EnterChild,
    PreviewInline,
    #[default]
    ReferenceOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub enum StarMapPortalPreviewPolicy {
    #[default]
    Auto,
    Always,
    Never,
}

/// 显示策略：控制节点/嵌入在不同缩放级别下的可见内容。
///
/// ## Scale 层级不变量
///
/// `min_visible_scale <= title_scale <= summary_scale <= detail_scale`
///
/// - `min_visible_scale`：节点开始可见的最低缩放
/// - `title_scale`：标题文字可读的缩放
/// - `summary_scale`：摘要可读的缩放
/// - `detail_scale`：完整详情可读的缩放
///
/// `importance` 影响自动布局中的节点排序权重。
/// `max_preview_chars` 限制内联预览文本长度（防止大文本拖慢渲染）。
/// `min_readable_px` 为平台端提供最小可读像素阈值参考。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapDisplayPolicy {
    pub importance: f32,
    pub min_visible_scale: f32,
    pub title_scale: f32,
    pub summary_scale: f32,
    pub detail_scale: f32,
    pub max_preview_chars: u32,
    pub min_readable_px: f32,
}

impl Default for StarMapDisplayPolicy {
    fn default() -> Self {
        Self {
            importance: 1.0,
            min_visible_scale: 0.1,
            title_scale: 0.2,
            summary_scale: 0.5,
            detail_scale: 1.0,
            max_preview_chars: 100,
            min_readable_px: 12.0,
        }
    }
}

/// 校验 DisplayPolicy 的 scale 层级不变量和数值合法性。
///
/// 不变量：`min_visible_scale <= title_scale <= summary_scale <= detail_scale`，
/// 所有值非 NaN、非负，`max_preview_chars ≤ 10000`。
pub fn validate_display_policy(dp: &StarMapDisplayPolicy) -> crate::error::Result<()> {
    if dp.importance.is_nan()
        || dp.importance < 0.0
        || dp.min_visible_scale.is_nan()
        || dp.min_visible_scale < 0.0
        || dp.title_scale.is_nan()
        || dp.title_scale < 0.0
        || dp.summary_scale.is_nan()
        || dp.summary_scale < 0.0
        || dp.detail_scale.is_nan()
        || dp.detail_scale < 0.0
        || dp.min_readable_px.is_nan()
        || dp.min_readable_px < 0.0
    {
        return Err(crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid display policy values",
        )));
    }

    if !(dp.min_visible_scale <= dp.title_scale
        && dp.title_scale <= dp.summary_scale
        && dp.summary_scale <= dp.detail_scale)
    {
        return Err(crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Display policy scales must be ordered: min_visible <= title <= summary <= detail",
        )));
    }
    if dp.max_preview_chars > 10000 {
        return Err(crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "max_preview_chars cannot exceed 10000",
        )));
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapOpenBehavior {
    #[default]
    Inspector,
    ExpandCard,
    WritingMode,
    JumpToAnchor,
    EnterPortal,
    #[serde(other)]
    Custom,
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

/// 深目标：描述跨星图层级的引用路径。
///
/// ## 路径结构
///
/// - `starmap_id`：起始星图 ID
/// - `path`：中间层级穿越段（目前只有 `EnterChild`——进入子星图空间）
/// - `target`：路径终点的具体引用（节点/锚点/章节范围等）
///
/// 路径中间层只允许进入子星图空间（`EnterChild`），节点是原子，
/// 不能作为路径段"进入"。节点只能作为终点的 `target`。
///
/// ## 验证
///
/// `resolve_deep_target` 会校验：深度 ≤ 32、无循环、每层星图存在、
/// 终节点/锚点在目标星图中存在、章节范围合法。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapDeepTarget {
    pub starmap_id: String,
    #[serde(default)]
    pub path: Vec<StarMapPathSegment>,
    pub target: StarMapTargetDetail,
}

/// 路径段：描述一次层级穿越。
///
/// 路径中间层只允许进入子星图空间，节点是原子，不能作为路径段"进入"。
/// 节点只能作为路径终点的 `target`（`StarMapTargetDetail::Node` / `Anchor`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StarMapPathSegment {
    /// 进入子星图空间。`starmap_id` 是目标子星图的 ID。
    EnterChild { starmap_id: String },
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapTargetDisplayStatus {
    #[default]
    Unresolved,
    TitleOnly,
    TitleSummary,
    MiniMap,
    ExpandedGraph,
}

/// 深目标解析状态。
///
/// - `Resolved`：路径完整可达
/// - `MissingStarmap/Node/Anchor`：引用的目标不存在
/// - `TooDeep`：路径超过 32 层深度限制
/// - `CycleDetected`：路径中存在循环引用
/// - `InvalidRange`：章节范围 range_start > range_end
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapTargetResolveStatus {
    #[default]
    Unresolved,
    Resolved,
    MissingStarmap,
    MissingNode,
    MissingAnchor,
    TooDeep,
    CycleDetected,
    InvalidRange,
}

/// 计算目标展示状态，只提供底层计算语义。
pub fn resolve_target_display_status(
    _deep_target: &StarMapDeepTarget,
    current_scale: f32,
    display_policy: Option<&StarMapDisplayPolicy>,
    is_resolved: bool,
) -> StarMapTargetDisplayStatus {
    if !is_resolved {
        return StarMapTargetDisplayStatus::Unresolved;
    }

    let default_policy = StarMapDisplayPolicy::default();
    let dp = display_policy.unwrap_or(&default_policy);

    if current_scale < dp.min_visible_scale {
        return StarMapTargetDisplayStatus::TitleOnly;
    }

    if current_scale >= dp.detail_scale {
        return StarMapTargetDisplayStatus::ExpandedGraph;
    } else if current_scale >= dp.summary_scale {
        return StarMapTargetDisplayStatus::MiniMap;
    } else if current_scale >= dp.title_scale {
        return StarMapTargetDisplayStatus::TitleSummary;
    }

    StarMapTargetDisplayStatus::TitleOnly
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // StarMapPathSegment 只有 EnterChild，没有 EnterNode
    // -----------------------------------------------------------------------
    #[test]
    fn test_path_segment_only_enter_child() {
        // StarMapPathSegment 只有 EnterChild 变体，节点是原子不能作为路径段
        let segment = StarMapPathSegment::EnterChild {
            starmap_id: "sm_child".to_string(),
        };

        // 序列化/反序列化 roundtrip
        let json = serde_json::to_string(&segment).unwrap();
        let deserialized: StarMapPathSegment = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, segment);
    }

    #[test]
    fn test_path_segment_rejects_enter_node_json() {
        // 旧格式的 enterNode JSON 不应该被反序列化为有效的 StarMapPathSegment
        // 因为 StarMapPathSegment 现在只有 EnterChild 变体，
        // serde 的 #[serde(tag = "type")] 会拒绝未知变体
        let old_enter_node_json = r#"{"type": "enterNode", "nodeId": "n1"}"#;
        let result: Result<StarMapPathSegment, _> = serde_json::from_str(old_enter_node_json);
        assert!(
            result.is_err(),
            "enterNode should not deserialize as a valid StarMapPathSegment"
        );
    }

    // -----------------------------------------------------------------------
    // 多层 child starmap -> node 合法
    // -----------------------------------------------------------------------
    #[test]
    fn test_deep_target_multi_layer_child_to_node() {
        // 合法路径：工具星图 -> AI工具星图 -> 大模型星图 -> GPT节点
        // 前面几层是 EnterChild，最后的 GPT 是 endpoint (StarMapTargetDetail::Node)
        let dt = StarMapDeepTarget {
            starmap_id: "sm_tools".to_string(),
            path: vec![
                StarMapPathSegment::EnterChild {
                    starmap_id: "sm_ai_tools".to_string(),
                },
                StarMapPathSegment::EnterChild {
                    starmap_id: "sm_llm".to_string(),
                },
            ],
            target: StarMapTargetDetail::Node {
                node_id: "gpt_node".to_string(),
            },
        };

        // 验证路径结构：中间层只有 EnterChild
        assert_eq!(dt.path.len(), 2);
        for seg in &dt.path {
            match seg {
                StarMapPathSegment::EnterChild { .. } => {} // 合法
            }
        }

        // 验证终点是 Node
        match &dt.target {
            StarMapTargetDetail::Node { node_id } => {
                assert_eq!(node_id, "gpt_node");
            }
            _ => panic!("Expected Node target"),
        }

        // 序列化/反序列化 roundtrip
        let json = serde_json::to_string(&dt).unwrap();
        let deserialized: StarMapDeepTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, dt);
    }

    #[test]
    fn test_deep_target_empty_path_to_node() {
        // 直接指向当前星图的节点，无中间层
        let dt = StarMapDeepTarget {
            starmap_id: "sm_1".to_string(),
            path: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        };

        assert!(dt.path.is_empty());
        let json = serde_json::to_string(&dt).unwrap();
        let deserialized: StarMapDeepTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, dt);
    }

    #[test]
    fn test_deep_target_multi_layer_to_anchor() {
        // 多层 child starmap -> anchor 合法
        let dt = StarMapDeepTarget {
            starmap_id: "sm_root".to_string(),
            path: vec![StarMapPathSegment::EnterChild {
                starmap_id: "sm_child".to_string(),
            }],
            target: StarMapTargetDetail::Anchor {
                node_id: "n1".to_string(),
                anchor_id: "a1".to_string(),
            },
        };

        let json = serde_json::to_string(&dt).unwrap();
        let deserialized: StarMapDeepTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, dt);
    }
}
