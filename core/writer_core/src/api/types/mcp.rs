//! Read-only DTOs exposed to the local MCP adapter.
//!
//! These types intentionally contain no chapter-body write capability: MCP
//! clients can list and page through body text, but may only mutate starmap,
//! tag, hyperlink and anchor metadata through the normal business methods.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpChapterListItemDto {
    pub project_id: String,
    pub project_title: String,
    pub volume_id: String,
    pub volume_title: String,
    pub chapter_id: String,
    pub title: String,
    pub order: i32,
    pub word_count: u32,
    pub hash: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpChapterReadDto {
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub title: String,
    pub hash: String,
    pub word_count: u32,
    pub total_chars: usize,
    pub offset_chars: usize,
    pub end_chars: usize,
    pub next_offset_chars: Option<usize>,
    pub content: String,
}
