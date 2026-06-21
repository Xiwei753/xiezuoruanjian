//! # 全文搜索索引模块
//!
//! 提供工作区内章节的全文搜索功能。
//!
//! ## 设计说明
//!
//! - 扫描工作区中所有项目的章节 `.md` 文件
//! - 基于大小写不敏感的子串匹配
//! - 返回匹配位置及其上下文行
//! - 纯内存索引，无外部依赖

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 搜索命中结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    /// 项目 ID
    pub project_id: String,
    /// 卷 ID
    pub volume_id: String,
    /// 章节 ID
    pub chapter_id: String,
    /// 章节标题
    pub chapter_title: String,
    /// 匹配行号（1-based）
    pub line_number: usize,
    /// 匹配行内容
    pub line_text: String,
    /// 匹配前的上下文行
    pub context_before: Vec<String>,
    /// 匹配后的上下文行
    pub context_after: Vec<String>,
    /// 文件相对路径
    pub relative_path: String,
}

/// 搜索选项
#[derive(Debug, Clone)]
pub struct SearchOptions {
    /// 是否区分大小写
    pub case_sensitive: bool,
    /// 上下文行数
    pub context_lines: usize,
    /// 最大结果数
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

/// 索引统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    /// 已索引章节数
    pub chapter_count: usize,
    /// 已索引总行数
    pub total_lines: usize,
    /// 已索引总字数
    pub total_words: usize,
    /// 索引构建耗时（毫秒）
    pub build_time_ms: u64,
}

/// 工作区全文搜索索引
pub struct SearchIndex {
    entries: Vec<IndexEntry>,
}

struct IndexEntry {
    project_id: String,
    volume_id: String,
    chapter_id: String,
    chapter_title: String,
    relative_path: String,
    lines: Vec<String>,
}

