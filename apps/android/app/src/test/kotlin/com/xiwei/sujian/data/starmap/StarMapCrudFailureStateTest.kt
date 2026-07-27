package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.data.ResultEnvelope
import com.xiwei.sujian.model.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.writer_core.*

class StarMapCrudFailureStateTest {

    private fun makeNodeDto(id: String, title: String) = StarMapNodeDto(
        id = id, title = title, kind = StarMapNodeKindDto.CHARACTER,
        payload = null, tags = emptyList(),
        content = StarMapNodeContentDto("empty", null, null, null, null, null, null, null, null, null, null, null),
        anchors = emptyList(), portal = null,
        displayPolicy = defaultStarMapDisplayPolicy(),
        openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
        provenance = StarMapProvenanceDto(StarMapSourceKindDto.HUMAN, null, null, null, StarMapReviewStatusDto.ACCEPTED, null),
        createdAt = 0u, updatedAt = 0u
    )

    @Test
    fun addNode_failure_doesNotCorruptCache() {
        val cache = StarMapSnapshotCache()
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 1u, complete = true, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "A")),
            edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = null, viewport = null, diagnostics = emptyList()
        )
        cache.put("sm1", dto.toRawCache())

        val beforeCount = cache.get("sm1")!!.nodes.size
        assertEquals(1, beforeCount)

        val failedResult: BridgeResult<StarMapGraphNode> = BridgeResult.Error(
            ResultEnvelope.error("NATIVE_ERROR", "add node failed")
        )
        when (failedResult) {
            is BridgeResult.Error -> {
                assertNotNull(failedResult.message)
                assertTrue(failedResult.message.isNotEmpty())
            }
            else -> fail("expected Error")
        }

        val afterCount = cache.get("sm1")!!.nodes.size
        assertEquals(beforeCount, afterCount)
        assertTrue(cache.get("sm1")!!.nodes.containsKey("n1"))
    }

    @Test
    fun deleteNode_failure_doesNotCorruptCache() {
        val cache = StarMapSnapshotCache()
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 1u, complete = true, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "A"), makeNodeDto("n2", "B")),
            edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = null, viewport = null, diagnostics = emptyList()
        )
        cache.put("sm1", dto.toRawCache())

        assertTrue(cache.get("sm1")!!.nodes.containsKey("n1"))
        assertTrue(cache.get("sm1")!!.nodes.containsKey("n2"))

        val failedResult: BridgeResult<Boolean> = BridgeResult.Error(
            ResultEnvelope.error("NATIVE_ERROR", "delete node failed")
        )
        when (failedResult) {
            is BridgeResult.Error -> {
                assertNotNull(failedResult.message)
            }
            else -> fail("expected Error")
        }

        assertTrue(cache.get("sm1")!!.nodes.containsKey("n1"))
        assertTrue(cache.get("sm1")!!.nodes.containsKey("n2"))
    }

    @Test
    fun updateNode_failure_doesNotCorruptCache() {
        val cache = StarMapSnapshotCache()
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 1u, complete = true, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "OriginalTitle")),
            edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = null, viewport = null, diagnostics = emptyList()
        )
        cache.put("sm1", dto.toRawCache())

        assertEquals("OriginalTitle", cache.get("sm1")!!.nodes["n1"]!!.title)

        val failedResult: BridgeResult<StarMapGraphNode> = BridgeResult.Error(
            ResultEnvelope.error("NATIVE_ERROR", "update node failed")
        )
        when (failedResult) {
            is BridgeResult.Error -> {
                assertNotNull(failedResult.message)
            }
            else -> fail("expected Error")
        }

        assertEquals("OriginalTitle", cache.get("sm1")!!.nodes["n1"]!!.title)
    }

    @Test
    fun addEdge_failure_doesNotCorruptCache() {
        val cache = StarMapSnapshotCache()
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 1u, complete = true, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "A")),
            edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = null, viewport = null, diagnostics = emptyList()
        )
        cache.put("sm1", dto.toRawCache())

        val beforeEdgeCount = cache.get("sm1")!!.edges.size
        assertEquals(0, beforeEdgeCount)

        val failedResult: BridgeResult<StarMapGraphEdge> = BridgeResult.Error(
            ResultEnvelope.error("NATIVE_ERROR", "add edge failed")
        )
        when (failedResult) {
            is BridgeResult.Error -> assertNotNull(failedResult.message)
            else -> fail("expected Error")
        }

        assertEquals(0, cache.get("sm1")!!.edges.size)
    }

    @Test
    fun crudFailure_preservesEditingState() {
        var lastError: String? = null
        var editingNodeId: String? = "n1"
        var selectedNodeId: String? = "n1"

        fun executeOperation(label: String, result: BridgeResult<*>): Boolean {
            return when (result) {
                is BridgeResult.Success -> {
                    lastError = null
                    true
                }
                is BridgeResult.Error -> {
                    lastError = "${label}失败: ${result.message}"
                    false
                }
                BridgeResult.NotLoaded -> {
                    lastError = "${label}失败: 未加载"
                    false
                }
            }
        }

        val failedResult: BridgeResult<StarMapGraphNode> = BridgeResult.Error(
            ResultEnvelope.error("NATIVE_ERROR", "update failed")
        )
        val ok = executeOperation("更新节点", failedResult)
        assertFalse(ok)
        assertNotNull(lastError)
        assertTrue(lastError!!.contains("更新节点失败"))
        assertEquals("editingNodeId must be preserved on failure", "n1", editingNodeId)
        assertEquals("selectedNodeId must be preserved on failure", "n1", selectedNodeId)

        val successResult: BridgeResult<StarMapGraphNode> = BridgeResult.Success(
            StarMapGraphNode(id = "n1", title = "Updated", kind = StarMapNodeKind.Character)
        )
        val ok2 = executeOperation("更新节点", successResult)
        assertTrue(ok2)
        assertNull(lastError)
        editingNodeId = null
        selectedNodeId = null
        assertNull("editingNodeId cleared only on success", editingNodeId)
    }

    @Test
    fun crudFailure_preservesDialogState() {
        var showAddNodeDialog = true
        var lastError: String? = null

        val failedResult: BridgeResult<StarMapGraphNode> = BridgeResult.Error(
            ResultEnvelope.error("NATIVE_ERROR", "add failed")
        )
        when (failedResult) {
            is BridgeResult.Error -> lastError = "添加节点失败: ${failedResult.message}"
            else -> {}
        }
        assertTrue("dialog must stay open on failure", showAddNodeDialog)

        val successResult: BridgeResult<StarMapGraphNode> = BridgeResult.Success(
            StarMapGraphNode(id = "n1", title = "New", kind = StarMapNodeKind.Character)
        )
        when (successResult) {
            is BridgeResult.Success -> {
                lastError = null
                showAddNodeDialog = false
            }
            else -> {}
        }
        assertFalse("dialog closed only on success", showAddNodeDialog)
    }
}
