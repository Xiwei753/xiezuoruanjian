use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchScope {
    ChapterBody,
    ChapterTitle,
    ChapterNote,
    ProjectTitle,
    VolumeTitle,
    StarmapTitle,
    StarmapNode,
    StarmapEdgeLabel,
    StarmapHyperlink,
    StarmapLink,
    StarmapEmbed,
    AnnotationTag,
    AnnotationEntry,
    AnnotationLink,
    Setting,
    All,
}

impl SearchScope {
    pub fn all_scopes() -> &'static [SearchScope] {
        &[
            SearchScope::ChapterBody,
            SearchScope::ChapterTitle,
            SearchScope::ChapterNote,
            SearchScope::ProjectTitle,
            SearchScope::VolumeTitle,
            SearchScope::StarmapTitle,
            SearchScope::StarmapNode,
            SearchScope::StarmapEdgeLabel,
            SearchScope::StarmapHyperlink,
            SearchScope::StarmapLink,
            SearchScope::StarmapEmbed,
            SearchScope::AnnotationTag,
            SearchScope::AnnotationEntry,
            SearchScope::AnnotationLink,
            SearchScope::Setting,
        ]
    }
}

/// 命中的字段：标题或正文。
///
/// `byte_start` / `byte_end` 始终对应原始文本（未被 `to_lowercase` 改写）的
/// UTF-8 字节偏移。正文命中对应正文原文，标题命中对应标题原文。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchField {
    Title,
    Body,
}

impl SearchField {
    pub(crate) fn sort_rank(self) -> u8 {
        match self {
            SearchField::Title => 0,
            SearchField::Body => 1,
        }
    }
}

/// 发起搜索时所在页面的上下文。只加权排序，不过滤其他范围。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchOriginScreen {
    #[default]
    General,
    Home,
    Project,
    Writing,
    Starmap,
    Settings,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchContext {
    pub origin_screen: SearchOriginScreen,
    pub current_project_id: Option<String>,
    pub current_volume_id: Option<String>,
    pub current_chapter_id: Option<String>,
    pub current_starmap_id: Option<String>,
    pub current_settings_section: Option<String>,
}

impl SearchContext {
    pub(crate) fn cursor_fingerprint(&self) -> String {
        format!(
            "{:?}|{}|{}|{}|{}|{}",
            self.origin_screen,
            self.current_project_id.as_deref().unwrap_or(""),
            self.current_volume_id.as_deref().unwrap_or(""),
            self.current_chapter_id.as_deref().unwrap_or(""),
            self.current_starmap_id.as_deref().unwrap_or(""),
            self.current_settings_section.as_deref().unwrap_or(""),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub object_id: String,
    pub title: String,
    /// 可读来源路径，例如「作品 / 第一卷 / 第一章」。
    pub path: String,
    /// 命中位置附近的摘要。标题命中时摘要就是标题。
    pub summary: String,
    /// `field` 命中在原始文本中的 UTF-8 字节起止。
    pub byte_start: usize,
    pub byte_end: usize,
    /// 命中在 `summary` 中的 UTF-8 字节起止，供结果列表高亮。
    pub summary_match_ranges: Vec<(usize, usize)>,
    pub field: SearchField,
    pub score: f64,
    pub scope: SearchScope,
    pub target: SearchTarget,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchResultPage {
    pub items: Vec<SearchResult>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchTarget {
    pub project_id: Option<String>,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    pub starmap_id: Option<String>,
    pub node_id: Option<String>,
    pub edge_id: Option<String>,
    pub link_id: Option<String>,
    pub hyperlink_id: Option<String>,
    pub embed_id: Option<String>,
    pub setting_key: Option<String>,
    pub setting_section: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub text: String,
    pub scope: SearchScope,
    pub limit: usize,
    pub cursor: Option<String>,
    #[serde(default)]
    pub context: SearchContext,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            scope: SearchScope::All,
            limit: 50,
            cursor: None,
            context: SearchContext::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchIndexStatus {
    pub total_entries: usize,
    pub scope_counts: Vec<(SearchScope, usize)>,
    pub last_rebuild_at: u64,
    pub is_rebuilding: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchIndexAction {
    Upsert,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchIndexUpdate {
    pub action: SearchIndexAction,
    pub object_id: String,
    pub scope: SearchScope,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<SearchTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    pub object_id: String,
    pub scope: SearchScope,
    pub title: String,
    pub body: String,
    pub target: SearchTarget,
}

/// 游标只保存上一页最后一条命中的排序键。索引或关键词变化时游标自然失效，
/// 不会把旧关键词的翻页位置套到新结果上。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SearchCursor {
    pub query_hash: u64,
    pub score: f64,
    pub object_id: String,
    pub field: SearchField,
    pub byte_start: usize,
}
