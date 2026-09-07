package com.xiwei.sujian.core.interop.common

import com.xiwei.sujian.core.interop.project.toModel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import uniffi.writer_core.ChapterContentDto
import uniffi.writer_core.ChapterMetaDto
import uniffi.writer_core.ChapterSaveReceiptDto
import uniffi.writer_core.ProjectDto
import uniffi.writer_core.ProjectStatsDto
import uniffi.writer_core.RecentEditDto
import uniffi.writer_core.VolumeDto

class BridgeMapperTest {
    @Test
    fun projectDto_toModel_mapsAllFields() {
        val dto =
            ProjectDto(
                "p1",
                "Test Project",
                "2023-01-01",
                "2023-01-02",
            )
        val model = dto.toModel()
        assertEquals("p1", model.id)
        assertEquals("Test Project", model.title)
        assertEquals("2023-01-01", model.createdAt)
        assertEquals("2023-01-02", model.updatedAt)
    }

    @Test
    fun projectStatsDto_toModel_mapsAllFields() {
        val dto =
            ProjectStatsDto(
                1000U,
                5U,
                50U,
            )
        val model = dto.toModel()
        assertEquals(1000, model.totalWordCount)
        assertEquals(5, model.volumeCount)
        assertEquals(50, model.chapterCount)
    }

    @Test
    fun volumeDto_toModel_mapsAllFields() {
        val dto =
            VolumeDto(
                "v1",
                "Test Volume",
                "2023-01-01",
                "2023-01-02",
                3,
            )
        val model = dto.toModel()
        assertEquals("v1", model.id)
        assertEquals("Test Volume", model.title)
        assertEquals("2023-01-01", model.createdAt)
        assertEquals("2023-01-02", model.updatedAt)
        assertEquals(3, model.order)
    }

    @Test
    fun chapterMetaDto_toModel_mapsAllFields() {
        val dto =
            ChapterMetaDto(
                "ch1",
                "Chapter One",
                "2023-01-01",
                "2023-01-02",
                2,
                500U,
                "abc123",
                "a note",
            )
        val model = dto.toModel()
        assertEquals("ch1", model.id)
        assertEquals("Chapter One", model.title)
        assertEquals("2023-01-01", model.createdAt)
        assertEquals("2023-01-02", model.updatedAt)
        assertEquals(2, model.order)
        assertEquals(500, model.wordCount)
        assertEquals("abc123", model.hash)
        assertEquals("a note", model.note)
    }

    @Test
    fun chapterMetaDto_toModel_mapsNullNote() {
        val dto =
            ChapterMetaDto(
                "ch2",
                "Chapter Two",
                "2023-01-01",
                "2023-01-02",
                1,
                0U,
                "h",
                null,
            )
        val model = dto.toModel()
        assertNull(model.note)
    }

    @Test
    fun chapterContentDto_toModel_mapsAllFields() {
        val metaDto =
            ChapterMetaDto(
                "ch1",
                "Ch",
                "2023-01-01",
                "2023-01-02",
                1,
                100U,
                "h1",
                "a note",
            )
        val dto = ChapterContentDto(metaDto, "Hello world")
        val model = dto.toModel()
        assertEquals("ch1", model.meta.id)
        assertEquals("Ch", model.meta.title)
        assertEquals("2023-01-01", model.meta.createdAt)
        assertEquals("2023-01-02", model.meta.updatedAt)
        assertEquals(1, model.meta.order)
        assertEquals(100, model.meta.wordCount)
        assertEquals("h1", model.meta.hash)
        assertEquals("a note", model.meta.note)
        assertEquals("Hello world", model.content)
    }

    @Test
    fun chapterSaveReceiptDto_toModel_mapsAllFields() {
        val dto =
            ChapterSaveReceiptDto(
                "projects/p1/volumes/v1/chapters/ch1.md",
                42U,
                "abc123",
                "def456",
                "2023-01-02T00:00:00Z",
                7U,
            )
        val model = dto.toModel()
        assertEquals("projects/p1/volumes/v1/chapters/ch1.md", model.chapterRelativePath)
        assertEquals(42L, model.contentLen)
        assertEquals("abc123", model.contentHash)
        assertEquals("def456", model.metaHash)
        assertEquals("2023-01-02T00:00:00Z", model.updatedAt)
        assertEquals(7, model.wordCount)
    }

    @Test
    fun recentEditDto_toModel_mapsAllFields() {
        val dto =
            RecentEditDto(
                "p1",
                "v1",
                "ch1",
                "2023-01-01T00:00:00Z",
            )
        val model = dto.toModel()
        assertEquals("p1", model.projectId)
        assertEquals("v1", model.volumeId)
        assertEquals("ch1", model.chapterId)
        assertEquals("2023-01-01T00:00:00Z", model.timestamp)
    }
}
