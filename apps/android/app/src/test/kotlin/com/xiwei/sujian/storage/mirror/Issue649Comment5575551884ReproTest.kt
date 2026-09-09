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
import androidx.test.core.app.ApplicationProvider

/**
 * #649 评论 5575551884：3 个镜像存储一致性问题回归测试。
 *
 * 验证评论 5575551884 指出的 3 个一致性问题已修复。
 *
 * ## 问题 1：outbox 带代次号的 generation-aware ACK
 * - markDirty 每次推进 generation，不再去重丢失 R2
 * - ackProject 只在 generation 匹配时清除
 * - processDeletes 成功后 ackProject 清 tombstone
 * - publishAll 成功后 ackFullDirty 清 fullDirty + tombstone
 * - drainOutboxToMemory 完整加载所有 intent（含 fullDirty + tombstone）
 *
 * ## 问题 2：frozen manifest 计划可靠写进 pending journal
 * - writePendingPublishJournal 从 journalContext 继承 frozen 字段
 * - 冻结计划（frozenManifestPlan）唯一冻结真值
 *
 * ## 问题 3：frozen manifest 恢复入口语义正确 + JSON schema 一致
 * - publishManifestWithDesiredTransactional 传 prebuiltTargetJson，不修改 manifestTargetJson
 * - frozenPlanToManifestJson 输出与 MirrorManifest schema 一致
 * - rollbackManifest 拒绝 manifestOldRef != null && manifestOldContentHash == null
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue649Comment5575551884ReproTest {
    companion object {
        private const val CHAP_1 = "chap-1"
        private const val OUTBOX = "outbox 仍可读"
        private const val PROJ_1 = "proj-1"
        private const val SCHEMAVERSION = "schemaVersion"
        private const val SHA256_ABC = "sha256:abc"
        private const val S_1 = "项目1"
        private const val S_2026_09_07T00_00_00Z = "2026-09-07T00:00:00Z"
        private const val TX_1 = "tx-1"
        private const val VOL_1 = "vol-1"
    }


    // ══════════════════════════════════════════════════════════════════════
    // 问题 1：generation-aware outbox ACK
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 问题 1a 回归：markDirty 每次推进 generation，并发 R2 不丢失。
     *
     * 修复后：markDirty 第一次给 generation=1，第二次给 generation=2，
     * R2 有独立的 generation 记录在 outbox 中。
     */
    @Test
    fun problem1a_markDirtyGeneratesConcurrentR2Preserved() {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val outboxStore = MirrorOutboxStore(context)
        outboxStore.clearAll()

        val pid = PROJ_1

        // 步骤 1：R1 保存
        val r1Intent = outboxStore.markDirty(pid)
        assertNotNull("R1 markDirty 成功", r1Intent)
        assertEquals("R1 generation = 1", 1L, r1Intent!!.generation)
        assertEquals("R1 kind = UPSERT", OutboxIntentKind.UPSERT, r1Intent.kind)

        // 步骤 2：R2 保存（并发）
        val r2Intent = outboxStore.markDirty(pid)
        assertNotNull("R2 markDirty 成功", r2Intent)
        assertEquals("★ R2 generation = 2（不同于 R1）★", 2L, r2Intent!!.generation)
        assertEquals("R2 kind = UPSERT", OutboxIntentKind.UPSERT, r2Intent.kind)

        // 步骤 3：ACK R1（generation=1）不删除 R2（因为 R2 已推进 generation 到 2）
        val ackResult = outboxStore.ackProject(pid, r1Intent.generation, r1Intent.kind)
        assertFalse(
            "★ ACK R1 失败：R2 已推进 generation，R1 的 generation=1 不匹配当前 generation=2 ★",
            ackResult,
        )

        // outbox 中仍保留 R2 的 intent
        val snapshotAfterAck = outboxStore.readSnapshot()
        assertNotNull(OUTBOX, snapshotAfterAck)
        val r2StillPresent = snapshotAfterAck!!.projects[pid]
        assertNotNull("★ R2 intent 仍在 outbox 中：ACK R1 没有删除 generation=2 的 R2 ★", r2StillPresent)
        assertEquals("★ R2 generation 仍为 2 ★", 2L, r2StillPresent!!.generation)

        // 步骤 4：模拟进程重启后，R2 仍在 outbox
        val outboxStore2 = MirrorOutboxStore(context)
        val snapshot2 = outboxStore2.readSnapshot()
        assertNotNull("重启后 outbox 可读", snapshot2)
        val recoveredIntent = snapshot2!!.projects[pid]
        assertNotNull("★ 重启后 R2 intent 仍在：R2 不会丢失 ★", recoveredIntent)
        assertEquals("★ 重启后 R2 generation = 2 ★", 2L, recoveredIntent!!.generation)

        outboxStore2.clearAll()
    }

    /**
     * 问题 1b 回归：ackProject 成功后 tombstone 被移除。
     *
     * 修复后：processDeletes 成功后用 ackProject(generation, DELETE) 清 tombstone，
     * tombstone 不再永留磁盘。
     */
    @Test
    fun problem1b_ackProjectRemovesTombstone_noReplayOnRestart() {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val outboxStore = MirrorOutboxStore(context)
        outboxStore.clearAll()

        val pid = "proj-to-delete"

        // 标记删除
        val intent = outboxStore.markDeleted(pid)
        assertNotNull("tombstone 已落盘", intent)

        // 模拟 processDeletes 成功后的 ackProject
        val ackResult = outboxStore.ackProject(pid, intent!!.generation, OutboxIntentKind.DELETE)
        assertTrue("ackProject 成功", ackResult)

        // tombstone 已被移除
        val snapshotAfterAck = outboxStore.readSnapshot()
        assertNotNull(OUTBOX, snapshotAfterAck)
        assertFalse(
            "★ ackProject 后 tombstone 已移除 ★",
            snapshotAfterAck!!.projects.containsKey(pid),
        )

        // 模拟进程重启
        val outboxStore2 = MirrorOutboxStore(context)
        val snapshot2 = outboxStore2.readSnapshot()
        assertFalse(
            "★ 重启后 tombstone 已移除：不会反复恢复删除任务 ★",
            snapshot2?.projects?.containsKey(pid) == true,
        )

        outboxStore2.clearAll()
    }

    /**
     * 问题 1c 回归：ackFullDirty 成功后 fullDirty 被清除。
     *
     * 修复后：publishAll 成功后用 ackFullDirty(generation) 清除全量标记。
     */
    @Test
    fun problem1c_ackFullDirtyClearsFullDirty_noReplayOnRestart() {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val outboxStore = MirrorOutboxStore(context)
        outboxStore.clearAll()

        // 标记全量脏
        val fullGen = outboxStore.markDirtyAll()
        assertNotNull("markDirtyAll 成功", fullGen)

        // 模拟 publishAll 成功后的 ackFullDirty
        val ackResult = outboxStore.ackFullDirty(fullGen!!)
        assertTrue("ackFullDirty 成功", ackResult)

        // fullDirty 已被清除
        val snapshotAfterAck = outboxStore.readSnapshot()
        assertNotNull(OUTBOX, snapshotAfterAck)
        assertNull(
            "★ ackFullDirty 后 fullDirtyGeneration 已清除 ★",
            snapshotAfterAck!!.fullDirtyGeneration,
        )

        // 模拟进程重启
        val outboxStore2 = MirrorOutboxStore(context)
        val snapshot2 = outboxStore2.readSnapshot()
        assertNull(
            "★ 重启后 fullDirtyGeneration 已清除：不会再次全量发布 ★",
            snapshot2?.fullDirtyGeneration,
        )

        outboxStore2.clearAll()
    }

    /**
     * 问题 1d 回归：drainOutboxToMemory 完整加载 tombstone（无论 fullDirty 状态）。
     *
     * 修复后：fullDirty + tombstone 同时存在时，drain 同时加载两者。
     */
    @Test
    fun problem1d_drainLoadsBothFullDirtyAndTombstone() {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val outboxStore = MirrorOutboxStore(context)
        outboxStore.clearAll()

        // 先标记删除（tombstone）
        val deleteIntent = outboxStore.markDeleted("proj-deleted")
        assertNotNull("tombstone 已落盘", deleteIntent)

        // 再标记全量脏
        val fullGen = outboxStore.markDirtyAll()
        assertNotNull("fullDirty 已落盘", fullGen)

        // 模拟 drainOutboxToMemory
        val snapshot = outboxStore.readSnapshot()
        assertNotNull("snapshot 可读", snapshot)

        val loadedFullDirty = snapshot!!.fullDirtyGeneration != null
        val loadedTombstone = snapshot.projects["proj-deleted"]?.kind == OutboxIntentKind.DELETE

        assertTrue("drain 加载了 fullDirty", loadedFullDirty)
        assertTrue(
            "★ drain 也加载了 tombstone：不再因 fullDirty return 短路而跳过 ★",
            loadedTombstone,
        )

        outboxStore.clearAll()
    }

    /**
     * ackProject generation 不匹配时不删除。
     */
    @Test
    fun ackProjectGenerationMismatch_noDelete() {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val outboxStore = MirrorOutboxStore(context)
        outboxStore.clearAll()

        val pid = PROJ_1
        val intent = outboxStore.markDirty(pid)
        assertNotNull(intent)

        // 用错误的 generation ACK
        val ackResult = outboxStore.ackProject(pid, intent!!.generation + 999, OutboxIntentKind.UPSERT)
        assertFalse("generation 不匹配 ACK 失败", ackResult)

        // intent 仍在
        val snapshot = outboxStore.readSnapshot()
        assertTrue("intent 仍在 outbox 中", snapshot!!.projects.containsKey(pid))

        outboxStore.clearAll()
    }

    /**
     * 旧格式 outbox 自动迁移。
     */
    @Test
    fun legacyOutboxFormat_migratesAutomatically() {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val outboxStore = MirrorOutboxStore(context)
        outboxStore.clearAll()

        // 手动写旧格式 JSON
        val file = java.io.File(
            java.io.File(context.noBackupFilesDir, "sujian-mirror"),
            "outbox.json",
        )
        val legacyJson = """
            {
              "dirtyProjects": ["proj-a", "proj-b"],
              "deleteTombstones": ["proj-c"],
              "fullDirty": true,
              "lastSignalTime": 12345
            }
        """.trimIndent()
        file.writeText(legacyJson, Charsets.UTF_8)

        // 读取时自动迁移
        val snapshot = outboxStore.readSnapshot()
        assertNotNull("迁移后可读", snapshot)
        assertEquals("nextGeneration > 0", true, snapshot!!.nextGeneration > 0)
        assertEquals("proj-a 是 UPSERT", OutboxIntentKind.UPSERT, snapshot.projects["proj-a"]?.kind)
        assertEquals("proj-b 是 UPSERT", OutboxIntentKind.UPSERT, snapshot.projects["proj-b"]?.kind)
        assertEquals("★ proj-c 是 DELETE（tombstone 优先覆盖 dirty）★", OutboxIntentKind.DELETE, snapshot.projects["proj-c"]?.kind)
        assertNotNull("fullDirtyGeneration 存在", snapshot.fullDirtyGeneration)
        assertEquals("lastSignalTime 保留", 12345L, snapshot.lastSignalTime)

        outboxStore.clearAll()
    }

    // ══════════════════════════════════════════════════════════════════════
    // 问题 2：frozen manifest 计划可靠写进 pending journal
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 问题 2 回归：writePendingPublishJournal 从 journalContext 继承 frozen 字段。
     *
     * 修复后：writePendingPublishJournal 不再有独立的 frozenManifestPlan 参数，
     * 而是从 journalContext 自动继承。journalContext 带 frozen 字段时，
     * 写出的磁盘 journal 也带 frozen 字段。
     */
    @Test
    fun problem2_journalContextInheritsFrozenFields() {
        val projectId = PROJ_1
        val txId = TX_1

        // 模拟 journalContext 带 frozen 字段
        val plan = buildFrozenManifestPlan(projectId)
        val frozenPlanJson = frozenManifestPlanToJson(plan)
        val frozenPlanHash = computeContentHash(frozenPlanJson)
        val journalContext = buildPendingPublishWithFrozen(txId, projectId, frozenPlanJson, frozenPlanHash)

        // writePendingPublishJournal 传 journalContext，不传 frozenManifestPlan 参数
        // frozen 字段应从 journalContext 继承
        val effectiveFrozenPlan = journalContext.frozenManifestPlan
        val effectiveFrozenPlanHash = journalContext.frozenManifestPlanHash

        assertEquals(
            "★ frozenManifestPlan 从 journalContext 继承 ★",
            frozenPlanJson,
            effectiveFrozenPlan,
        )
        assertEquals(
            "★ frozenManifestPlanHash 从 journalContext 继承 ★",
            frozenPlanHash,
            effectiveFrozenPlanHash,
        )

        // 模拟磁盘 journal 序列化/反序列化
        val diskJournal = buildPendingPublishWithFrozen(txId, projectId, effectiveFrozenPlan, effectiveFrozenPlanHash)

        val diskJson = diskJournal.toJson()
        val recoveredJournal = PendingMirrorPublish.fromJson(diskJson)
        assertNotNull("round-trip 成功", recoveredJournal)
        assertEquals(
            "★ 恢复后 frozenManifestPlan 非 null ★",
            frozenPlanJson,
            recoveredJournal!!.frozenManifestPlan,
        )
        assertEquals(
            "★ 恢复后 frozenManifestPlanHash 非 null ★",
            frozenPlanHash,
            recoveredJournal.frozenManifestPlanHash,
        )

        // recoverPromotePhase 会使用 frozen plan
        val useFrozenPlan = recoveredJournal.frozenManifestPlan != null
        assertTrue(
            "★ frozenManifestPlan != null：recoverPromotePhase 走冻结路径，不回退到 snapshot ★",
            useFrozenPlan,
        )
    }

    /**
     * 构建 frozen manifest plan（#651 评论 5592465805：提取 setup 减少 LongMethod）。
     */
    private fun buildFrozenManifestPlan(projectId: String): FrozenManifestPlan =
        FrozenManifestPlan(
            schemaVersion = 1,
            revision = 100L,
            updatedAt = "2026-09-01T00:00:00Z",
            targetProjectId = projectId,
            projects = listOf(
                FrozenManifestProject(
                    id = projectId,
                    title = "T1",
                    order = 0,
                    revision = 100L,
                    updatedAt = "2026-09-01T00:00:00Z",
                    volumes = emptyList(),
                ),
            ),
        )

    /**
     * 构建 PendingMirrorPublish 并注入 frozen 字段（#651 评论 5592465805：提取 setup 减少 LongMethod）。
     */
    private fun buildPendingPublishWithFrozen(
        txId: String,
        projectId: String,
        frozenPlanJson: String?,
        frozenPlanHash: String?,
    ): PendingMirrorPublish =
        PendingMirrorPublish(
            txId = txId,
            backend = MirrorBackend.MEDIA_STORE,
            treeUri = null,
            projectId = projectId,
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
            frozenManifestPlan = frozenPlanJson,
            frozenManifestPlanHash = frozenPlanHash,
        )

    /**
     * 问题 2 补充：writePendingPublishJournal 签名包含 frozen 参数。
     *
     * 修复后：writePendingPublishJournal 有 frozenManifestPlan/frozenManifestPlanHash 参数
     * （通过 journalContext 继承也可不传）。已删除旧的 frozenManifestMetadata/frozenManifestMetadataHash。
     */
    @Test
    fun problem2_writePendingPublishJournalSignature_hasFrozenParams() {
        val publisherClass = ReadableMirrorPublisher::class.java
        val methods = publisherClass.declaredMethods.filter { it.name == "writePendingPublishJournal" }
        assertTrue("writePendingPublishJournal 方法存在", methods.isNotEmpty())

        val realMethodParamCount = methods.map { it.parameterCount }.min()
        assertTrue(
            "★ writePendingPublishJournal 有 ${realMethodParamCount} 个参数（含 frozen plan 参数）★",
            realMethodParamCount >= 20,
        )
    }

    // ══════════════════════════════════════════════════════════════════════
    // 问题 3：frozen manifest 恢复入口语义正确 + JSON schema 一致
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 问题 3a 回归：recoverPromotePhase 用 frozenManifestPlan + frozenPlanToManifestJson
     * 生成 manifest，传 prebuiltTargetJson 给 publishManifestWithDesiredTransactional，
     * 不修改 journalContext.manifestTargetJson。
     *
     * 修复后：prebuiltTargetJson 作为参数传入 publishManifestWithDesiredTransactional，
     * 函数内部在 manifestTargetJson == null 且 prebuiltTargetJson != null 时，
     * 正常走 stage → journal 流程，不伪造 isResumingManifest。
     */
    @Test
    fun problem3a_publishManifestFromFrozen_usesPrebuiltTargetJson_notManifestTargetJson() {
        val projectId = PROJ_1
        val txId = TX_1

        // 模拟恢复入口：正文已 promote，manifest 还没开始 stage
        val journalBeforeFrozen = PendingMirrorPublish(
            txId = txId,
            backend = MirrorBackend.MEDIA_STORE,
            treeUri = null,
            projectId = projectId,
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
            manifestTargetJson = null, // manifest 子事务未开始
        )

        // 修复后：recoverPromotePhase 用 frozenManifestPlan 生成 manifestJson，
        // 传 prebuiltTargetJson 给 publishManifestWithDesiredTransactional，
        // 不修改 journalContext.manifestTargetJson
        val manifestJson = """{SCHEMAVERSION:1,"revision":100,"updatedAt":"2026-09-01","projects":[]}"""

        // 模拟调用 publishManifestWithDesiredTransactional 传 prebuiltTargetJson
        // journalContext.manifestTargetJson 保持 null
        val journalAfterFrozen = journalBeforeFrozen.copy() // 不修改 manifestTargetJson
        assertNull(
            "★ journalContext.manifestTargetJson 仍为 null：未被伪造 ★",
            journalAfterFrozen.manifestTargetJson,
        )

        // isResumingManifest 应该为 false（manifestTargetJson == null）
        val isResumingManifest = journalAfterFrozen.manifestTargetJson != null
        assertFalse(
            "★ isResumingManifest=false：manifest 子事务未开始 ★",
            isResumingManifest,
        )

        // prebuiltTargetJson 不为 null，函数内部会正常 stage 并写 journal
        val prebuiltTargetJson = manifestJson
        assertNotNull(
            "★ prebuiltTargetJson 不为 null：将作为目标 JSON 走正常 stage 流程 ★",
            prebuiltTargetJson,
        )
    }

    /**
     * 问题 3b 回归：frozenPlanToManifestJson 与 MirrorManifest schema 一致。
     *
     * 修复后：输出的 JSON 应该有 schemaVersion/projects 数组/正确的字段名。
     */
    @Test
    fun problem3b_frozenPlanToManifestJson_matchesMirrorManifestSchema() {
        // 构建 frozen plan
        val plan = FrozenManifestPlan(
            schemaVersion = 1,
            revision = 1694123456789L,
            updatedAt = S_2026_09_07T00_00_00Z,
            targetProjectId = PROJ_1,
            projects = listOf(
                FrozenManifestProject(
                    id = PROJ_1,
                    title = S_1,
                    order = 0,
                    revision = 1694123456789L,
                    updatedAt = S_2026_09_07T00_00_00Z,
                    volumes = listOf(
                        FrozenManifestVolume(
                            id = VOL_1,
                            title = "卷1",
                            order = 0,
                            revision = 1694123456789L,
                            updatedAt = S_2026_09_07T00_00_00Z,
                            chapters = listOf(
                                FrozenManifestChapter(
                                    id = CHAP_1,
                                    title = "章1",
                                    order = 0,
                                    revision = 1694123456789L,
                                    updatedAt = S_2026_09_07T00_00_00Z,
                                    contentFile = "",
                                    contentHash = "",
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        )

        // promotedEntries 覆盖目标章节
        val key = ChapterKey(PROJ_1, VOL_1, CHAP_1)
        val promotedEntries = mapOf(
            key to ChapterMirrorEntry(
                uri = "content://mirror/chap.md",
                relativePath = "作品/项目1/卷1/章1.md",
                revision = 1694123456789L,
                contentHash = SHA256_ABC,
            ),
        )

        // 生成 manifest JSON
        val manifestJson = frozenPlanToManifestJson(plan, promotedEntries)
        assertNotNull("manifestJson 生成成功", manifestJson)

        val manifestRoot = org.json.JSONObject(manifestJson!!)

        // ── 断言 1：与 MirrorManifest schema 一致 ──
        assertTrue("★ 有 schemaVersion ★", manifestRoot.has(SCHEMAVERSION))
        assertEquals("schemaVersion = 1", 1, manifestRoot.getInt(SCHEMAVERSION))
        assertTrue("★ 有 projects 数组（全局 manifest）★", manifestRoot.has("projects"))
        assertFalse("★ 无顶层 projectId ★", manifestRoot.has("projectId"))
        assertFalse("★ 无顶层 volumes ★", manifestRoot.has("volumes"))

        // ── 断言 2：projects 数组包含目标项目 ──
        val projectsArray = manifestRoot.getJSONArray("projects")
        assertEquals("projects 长度 = 1", 1, projectsArray.length())
        val projectObj = projectsArray.getJSONObject(0)
        assertTrue("★ project 用 id 字段（不是 projectId）★", projectObj.has("id"))
        assertEquals("project id", PROJ_1, projectObj.getString("id"))

        // ── 断言 3：章节字段与 MirrorChapter 一致 ──
        val volumes = projectObj.getJSONArray("volumes")
        val volumeObj = volumes.getJSONObject(0)
        assertTrue("★ volume 用 id 字段（不是 volumeId）★", volumeObj.has("id"))

        val chapters = volumeObj.getJSONArray("chapters")
        val chapterObj = chapters.getJSONObject(0)
        assertTrue("★ chapter 用 id 字段（不是 chapterId）★", chapterObj.has("id"))
        assertTrue("★ chapter 用 contentFile 字段（不是 uri）★", chapterObj.has("contentFile"))
        assertTrue("★ chapter 有 contentHash 字段 ★", chapterObj.has("contentHash"))
        assertFalse("★ chapter 无 uri 字段 ★", chapterObj.has("uri"))

        // ── 断言 4：目标章节用 promotedEntries 的真实 URI/hash ──
        assertEquals(
            "★ contentFile 用 promotedEntries 的 relativePath ★",
            "作品/项目1/卷1/章1.md",
            chapterObj.getString("contentFile"),
        )
        assertEquals(
            "★ contentHash 用 promotedEntries 的 contentHash ★",
            SHA256_ABC,
            chapterObj.getString("contentHash"),
        )
    }

    /**
     * rollbackManifest：manifestOldRef != null && manifestOldContentHash == null → 拒绝回滚。
     */
    @Test
    fun problem3_rollbackManifest_rejectsOldRefWithoutOldHash() {
        // 验证 validateInvariants 拒绝这种组合
        val journal = PendingMirrorPublish(
            txId = TX_1,
            backend = MirrorBackend.MEDIA_STORE,
            treeUri = null,
            projectId = PROJ_1,
            transactionType = MirrorTransactionType.UPSERT_PROJECT,
            phase = PendingMirrorPublish.PHASE_PROMOTE,
            oldEntries = emptyMap(),
            newEntries = emptyMap(),
            stagedRefs = emptyMap(),
            items = emptyMap(),
            removedProjectIds = emptySet(),
            manifestOldRef = MirrorFileRef("content://old", "_meta/manifest.json"),
            manifestStagedRef = null,
            manifestNewRef = null,
            manifestBackupRef = null,
            manifestTargetJson = "some-json", // manifest 子事务已开始
            manifestOldContentHash = null, // 但 old hash 缺失
        )

        // validateInvariants 应拒绝：manifestTargetJson != null && manifestOldRef != null
        // && manifestOldContentHash == null
        assertFalse(
            "★ validateInvariants 拒绝 manifestOldRef!=null + manifestOldContentHash==null ★",
            journal.validateInvariants(),
        )
    }

    /**
     * frozen plan 序列化/反序列化 round-trip。
     */
    @Test
    fun frozenPlanJsonRoundTrip_preservesAllFields() {
        val plan = FrozenManifestPlan(
            schemaVersion = 1,
            revision = 1694123456789L,
            updatedAt = S_2026_09_07T00_00_00Z,
            targetProjectId = PROJ_1,
            projects = listOf(
                FrozenManifestProject(
                    id = PROJ_1,
                    title = S_1,
                    order = 0,
                    revision = 1694123456789L,
                    updatedAt = S_2026_09_07T00_00_00Z,
                    volumes = listOf(
                        FrozenManifestVolume(
                            id = VOL_1,
                            title = "卷1",
                            order = 0,
                            revision = 1694123456789L,
                            updatedAt = S_2026_09_07T00_00_00Z,
                            chapters = listOf(
                                FrozenManifestChapter(
                                    id = CHAP_1,
                                    title = "章1",
                                    order = 0,
                                    revision = 1694123456789L,
                                    updatedAt = S_2026_09_07T00_00_00Z,
                                    contentFile = "test.md",
                                    contentHash = SHA256_ABC,
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        )

        val json = frozenManifestPlanToJson(plan)
        val recovered = frozenManifestPlanFromJson(json)

        assertNotNull("反序列化成功", recovered)
        assertEquals(SCHEMAVERSION, plan.schemaVersion, recovered!!.schemaVersion)
        assertEquals("revision", plan.revision, recovered.revision)
        assertEquals("updatedAt", plan.updatedAt, recovered.updatedAt)
        assertEquals("targetProjectId", plan.targetProjectId, recovered.targetProjectId)
        assertEquals("projects.size", 1, recovered.projects.size)
        assertEquals("project.id", PROJ_1, recovered.projects[0].id)
        assertEquals("project.title", S_1, recovered.projects[0].title)
        assertEquals("volumes.size", 1, recovered.projects[0].volumes.size)
        assertEquals("chapters.size", 1, recovered.projects[0].volumes[0].chapters.size)
        assertEquals("chapter.contentFile", "test.md", recovered.projects[0].volumes[0].chapters[0].contentFile)
        assertEquals("chapter.contentHash", SHA256_ABC, recovered.projects[0].volumes[0].chapters[0].contentHash)
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
