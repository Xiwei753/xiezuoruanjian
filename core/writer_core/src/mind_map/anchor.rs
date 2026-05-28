//! # 思维导图锚点和链接管理模块
//!
//! 本模块负责管理思维导图节点与章节内容之间的关联关系，通过锚点（Anchor）
//! 和链接（Link）机制实现思维导图与文本内容的深度集成。
//!
//! ## 核心概念
//!
//! ### 锚点 (`MindMapAnchor`)
//! 锚点表示章节文本中的一个位置范围，用于关联思维导图节点：
//! - `id`：锚点唯一标识符
//! - `project_id`：所属项目ID
//! - `chapter_id`：关联的章节ID
//! - `start_offset`：起始字符偏移量
//! - `end_offset`：结束字符偏移量
//! - `selected_text`：选中的文本内容
//! - `prefix_text`：选中文本前的上下文
//! - `suffix_text`：选中文本后的上下文
//! - `checksum`：文本内容的校验和
//! - `created_at`：创建时间戳
//! - `updated_at`：更新时间戳
//!
//! ### 链接 (`MindMapLink`)
//! 链接表示思维导图节点与锚点之间的关联关系：
//! - `id`：链接唯一标识符
//! - `node_id`：思维导图节点ID
//! - `anchor_id`：锚点ID
//! - `kind`：链接类型（如"Primary"、"Reference"）
//! - `created_at`：创建时间戳
//! - `updated_at`：更新时间戳
//!
//! ### 锚点解析结果 (`ResolveAnchorResult`)
//! - `Success`：成功解析，返回新的偏移量
//! - `BrokenAnchor`：锚点失效，无法定位
//!
//! ## 锚点解析算法
//!
//! `resolve_anchor`函数实现了智能锚点解析：
//!
//! 1. **精确匹配**：首先尝试使用原始偏移量定位
//!    - 检查偏移量是否在有效范围内
//!    - 验证偏移量处的文本是否与选中文本匹配
//!
//! 2. **上下文匹配**：如果精确匹配失败，使用上下文重新定位
//!    - 构建搜索模式：`prefix_text + selected_text + suffix_text`
//!    - 在章节内容中搜索该模式
//!    - 计算新的偏移量
//!
//! 3. **失效标记**：如果都无法定位，返回`BrokenAnchor`
//!
//! ## 核心函数
//! - `resolve_anchor`：解析锚点，返回新的位置或失效状态
//!
//! ## 依赖关系
//! - `serde`：JSON序列化/反序列化
//!
//! ## 使用场景
//! - 思维导图节点与章节内容的关联
//! - 文本编辑后自动更新锚点位置
//! - 检测和处理失效的锚点
//! - 支持内容变更后的自动重新定位

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapAnchor {
    pub id: String,
    pub project_id: String,
    pub chapter_id: String,
    pub start_offset: usize,
    pub end_offset: usize,
    pub selected_text: String,
    pub prefix_text: String,
    pub suffix_text: String,
    pub checksum: String,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapLink {
    pub id: String,
    pub node_id: String,
    pub anchor_id: String,
    pub kind: String, // E.g. "Primary", "Reference"
    pub created_at: u64,
    pub updated_at: u64,
}

pub enum ResolveAnchorResult {
    Success { new_start: usize, new_end: usize },
    BrokenAnchor,
}

pub fn resolve_anchor(anchor: &MindMapAnchor, chapter_content: &str) -> ResolveAnchorResult {
    // 1. Try exact offset match first
    if anchor.end_offset <= chapter_content.len() && anchor.start_offset < anchor.end_offset {
        let text_at_offset = &chapter_content[anchor.start_offset..anchor.end_offset];
        if text_at_offset == anchor.selected_text {
            return ResolveAnchorResult::Success {
                new_start: anchor.start_offset,
                new_end: anchor.end_offset,
            };
        }
    }

    // 2. Fallback: Search using prefix + selectedText + suffix
    let search_pattern = format!(
        "{}{}{}",
        anchor.prefix_text, anchor.selected_text, anchor.suffix_text
    );

    if let Some(index) = chapter_content.find(&search_pattern) {
        let new_start = index + anchor.prefix_text.len();
        let new_end = new_start + anchor.selected_text.len();
        return ResolveAnchorResult::Success { new_start, new_end };
    }

    // Still not found? Try finding just the selected text if we really want to be aggressive,
    // but the prompt specifies using selectedText + prefix/suffix.
    // We stick to the rule: "不匹配时用 selectedText + prefix/suffix 在正文里重定位"

    ResolveAnchorResult::BrokenAnchor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_anchor_exact_offset() {
        let text = "This is a test chapter content to resolve anchor.";
        let anchor = MindMapAnchor {
            id: "1".into(),
            project_id: "p1".into(),
            chapter_id: "c1".into(),
            start_offset: 10,
            end_offset: 14,
            selected_text: "test".into(),
            prefix_text: "a ".into(),
            suffix_text: " cha".into(),
            checksum: "".into(),
            created_at: 0,
            updated_at: 0,
        };
        match resolve_anchor(&anchor, text) {
            ResolveAnchorResult::Success { new_start, new_end } => {
                assert_eq!(new_start, 10);
                assert_eq!(new_end, 14);
            }
            _ => panic!("Expected Success"),
        }
    }

    #[test]
    fn test_resolve_anchor_drifted_offset() {
        // Offset changed from 10 to 15
        let text = "Prefix This is a test chapter content to resolve anchor.";
        let anchor = MindMapAnchor {
            id: "1".into(),
            project_id: "p1".into(),
            chapter_id: "c1".into(),
            start_offset: 10, // Incorrect now
            end_offset: 14,
            selected_text: "test".into(),
            prefix_text: "a ".into(),
            suffix_text: " cha".into(),
            checksum: "".into(),
            created_at: 0,
            updated_at: 0,
        };
        match resolve_anchor(&anchor, text) {
            ResolveAnchorResult::Success { new_start, new_end } => {
                assert_eq!(new_start, 17);
                assert_eq!(new_end, 21);
            }
            _ => panic!("Expected Success with new offset"),
        }
    }

    #[test]
    fn test_resolve_anchor_broken() {
        let text = "Prefix This is a completely different chapter content.";
        let anchor = MindMapAnchor {
            id: "1".into(),
            project_id: "p1".into(),
            chapter_id: "c1".into(),
            start_offset: 10,
            end_offset: 14,
            selected_text: "test".into(),
            prefix_text: "a ".into(),
            suffix_text: " cha".into(),
            checksum: "".into(),
            created_at: 0,
            updated_at: 0,
        };
        match resolve_anchor(&anchor, text) {
            ResolveAnchorResult::BrokenAnchor => {}
            _ => panic!("Expected BrokenAnchor"),
        }
    }
}
