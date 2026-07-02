//! # ????????
//!
//! ????????????????
//!
//! ## ????
//!
//! - ????????????? `.md` ??
//! - ?????????????
//! - ????????????
//! - ?????,?????

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// ??????
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    /// ?? ID
    pub project_id: String,
    /// ? ID
    pub volume_id: String,
    /// ?? ID
    pub chapter_id: String,
    /// ????
    pub chapter_title: String,
    /// ????(1-based)
    pub line_number: usize,
    /// ?????
    pub line_text: String,
    /// ????????
    pub context_before: Vec<String>,
    /// ????????
    pub context_after: Vec<String>,
    /// ??????
    pub relative_path: String,
}

/// ????
#[derive(Debug, Clone)]
pub struct SearchOptions {
    /// ???????
    pub case_sensitive: bool,
    /// ?????
    pub context_lines: usize,
    /// ?????
    pub max_results: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            context_lines: 2,
            max_results: 200,
        }
    }
}

/// ??????
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    /// ??????
    pub chapter_count: usize,
    /// ??????
    pub total_lines: usize,
    /// ??????
    pub total_words: usize,
    /// ??????(??)
    pub build_time_ms: u64,
}

/// ?????????
pub struct SearchIndex {
    entries: Vec<IndexEntry>,
}

struct IndexEntry {
    project_id: String,
    volume_id: String,
    chapter_id: String,
    chapter_path: PathBuf,
    chapter_title: std::sync::OnceLock<String>,
    relative_path: String,
    lines: Vec<String>,
}

impl SearchIndex {
    /// ????:??????????
    pub fn build(workspace: &Path) -> Result<Self> {
        let start = std::time::Instant::now();
        let mut entries = Vec::new();
        let projects_dir = workspace.join("projects");

        if !projects_dir.exists() {
            return Ok(Self { entries });
        }

        let mut chapter_paths = Vec::new();
        let projects = scan_projects(&projects_dir)?;
        for (project_id, project_path) in projects {
            let volumes_dir = project_path.join("volumes");
            if !volumes_dir.exists() {
                continue;
            }
            let volumes = scan_subdirs(&volumes_dir)?;
            for (volume_id, volume_path) in volumes {
                let chapters_dir = volume_path.join("chapters");
                if !chapters_dir.exists() {
                    continue;
                }
                let chapters = scan_subdirs(&chapters_dir)?;
                for (chapter_id, chapter_path) in chapters {
                    let md_path = chapter_path.join("chapter.md");
                    if md_path.exists() {
                        chapter_paths.push((
                            project_id.clone(),
                            volume_id.clone(),
                            chapter_id,
                            chapter_path,
                            md_path,
                        ));
                    }
                }
            }
        }

        use rayon::prelude::*;
        entries = chapter_paths
            .into_par_iter()
            .filter_map(
                |(project_id, volume_id, chapter_id, chapter_path, md_path)| {
                    let content = fs::read_to_string(&md_path).ok()?;
                    let relative_path = format!(
                        "projects/{}/volumes/{}/chapters/{}/chapter.md",
                        project_id, volume_id, chapter_id
                    );
                    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

                    Some(IndexEntry {
                        project_id,
                        volume_id,
                        chapter_id,
                        chapter_path,
                        chapter_title: std::sync::OnceLock::new(),
                        relative_path,
                        lines,
                    })
                },
            )
            .collect();

        let _elapsed = start.elapsed().as_millis() as u64;
        Ok(Self { entries })
    }

