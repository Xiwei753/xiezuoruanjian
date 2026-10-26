use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};

use super::model::NamedEntry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoMatch {
    pub start_byte: usize,
    pub end_byte: usize,
    pub matched_text: String,
    /// More than one entry here is an unresolved same-name collision.
    pub candidate_entry_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoMatchSnapshot {
    pub project_id: String,
    pub chapter_id: String,
    pub text_revision: u64,
    pub matches: Vec<AutoMatch>,
}

#[derive(Debug, Clone)]
struct CachedSnapshot {
    text_revision: u64,
    input_fingerprint: u64,
    snapshot: AutoMatchSnapshot,
}

/// In-memory chapter matcher. Its snapshots are derived from the chapter text
/// and project NamedEntry records; they are never persisted as object links.
#[derive(Debug, Default)]
pub struct AutoLinkMatcher {
    snapshots: HashMap<(String, String), CachedSnapshot>,
}

impl AutoLinkMatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reuse a chapter snapshot while its text revision, registered names, and
    /// manual-link ranges are unchanged; otherwise recompute and replace it.
    pub fn update_chapter(
        &mut self,
        project_id: &str,
        chapter_id: &str,
        text_revision: u64,
        text: &str,
        entries: &[NamedEntry],
        manual_ranges: &[(usize, usize)],
    ) -> AutoMatchSnapshot {
        let key = (project_id.to_string(), chapter_id.to_string());
        let input_fingerprint = matcher_fingerprint(text, entries, manual_ranges);
        if let Some(cached) = self.snapshots.get(&key) {
            if cached.text_revision == text_revision
                && cached.input_fingerprint == input_fingerprint
            {
                return cached.snapshot.clone();
            }
        }

        let snapshot = AutoMatchSnapshot {
            project_id: project_id.to_string(),
            chapter_id: chapter_id.to_string(),
            text_revision,
            matches: find_auto_matches(text, entries, manual_ranges),
        };
        self.snapshots.insert(
            key,
            CachedSnapshot {
                text_revision,
                input_fingerprint,
                snapshot: snapshot.clone(),
            },
        );
        snapshot
    }

    pub fn invalidate_chapter(&mut self, project_id: &str, chapter_id: &str) {
        self.snapshots
            .remove(&(project_id.to_string(), chapter_id.to_string()));
    }

    pub fn invalidate_project(&mut self, project_id: &str) {
        self.snapshots
            .retain(|(cached_project_id, _), _| cached_project_id != project_id);
    }
}

/// Find exact registered names and aliases. The leftmost match wins against an
/// overlapping later start, and the longest name wins at the same start. Equal
/// longest names return every candidate so the caller can ask the user to pick.
pub fn find_auto_matches(
    text: &str,
    entries: &[NamedEntry],
    manual_ranges: &[(usize, usize)],
) -> Vec<AutoMatch> {
    let mut by_start: HashMap<usize, (usize, Vec<String>)> = HashMap::new();
    for entry in entries
        .iter()
        .filter(|entry| entry.deleted_at_ms.is_none() && entry.auto_link_enabled)
    {
        let mut names = Vec::with_capacity(entry.aliases.len() + 1);
        names.push(entry.canonical_name.as_str());
        names.extend(entry.aliases.iter().map(String::as_str));
        names.sort_unstable();
        names.dedup();
        for name in names.into_iter().filter(|name| !name.is_empty()) {
            for (start, end) in occurrences(text, name) {
                let slot = by_start.entry(start).or_insert_with(|| (end, Vec::new()));
                if end > slot.0 {
                    slot.0 = end;
                    slot.1.clear();
                    slot.1.push(entry.entry_id.clone());
                } else if end == slot.0 && !slot.1.contains(&entry.entry_id) {
                    slot.1.push(entry.entry_id.clone());
                }
            }
        }
    }

    let mut candidates: Vec<_> = by_start
        .into_iter()
        .map(|(start_byte, (end_byte, mut candidate_entry_ids))| {
            candidate_entry_ids.sort();
            AutoMatch {
                start_byte,
                end_byte,
                matched_text: text[start_byte..end_byte].to_string(),
                candidate_entry_ids,
            }
        })
        .collect();
    candidates.sort_by_key(|item| (item.start_byte, std::cmp::Reverse(item.end_byte)));

    let mut matches = Vec::new();
    let mut selected_end = 0usize;
    for candidate in candidates {
        if candidate.start_byte < selected_end {
            continue;
        }
        if manual_ranges.iter().any(|(start, end)| {
            start < end && candidate.start_byte < *end && *start < candidate.end_byte
        }) {
            selected_end = candidate.end_byte;
            continue;
        }
        selected_end = candidate.end_byte;
        matches.push(candidate);
    }
    matches
}

fn occurrences(text: &str, needle: &str) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();
    let mut search_from = 0usize;
    while search_from < text.len() {
        let Some(relative_start) = text[search_from..].find(needle) else {
            break;
        };
        let start = search_from + relative_start;
        let end = start + needle.len();
        matches.push((start, end));
        let next_char_len = text[start..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(needle.len());
        search_from = start.saturating_add(next_char_len);
    }
    matches
}

fn matcher_fingerprint(text: &str, entries: &[NamedEntry], manual_ranges: &[(usize, usize)]) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    let mut entries: Vec<_> = entries
        .iter()
        .filter(|entry| entry.deleted_at_ms.is_none() && entry.auto_link_enabled)
        .collect();
    entries.sort_by(|left, right| left.entry_id.cmp(&right.entry_id));
    for entry in entries {
        entry.entry_id.hash(&mut hasher);
        entry.canonical_name.hash(&mut hasher);
        let mut aliases = entry.aliases.clone();
        aliases.sort();
        aliases.hash(&mut hasher);
    }
    let mut manual_ranges = manual_ranges.to_vec();
    manual_ranges.sort_unstable();
    manual_ranges.hash(&mut hasher);
    hasher.finish()
}