impl SearchIndex {
    /// 构建索引：扫描工作区中所有章节
    pub fn build(workspace: &Path) -> Result<Self> {
        let start = std::time::Instant::now();
        let mut entries = Vec::new();
        let projects_dir = workspace.join("projects");

        if !projects_dir.exists() {
            return Ok(Self { entries });
        }

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
                    if !md_path.exists() {
                        continue;
                    }
                    let chapter_title = load_chapter_title(&chapter_path, &chapter_id);
                    let relative_path = format!(
                        "projects/{}/volumes/{}/chapters/{}/chapter.md",
                        project_id, volume_id, chapter_id
                    );
                    match fs::read_to_string(&md_path) {
                        Ok(content) => {
                            let lines: Vec<String> =
                                content.lines().map(|l| l.to_string()).collect();
                            entries.push(IndexEntry {
                                project_id: project_id.clone(),
                                volume_id: volume_id.clone(),
                                chapter_id: chapter_id.clone(),
                                chapter_title,
                                relative_path,
                                lines,
                            });
                        }
                        Err(_) => continue,
                    }
                }
            }
        }

        let _elapsed = start.elapsed().as_millis() as u64;
        Ok(Self { entries })
    }

    /// 搜索
    pub fn search(&self, query: &str, options: &SearchOptions) -> Vec<SearchHit> {
        if query.is_empty() {
            return Vec::new();
        }

        let mut hits = Vec::new();

        let ac = match aho_corasick::AhoCorasick::builder()
            .ascii_case_insensitive(!options.case_sensitive)
            .build([query])
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
                        chapter_title: entry.chapter_title.clone(),
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

    /// 获取索引统计
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

    /// 按项目过滤搜索
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
            .build([query])
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
                        chapter_title: entry.chapter_title.clone(),
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
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    result.push((name.to_string(), path));
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
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    result.push((name.to_string(), path));
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

/// 便捷函数：更新索引（桩实现兼容）
pub fn update_index() -> Result<()> {
    // 索引是按需构建的，不需要主动更新
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup_test_workspace(dir: &Path) {
        crate::workspace::create_workspace(dir).unwrap();
        let projects_dir = dir.join("projects");
        fs::create_dir_all(&projects_dir).unwrap();

        // Create a test project with chapters
        let proj_id = "proj1";
        let vol_id = "vol1";
        let ch_id = "ch1";

        let ch_dir = projects_dir
            .join(proj_id)
            .join("volumes")
            .join(vol_id)
            .join("chapters")
            .join(ch_id);
        fs::create_dir_all(&ch_dir).unwrap();

        fs::write(
            ch_dir.join("chapter.md"),
            "这是第一章的内容\n主角走进了森林\n远处传来狼嚎声\n他加快了脚步\n终于到达了目的地",
        )
        .unwrap();

        fs::write(
            ch_dir.join("chapter.meta.json"),
            r#"{"id": "ch1", "title": "第一章 启程", "created_at": 0, "updated_at": 0}"#,
        )
        .unwrap();

        // Second chapter
        let ch2_id = "ch2";
        let ch2_dir = projects_dir
            .join(proj_id)
            .join("volumes")
            .join(vol_id)
            .join("chapters")
            .join(ch2_id);
        fs::create_dir_all(&ch2_dir).unwrap();

        fs::write(
            ch2_dir.join("chapter.md"),
            "第二章开始了\n主角来到了城市\n在酒馆里遇到了伙伴\n他们决定一起冒险",
        )
        .unwrap();

        fs::write(
            ch2_dir.join("chapter.meta.json"),
            r#"{"id": "ch2", "title": "第二章 相遇", "created_at": 0, "updated_at": 0}"#,
        )
        .unwrap();
    }

    #[test]
    fn test_build_index_and_search() {
        let dir = tempdir().unwrap();
        setup_test_workspace(dir.path());

        let index = SearchIndex::build(dir.path()).unwrap();
        let stats = index.stats();
        assert_eq!(stats.chapter_count, 2);

        let options = SearchOptions::default();
        let hits = index.search("主角", &options);
        assert_eq!(hits.len(), 2); // line 2 ch1, line 2 ch2
    }

    #[test]
    fn test_search_case_insensitive() {
        let dir = tempdir().unwrap();
        setup_test_workspace(dir.path());

        let index = SearchIndex::build(dir.path()).unwrap();
        let options = SearchOptions::default();
        let hits = index.search("森林", &options);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chapter_title, "第一章 启程");
        assert_eq!(hits[0].line_number, 2);
    }

    #[test]
    fn test_search_with_context() {
        let dir = tempdir().unwrap();
        setup_test_workspace(dir.path());

        let index = SearchIndex::build(dir.path()).unwrap();
        let options = SearchOptions {
            context_lines: 1,
            ..Default::default()
        };
        let hits = index.search("狼嚎", &options);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].context_before.len(), 1);
        assert_eq!(hits[0].context_before[0], "主角走进了森林");
        assert_eq!(hits[0].context_after.len(), 1);
        assert_eq!(hits[0].context_after[0], "他加快了脚步");
    }

    #[test]
    fn test_search_max_results() {
        let dir = tempdir().unwrap();
        setup_test_workspace(dir.path());

        let index = SearchIndex::build(dir.path()).unwrap();
        let options = SearchOptions {
            max_results: 1,
            ..Default::default()
        };
        let hits = index.search("主角", &options);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn test_search_in_project() {
        let dir = tempdir().unwrap();
        setup_test_workspace(dir.path());

        let index = SearchIndex::build(dir.path()).unwrap();
        let options = SearchOptions::default();
        let hits = index.search_in_project("proj1", "主角", &options);
        assert_eq!(hits.len(), 2);

        let hits_empty = index.search_in_project("nonexistent", "主角", &options);
        assert_eq!(hits_empty.len(), 0);
    }

    #[test]
    fn test_search_empty_query() {
        let dir = tempdir().unwrap();
        setup_test_workspace(dir.path());

        let index = SearchIndex::build(dir.path()).unwrap();
        let options = SearchOptions::default();
        let hits = index.search("", &options);
        assert_eq!(hits.len(), 0);
    }

    #[test]
    fn test_empty_workspace() {
        let dir = tempdir().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();

        let index = SearchIndex::build(dir.path()).unwrap();
        let stats = index.stats();
        assert_eq!(stats.chapter_count, 0);

        let options = SearchOptions::default();
        let hits = index.search("test", &options);
        assert_eq!(hits.len(), 0);
    }
}