    /// ??
    pub fn search(&self, query: &str, options: &SearchOptions) -> Vec<SearchHit> {
        if query.is_empty() {
            return Vec::new();
        }

        let mut hits = Vec::new();

        let ac = match aho_corasick::AhoCorasick::builder()
            .ascii_case_insensitive(!options.case_sensitive)
            .build(&[query])
        {
            Ok(ac) => ac,
            Err(_) => return hits, // Fallback for empty/invalid patterns, though build rarely fails for string slices
        };

        for entry in &self.entries {
            for (line_idx, line) in entry.lines.iter().enumerate() {
                if ac.is_match(line) {
                    let ctx_start = line_idx.saturating_sub(options.context_lines);
                    let ctx_end = (line_idx + options.context_lines + 1).min(entry.lines.len());

                    let context_before: Vec<String> = entry.lines[ctx_start..line_idx].to_vec();
                    let context_after: Vec<String> = entry.lines[line_idx + 1..ctx_end].to_vec();

                    hits.push(SearchHit {
                        project_id: entry.project_id.clone(),
                        volume_id: entry.volume_id.clone(),
                        chapter_id: entry.chapter_id.clone(),
                        chapter_title: entry
                            .chapter_title
                            .get_or_init(|| load_chapter_title(&entry.chapter_path, &entry.chapter_id))
                            .clone(),
                        line_number: line_idx + 1,
                        line_text: line.clone(),
                        context_before,
                        context_after,
                        relative_path: entry.relative_path.clone(),
                    });

                    if hits.len() >= options.max_results {
                        return hits;
                    }
                }
            }
        }

        hits
    }

    /// ??????
    pub fn stats(&self) -> IndexStats {
        let total_lines = self.entries.iter().map(|e| e.lines.len()).sum();
        let total_words = self
            .entries
            .iter()
            .map(|e| {
                e.lines
                    .iter()
                    .map(|l| l.split_whitespace().count())
                    .sum::<usize>()
            })
            .sum();
        IndexStats {
            chapter_count: self.entries.len(),
            total_lines,
            total_words,
            build_time_ms: 0,
        }
    }

    /// ???????
    pub fn search_in_project(
        &self,
        project_id: &str,
        query: &str,
        options: &SearchOptions,
    ) -> Vec<SearchHit> {
        if query.is_empty() {
            return Vec::new();
        }

        let mut hits = Vec::new();

        let ac = match aho_corasick::AhoCorasick::builder()
            .ascii_case_insensitive(!options.case_sensitive)
            .build(&[query])
        {
            Ok(ac) => ac,
            Err(_) => return hits,
        };

        for entry in &self.entries {
            if entry.project_id != project_id {
                continue;
            }
            for (line_idx, line) in entry.lines.iter().enumerate() {
                if ac.is_match(line) {
                    let ctx_start = line_idx.saturating_sub(options.context_lines);
                    let ctx_end = (line_idx + options.context_lines + 1).min(entry.lines.len());

                    let context_before: Vec<String> = entry.lines[ctx_start..line_idx].to_vec();
                    let context_after: Vec<String> = entry.lines[line_idx + 1..ctx_end].to_vec();

                    hits.push(SearchHit {
                        project_id: entry.project_id.clone(),
                        volume_id: entry.volume_id.clone(),
                        chapter_id: entry.chapter_id.clone(),
                        chapter_title: entry
                            .chapter_title
                            .get_or_init(|| load_chapter_title(&entry.chapter_path, &entry.chapter_id))
                            .clone(),
                        line_number: line_idx + 1,
                        line_text: line.clone(),
                        context_before,
                        context_after,
                        relative_path: entry.relative_path.clone(),
                    });

                    if hits.len() >= options.max_results {
                        return hits;
                    }
                }
            }
        }

        hits
    }
}

fn scan_projects(projects_dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut result = Vec::new();
    if let Ok(entries) = fs::read_dir(projects_dir) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() || file_type.is_symlink() {
                    let path = entry.path();
                    if file_type.is_dir() || path.is_dir() {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            result.push((name.to_string(), path));
                        }
                    }
                }
            }
        }
    }
    Ok(result)
}

fn scan_subdirs(dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut result = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() || file_type.is_symlink() {
                    let path = entry.path();
                    if file_type.is_dir() || path.is_dir() {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            result.push((name.to_string(), path));
                        }
                    }
                }
            }
        }
    }
    Ok(result)
}

fn load_chapter_title(chapter_path: &Path, fallback_id: &str) -> String {
    let meta_path = chapter_path.join("chapter.meta.json");
    if let Ok(content) = fs::read_to_string(&meta_path) {
        if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(title) = meta.get("title").and_then(|v| v.as_str()) {
                return title.to_string();
            }
        }
    }
    fallback_id.to_string()
}

/// ????:????(?????)
pub fn update_index() -> Result<()> {
    // ????????,???????
    Ok(())
}
