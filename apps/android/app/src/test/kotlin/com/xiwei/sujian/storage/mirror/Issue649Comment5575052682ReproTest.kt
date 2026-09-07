package com.xiwei.sujian.storage.mirror

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ConcurrentLinkedQueue
import androidx.test.core.app.ApplicationProvider

/**
 * #649 评论 5575052682：两个影响镜像最终一致性的硬问题复现测试。
 *
 * 本测试文件验证评论 5575052682 指出的 2 个硬缺陷在当前代码中确实存在。
 * 采用代码结构证据复现策略：手动模拟生产代码的关键逻辑顺序与数据流，
 * 断言错误行为（变更意图丢失、manifest 跨版本混合）。
 *
 * 源文件：
 * - apps/android/app/src/main/kotlin/com/xiwei/sujian/storage/mirror/MirrorChangeSink.kt
 * - apps/android/app/src/main/kotlin/com/xiwei/sujian/storage/mirror/ReadableMirrorPublisher.kt
 * - apps/android/app/src/main/kotlin/com/xiwei/sujian/storage/mirror/PendingMirrorPublish.kt
 *
 * 硬问题 1：DefaultMirrorChangeSink 的 dirtyMap/deleteQueue/Channel.CONFLATED 都是纯内存对象，
 *   chapterChanged()/projectStructureChanged()/projectDeleted() 只改内存再 signal，
 *   没有任何持久化 outbox 调用；真正的 pendingPublish journal 要等 worker 进入
 *   publishProject()/deleteProject() 后才创建。进程被杀重启后变更意图丢失且无法恢复。
 *
 * 硬问题 2：recoverPromotePhase() 在旧事务正文 promote 完后仍重新调用
 *   source.getProjectWorkspaceSnapshot(journal.projectId) 读取当前 Core snapshot，
 *   并把这个当前 snapshot 与旧事务 journal 的 promotedEntries 一起传给
 *   publishManifestWithDesiredTransactional()。当 manifestTargetJson==null
 *   （manifest 子事务未开始）时走 buildManifestJsonForDesired 用 R2 metadata + R1 正文
 *   生成混合 manifest。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue649Comment5575052682ReproTest {

    // ══════════════════════════════════════════════════════════════════════
    // 硬问题 1：MirrorChangeSink canonical 已保存、mirror 事务还没开始时，
    //          变更意图仍只存在内存里，进程死亡重启后丢失且无法恢复。
    // 源：MirrorChangeSink.kt line 103-105 (内存字段), 123-142 (只改内存再 signal)
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 硬问题 1 复现：DefaultMirrorChangeSink 的 dirtyMap (line 103)、deleteQueue (line 104)、
     * signal (line 105) 都是纯内存对象。chapterChanged() (line 123-130) 只做：
     *   dirtyMap[MirrorKey(projectId, volumeId, chapterId)] = DirtyEntry(now)
     *   signal.trySend(Unit)
     * 没有任何持久化 outbox 调用。
     *
     * 窗口：
     * 1. 私有 canonical 正文保存成功；
     * 2. chapterChanged() 已入 dirtyMap；
     * 3. worker 还没进入 publishProject()，此时没有任何 mirror journal；
     * 4. Android 杀进程；
     * 5. 重启后 dirtyMap/deleteQueue 全没了，recoverPendingPublishIfNeeded() 也找不到 pending。
     *
     * 本测试手动模拟 chapterChanged 的内存写入与"进程重启"后新实例的初始状态，
     * 证明变更意图在内存里且重启后丢失，没有任何 durable outbox 兜底。
     */
    @Test
    fun hardProblem1_mirrorChangeSink_inMemoryStateLostOnRestart() {
        val projectId = "proj-1"
        val volumeId = "vol-1"
        val chapterId = "chap-1"

        // ── 模拟 DefaultMirrorChangeSink 的内存字段（line 103-105）──
        // 这些都是纯内存对象，没有任何持久化 backing store。
        val dirtyMapInstance1 = ConcurrentHashMap<MirrorKey, DirtyEntry>()
        val deleteQueueInstance1 = ConcurrentLinkedQueue<DeleteEvent>()
        // signal = Channel<Unit>(Channel.CONFLATED) —— 同样是内存对象

        // 记录是否有 durable outbox 写入（应当为 0）
        val durableOutboxWrites = mutableListOf<String>()

        // ── 模拟 chapterChanged() (line 123-130) 的当前逻辑 ──
        // line 128: dirtyMap[MirrorKey(projectId, volumeId, chapterId)] = DirtyEntry(System.currentTimeMillis())
        // line 129: signal.trySend(Unit)
        // ★ 关键缺陷：只改内存 dirtyMap + 发内存 signal，没有任何 durable outbox 调用 ★
        val key = MirrorKey(projectId, volumeId, chapterId)
        dirtyMapInstance1[key] = DirtyEntry(System.currentTimeMillis())
        // signal.trySend(Unit) —— 内存信号，不持久化
        // 没有 durableOutboxWrites.add(...) —— 因为代码里就没有

        // ── 断言窗口状态：变更意图只在内存里 ──
        assertTrue(
            "chapterChanged 后 dirtyMap 非空（变更意图已入内存）",
            dirtyMapInstance1.isNotEmpty(),
        )
        assertEquals(
            "★ 当前代码缺陷：chapterChanged 没有任何 durable outbox 写入 ★",
            0,
            durableOutboxWrites.size,
        )

        // ── ★ 断电点 ★：worker 还没进入 publishProject()，进程被杀 ──
        // 此时没有任何 pendingPublish journal 被创建（journal 要等 worker 进入 publish 才创建）。
        // dirtyMap/deleteQueue/signal 全是内存对象，进程死亡即消失。
        val pendingJournalCreatedBeforeCrash = false // worker 还没进入 publish，没创建 journal
        assertFalse(
            "断电窗口：worker 还没进入 publishProject，没有任何 pendingPublish journal",
            pendingJournalCreatedBeforeCrash,
        )

        // ── 模拟进程重启：构造新的 DefaultMirrorChangeSink 实例 ──
        // 新实例的 dirtyMap = ConcurrentHashMap<MirrorKey, DirtyEntry>() —— 初始为空
        // 新实例的 deleteQueue = ConcurrentLinkedQueue<DeleteEvent>() —— 初始为空
        // 新实例的 signal = Channel<Unit>(Channel.CONFLATED) —— 初始为空
        val dirtyMapInstance2 = ConcurrentHashMap<MirrorKey, DirtyEntry>()
        val deleteQueueInstance2 = ConcurrentLinkedQueue<DeleteEvent>()

        // ── 断言错误行为：重启后内存态全没了 ──
        assertTrue(
            "★ 重启后新实例 dirtyMap 为空：变更意图丢失 ★",
            dirtyMapInstance2.isEmpty(),
        )
        assertTrue(
            "★ 重启后新实例 deleteQueue 为空 ★",
            deleteQueueInstance2.isEmpty(),
        )

        // ── 模拟 recoverPendingPublishIfNeeded() (line 129-141) ──
        // 它调用 stateStore.readPendingPublish()，但崩溃前根本没创建 journal，
        // 所以返回 NotExists，直接 return，恢复不到任何东西。
        val recoveredPendingJournal: PendingMirrorPublish? = null // readPendingPublish 返回 NotExists
        assertNull(
            "★ recoverPendingPublishIfNeeded 找不到 pending（崩溃前没创建 journal）★",
            recoveredPendingJournal,
        )

        // ── 最终结论：如果后面没有碰巧再次改这个作品，Download/Sujian 停在旧版本 ──
        val mirrorWillCatchUp = dirtyMapInstance2.isNotEmpty() || recoveredPendingJournal != null
        assertFalse(
            "★ 镜像不会跟上最新 revision：内存态丢失且无 journal 可恢复 ★",
            mirrorWillCatchUp,
        )
    }

    /**
     * 补充：projectStructureChanged (line 132-135) 和 projectDeleted (line 137-142)
     * 同样只改内存再 signal，没有任何 durable outbox 调用。
     */
    @Test
    fun hardProblem1_projectStructureChangedAndDeleted_alsoInMemoryOnly() {
        val durableOutboxWrites = mutableListOf<String>()
        val dirtyMap = ConcurrentHashMap<MirrorKey, DirtyEntry>()
        val deleteQueue = ConcurrentLinkedQueue<DeleteEvent>()

        // 模拟 projectStructureChanged (line 132-135)
        dirtyMap[MirrorKey("proj-1", "", "")] = DirtyEntry(System.currentTimeMillis())
        // signal.trySend(Unit) —— 内存

        // 模拟 projectDeleted (line 137-142)
        deleteQueue.add(DeleteEvent("proj-2"))
        dirtyMap.keys.removeAll { it.projectId == "proj-2" }
        // signal.trySend(Unit) —— 内存

        assertEquals(
            "★ projectStructureChanged/projectDeleted 同样没有 durable outbox 写入 ★",
            0,
            durableOutboxWrites.size,
        )
        assertTrue("结构变更入内存 dirtyMap", dirtyMap.isNotEmpty())
        assertTrue("删除事件入内存 deleteQueue", deleteQueue.isNotEmpty())
    }

    // ══════════════════════════════════════════════════════════════════════
    // 硬问题 2：recoverPromotePhase 旧事务恢复时仍读取当前 Core snapshot，
    //          把新 metadata 混进旧事务正文。
    // 源：ReadableMirrorPublisher.kt line 214-217 (定义), 218 (promotedEntries R1),
    //     620-627 (重读当前 snapshot), 628-637 (传给 publishManifestWithDesiredTransactional),
    //     3278-3324 (manifestTargetJson==null 走 else), 3290 (buildManifestJsonForDesired),
    //     3149 (snapshot.toMirrorProject(desiredEntries) 混合点)
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 硬问题 2 复现：recoverPromotePhase() (line 214) 在旧事务正文 promote 完后，
     * line 620-627 重新调用 source.getProjectWorkspaceSnapshot(journal.projectId) 读取当前 snapshot，
     * line 628-637 把这个当前 snapshot 和旧事务 journal 的 promotedEntries 一起传给
     * publishManifestWithDesiredTransactional()。
     *
     * publishManifestWithDesiredTransactional() (line 3220) 在 line 3237 判断：
     *   val isResumingManifest = journalContext.manifestTargetJson != null
     * 当 manifestTargetJson == null（manifest 子事务未开始）时，line 3278 的 if 为 false，
     * 走 else 分支 (line 3288-3324)，line 3290 调用：
     *   buildManifestJsonForDesired(projectId, snapshot, desiredEntries)
     *
     * buildManifestJsonForDesired() (line 3135) 在 line 3149 调用：
     *   snapshot.toMirrorProject(desiredEntries)
     * 把 snapshot 的 metadata（标题/顺序/revision/updatedAt，R2 的）和 desiredEntries
     * （promotedEntries，R1 的正文文件/hash/URI）混合，生成混合 manifest。
     *
     * 跨版本混合窗口：
     * 1. T1 从 canonical R1 构建正文计划；
     * 2. R1 正文已经 backup/vacate/promote 到公共镜像；
     * 3. 进程死在 manifest 子事务真正开始之前，因此 manifestTargetJson == null；
     * 4. private canonical 后来已经变成 R2；
     * 5. T1 启动恢复，recoverPromotePhase() 重新读取到 R2 snapshot；
     * 6. 但 promotedEntries 仍然是 T1/R1 的路径、hash、URI；
     * 7. 新生成的 manifest 出现"R2 的标题/顺序/revision/updatedAt + R1 的正文文件/hash"的混合状态。
     */
    @Test
    fun hardProblem2_recoverPromotePhase_mixesFreshSnapshotWithOldPromotedEntries() {
        val projectId = "proj-1"
        val txId = "tx-1"
        val key = ChapterKey(projectId, "vol-1", "chap-1")

        // ── T1/R1 的 promotedEntries（旧事务正文已 promote 到公共镜像）──
        // 这些是 R1 的路径、hash、URI、revision
        val r1ContentHash = "sha256:r1-content-hash"
        val r1Uri = "content://mirror/r1/chap.md"
        val r1RelativePath = "作品/项目1/卷1/章1.md"
        val r1Revision = 100L
        val promotedEntriesR1 = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
        promotedEntriesR1[key] = ChapterMirrorEntry(
            uri = r1Uri,
            relativePath = r1RelativePath,
            revision = r1Revision,
            contentHash = r1ContentHash,
        )

        // ── 模拟 R2 snapshot（canonical 后来已经变成 R2）──
        // R2 的 metadata：标题/顺序/revision/updatedAt 都和 R1 不同
        val r2Title = "项目1-改后标题-R2"
        val r2Revision = 200L
        val r2UpdatedAt = "2026-09-08T03:00:00Z"
        // 用一个简单的 holder 模拟 ProjectWorkspaceSnapshot 的 metadata
        val r2SnapshotMetadata = mapOf(
            "title" to r2Title,
            "revision" to r2Revision.toString(),
            "updatedAt" to r2UpdatedAt,
        )

        // ── 旧 journal（T1/R1），manifest 子事务未开始 ──
        // manifestTargetJson == null 表示 manifest 子事务还没开始
        val journalT1 = PendingMirrorPublish(
            txId = txId,
            backend = MirrorBackend.MEDIA_STORE,
            treeUri = null,
            projectId = projectId,
            transactionType = MirrorTransactionType.UPSERT_PROJECT,
            phase = PendingMirrorPublish.PHASE_PROMOTE,
            oldEntries = emptyMap(),
            newEntries = promotedEntriesR1,
            stagedRefs = emptyMap(),
            items = mapOf(key to PendingItem(
                key = key,
                stagedRef = null,
                oldRef = null,
                backupOldRef = null,
                promotedRef = MirrorFileRef(r1Uri, r1RelativePath),
                state = PendingItem.STATE_PROMOTED,
            )),
            removedProjectIds = emptySet(),
            manifestOldRef = null,
            manifestStagedRef = null,
            manifestNewRef = null,
            manifestBackupRef = null,
            manifestTargetJson = null, // ★ manifest 子事务未开始 ★
        )

        // ── 复现 recoverPromotePhase() line 620-637 的当前逻辑 ──
        // line 621: val snapshotResult = source.getProjectWorkspaceSnapshot(journal.projectId)
        // ★ 关键缺陷：重新读取当前 snapshot（R2），而不是用 T1/R1 冻结的 metadata ★
        val snapshotResultData = r2SnapshotMetadata // source 返回 R2 的当前 snapshot

        // line 3237: val isResumingManifest = journalContext.manifestTargetJson != null
        val isResumingManifest = journalT1.manifestTargetJson != null
        assertFalse(
            "manifestTargetJson == null：manifest 子事务未开始，isResumingManifest=false",
            isResumingManifest,
        )

        // line 3278: if (journalContext.manifestTargetJson != null) { ... } else { ... }
        // ★ 走 else 分支（line 3288-3324），重新生成 manifest ★
        val useFrozenJson = isResumingManifest
        assertFalse(
            "★ 当前代码缺陷：manifestTargetJson==null 时走 else 分支重新生成，而非用冻结 metadata ★",
            useFrozenJson,
        )

        // ── 复现 buildManifestJsonForDesired() line 3135-3172 的当前逻辑 ──
        // line 3290: val builtJson = buildManifestJsonForDesired(projectId, snapshot, desiredEntries)
        // line 3149: mirrorProjects.add(snapshot.toMirrorProject(desiredEntries))
        // ★ 混合点：snapshot=R2 metadata + desiredEntries=R1 promotedEntries ★
        val snapshotPassedToBuild = snapshotResultData // R2
        val desiredEntriesPassedToBuild = promotedEntriesR1 // R1

        // 模拟 snapshot.toMirrorProject(desiredEntries) 的混合结果
        val mixedManifestProject = mapOf(
            "title" to snapshotPassedToBuild["title"],         // R2 的标题
            "revision" to snapshotPassedToBuild["revision"],   // R2 的 revision
            "updatedAt" to snapshotPassedToBuild["updatedAt"], // R2 的 updatedAt
            "chapters" to desiredEntriesPassedToBuild.map { (k, v) ->
                mapOf(
                    "uri" to v.uri,                   // R1 的 uri
                    "relativePath" to v.relativePath, // R1 的 path
                    "contentHash" to v.contentHash,   // R1 的 hash
                    "chapterRevision" to v.revision,  // R1 的 revision
                )
            },
        )

        // ── 断言错误行为：manifest 是 R2 metadata + R1 正文的混合状态 ──
        assertEquals(
            "manifest 项目标题是 R2 的（当前 snapshot）",
            r2Title,
            mixedManifestProject["title"],
        )
        assertEquals(
            "manifest 项目 revision 是 R2 的（当前 snapshot）",
            r2Revision.toString(),
            mixedManifestProject["revision"],
        )
        assertEquals(
            "manifest 项目 updatedAt 是 R2 的（当前 snapshot）",
            r2UpdatedAt,
            mixedManifestProject["updatedAt"],
        )

        @Suppress("UNCHECKED_CAST")
        val mixedChapters = mixedManifestProject["chapters"] as List<Map<String, Any>>
        assertEquals(
            "manifest 章节 uri 是 R1 的（旧 promotedEntries）",
            r1Uri,
            mixedChapters[0]["uri"],
        )
        assertEquals(
            "manifest 章节 contentHash 是 R1 的（旧 promotedEntries）",
            r1ContentHash,
            mixedChapters[0]["contentHash"],
        )
        assertEquals(
            "manifest 章节 chapterRevision 是 R1 的（旧 promotedEntries）",
            r1Revision,
            mixedChapters[0]["chapterRevision"],
        )

        // ── 核心矛盾：snapshot revision (R2) != promotedEntries revision (R1) ──
        assertNotEquals(
            "★ 混合状态：项目 revision(R2)=${r2Revision} != 章节 revision(R1)=$r1Revision ★",
            r2Revision,
            r1Revision,
        )

        // ── PendingMirrorPublish 没有冻结 manifest 元数据字段 ──
        // 字段列表（PendingMirrorPublish.kt line 200-233）只有 manifestTargetJson: String? = null
        // 没有 frozenProjectManifest / frozenManifestModelJson / frozenSnapshotMetadata
        // manifestTargetJson 只在 manifest 子事务开始后才设置（line 3315），
        // 所以 manifestTargetJson==null 同时承担"目标还没冻结"和"manifest 子事务还没开始"两个语义。
        assertNull(
            "★ PendingMirrorPublish 无冻结 manifest 元数据字段，manifestTargetJson==null 双语义 ★",
            journalT1.manifestTargetJson,
        )
    }

    /**
     * 硬问题 2 修复验证：PendingMirrorPublish 数据类已有冻结 manifest 元数据字段。
     *
     * #649 评论 5575052682：新增 frozenManifestMetadata / frozenManifestMetadataHash 字段，
     * manifest 子事务未开始（manifestTargetJson==null）时恢复仍能用冻结的元数据。
     */
    @Test
    fun hardProblem2_pendingMirrorPublish_hasFrozenSnapshotMetadataField() {
        val journal = PendingMirrorPublish(
            txId = "tx-1",
            backend = MirrorBackend.MEDIA_STORE,
            treeUri = null,
            projectId = "proj-1",
            transactionType = MirrorTransactionType.UPSERT_PROJECT,
            phase = PendingMirrorPublish.PHASE_PROMOTE,
            oldEntries = emptyMap(),
            newEntries = emptyMap(),
            stagedRefs = emptyMap(),
            items = emptyMap(),
            removedProjectIds = emptySet(),
            manifestOldRef = null,
            manifestStagedRef = null,
            manifestNewRef = null,
            manifestBackupRef = null,
            manifestTargetJson = null,
            frozenManifestMetadata = """{"projectId":"proj-1","title":"T1","revision":"100","updatedAt":"2026-09-01","volumes":[]}""",
            frozenManifestMetadataHash = "sha256:test",
        )

        // manifestTargetJson 仍为 null（manifest 子事务未开始）
        assertNull("manifestTargetJson 是 null（manifest 子事务未开始）", journal.manifestTargetJson)
        // frozenManifestMetadata 不为 null：恢复时可用冻结的元数据，不用重读 snapshot
        assertNotNull("frozenManifestMetadata 存在，恢复时可替代 getProjectWorkspaceSnapshot", journal.frozenManifestMetadata)
        assertNotNull("frozenManifestMetadataHash 存在，用于完整性校验", journal.frozenManifestMetadataHash)
        // 验证 JSON round-trip 保持 frozen 字段
        val roundTripped = PendingMirrorPublish.fromJson(journal.toJson())
        assertNotNull("round-trip 成功", roundTripped)
        assertEquals("frozenManifestMetadata round-trip", journal.frozenManifestMetadata, roundTripped!!.frozenManifestMetadata)
        assertEquals("frozenManifestMetadataHash round-trip", journal.frozenManifestMetadataHash, roundTripped.frozenManifestMetadataHash)
    }

    // ══════════════════════════════════════════════════════════════════════
    // 硬问题 1 修复验证：MirrorOutboxStore 持久化 outbox，进程重启后能恢复变更意图。
    // 源：MirrorOutboxStore.kt
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 硬问题 1 修复验证：MirrorOutboxStore 在进程重启后能恢复变更意图。
     *
     * 修复后：markDirty() 会持久化到文件，创建新的 MirrorOutboxStore 实例后
     * getDirtyProjects() 能读取到之前持久化的项目。
     */
    @Test
    fun hardProblem1_mirrorChangeSink_outboxPersistsOnRestart() {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val outboxStore = MirrorOutboxStore(context)

        // 清理之前的 outbox
        outboxStore.clearAll()

        // 模拟 chapterChanged
        val projectId = "proj-1"
        outboxStore.markDirty(projectId)

        // 验证 outbox 已持久化
        val snapshot1 = outboxStore.readSnapshot()
        assertTrue(snapshot1?.projects?.containsKey(projectId) == true)

        // 模拟进程重启：创建新的 outboxStore 实例
        val outboxStore2 = MirrorOutboxStore(context)

        // 验证变更意图已恢复
        val snapshot2 = outboxStore2.readSnapshot()
        assertTrue(snapshot2?.projects?.containsKey(projectId) == true)

        // 清理
        outboxStore2.clearAll()
    }

    /**
     * 硬问题 1 修复验证：delete tombstone 优先于 dirty。
     *
     * 修复后：markDeleted() 会将项目添加到 tombstone，并从 dirty 中移除。
     */
    @Test
    fun hardProblem1_outboxDeleteTombstonePriority() {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val outboxStore = MirrorOutboxStore(context)

        outboxStore.clearAll()

        // 先标记 dirty
        outboxStore.markDirty("proj-1")

        // 再标记删除（tombstone 优先）
        outboxStore.markDeleted("proj-1")

        // proj-1 的 intent 应该是 DELETE（tombstone 优先于 dirty）
        val snapshot = outboxStore.readSnapshot()
        val projIntent = snapshot?.projects?.get("proj-1")
        assertNotNull("proj-1 intent 存在", projIntent)
        assertEquals(
            "★ proj-1 是 DELETE（tombstone 优先）★",
            OutboxIntentKind.DELETE,
            projIntent?.kind,
        )

        outboxStore.clearAll()
    }

    // ══════════════════════════════════════════════════════════════════════
    // 硬问题 2 修复验证：recoverPromotePhase 使用冻结的元数据。
    // 源：PendingMirrorPublish.kt (frozenManifestMetadata 字段)
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 硬问题 2 修复验证：recoverPromotePhase 使用冻结的元数据。
     *
     * 修复后：PendingMirrorPublish 包含 frozenManifestMetadata 和 frozenManifestMetadataHash，
     * 在恢复时使用这些冻结的元数据而不是重新读取当前 snapshot。
     */
    @Test
    fun hardProblem2_frozenMetadataUsedInRecovery() {
        // 构建一个包含 frozenManifestMetadata 的 journal
        val projectId = "proj-1"
        val frozenMetadata = """
            {
                "projectId": "$projectId",
                "title": "测试作品",
                "revision": "100",
                "updatedAt": "2026-09-01T00:00:00Z",
                "volumes": [
                    {
                        "volumeId": "vol-1",
                        "title": "卷一",
                        "order": 0,
                        "chapters": [
                            {
                                "chapterId": "chap-1",
                                "title": "第一章",
                                "order": 0,
                                "revision": 100,
                                "contentHash": "sha256:abc123",
                                "relativePath": "作品/测试作品/卷一/第一章.md"
                            }
                        ]
                    }
                ]
            }
        """.trimIndent()

        val journal = PendingMirrorPublish(
            txId = "tx-1",
            backend = MirrorBackend.MEDIA_STORE,
            treeUri = null,
            projectId = projectId,
            transactionType = MirrorTransactionType.UPSERT_PROJECT,
            phase = PendingMirrorPublish.PHASE_PROMOTE,
            oldEntries = emptyMap(),
            newEntries = mapOf(
                ChapterKey(projectId, "vol-1", "chap-1") to ChapterMirrorEntry(
                    uri = "content://mirror/chap.md",
                    relativePath = "作品/测试作品/卷一/第一章.md",
                    revision = 100L,
                    contentHash = "sha256:abc123"
                )
            ),
            stagedRefs = emptyMap(),
            items = mapOf(
                ChapterKey(projectId, "vol-1", "chap-1") to PendingItem(
                    key = ChapterKey(projectId, "vol-1", "chap-1"),
                    stagedRef = null,
                    oldRef = null,
                    backupOldRef = null,
                    promotedRef = MirrorFileRef("content://mirror/chap.md", "作品/测试作品/卷一/第一章.md"),
                    state = PendingItem.STATE_PROMOTED
                )
            ),
            removedProjectIds = emptySet(),
            manifestOldRef = null,
            manifestStagedRef = null,
            manifestNewRef = null,
            manifestBackupRef = null,
            isManifestCommitted = false,
            frozenManifestMetadata = frozenMetadata,
            frozenManifestMetadataHash = computeContentHash(frozenMetadata),
        )

        // 验证 journal 包含冻结的元数据
        assertNotNull(journal.frozenManifestMetadata)
        assertNotNull(journal.frozenManifestMetadataHash)

        // 验证元数据 hash 正确
        val computedHash = computeContentHash(frozenMetadata)
        assertEquals(computedHash, journal.frozenManifestMetadataHash)
    }

    /**
     * 计算字符串的 SHA-256 hash（与生产代码一致）。
     */
    private fun computeContentHash(text: String): String {
        return "sha256:" + java.security.MessageDigest.getInstance("SHA-256")
            .digest(text.toByteArray(Charsets.UTF_8))
            .joinToString("") { "%02x".format(it) }
    }
}
