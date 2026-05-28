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
