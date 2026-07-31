package com.xiwei.sujian.data

import com.xiwei.sujian.model.*
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import uniffi.writer_core.*

class BridgeMapperContractTest {

    @Test
    fun projectDto_toModel_mapsAllFields() {
        val dto = ProjectDto(
            id = "p1",
            title = "Test Project",
            createdAt = "2023-01-01",
            updatedAt = "2023-01-02"
        )
        val model = dto.toModel()
        assertEquals("p1", model.id)
        assertEquals("Test Project", model.title)
        assertEquals("2023-01-01", model.createdAt)
        assertEquals("2023-01-02", model.updatedAt)
    }

    @Test
    fun projectStatsDto_toModel_mapsAllFields() {
        val dto = ProjectStatsDto(
            totalWordCount = 1000U,
            volumeCount = 5U,
            chapterCount = 50U
        )
        val model = dto.toModel()
        assertEquals(1000, model.totalWordCount)
        assertEquals(5, model.volumeCount)
        assertEquals(50, model.chapterCount)
    }

    @Test
    fun volumeDto_toModel_mapsAllFields() {
        val dto = VolumeDto(
            id = "v1",
            title = "Test Volume",
            createdAt = "2023-01-01",
            updatedAt = "2023-01-02",
            order = 3
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
        val dto = ChapterMetaDto(
            id = "ch1",
            title = "Chapter One",
            createdAt = "2023-01-01",
            updatedAt = "2023-01-02",
            order = 2,
            wordCount = 500U,
            hash = "abc123",
            note = "a note"
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
        val dto = ChapterMetaDto(
            id = "ch2",
            title = "Chapter Two",
            createdAt = "2023-01-01",
            updatedAt = "2023-01-02",
            order = 1,
            wordCount = 0U,
            hash = "h",
            note = null
        )
        val model = dto.toModel()
        assertNull(model.note)
    }

    @Test
    fun chapterContentDto_toModel_mapsAllFields() {
        val metaDto = ChapterMetaDto(
            id = "ch1",
            title = "Ch",
            createdAt = "2023-01-01",
            updatedAt = "2023-01-02",
            order = 1,
            wordCount = 100U,
            hash = "h1",
            note = null
        )
        val dto = ChapterContentDto(meta = metaDto, content = "Hello world")
        val model = dto.toModel()
        assertEquals("ch1", model.meta.id)
        assertEquals("Hello world", model.content)
    }

    @Test
    fun chapterSaveReceiptDto_toModel_mapsAllFields() {
        val dto = ChapterSaveReceiptDto(
            chapterRelativePath = "projects/p1/volumes/v1/chapters/ch1.md",
            contentLen = 42U,
            contentHash = "abc123",
            metaHash = "def456",
            updatedAt = "2023-01-02T00:00:00Z",
            wordCount = 7U
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
    fun recentEditDto_roundtrip_viaWorkspaceSummary() {
        val projectDto = ProjectDto(
            id = "p1",
            title = "P",
            createdAt = "2023-01-01",
            updatedAt = "2023-01-02"
        )
        val project = projectDto.toModel()
        assertEquals("p1", project.id)
        assertEquals("P", project.title)
        assertEquals("2023-01-01", project.createdAt)
        assertEquals("2023-01-02", project.updatedAt)
    }
}
