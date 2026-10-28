//! Core-owned adapter methods for the external MCP server.
//!
//! The MCP binary only performs schema parsing and routing. Chapter paging,
//! chapter-id validation, UTF-8 boundaries and anchor persistence stay in Core
//! so that the future in-app AI entrypoint can reuse the same capability.

use crate::api::service::{ApiResult, WriterCoreApi};
use crate::api::types::{
    AnnotationTextAnchorDto, McpChapterListItemDto, McpChapterReadDto, ProjectDto,
};
use crate::api::WriterError;

const DEFAULT_CHAPTER_PAGE_CHARS: usize = 8_000;
const MAX_CHAPTER_PAGE_CHARS: usize = 50_000;

fn invalid(message: impl Into<String>) -> WriterError {
    WriterError::Other(message.into())
}

fn char_to_byte(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

impl WriterCoreApi {
    /// List chapters (optionally across every project) with their volume
    /// identity, so an MCP client can navigate without reading body text.
    pub fn mcp_list_chapters(
        &self,
        project_id: Option<&str>,
    ) -> ApiResult<Vec<McpChapterListItemDto>> {
        let projects: Vec<ProjectDto> = match project_id {
            Some(id) => self
                .list_projects()?
                .into_iter()
                .filter(|project| project.id == id)
                .collect(),
            None => self.list_projects()?,
        };
        if project_id.is_some() && projects.is_empty() {
            return Err(invalid("project does not exist"));
        }

        let mut result = Vec::new();
        for project in projects {
            for volume in self.list_volumes(&project.id)? {
                for chapter in self.list_chapters(&project.id, &volume.id)? {
                    result.push(McpChapterListItemDto {
                        project_id: project.id.clone(),
                        project_title: project.title.clone(),
                        volume_id: volume.id.clone(),
                        volume_title: volume.title.clone(),
                        chapter_id: chapter.id,
                        title: chapter.title,
                        order: chapter.order,
                        word_count: chapter.word_count,
                        hash: chapter.hash,
                        updated_at: chapter.updated_at,
                    });
                }
            }
        }
        Ok(result)
    }

    /// Read a chapter by character range. Character offsets are used instead of
    /// raw bytes so callers never produce a split UTF-8 sequence.
    pub fn mcp_read_chapter(
        &self,
        project_id: &str,
        chapter_id: &str,
        volume_id: Option<&str>,
        offset_chars: Option<usize>,
        max_chars: Option<usize>,
    ) -> ApiResult<McpChapterReadDto> {
        let location = self.mcp_find_chapter(project_id, chapter_id, volume_id)?;
        let chapter = self.open_chapter(project_id, &location.0, chapter_id)?;
        let total_chars = chapter.content.chars().count();
        let offset = offset_chars.unwrap_or(0);
        if offset > total_chars {
            return Err(invalid("offset_chars is past the end of the chapter"));
        }
        let limit = max_chars
            .unwrap_or(DEFAULT_CHAPTER_PAGE_CHARS)
            .clamp(1, MAX_CHAPTER_PAGE_CHARS);
        let end = offset.saturating_add(limit).min(total_chars);
        let start_byte = char_to_byte(&chapter.content, offset);
        let end_byte = char_to_byte(&chapter.content, end);
        Ok(McpChapterReadDto {
            project_id: project_id.to_string(),
            volume_id: location.0,
            chapter_id: chapter_id.to_string(),
            title: chapter.meta.title,
            hash: chapter.meta.hash,
            word_count: chapter.meta.word_count,
            total_chars,
            offset_chars: offset,
            end_chars: end,
            next_offset_chars: (end < total_chars).then_some(end),
            content: chapter.content[start_byte..end_byte].to_string(),
        })
    }

    /// Persist a chapter text anchor. Core re-reads the saved chapter body and
    /// validates the UTF-8 byte range; the chapter text file is never modified.
    pub fn mcp_create_chapter_text_anchor(
        &self,
        project_id: &str,
        chapter_id: &str,
        start_byte: u32,
        end_byte: u32,
        expected_hash: Option<&str>,
    ) -> ApiResult<AnnotationTextAnchorDto> {
        let (volume_id, _) = self.mcp_find_chapter(project_id, chapter_id, None)?;
        let chapter = self.open_chapter(project_id, &volume_id, chapter_id)?;
        if expected_hash.is_some_and(|expected| expected != chapter.meta.hash) {
            return Err(invalid(
                "chapter body changed since it was read; re-read before creating the anchor",
            ));
        }
        self.create_text_anchor(
            project_id.to_string(),
            chapter_id.to_string(),
            start_byte,
            end_byte,
            0,
            chapter.content,
        )
    }

    fn mcp_find_chapter(
        &self,
        project_id: &str,
        chapter_id: &str,
        volume_id: Option<&str>,
    ) -> ApiResult<(String, String)> {
        if !self
            .list_projects()?
            .iter()
            .any(|project| project.id == project_id)
        {
            return Err(invalid("project does not exist"));
        }
        let volumes = self.list_volumes(project_id)?;
        for volume in volumes {
            if volume_id.is_some_and(|requested| requested != volume.id) {
                continue;
            }
            let chapter = self
                .list_chapters(project_id, &volume.id)?
                .into_iter()
                .find(|chapter| chapter.id == chapter_id);
            if let Some(chapter) = chapter {
                return Ok((volume.id, chapter.title));
            }
        }
        Err(invalid("chapter does not exist in this project"))
    }
}
