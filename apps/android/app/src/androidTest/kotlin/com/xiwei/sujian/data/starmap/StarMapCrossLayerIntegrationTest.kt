package com.xiwei.sujian.data.starmap

import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.data.StarMapBridge
import com.xiwei.sujian.model.StarMapMeta
import com.xiwei.sujian.model.StarMapGraphNode
import com.xiwei.sujian.model.StarMapNodeKind
import com.xiwei.sujian.support.AndroidTestEnvironment
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class StarMapCrossLayerIntegrationTest {

    @get:Rule
    val testRule = AndroidTestEnvironment.TestDependenciesRule()

    private fun countingBridgeAndRepo(): Triple<CallCountingStarMapBridgeOps, StarMapBridge, StarMapRepository> {
        val session = AndroidTestEnvironment.requireCurrentSession()
        val bridge = session.deps.appServiceBridge.starMapBridge
        val countingOps = CallCountingStarMapBridgeOps(bridge)
        val repo = StarMapRepository(countingOps, StarMapSnapshotCache())
        return Triple(countingOps, bridge, repo)
    }

    private fun requireStarmapId(createResult: BridgeResult<StarMapMeta>, operation: String): String {
        when (createResult) {
            is BridgeResult.Success -> return createResult.data.starmapId
            is BridgeResult.Error -> throw AssertionError(
                "$operation: createStarmap returned BridgeResult.Error: " +
                    "errorCode=${createResult.code}, message=${createResult.message}, " +
                    "rawError=${createResult.envelope.rawError}, " +
                    "messageKey=${createResult.envelope.messageKey}"
            )
            BridgeResult.NotLoaded -> throw AssertionError(
                "$operation: createStarmap returned NotLoaded — native library not available"
            )
        }
    }

    @Test
    fun progressiveLoading_threePhases_neverCallsGetStarMapGraph() {
        val (countingOps, bridge, repo) = countingBridgeAndRepo()

        val createResult = repo.createStarmap("跨层渐进加载测试", "验证三阶段不调用getStarMapGraph")
        val starmapId = requireStarmapId(createResult, "progressiveLoading_threePhases")

        var testException: Throwable? = null
        try {
            val n1 = StarMapGraphNode(id = UUID.randomUUID().toString(), title = "角色A", kind = StarMapNodeKind.Character)
            val n2 = StarMapGraphNode(id = UUID.randomUUID().toString(), title = "事件B", kind = StarMapNodeKind.Event)
            val n3 = StarMapGraphNode(id = UUID.randomUUID().toString(), title = "地点C", kind = StarMapNodeKind.Location)
            assertTrue("add n1", repo.addStarmapNode(starmapId, n1, 50f, 60f) is BridgeResult.Success)
            assertTrue("add n2", repo.addStarmapNode(starmapId, n2, 200f, 60f) is BridgeResult.Success)
            assertTrue("add n3", repo.addStarmapNode(starmapId, n3, 350f, 60f) is BridgeResult.Success)

            val phase1 = repo.getStarmapPhasedSnapshot(starmapId)
            assertTrue("phase1 must succeed", phase1 is BridgeResult.Success)
            val phase1Data = (phase1 as BridgeResult.Success).data.data
            assertTrue("phase1 must have nodes", phase1Data.graph.nodes.isNotEmpty())
            val phase1Rev = phase1Data.packageRevision

            val phase2 = repo.advanceLoadPhase(starmapId, "PrefetchNearbyObjects", phase1Rev)
            assertTrue("phase2 must succeed", phase2 is BridgeResult.Success)
            val phase2Data = (phase2 as BridgeResult.Success).data.data
            assertTrue("phase2 nodes >= phase1 nodes", phase2Data.graph.nodes.size >= phase1Data.graph.nodes.size)
            val phase2Rev = phase2Data.packageRevision

            val phase3 = repo.advanceLoadPhase(starmapId, "BackgroundFullLoad", phase2Rev)
            assertTrue("phase3 must succeed", phase3 is BridgeResult.Success)
            val phase3Data = (phase3 as BridgeResult.Success).data.data
            assertTrue("phase3 must have all 3 nodes", phase3Data.graph.nodes.size >= 3)

            assertEquals("getStarMapGraph must not be called", 0, countingOps.getStarMapGraphCallCount)
        } catch (t: Throwable) {
            testException = t
        } finally {
            try { bridge.closeStarmapStore(starmapId) } catch (cleanup: Throwable) {
                if (testException != null) {
                    testException.addSuppressed(cleanup)
                } else {
                    testException = cleanup
                }
            }
        }
        if (testException != null) throw testException
    }

    @Test
    fun progressiveLoading_sameRevisionAdvancesThreePhases() {
        val (countingOps, bridge, repo) = countingBridgeAndRepo()

        val createResult = repo.createStarmap("同版本三阶段测试", "验证同一packageRevision连续推进")
        val starmapId = requireStarmapId(createResult, "progressiveLoading_sameRevision")

        var testException: Throwable? = null
        try {
            val n1 = StarMapGraphNode(id = UUID.randomUUID().toString(), title = "节点1", kind = StarMapNodeKind.Character)
            val n2 = StarMapGraphNode(id = UUID.randomUUID().toString(), title = "节点2", kind = StarMapNodeKind.Event)
            assertTrue("add n1", repo.addStarmapNode(starmapId, n1, 50f, 60f) is BridgeResult.Success)
            assertTrue("add n2", repo.addStarmapNode(starmapId, n2, 200f, 60f) is BridgeResult.Success)

            val phase1 = repo.getStarmapPhasedSnapshot(starmapId, targetPhase = "CurrentViewportObjects")
            assertTrue("phase1 must succeed", phase1 is BridgeResult.Success)
            val rev1 = (phase1 as BridgeResult.Success).data.data.packageRevision

            val phase2 = repo.advanceLoadPhase(starmapId, "PrefetchNearbyObjects", rev1)
            assertTrue("phase2 must succeed", phase2 is BridgeResult.Success)
            val rev2 = (phase2 as BridgeResult.Success).data.data.packageRevision
            assertEquals("phase2 revision must be same as phase1 (no mutations)", rev1, rev2)

            val phase3 = repo.advanceLoadPhase(starmapId, "BackgroundFullLoad", rev2)
            assertTrue("phase3 must succeed", phase3 is BridgeResult.Success)
            val rev3 = (phase3 as BridgeResult.Success).data.data.packageRevision
            assertEquals("phase3 revision must be same as phase1 (no mutations)", rev1, rev3)

            assertEquals("getStarMapGraph must not be called", 0, countingOps.getStarMapGraphCallCount)
        } catch (t: Throwable) {
            testException = t
        } finally {
            try { bridge.closeStarmapStore(starmapId) } catch (cleanup: Throwable) {
                if (testException != null) {
                    testException.addSuppressed(cleanup)
                } else {
                    testException = cleanup
                }
            }
        }
        if (testException != null) throw testException
    }

    @Test
    fun dtoModelConversion_fieldsMatchCore() {
        val (countingOps, bridge, repo) = countingBridgeAndRepo()

        val createResult = repo.createStarmap("DTO转换一致性测试", "验证DTO到Model字段一致")
        val starmapId = requireStarmapId(createResult, "dtoModelConversion")

        var testException: Throwable? = null
        try {
            val n1 = StarMapGraphNode(id = UUID.randomUUID().toString(), title = "角色X", kind = StarMapNodeKind.Character)
            val addResult1 = repo.addStarmapNode(starmapId, n1, 100f, 200f)
            assertTrue("add n1 must succeed", addResult1 is BridgeResult.Success)
            val node1Id = (addResult1 as BridgeResult.Success).data.id

            val n2 = StarMapGraphNode(id = UUID.randomUUID().toString(), title = "主题Y", kind = StarMapNodeKind.Theme)
            val addResult2 = repo.addStarmapNode(starmapId, n2, 300f, 200f)
            assertTrue("add n2 must succeed", addResult2 is BridgeResult.Success)
            val node2Id = (addResult2 as BridgeResult.Success).data.id

            val snapshot = repo.getStarmapPhasedSnapshot(starmapId)
            assertTrue("snapshot must succeed", snapshot is BridgeResult.Success)
            val data = (snapshot as BridgeResult.Success).data.data

            val snapshotN1 = data.graph.nodes.find { it.id == node1Id }
            assertNotNull("node1 must be in snapshot", snapshotN1)
            assertEquals("角色X", snapshotN1!!.title)
            assertEquals(StarMapNodeKind.Character, snapshotN1.kind)

            val snapshotN2 = data.graph.nodes.find { it.id == node2Id }
            assertNotNull("node2 must be in snapshot", snapshotN2)
            assertEquals("主题Y", snapshotN2!!.title)
            assertEquals(StarMapNodeKind.Theme, snapshotN2.kind)

            assertEquals("getStarMapGraph must not be called", 0, countingOps.getStarMapGraphCallCount)
        } catch (t: Throwable) {
            testException = t
        } finally {
            try { bridge.closeStarmapStore(starmapId) } catch (cleanup: Throwable) {
                if (testException != null) {
                    testException.addSuppressed(cleanup)
                } else {
                    testException = cleanup
                }
            }
        }
        if (testException != null) throw testException
    }

    @Test
    fun incrementalMerge_cacheConsistentWithCore() {
        val (countingOps, bridge, repo) = countingBridgeAndRepo()

        val createResult = repo.createStarmap("增量合并一致性测试", "验证增量合并后缓存与Core一致")
        val starmapId = requireStarmapId(createResult, "incrementalMerge")

        var testException: Throwable? = null
        try {
            val n1 = StarMapGraphNode(id = UUID.randomUUID().toString(), title = "视口内节点", kind = StarMapNodeKind.Character)
            assertTrue("add n1", repo.addStarmapNode(starmapId, n1, 50f, 60f) is BridgeResult.Success)

            val phase1 = repo.getStarmapPhasedSnapshot(starmapId, targetPhase = "CurrentViewportObjects")
            assertTrue("phase1 must succeed", phase1 is BridgeResult.Success)
            val phase1Data = (phase1 as BridgeResult.Success).data.data
            val phase1NodeCount = phase1Data.graph.nodes.size
            assertTrue("phase1 must have at least 1 node", phase1NodeCount >= 1)

            val n2 = StarMapGraphNode(id = UUID.randomUUID().toString(), title = "附近节点", kind = StarMapNodeKind.Event)
            assertTrue("add n2", repo.addStarmapNode(starmapId, n2, 200f, 60f) is BridgeResult.Success)

            val phase2 = repo.advanceLoadPhase(starmapId, "PrefetchNearbyObjects", phase1Data.packageRevision)
            assertTrue("phase2 must succeed", phase2 is BridgeResult.Success)
            val phase2Data = (phase2 as BridgeResult.Success).data.data
            assertTrue("phase2 must have at least as many nodes as phase1", phase2Data.graph.nodes.size >= phase1NodeCount)

            val n3 = StarMapGraphNode(id = UUID.randomUUID().toString(), title = "远处节点", kind = StarMapNodeKind.Location)
            assertTrue("add n3", repo.addStarmapNode(starmapId, n3, 500f, 60f) is BridgeResult.Success)

            val phase3 = repo.advanceLoadPhase(starmapId, "BackgroundFullLoad", phase2Data.packageRevision)
            assertTrue("phase3 must succeed", phase3 is BridgeResult.Success)
            val phase3Data = (phase3 as BridgeResult.Success).data.data
            assertTrue("phase3 must have all 3 nodes", phase3Data.graph.nodes.size >= 3)

            val allTitles = phase3Data.graph.nodes.map { it.title }.toSet()
            assertTrue("视口内节点 must exist", allTitles.contains("视口内节点"))
            assertTrue("附近节点 must exist", allTitles.contains("附近节点"))
            assertTrue("远处节点 must exist", allTitles.contains("远处节点"))

            assertEquals("getStarMapGraph must not be called", 0, countingOps.getStarMapGraphCallCount)
        } catch (t: Throwable) {
            testException = t
        } finally {
            try { bridge.closeStarmapStore(starmapId) } catch (cleanup: Throwable) {
                if (testException != null) {
                    testException.addSuppressed(cleanup)
                } else {
                    testException = cleanup
                }
            }
        }
        if (testException != null) throw testException
    }

    @Test
    fun crudUpdateNode_cacheConsistentWithCore() {
        val (countingOps, bridge, repo) = countingBridgeAndRepo()

        val createResult = repo.createStarmap("更新节点一致性测试", "验证更新后缓存与Core一致")
        val starmapId = requireStarmapId(createResult, "crudUpdateNode")

        var testException: Throwable? = null
        try {
            val node = StarMapGraphNode(id = UUID.randomUUID().toString(), title = "原始标题", kind = StarMapNodeKind.Character)
            val addResult = repo.addStarmapNode(starmapId, node, 100f, 100f)
            assertTrue("addStarmapNode must succeed", addResult is BridgeResult.Success)
            val nodeId = (addResult as BridgeResult.Success).data.id

            val snapshotBefore = repo.getStarmapPhasedSnapshot(starmapId)
            assertTrue("snapshot before update must succeed", snapshotBefore is BridgeResult.Success)

            val updateResult = repo.updateStarmapNode(starmapId, nodeId, title = "更新后标题")
            assertTrue("updateStarmapNode must succeed", updateResult is BridgeResult.Success)
            val updatedNode = (updateResult as BridgeResult.Success).data
            assertEquals("更新后标题", updatedNode.title)
            assertEquals(nodeId, updatedNode.id)

            val snapshotAfter = repo.getStarmapPhasedSnapshot(starmapId)
            assertTrue("snapshot after update must succeed", snapshotAfter is BridgeResult.Success)
            val dataAfter = (snapshotAfter as BridgeResult.Success).data.data
            val nodeInSnapshot = dataAfter.graph.nodes.find { it.id == nodeId }
            assertNotNull("updated node must be in snapshot", nodeInSnapshot)
            assertEquals("更新后标题", nodeInSnapshot!!.title)

            assertEquals("getStarMapGraph must not be called", 0, countingOps.getStarMapGraphCallCount)
        } catch (t: Throwable) {
            testException = t
        } finally {
            try { bridge.closeStarmapStore(starmapId) } catch (cleanup: Throwable) {
                if (testException != null) {
                    testException.addSuppressed(cleanup)
                } else {
                    testException = cleanup
                }
            }
        }
        if (testException != null) throw testException
    }

    @Test
    fun crudDeleteNode_cacheConsistentWithCore() {
        val (countingOps, bridge, repo) = countingBridgeAndRepo()

        val createResult = repo.createStarmap("删除节点一致性测试", "验证删除后缓存与Core一致")
        val starmapId = requireStarmapId(createResult, "crudDeleteNode")

        var testException: Throwable? = null
        try {
            val n1 = StarMapGraphNode(id = UUID.randomUUID().toString(), title = "保留节点", kind = StarMapNodeKind.Character)
            val addResult1 = repo.addStarmapNode(starmapId, n1, 50f, 60f)
            assertTrue("add n1 must succeed", addResult1 is BridgeResult.Success)
            val node1Id = (addResult1 as BridgeResult.Success).data.id

            val n2 = StarMapGraphNode(id = UUID.randomUUID().toString(), title = "待删除节点", kind = StarMapNodeKind.Event)
            val addResult2 = repo.addStarmapNode(starmapId, n2, 200f, 60f)
            assertTrue("add n2 must succeed", addResult2 is BridgeResult.Success)
            val node2Id = (addResult2 as BridgeResult.Success).data.id

            val snapshotBefore = repo.getStarmapPhasedSnapshot(starmapId)
            assertTrue("snapshot before delete must succeed", snapshotBefore is BridgeResult.Success)
            val dataBefore = (snapshotBefore as BridgeResult.Success).data.data
            assertTrue("must have at least 2 nodes before delete", dataBefore.graph.nodes.size >= 2)

            val deleteResult = repo.deleteStarmapNode(starmapId, node2Id)
            assertTrue("deleteStarmapNode must succeed", deleteResult is BridgeResult.Success)

            val snapshotAfter = repo.getStarmapPhasedSnapshot(starmapId)
            assertTrue("snapshot after delete must succeed", snapshotAfter is BridgeResult.Success)
            val dataAfter = (snapshotAfter as BridgeResult.Success).data.data
            assertNull("deleted node must not be in snapshot", dataAfter.graph.nodes.find { it.id == node2Id })
            assertNotNull("remaining node must be in snapshot", dataAfter.graph.nodes.find { it.id == node1Id })

            assertEquals("getStarMapGraph must not be called", 0, countingOps.getStarMapGraphCallCount)
        } catch (t: Throwable) {
            testException = t
        } finally {
            try { bridge.closeStarmapStore(starmapId) } catch (cleanup: Throwable) {
                if (testException != null) {
                    testException.addSuppressed(cleanup)
                } else {
                    testException = cleanup
                }
            }
        }
        if (testException != null) throw testException
    }

    @Test
    fun deleteFlushReload_nodeRemovedAcrossFlushBoundary() {
        val (countingOps, bridge, repo) = countingBridgeAndRepo()

        val createResult = repo.createStarmap("删除冲刷重载测试", "验证删除+flush+reload后节点消失")
        val starmapId = requireStarmapId(createResult, "deleteFlushReload")

        var testException: Throwable? = null
        try {
            val n1 = StarMapGraphNode(id = UUID.randomUUID().toString(), title = "保留节点", kind = StarMapNodeKind.Character)
            val n2 = StarMapGraphNode(id = UUID.randomUUID().toString(), title = "待删节点", kind = StarMapNodeKind.Event)
            assertTrue("add n1", repo.addStarmapNode(starmapId, n1, 50f, 60f) is BridgeResult.Success)
            assertTrue("add n2", repo.addStarmapNode(starmapId, n2, 200f, 60f) is BridgeResult.Success)

            val before = repo.getStarmapPhasedSnapshot(starmapId)
            assertTrue("before delete must succeed", before is BridgeResult.Success)
            val beforeNodes = (before as BridgeResult.Success).data.data.graph.nodes
            assertTrue("must have 2 nodes before delete", beforeNodes.size >= 2)

            assertTrue("delete must succeed", repo.deleteStarmapNode(starmapId, n2.id) is BridgeResult.Success)
            assertTrue("flush must succeed", repo.flushStarmapStore(starmapId) is BridgeResult.Success)

            val after = repo.getStarmapPhasedSnapshot(starmapId)
            assertTrue("after reload must succeed", after is BridgeResult.Success)
            val afterData = (after as BridgeResult.Success).data.data
            assertNull("deleted node must not appear", afterData.graph.nodes.find { it.id == n2.id })
            assertNotNull("remaining node must appear", afterData.graph.nodes.find { it.id == n1.id })

            assertEquals("getStarMapGraph must not be called", 0, countingOps.getStarMapGraphCallCount)
        } catch (t: Throwable) {
            testException = t
        } finally {
            try { bridge.closeStarmapStore(starmapId) } catch (cleanup: Throwable) {
                if (testException != null) {
                    testException.addSuppressed(cleanup)
                } else {
                    testException = cleanup
                }
            }
        }
        if (testException != null) throw testException
    }

    @Test
    fun deleteThenRecreateSameId_restoresNode() {
        val (countingOps, bridge, repo) = countingBridgeAndRepo()

        val createResult = repo.createStarmap("删除重建测试", "验证删除同ID节点后重新添加可恢复")
        val starmapId = requireStarmapId(createResult, "deleteThenRecreateSameId")

        var testException: Throwable? = null
        try {
            val nodeId = UUID.randomUUID().toString()
            val n1 = StarMapGraphNode(id = nodeId, title = "原始标题", kind = StarMapNodeKind.Character)
            assertTrue("add n1", repo.addStarmapNode(starmapId, n1, 50f, 60f) is BridgeResult.Success)
            assertTrue("flush must succeed", repo.flushStarmapStore(starmapId) is BridgeResult.Success)

            assertTrue("delete must succeed", repo.deleteStarmapNode(starmapId, nodeId) is BridgeResult.Success)
            assertTrue("flush after delete must succeed", repo.flushStarmapStore(starmapId) is BridgeResult.Success)

            val n2 = StarMapGraphNode(id = nodeId, title = "重建标题", kind = StarMapNodeKind.Character)
            assertTrue("recreate with same ID", repo.addStarmapNode(starmapId, n2, 50f, 60f) is BridgeResult.Success)
            assertTrue("flush after recreate must succeed", repo.flushStarmapStore(starmapId) is BridgeResult.Success)

            val snapshot = repo.getStarmapPhasedSnapshot(starmapId)
            assertTrue("final snapshot must succeed", snapshot is BridgeResult.Success)
            val data = (snapshot as BridgeResult.Success).data.data
            val found = data.graph.nodes.find { it.id == nodeId }
            assertNotNull("recreated node must be in snapshot", found)
            assertEquals("recreated node must have new title", "重建标题", found!!.title)

            assertEquals("getStarMapGraph must not be called", 0, countingOps.getStarMapGraphCallCount)
        } catch (t: Throwable) {
            testException = t
        } finally {
            try { bridge.closeStarmapStore(starmapId) } catch (cleanup: Throwable) {
                if (testException != null) {
                    testException.addSuppressed(cleanup)
                } else {
                    testException = cleanup
                }
            }
        }
        if (testException != null) throw testException
    }
}
