use crate::facade::ChapterOpenResult;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct VolumeDto {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub order: i32,
}

impl From<crate::volume::Volume> for VolumeDto {
    fn from(v: crate::volume::Volume) -> Self {
        Self {
            id: v.id,
            title: v.title,
            created_at: v.created_at,
            updated_at: v.updated_at,
            order: v.order,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ChapterMetaDto {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub order: i32,
    pub word_count: u32,
    pub hash: String,
    pub note: Option<String>,
}

impl From<crate::chapter::Chapter> for ChapterMetaDto {
    fn from(c: crate::chapter::Chapter) -> Self {
        Self {
            id: c.id,
            title: c.title,
            created_at: c.created_at,
            updated_at: c.updated_at,
            order: c.order,
            word_count: c.word_count,
            hash: c.hash,
            note: c.note,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ChapterContentDto {
    pub meta: ChapterMetaDto,
    pub content: String,
}

impl From<crate::chapter::ChapterContent> for ChapterContentDto {
    fn from(c: crate::chapter::ChapterContent) -> Self {
        Self {
            meta: c.meta.into(),
            content: c.content,
        }
    }
}

impl From<ChapterOpenResult> for ChapterContentDto {
    fn from(c: ChapterOpenResult) -> Self {
        Self {
            meta: c.meta.into(),
            content: c.content,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ChapterSaveReceiptDto {
    pub chapter_relative_path: String,
    pub content_len: u32,
    pub content_hash: String,
    pub meta_hash: String,
    pub updated_at: String,
    pub word_count: u32,
}

impl From<crate::chapter::ChapterSaveReceipt> for ChapterSaveReceiptDto {
    fn from(r: crate::chapter::ChapterSaveReceipt) -> Self {
        Self {
            chapter_relative_path: r.chapter_relative_path,
            content_len: r.content_len as u32,
            content_hash: r.content_hash,
            meta_hash: r.meta_hash,
            updated_at: r.updated_at,
            word_count: r.word_count,
        }
    }
}
