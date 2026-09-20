package com.xiwei.sujian.storage.mirror

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.project.data.model.ChapterOpenResult
import com.xiwei.sujian.feature.project.data.model.Project
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #726 评论 5750735839 修复测试 — manifest promote hash mismatch 重试风暴。
 *
 * ## 问题本质
 * `MirrorManifestTransactionExecutor.handlePromoteManifestFound()` 原逻辑：
 * `workspace.readManifest()` 非空时只比较 `currentHash` 和 `desiredNewHash`，
 * 当 `currentHash != desiredNewHash` 就 `Aborted` 并保留 journal。
 *
 * 但 prepare 阶段已明确"private manifest 不再 vacate"
 * （`MirrorManifestPrepareExecutor.handleBackupFoundNeedsVacate` 不删除旧 manifest）。
 * 所以 promote 时私有目录里 manifest 存在且 `hash == oldHash` 是**正常更新中间状态**，
 * 不是异常。原代码把它当异常 `Aborted`，导致 journal 保留、重试时再次走到同一分支、
 * 永远不前进 → 57 轮重试风暴。
 *
 * ## 修复后的三态行为
 * `handlePromoteManifestFound()` 分三种状态：
 * 1. `currentHash == newHash`：已 promote，幂等恢复（setManifestUri / commit）→ Completed。
 * 2. `currentHash == oldHash`：正常更新，从 staged ref（没有时用 journal
 *    `manifestTargetJson`）读取目标 JSON，`workspace.writeManifest()` 原子覆盖，
 *    验证写入后 `hash == newHash`，写 `MANIFEST_PROMOTED` journal，返回 `Proceed`。
 * 3. `currentHash` 同时不等于 `oldHash` 和 `newHash`：真正的 unknown state，
 *    保留 journal 并 `Aborted`。
 *
 * ## 测试策略
 * 用反射调用 private 方法（`handlePromoteManifestFound` / `promoteManifestStaged` /
 * `handlePromoteManifestMissing` / `atomicPromoteManifest`），覆盖：
 * - 三条分支（oldHash → Proceed / newHash → Completed / unknown → Aborted）
 * - manifest 缺失（handlePromoteManifestMissing）→ Proceed（共享函数路径）
 * - 原子覆盖后 hash 验证失败 → Aborted（从 backup 恢复）
 * - 原子覆盖成功后 workspace.readManifest() hash == newHash
 *
 * 相关源文件：
 * - `MirrorManifestTransactionExecutor.kt`（`handlePromoteManifestFound` / `atomicPromoteManifest`）。
 * - `MirrorManifestPrepareExecutor.kt`（`handleBackupFoundNeedsVacate` 不 vacate）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue726Comment5750735839PromoteReproTest {
    private companion object {
        const val PROJECT_ID = "p726"
        const val TX_ID = "tx-726"
        const val MANIFEST_RELATIVE_PATH = "_meta/manifest.json"

        /** 旧 manifest JSON（revision=1，代表冻结的 committed baseline）。 */
        const val OLD_MANIFEST_JSON =
            """{"schemaVersion":1,"revision":1,"updatedAt":"2026-01-01T00:00:00Z","projects":[]}"""

        /** 新 manifest JSON（revision=2，代表本次事务要 promote 的目标）。 */
        const val NEW_MANIFEST_JSON =
            """{"schemaVersion":1,"revision":2,"updatedAt":"2026-09-20T00:00:00Z","projects":[]}"""

        /** 与 old/new 都不同的 manifest JSON（模拟真正的 unknown state）。 */
        const val UNKNOWN_MANIFEST_JSON =
            """{"schemaVersion":1,"revision":999,"updatedAt":"2025-01-01T00:00:00Z","projects":[]}"""

        /** detekt StringLiteralDuplication：提取重复的断言消息和 MIME 类型为常量。 */
        const val MSG_WRITE_OLD_SUCCESS = "writeManifest(old) 应成功"
        const val MSG_STAGE_NEW_SUCCESS = "stageText(new manifest) 应成功"
        const val MIME_JSON = "application/json"
    }

    private lateinit var context: Context
    private lateinit var workspace: MirrorTransactionWorkspace
    private lateinit var stateStore: ReadableMirrorStateStore
    private lateinit var journalWriter: MirrorJournalWriter
    private lateinit var executor: MirrorManifestTransactionExecutor

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        workspace = MirrorTransactionWorkspace(context)
        stateStore = ReadableMirrorStateStore(context)
        journalWriter = MirrorJournalWriter(stateStore)
        // planner 在 handlePromoteManifestFound 路径中不被使用，
        // 但构造 executor 需要一个非 null 实例。
        val planner = MirrorPublishPlanner(NoopMirrorSnapshotSource(), stateStore, MirrorManifestCodec())
        executor = MirrorManifestTransactionExecutor(stateStore, journalWriter, planner, workspace)
        // 清理上次测试可能残留的私有 manifest，保证测试隔离。
        workspace.deleteManifest()
    }

    /**
     * #726 主修复测试：私有 manifest 处于冻结 oldHash 时，handlePromoteManifestFound
     * 走 atomic promote 返回 `Proceed`，且原子覆盖后 workspace.readManifest() 的
     * hash == newHash。
     *
     * 修复前：返回 `Aborted`（bug，导致 57 轮重试风暴）。
     * 修复后：返回 `Proceed`（atomic promote 成功覆盖旧 manifest）。
     */
    @Test
    fun fixed_frozenOldHash_promoteProceedsWithAtomicOverwrite() {
        val oldHash = computeContentHash(OLD_MANIFEST_JSON)
        val newHash = computeContentHash(NEW_MANIFEST_JSON)
        assertNotEquals("oldHash 和 newHash 必须不同（否则测试无意义）", oldHash, newHash)

        // 1. 私有目录中已有 manifest，内容是 OLD_MANIFEST_JSON（hash == oldHash）。
        //    这是 prepare 阶段不 vacate private manifest 的正常中间状态
        //    （handleBackupFoundNeedsVacate 不删除旧 manifest）。
        assertTrue(MSG_WRITE_OLD_SUCCESS, workspace.writeManifest(OLD_MANIFEST_JSON))

        // 2. 构造 journal 和 stageContext。
        val journal =
            buildPendingJournal(
                newHash = newHash,
                oldHash = oldHash,
                newJson = NEW_MANIFEST_JSON,
            )
        val ctx = buildCtx(journal)
        val stageContext = buildStageContext(newHash = newHash, oldHash = oldHash, staged = null, journal = journal)
        val backupOutcome =
            MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed(
                backupRef = null,
                currentJournal = journal,
            )

        // 3. 反射调用 private handlePromoteManifestFound。
        //    currentContent = OLD_MANIFEST_JSON（即 workspace.readManifest() 的返回值）。
        val result = invokeHandlePromoteManifestFound(ctx, stageContext, backupOutcome, OLD_MANIFEST_JSON)

        // 4. 断言修复后行为：Proceed（atomic promote 成功）。
        assertTrue(
            "修复后：私有 manifest hash == oldHash（正常中间状态）时 " +
                "handlePromoteManifestFound 应返回 Proceed（atomic promote 成功）。",
            result is MirrorManifestTransactionExecutor.ManifestPromoteOutcome.Proceed,
        )

        // 5. 验证原子覆盖成功：manifest 内容已是 new（hash == newHash）。
        val currentContent = workspace.readManifest()
        assertEquals(
            "修复后：私有 manifest 已被 atomic promote 覆盖为 new 内容",
            NEW_MANIFEST_JSON,
            currentContent,
        )
        assertEquals(
            "修复后：私有 manifest hash == newHash",
            newHash,
            computeContentHash(currentContent!!),
        )
    }

    /**
     * 对照测试 1：私有 manifest hash == newHash 时，handlePromoteManifestFound
     * 返回 `Completed`（幂等恢复，正确行为）。
     *
     * 此测试验证 `currentHash == newHash` 分支是正确的，作为对照，证明 bug 只在
     * `currentHash == oldHash` 分支。
     *
     * 修复前后行为一致。
     */
    @Test
    fun control_alreadyNewHash_returnsCompleted() {
        val oldHash = computeContentHash(OLD_MANIFEST_JSON)
        val newHash = computeContentHash(NEW_MANIFEST_JSON)

        // 私有目录中已是新 manifest（hash == newHash）。
        assertTrue("writeManifest(new) 应成功", workspace.writeManifest(NEW_MANIFEST_JSON))

        val journal = buildPendingJournal(newHash = newHash, oldHash = oldHash, newJson = NEW_MANIFEST_JSON)
        val ctx = buildCtx(journal)
        val stageContext = buildStageContext(newHash = newHash, oldHash = oldHash, staged = null, journal = journal)
        val backupOutcome =
            MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed(
                backupRef = null,
                currentJournal = journal,
            )

        val result = invokeHandlePromoteManifestFound(ctx, stageContext, backupOutcome, NEW_MANIFEST_JSON)

        assertTrue(
            "对照：currentHash == newHash 时返回 Completed（幂等恢复，正确行为）",
            result is MirrorManifestTransactionExecutor.ManifestPromoteOutcome.Completed,
        )
    }

    /**
     * 对照测试 2：私有 manifest hash 既不等于 oldHash 也不等于 newHash 时，
     * handlePromoteManifestFound 返回 `Aborted`（真正的 unknown state，正确行为）。
     *
     * 此测试验证 `currentHash == unknown` 分支的 Aborted 是正确的，作为对照，
     * 证明 bug 不是"所有 Aborted 都错"，而是"oldHash 不应 Aborted"。
     *
     * 修复前后行为一致。
     */
    @Test
    fun control_unknownHash_returnsAborted() {
        val oldHash = computeContentHash(OLD_MANIFEST_JSON)
        val newHash = computeContentHash(NEW_MANIFEST_JSON)
        val unknownHash = computeContentHash(UNKNOWN_MANIFEST_JSON)
        assertNotEquals("unknownHash 必须与 oldHash 不同", oldHash, unknownHash)
        assertNotEquals("unknownHash 必须与 newHash 不同", newHash, unknownHash)

        // 私有目录中是 unknown manifest（hash 既不是 old 也不是 new）。
        assertTrue("writeManifest(unknown) 应成功", workspace.writeManifest(UNKNOWN_MANIFEST_JSON))

        val journal = buildPendingJournal(newHash = newHash, oldHash = oldHash, newJson = NEW_MANIFEST_JSON)
        val ctx = buildCtx(journal)
        val stageContext = buildStageContext(newHash = newHash, oldHash = oldHash, staged = null, journal = journal)
        val backupOutcome =
            MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed(
                backupRef = null,
                currentJournal = journal,
            )

        val result = invokeHandlePromoteManifestFound(ctx, stageContext, backupOutcome, UNKNOWN_MANIFEST_JSON)

        assertTrue(
            "对照：currentHash == unknown（既非 old 也非 new）时返回 Aborted（正确行为，保留 journal）",
            result is MirrorManifestTransactionExecutor.ManifestPromoteOutcome.Aborted,
        )
    }

    /**
     * 端到端修复测试：通过 `promoteManifestStaged`（而非直接 `handlePromoteManifestFound`）
     * 验证当私有 manifest 存在且 hash == oldHash 时，promote 走 found 分支后返回 Proceed。
     *
     * 这更贴近真实调用路径：`promoteManifestStaged` 先 `workspace.readManifest()`，
     * 非空时调用 `handlePromoteManifestFound`。
     *
     * 修复前：Aborted（bug）。
     * 修复后：Proceed（atomic promote 成功）。
     */
    @Test
    fun fixed_promoteManifestStaged_frozenOldHash_proceeds() {
        val oldHash = computeContentHash(OLD_MANIFEST_JSON)
        val newHash = computeContentHash(NEW_MANIFEST_JSON)

        // 私有目录中已有 old manifest（冻结的 committed baseline）。
        assertTrue(MSG_WRITE_OLD_SUCCESS, workspace.writeManifest(OLD_MANIFEST_JSON))

        // staged ref 中放入新 manifest 内容（模拟 prepare 阶段已 stage）。
        val stagedRef =
            workspace.stageText(
                TX_ID,
                MANIFEST_RELATIVE_PATH,
                MIME_JSON,
                NEW_MANIFEST_JSON,
            )
        assertTrue(MSG_STAGE_NEW_SUCCESS, stagedRef != null)

        val journal = buildPendingJournal(newHash = newHash, oldHash = oldHash, newJson = NEW_MANIFEST_JSON)
        val ctx = buildCtx(journal)

        // 构造 ManifestStageContext（resumeState = MANIFEST_OLD_VACATED，表示 backup 已完成）。
        val stageContext =
            MirrorManifestPrepareExecutor.ManifestStageContext(
                newContentHash = newHash,
                oldContentHash = oldHash,
                staged = stagedRef,
                resumeState = ManifestTransactionState.MANIFEST_OLD_VACATED,
                currentJournal = journal,
            )

        val backupOutcome =
            MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed(
                backupRef = null,
                currentJournal = journal,
            )

        // 反射调用 private promoteManifestStaged。
        val result = invokePromoteManifestStaged(ctx, stageContext, backupOutcome)

        assertTrue(
            "修复后（promoteManifestStaged 入口）：私有 manifest hash == oldHash 时 " +
                "promoteManifestStaged 返回 Proceed（atomic promote 成功）。",
            result is MirrorManifestTransactionExecutor.ManifestPromoteOutcome.Proceed,
        )

        // 验证原子覆盖成功：manifest 内容已是 new。
        assertEquals(
            "修复后：私有 manifest 已被 atomic promote 覆盖为 new 内容",
            NEW_MANIFEST_JSON,
            workspace.readManifest(),
        )
    }

    /**
     * 新增测试：manifest 缺失（handlePromoteManifestMissing）→ Proceed（共享函数路径）。
     *
     * 验证 `handlePromoteManifestMissing` 委托给 `atomicPromoteManifest`，
     * 从 staged ref 读取目标 JSON，原子写入，返回 Proceed。
     */
    @Test
    fun fixed_manifestMissing_proceedsViaAtomicPromote() {
        val oldHash = computeContentHash(OLD_MANIFEST_JSON)
        val newHash = computeContentHash(NEW_MANIFEST_JSON)

        // 私有目录中没有 manifest（setUp 已 deleteManifest）。
        // staged ref 中放入新 manifest 内容。
        val stagedRef =
            workspace.stageText(
                TX_ID,
                MANIFEST_RELATIVE_PATH,
                MIME_JSON,
                NEW_MANIFEST_JSON,
            )
        assertTrue(MSG_STAGE_NEW_SUCCESS, stagedRef != null)

        val journal = buildPendingJournal(newHash = newHash, oldHash = oldHash, newJson = NEW_MANIFEST_JSON)
        val ctx = buildCtx(journal)
        val stageContext =
            MirrorManifestPrepareExecutor.ManifestStageContext(
                newContentHash = newHash,
                oldContentHash = oldHash,
                staged = stagedRef,
                resumeState = ManifestTransactionState.MANIFEST_OLD_VACATED,
                currentJournal = journal,
            )
        val backupOutcome =
            MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed(
                backupRef = null,
                currentJournal = journal,
            )

        // 反射调用 private handlePromoteManifestMissing。
        val result = invokeHandlePromoteManifestMissing(ctx, stageContext, backupOutcome)

        assertTrue(
            "manifest 缺失时 handlePromoteManifestMissing 应返回 Proceed（共享 atomicPromoteManifest 路径）",
            result is MirrorManifestTransactionExecutor.ManifestPromoteOutcome.Proceed,
        )
        assertEquals(
            "manifest 缺失路径：原子写入后 manifest 内容应为 new",
            NEW_MANIFEST_JSON,
            workspace.readManifest(),
        )
    }

    /**
     * 新增测试：原子覆盖后 hash 验证失败 → Aborted（从 backup 恢复）。
     *
     * 构造 `stageContext.newContentHash` 与 staged 内容的实际 hash 不一致的场景，
     * 验证 `atomicPromoteManifest` 写入后重新读取校验失败时返回 Aborted，
     * 并从 backup 恢复 manifest 内容。
     */
    @Test
    fun fixed_atomicPromote_hashVerificationFails_abortsAndRestoresFromBackup() {
        val oldHash = computeContentHash(OLD_MANIFEST_JSON)
        val newHash = computeContentHash(NEW_MANIFEST_JSON)
        // 构造一个错误的 newContentHash（用 oldHash 代替），使写入后校验失败。
        val wrongNewHash = oldHash

        // 私有目录中先放入 old manifest（模拟 prepare 不 vacate 的中间状态）。
        assertTrue(MSG_WRITE_OLD_SUCCESS, workspace.writeManifest(OLD_MANIFEST_JSON))

        // staged ref 中放入新 manifest 内容（实际 hash == newHash）。
        val stagedRef =
            workspace.stageText(
                TX_ID,
                MANIFEST_RELATIVE_PATH,
                MIME_JSON,
                NEW_MANIFEST_JSON,
            )
        assertTrue(MSG_STAGE_NEW_SUCCESS, stagedRef != null)

        // 准备 backup：把 old manifest 内容写入 backup 目录，供恢复使用。
        val backupRef =
            workspace.prepareBackup(
                TX_ID,
                MirrorFileRef(uri = "unused", relativePath = MANIFEST_RELATIVE_PATH),
                OLD_MANIFEST_JSON,
            )
        assertTrue("prepareBackup 应成功", backupRef != null)

        val journal = buildPendingJournal(newHash = wrongNewHash, oldHash = oldHash, newJson = NEW_MANIFEST_JSON)
        val ctx = buildCtx(journal)
        // stageContext.newContentHash = wrongNewHash（与 staged 内容实际 hash 不一致）
        val stageContext =
            MirrorManifestPrepareExecutor.ManifestStageContext(
                newContentHash = wrongNewHash,
                oldContentHash = oldHash,
                staged = stagedRef,
                resumeState = ManifestTransactionState.MANIFEST_OLD_VACATED,
                currentJournal = journal,
            )
        val backupOutcome =
            MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed(
                backupRef = backupRef,
                currentJournal = journal,
            )

        // 反射调用 private atomicPromoteManifest。
        val result = invokeAtomicPromoteManifest(ctx, stageContext, backupOutcome)

        assertTrue(
            "原子覆盖后 hash 验证失败时应返回 Aborted",
            result is MirrorManifestTransactionExecutor.ManifestPromoteOutcome.Aborted,
        )
        // 验证从 backup 恢复：manifest 内容恢复为 old。
        assertEquals(
            "hash 验证失败后应从 backup 恢复 old manifest 内容",
            OLD_MANIFEST_JSON,
            workspace.readManifest(),
        )
    }

    // ── 辅助构造 ──

    private fun buildCtx(journal: PendingMirrorPublish): MirrorManifestPrepareExecutor.ManifestTransactionContext {
        return MirrorManifestPrepareExecutor.ManifestTransactionContext(
            projectId = PROJECT_ID,
            snapshot = null,
            desiredEntries = emptyMap(),
            txId = TX_ID,
            journalContext = journal,
            items = emptyMap(),
            storage = FakeReadableMirrorStorage(),
            prebuiltTargetJson = null,
            manifestRelativePath = MANIFEST_RELATIVE_PATH,
            committedManifestHash = null,
            oldBaselineExists = false,
        )
    }

    private fun buildStageContext(
        newHash: String,
        oldHash: String?,
        staged: StagedMirrorRef?,
        journal: PendingMirrorPublish,
    ): MirrorManifestPrepareExecutor.ManifestStageContext {
        return MirrorManifestPrepareExecutor.ManifestStageContext(
            newContentHash = newHash,
            oldContentHash = oldHash,
            staged = staged,
            resumeState = ManifestTransactionState.MANIFEST_OLD_VACATED,
            currentJournal = journal,
        )
    }

    private fun buildPendingJournal(
        newHash: String,
        oldHash: String,
        newJson: String,
    ): PendingMirrorPublish {
        return PendingMirrorPublish(
            txId = TX_ID,
            backend = MirrorBackend.DOCUMENT_TREE,
            treeUri = "content://tree/doc",
            projectId = PROJECT_ID,
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
            isManifestCommitted = false,
            manifestSwapState = ManifestTransactionState.MANIFEST_OLD_VACATED,
            manifestNewContentHash = newHash,
            manifestOldContentHash = oldHash,
            manifestTargetJson = newJson,
        )
    }

    // ── 反射工具 ──

    private fun invokeHandlePromoteManifestFound(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        stageContext: MirrorManifestPrepareExecutor.ManifestStageContext,
        backupOutcome: MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed,
        currentContent: String,
    ): MirrorManifestTransactionExecutor.ManifestPromoteOutcome {
        val method =
            MirrorManifestTransactionExecutor::class.java.getDeclaredMethod(
                "handlePromoteManifestFound",
                MirrorManifestPrepareExecutor.ManifestTransactionContext::class.java,
                MirrorManifestPrepareExecutor.ManifestStageContext::class.java,
                MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed::class.java,
                String::class.java,
            )
        method.isAccessible = true
        return method.invoke(executor, ctx, stageContext, backupOutcome, currentContent)
            as MirrorManifestTransactionExecutor.ManifestPromoteOutcome
    }

    private fun invokePromoteManifestStaged(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        stageContext: MirrorManifestPrepareExecutor.ManifestStageContext,
        backupOutcome: MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed,
    ): MirrorManifestTransactionExecutor.ManifestPromoteOutcome {
        val method =
            MirrorManifestTransactionExecutor::class.java.getDeclaredMethod(
                "promoteManifestStaged",
                MirrorManifestPrepareExecutor.ManifestTransactionContext::class.java,
                MirrorManifestPrepareExecutor.ManifestStageContext::class.java,
                MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed::class.java,
            )
        method.isAccessible = true
        return method.invoke(executor, ctx, stageContext, backupOutcome)
            as MirrorManifestTransactionExecutor.ManifestPromoteOutcome
    }

    private fun invokeHandlePromoteManifestMissing(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        stageContext: MirrorManifestPrepareExecutor.ManifestStageContext,
        backupOutcome: MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed,
    ): MirrorManifestTransactionExecutor.ManifestPromoteOutcome {
        val method =
            MirrorManifestTransactionExecutor::class.java.getDeclaredMethod(
                "handlePromoteManifestMissing",
                MirrorManifestPrepareExecutor.ManifestTransactionContext::class.java,
                MirrorManifestPrepareExecutor.ManifestStageContext::class.java,
                MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed::class.java,
            )
        method.isAccessible = true
        return method.invoke(executor, ctx, stageContext, backupOutcome)
            as MirrorManifestTransactionExecutor.ManifestPromoteOutcome
    }

    private fun invokeAtomicPromoteManifest(
        ctx: MirrorManifestPrepareExecutor.ManifestTransactionContext,
        stageContext: MirrorManifestPrepareExecutor.ManifestStageContext,
        backupOutcome: MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed,
    ): MirrorManifestTransactionExecutor.ManifestPromoteOutcome {
        val method =
            MirrorManifestTransactionExecutor::class.java.getDeclaredMethod(
                "atomicPromoteManifest",
                MirrorManifestPrepareExecutor.ManifestTransactionContext::class.java,
                MirrorManifestPrepareExecutor.ManifestStageContext::class.java,
                MirrorManifestPrepareExecutor.ManifestBackupOutcome.Proceed::class.java,
            )
        method.isAccessible = true
        return method.invoke(executor, ctx, stageContext, backupOutcome)
            as MirrorManifestTransactionExecutor.ManifestPromoteOutcome
    }

    /**
     * 空实现 [MirrorSnapshotSource]，仅用于构造 [MirrorPublishPlanner]。
     *
     * `handlePromoteManifestFound` 路径不使用 planner，所以这些方法永远不会被调用，
     * 返回 [BridgeResult.NotLoaded] 仅满足类型契约。
     */
    private class NoopMirrorSnapshotSource : MirrorSnapshotSource {
        override fun listProjects(): BridgeResult<List<Project>> = BridgeResult.NotLoaded

        override fun getProjectWorkspaceSnapshot(projectId: String): BridgeResult<ProjectWorkspaceSnapshot> =
            BridgeResult.NotLoaded

        override fun openChapter(
            projectId: String,
            volumeId: String,
            chapterId: String,
        ): BridgeResult<ChapterOpenResult> = BridgeResult.NotLoaded
    }
}
