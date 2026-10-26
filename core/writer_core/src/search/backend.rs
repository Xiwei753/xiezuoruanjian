use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use super::types::*;

/// 原文折叠大小写后的字节位置 -> 原始 UTF-8 字符范围映射。
///
/// `to_lowercase()` 可能改变字节长度（例如 `İ` 折叠为 `i\u{307}`），因此不能把
/// 折叠文本上的偏移直接当作原文偏移。这里逐个折叠字节记录其来源字符的完整
/// UTF-8 范围，命中跨越字符内部时也能还原为完整原文字符。
fn fold_with_offsets(text: &str) -> (String, Vec<(usize, usize)>) {
    let mut folded = String::with_capacity(text.len());
    let mut map = Vec::with_capacity(text.len());
    for (byte_index, ch) in text.char_indices() {
        let char_end = byte_index + ch.len_utf8();
        let before = folded.len();
        for lower in ch.to_lowercase() {
            folded.push(lower);
        }
        for _ in before..folded.len() {
            map.push((byte_index, char_end));
        }
    }
    (folded, map)
}

fn next_char_boundary(text: &str, pos: usize) -> usize {
    if pos >= text.len() {
        return text.len().saturating_add(1);
    }
    pos + text[pos..].chars().next().map(char::len_utf8).unwrap_or(1)
}

fn find_all_occurrences(
    folded: &str,
    needle: &str,
    map: Option<&[(usize, usize)]>,
) -> Vec<(usize, usize)> {
    if needle.is_empty() || folded.is_empty() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    let mut seen = HashSet::new();
    let mut search_from = 0;
    while search_from < folded.len() {
        let Some(relative) = folded[search_from..].find(needle) else {
            break;
        };
        let start = search_from + relative;
        let end = start + needle.len();
        let (orig_start, orig_end) = match map {
            Some(map) => {
                let orig_start = map
                    .get(start)
                    .map(|(start, _)| *start)
                    .unwrap_or_else(|| map.last().map(|(_, end)| *end).unwrap_or(0));
                let orig_end = if end == start {
                    orig_start
                } else {
                    map.get(end - 1).map(|(_, end)| *end).unwrap_or(orig_start)
                };
                (orig_start, orig_end)
            }
            None => (start, end),
        };
        if orig_end > orig_start && seen.insert((orig_start, orig_end)) {
            matches.push((orig_start, orig_end));
        }
        // 允许重叠命中：下一次只前进一个原文字符，而不是跳过整个命中。
        search_from = next_char_boundary(folded, start);
    }
    matches
}

