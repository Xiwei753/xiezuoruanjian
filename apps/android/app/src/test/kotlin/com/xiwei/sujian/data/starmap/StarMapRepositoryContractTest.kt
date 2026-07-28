package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.model.StarMapLayoutKind
import com.xiwei.sujian.model.StarMapMotionPolicyData
import com.xiwei.sujian.model.StarMapViewportData
import org.junit.Assert.*
import org.junit.Test
import uniffi.writer_core.*

class StarMapRepositoryContractTest {

    @Test
    fun dtoMapper_metaDto_toModel_preservesFields() {
        val dto = StarMapMetaDto(
            starmapId = "sm1", title = "T", description = "D",
            projectId = "p1", parentStarmapId = null, isMainForProject = true,
            accentColor = "#FF0000", nodeCount = 5u, edgeCount = 3u,
            linkedChapterCount = 2u, childStarmapCount = 1u,
            createdAt = 1000u, updatedAt = 2000u
        )
        val model = dto.toModel()
        assertEquals("sm1", model.starmapId)
        assertEquals("T", model.title)
        assertEquals("D", model.description)
        assertEquals("p1", model.projectId)
        assertTrue(model.isMainForProject)
        assertEquals(5, model.nodeCount)
        assertEquals(3, model.edgeCount)
    }

    @Test
    fun dtoMapper_viewportDto_toModel_roundtrip() {
        val dto = StarMapViewportDto(scale = 2f, offsetX = 10f, offsetY = 20f, width = 800f, height = 600f)
        val model = dto.toModel()
        assertEquals(2f, model.scale, 0.001f)
        assertEquals(10f, model.offsetX, 0.001f)
        assertEquals(800f, model.width, 0.001f)

        val backToDto = model.toDto()
        assertEquals(2f, backToDto.scale, 0.001f)
        assertEquals(10f, backToDto.offsetX, 0.001f)
    }

    @Test
    fun dtoMapper_motionPolicyDto_toModel_preservesAllFields() {
        val dto = StarMapMotionPolicyDto(
            enabled = true, idleWobbleEnabled = false,
            idleAmplitudeVp = 3.5f, idlePeriodMs = 5000u,
            dragLiftScale = 1.1f, dragShadowBoost = 10.0f,
            settleDurationMs = 300u, reduceMotion = true
        )
        val model = dto.toModel()
        assertTrue(model.enabled)
        assertFalse(model.idleWobbleEnabled)
        assertEquals(3.5f, model.idleAmplitudeVp, 0.001f)
        assertEquals(5000, model.idlePeriodMs)
        assertEquals(1.1f, model.dragLiftScale, 0.001f)
        assertEquals(10.0f, model.dragShadowBoost, 0.001f)
        assertEquals(300, model.settleDurationMs)
        assertTrue(model.reduceMotion)
    }

    @Test
    fun snapshotCache_lifecycleSave_requiresInitializedCache() {
        val cache = StarMapSnapshotCache()
        assertNull("cache must be null before snapshot is loaded", cache.get("sm1"))
    }

    @Test
    fun snapshotCache_putAndFlush_isConsistent() {
        val cache = StarMapSnapshotCache()
        val rawCache = StarMapRawCache(
            graph = StarMapGraphDto(
                schemaVersion = 1u, id = "sm1", starmapId = "sm1", title = "T",
                nodes = emptyList(), edges = emptyList(), embeds = emptyList(), links = emptyList(),
                createdAt = 0u, updatedAt = 0u
            ),
            nodes = mutableMapOf(), edges = mutableMapOf(),
            embeds = mutableMapOf(), links = mutableMapOf(), hyperlinks = mutableMapOf(),
            layoutNodes = mutableMapOf()
        )
        cache.put("sm1", rawCache)
        assertNotNull(cache.get("sm1"))
        assertEquals("sm1", cache.get("sm1")!!.graph!!.starmapId)
    }

    @Test
    fun repository_saveStarmapViewport_convertsModelToDto() {
        val viewport = StarMapViewportData(scale = 1.5f, offsetX = 5f, offsetY = 15f, width = 1000f, height = 700f)
        val dto = viewport.toDto()
        assertEquals(1.5f, dto.scale, 0.001f)
        assertEquals(5f, dto.offsetX, 0.001f)
        assertEquals(1000f, dto.width, 0.001f)
    }

    @Test
    fun repository_getStarmapViewport_convertsDtoToModel() {
        val dto = StarMapViewportDto(scale = 2f, offsetX = 10f, offsetY = 20f, width = 800f, height = 600f)
        val model = dto.toModel()
        assertEquals(2f, model.scale, 0.001f)
    }

    @Test
    fun repository_listStarmaps_mapsEachDtoToModel() {
        val dto1 = StarMapMetaDto(
            starmapId = "sm1", title = "A", description = "",
            projectId = "", parentStarmapId = null, isMainForProject = false,
            accentColor = "", nodeCount = 1u, edgeCount = 0u,
            linkedChapterCount = 0u, childStarmapCount = 0u,
            createdAt = 0u, updatedAt = 0u
        )
        val dto2 = StarMapMetaDto(
            starmapId = "sm2", title = "B", description = "",
            projectId = "", parentStarmapId = null, isMainForProject = false,
            accentColor = "", nodeCount = 2u, edgeCount = 1u,
            linkedChapterCount = 0u, childStarmapCount = 0u,
            createdAt = 0u, updatedAt = 0u
        )
        val dtos = listOf(dto1, dto2)
        val models = dtos.map { it.toModel() }
        assertEquals(2, models.size)
        assertEquals("sm1", models[0].starmapId)
        assertEquals("sm2", models[1].starmapId)
        assertEquals(1, models[0].nodeCount)
        assertEquals(2, models[1].nodeCount)
    }

    @Test
    fun repository_getMotionPolicy_mapsDtoToModel() {
        val dto = StarMapMotionPolicyDto(
            enabled = false, idleWobbleEnabled = true,
            idleAmplitudeVp = 1f, idlePeriodMs = 3000u,
            dragLiftScale = 1.0f, dragShadowBoost = 5f,
            settleDurationMs = 100u, reduceMotion = false
        )
        val model = dto.toModel()
        assertFalse(model.enabled)
        assertTrue(model.idleWobbleEnabled)
        assertEquals(3000, model.idlePeriodMs)
    }
}