fn char_boundary_before(text: &str, mut pos: usize) -> usize {
    pos = pos.min(text.len());
    while pos > 0 && !text.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

fn char_boundary_after(text: &str, mut pos: usize) -> usize {
    pos = pos.min(text.len());
    while pos < text.len() && !text.is_char_boundary(pos) {
        pos += 1;
    }
    pos
}

/// 生成正文命中摘要，并返回命中在摘要内的字节范围。
fn body_summary(body: &str, start: usize, end: usize) -> (String, Vec<(usize, usize)>) {
    const BEFORE: usize = 50;
    const AFTER: usize = 130;
    let window_start = char_boundary_before(body, start.saturating_sub(BEFORE));
    let window_end = char_boundary_after(body, (end + AFTER).min(body.len()));
    let prefix = if window_start > 0 { "..." } else { "" };
    let suffix = if window_end < body.len() { "..." } else { "" };
    let summary = format!("{}{}{}", prefix, &body[window_start..window_end], suffix);
    let highlight_start = prefix.len() + (start - window_start);
    let highlight_end = prefix.len() + (end - window_start);
    (summary, vec![(highlight_start, highlight_end)])
}

fn searchable_fields(scope: SearchScope) -> (bool, bool) {
    match scope {
        SearchScope::ChapterBody | SearchScope::ChapterNote => (false, true),
        SearchScope::ProjectTitle
        | SearchScope::VolumeTitle
        | SearchScope::ChapterTitle
        | SearchScope::StarmapTitle => (true, false),
        SearchScope::StarmapNode
        | SearchScope::StarmapEdgeLabel
        | SearchScope::StarmapHyperlink
        | SearchScope::StarmapLink
        | SearchScope::StarmapEmbed
        | SearchScope::AnnotationTag
        | SearchScope::AnnotationEntry
        | SearchScope::AnnotationLink
        | SearchScope::Setting => (true, true),
        SearchScope::All => (true, true),
    }
}

fn base_text_score(scope: SearchScope, field: SearchField) -> f64 {
    let scope_weight = match scope {
        SearchScope::ChapterTitle => 3.4,
        SearchScope::ProjectTitle => 3.2,
        SearchScope::VolumeTitle => 3.0,
        SearchScope::ChapterBody => 2.4,
        SearchScope::StarmapTitle => 2.8,
        SearchScope::Setting => 2.6,
        SearchScope::StarmapNode => 2.4,
        SearchScope::StarmapEdgeLabel => 2.2,
        SearchScope::StarmapHyperlink => 2.1,
        SearchScope::StarmapLink => 2.0,
        SearchScope::StarmapEmbed => 2.0,
        SearchScope::AnnotationTag => 2.7,
        SearchScope::AnnotationEntry => 2.5,
        SearchScope::AnnotationLink => 2.1,
        SearchScope::ChapterNote => 1.8,
        SearchScope::All => 1.0,
    };
    scope_weight
        + match field {
            SearchField::Title => 1.0,
            SearchField::Body => 0.0,
        }
}

fn context_boost(scope: SearchScope, target: &SearchTarget, context: &SearchContext) -> f64 {
    let same = |value: Option<&str>, current: Option<&str>| match (value, current) {
        (Some(value), Some(current)) => value == current,
        _ => false,
    };
    let mut boost = 0.0;
    match scope {
        SearchScope::ChapterBody | SearchScope::ChapterTitle | SearchScope::ChapterNote => {
            if same(
                target.chapter_id.as_deref(),
                context.current_chapter_id.as_deref(),
            ) {
                boost += 6.0;
            } else if same(
                target.volume_id.as_deref(),
                context.current_volume_id.as_deref(),
            ) {
                boost += 3.0;
            } else if same(
                target.project_id.as_deref(),
                context.current_project_id.as_deref(),
            ) {
                boost += 1.5;
            }
        }
        SearchScope::ProjectTitle
        | SearchScope::VolumeTitle
        | SearchScope::AnnotationTag
        | SearchScope::AnnotationEntry
        | SearchScope::AnnotationLink => {
            if same(
                target.project_id.as_deref(),
                context.current_project_id.as_deref(),
            ) {
                boost += 2.0;
            }
        }
        SearchScope::StarmapTitle
        | SearchScope::StarmapNode
        | SearchScope::StarmapEdgeLabel
        | SearchScope::StarmapHyperlink
        | SearchScope::StarmapLink
        | SearchScope::StarmapEmbed => {
            if same(
                target.starmap_id.as_deref(),
                context.current_starmap_id.as_deref(),
            ) {
                boost += 4.0;
            } else if same(
                target.project_id.as_deref(),
                context.current_project_id.as_deref(),
            ) {
                boost += 1.2;
            }
        }
        SearchScope::Setting => {
            if context.origin_screen == SearchOriginScreen::Settings {
                boost += 2.5;
            }
            if same(
                target.setting_section.as_deref(),
                context.current_settings_section.as_deref(),
            ) {
                boost += 1.5;
            }
        }
        SearchScope::All => {}
    }

    if same(
        target.project_id.as_deref(),
        context.current_project_id.as_deref(),
    ) {
        boost += 0.4;
    }
    match context.origin_screen {
        SearchOriginScreen::Writing
            if matches!(
                scope,
                SearchScope::ChapterBody | SearchScope::ChapterTitle | SearchScope::ChapterNote
            ) =>
        {
            boost += 0.4;
        }
        SearchOriginScreen::Starmap
            if matches!(
                scope,
                SearchScope::StarmapTitle
                    | SearchScope::StarmapNode
                    | SearchScope::StarmapEdgeLabel
                    | SearchScope::StarmapHyperlink
                    | SearchScope::StarmapLink
                    | SearchScope::StarmapEmbed
            ) =>
        {
            boost += 0.4;
        }
        SearchOriginScreen::Writing
            if matches!(
                scope,
                SearchScope::AnnotationTag
                    | SearchScope::AnnotationEntry
                    | SearchScope::AnnotationLink
            ) =>
        {
            boost += 0.4;
        }
        SearchOriginScreen::Settings if scope == SearchScope::Setting => {
            boost += 0.4;
        }
        _ => {}
    }
    boost
}

#[derive(Default)]
struct PathLookup {
    projects: HashMap<String, String>,
    volumes: HashMap<String, String>,
    chapters: HashMap<String, String>,
    starmaps: HashMap<String, String>,
}

impl PathLookup {
    fn build(entries: &HashMap<String, IndexEntry>) -> Self {
        let mut lookup = Self::default();
        for entry in entries.values() {
            match entry.scope {
                SearchScope::ProjectTitle => {
                    if let Some(id) = entry.target.project_id.as_deref() {
                        lookup.projects.insert(id.to_string(), entry.title.clone());
                    }
                }
                SearchScope::VolumeTitle => {
                    if let Some(id) = entry.target.volume_id.as_deref() {
                        lookup.volumes.insert(id.to_string(), entry.title.clone());
                    }
                }
                SearchScope::ChapterTitle => {
                    if let Some(id) = entry.target.chapter_id.as_deref() {
                        lookup.chapters.insert(id.to_string(), entry.title.clone());
                    }
                }
                SearchScope::StarmapTitle => {
                    if let Some(id) = entry.target.starmap_id.as_deref() {
                        lookup.starmaps.insert(id.to_string(), entry.title.clone());
                    }
                }
                _ => {}
            }
        }
        lookup
    }

    fn display_path(&self, entry: &IndexEntry) -> String {
        let project = entry
            .target
            .project_id
            .as_deref()
            .and_then(|id| self.projects.get(id))
            .cloned();
        match entry.scope {
            SearchScope::ProjectTitle => entry.title.clone(),
            SearchScope::VolumeTitle => join_path(&[project, Some(entry.title.clone())]),
            SearchScope::ChapterTitle | SearchScope::ChapterBody | SearchScope::ChapterNote => {
                let volume = entry
                    .target
                    .volume_id
                    .as_deref()
                    .and_then(|id| self.volumes.get(id))
                    .cloned();
                let chapter = entry
                    .target
                    .chapter_id
                    .as_deref()
                    .and_then(|id| self.chapters.get(id))
                    .cloned()
                    .unwrap_or_else(|| entry.title.clone());
                join_path(&[project, volume, Some(chapter)])
            }
            SearchScope::StarmapTitle => {
                let starmap = entry.title.clone();
                join_path(&[project, Some(starmap)])
            }
            SearchScope::StarmapNode
            | SearchScope::StarmapEdgeLabel
            | SearchScope::StarmapHyperlink
            | SearchScope::StarmapLink
            | SearchScope::StarmapEmbed => {
                let starmap = entry
                    .target
                    .starmap_id
                    .as_deref()
                    .and_then(|id| self.starmaps.get(id))
                    .cloned();
                join_path(&[project, starmap])
            }
            SearchScope::Setting => {
                let section = entry
                    .target
                    .setting_section
                    .as_deref()
                    .map(settings_section_label);
                join_path(&[Some("设置".to_string()), section, Some(entry.title.clone())])
            }
            SearchScope::AnnotationTag => {
                join_path(&[project, Some("标签".to_string()), Some(entry.title.clone())])
            }
            SearchScope::AnnotationEntry => join_path(&[
                project,
                Some("专有名词".to_string()),
                Some(entry.title.clone()),
            ]),
            SearchScope::AnnotationLink => {
                join_path(&[project, Some("链接".to_string()), Some(entry.title.clone())])
            }
            SearchScope::All => entry.title.clone(),
        }
    }
}

fn join_path(parts: &[Option<String>]) -> String {
    parts
        .iter()
        .filter_map(|part| part.as_deref())
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

fn settings_section_label(section: &str) -> String {
    match section {
        "appearance" => "外观",
        "editor" => "编辑器和动画",
        "ai" => "AI",
        "save_sync" => "保存和同步",
        "diagnostics" => "诊断与日志",
        "about" => "关于",
        other => other,
    }
    .to_string()
}

fn cursor_hash(query: &str, scope: SearchScope, context: &SearchContext) -> u64 {
    let mut hasher = DefaultHasher::new();
    query.hash(&mut hasher);
    scope.hash(&mut hasher);
    context.cursor_fingerprint().hash(&mut hasher);
    hasher.finish()
}

fn is_after_cursor(result: &SearchResult, cursor: &SearchCursor) -> bool {
    match result.score.total_cmp(&cursor.score) {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Greater => false,
        std::cmp::Ordering::Equal => {
            let left = (
                result.object_id.as_str(),
                result.field.sort_rank(),
                result.byte_start,
            );
            let right = (
                cursor.object_id.as_str(),
                cursor.field.sort_rank(),
                cursor.byte_start,
            );
            left > right
        }
    }
}

#[derive(Default)]
pub struct SearchBackend {
    entries: HashMap<String, IndexEntry>,
    scope_index: HashMap<SearchScope, Vec<String>>,
}

impl SearchBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, entry: IndexEntry) {
        let id = entry.object_id.clone();
        let scope = entry.scope;
        if let Some(old_entry) = self.entries.insert(id.clone(), entry) {
            if old_entry.scope != scope {
                if let Some(ids) = self.scope_index.get_mut(&old_entry.scope) {
                    ids.retain(|i| i != &id);
                }
                self.scope_index.entry(scope).or_default().push(id);
            }
        } else {
            self.scope_index.entry(scope).or_default().push(id);
        }
    }

    pub fn remove(&mut self, object_id: &str) {
        if let Some(entry) = self.entries.remove(object_id) {
            if let Some(ids) = self.scope_index.get_mut(&entry.scope) {
                ids.retain(|id| id != object_id);
            }
        }
    }

    pub fn remove_by_prefix(&mut self, prefix: &str) {
        let ids_to_remove: Vec<String> = self
            .entries
            .keys()
            .filter(|id| id.starts_with(prefix))
            .cloned()
            .collect();
        for id in ids_to_remove {
            self.remove(&id);
        }
    }

    /// 按「每个出现位置」返回命中。
    pub fn search(
        &self,
        query: &str,
        scope: SearchScope,
        limit: usize,
        cursor: Option<&str>,
        context: &SearchContext,
    ) -> SearchResultPage {
        let query = query.trim();
        if query.is_empty() {
            return SearchResultPage::default();
        }
        let limit = limit.clamp(1, 200);
        let (query_folded, _) = fold_with_offsets(query);
        if query_folded.is_empty() {
            return SearchResultPage::default();
        }

        let mut candidate_ids: Vec<String> = if scope == SearchScope::All {
            self.entries.keys().cloned().collect()
        } else {
            self.scope_index.get(&scope).cloned().unwrap_or_default()
        };
        candidate_ids.sort();

        let lookup = PathLookup::build(&self.entries);
        let mut all_results = Vec::new();

        for id in &candidate_ids {
            let Some(entry) = self.entries.get(id) else {
                continue;
            };
            let (mut search_title, mut search_body) = searchable_fields(entry.scope);
            if scope == SearchScope::All {
                // “全部”范围下，章节标题由独立的 ChapterTitle 条目负责；
                // 正文/备注条目不能再按 title 命中，否则同一标题会重复，
                // 且平台会把指向标题的 byte 偏移误当成正文偏移去定位。
                match entry.scope {
                    SearchScope::ChapterBody | SearchScope::ChapterNote => {
                        search_title = false;
                        search_body = true;
                    }
                    SearchScope::ChapterTitle
                    | SearchScope::ProjectTitle
                    | SearchScope::VolumeTitle
                    | SearchScope::StarmapTitle => {
                        search_title = true;
                        search_body = false;
                    }
                    _ => {}
                }
            }
            let path = lookup.display_path(entry);
            let mut seen: HashSet<(u8, usize, usize)> = HashSet::new();

            if search_title && !entry.title.is_empty() {
                let (title_folded, title_map) = fold_with_offsets(&entry.title);
                for (start, end) in
                    find_all_occurrences(&title_folded, &query_folded, Some(&title_map))
                {
                    if !seen.insert((SearchField::Title.sort_rank() as u8, start, end)) {
                        continue;
                    }
                    let exact = entry.title.trim().eq_ignore_ascii_case(query);
                    let score = base_text_score(entry.scope, SearchField::Title)
                        + context_boost(entry.scope, &entry.target, context)
                        + if exact { 1.5 } else { 0.0 };
                    all_results.push(SearchResult {
                        object_id: entry.object_id.clone(),
                        title: entry.title.clone(),
                        path: path.clone(),
                        summary: entry.title.clone(),
                        byte_start: start,
                        byte_end: end,
                        summary_match_ranges: vec![(start, end)],
                        field: SearchField::Title,
                        score,
                        scope: entry.scope,
                        target: entry.target.clone(),
                    });
                }
            }

            if search_body && !entry.body.is_empty() && !(search_title && entry.body == entry.title)
            {
                let (body_folded, body_map) = fold_with_offsets(&entry.body);
                for (start, end) in
                    find_all_occurrences(&body_folded, &query_folded, Some(&body_map))
                {
                    if !seen.insert((SearchField::Body.sort_rank() as u8, start, end)) {
                        continue;
                    }
                    let (summary, summary_ranges) = body_summary(&entry.body, start, end);
                    let score = base_text_score(entry.scope, SearchField::Body)
                        + context_boost(entry.scope, &entry.target, context);
                    all_results.push(SearchResult {
                        object_id: entry.object_id.clone(),
                        title: entry.title.clone(),
                        path: path.clone(),
                        summary,
                        byte_start: start,
                        byte_end: end,
                        summary_match_ranges: summary_ranges,
                        field: SearchField::Body,
                        score,
                        scope: entry.scope,
                        target: entry.target.clone(),
                    });
                }
            }
        }

        all_results.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.object_id.cmp(&b.object_id))
                .then_with(|| a.field.sort_rank().cmp(&b.field.sort_rank()))
                .then_with(|| a.byte_start.cmp(&b.byte_start))
        });

        let query_hash = cursor_hash(query, scope, context);
        let mut start_index = 0;
        if let Some(raw_cursor) = cursor {
            if let Ok(parsed) = serde_json::from_str::<SearchCursor>(raw_cursor) {
                if parsed.query_hash == query_hash {
                    start_index = all_results
                        .iter()
                        .position(|result| is_after_cursor(result, &parsed))
                        .unwrap_or(all_results.len());
                } else {
                    // 关键词/范围/上下文已变化，旧游标失效。
                    return SearchResultPage::default();
                }
            } else if let Some(position) = all_results
                .iter()
                .position(|result| result.object_id == raw_cursor)
            {
                // 兼容旧版把 object_id 当游标的调用方。
                start_index = position + 1;
            } else {
                return SearchResultPage::default();
            }
        }

        let remaining = all_results
            .into_iter()
            .skip(start_index)
            .collect::<Vec<_>>();
        let has_more = remaining.len() > limit;
        let items = remaining.into_iter().take(limit).collect::<Vec<_>>();
        let next_cursor = if has_more {
            items.last().and_then(|last| {
                serde_json::to_string(&SearchCursor {
                    query_hash,
                    score: last.score,
                    object_id: last.object_id.clone(),
                    field: last.field,
                    byte_start: last.byte_start,
                })
                .ok()
            })
        } else {
            None
        };

        SearchResultPage { items, next_cursor }
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn scope_count(&self, scope: SearchScope) -> usize {
        self.scope_index
            .get(&scope)
            .map(|ids| ids.len())
            .unwrap_or(0)
    }

    /// Return current annotation IDs for one project so a refreshed projection
    /// can delete derived entries whose backing record disappeared.
    pub fn annotation_object_ids_for_project(&self, project_id: &str) -> Vec<String> {
        let prefix = format!("annotation:{project_id}:");
        let mut ids: Vec<_> = self
            .entries
            .iter()
            .filter(|(id, _)| id.starts_with(&prefix))
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
    }

    pub fn remove_by_target_project_id(&mut self, project_id: &str) {
        let ids_to_remove: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.target.project_id.as_deref() == Some(project_id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids_to_remove {
            self.remove(&id);
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.scope_index.clear();
    }
}
