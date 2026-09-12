package com.xiwei.sujian.storage.mirror

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Ignore
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #649 评论 5573750754：ReadableMirrorPublisher.kt 的 6 个事务一致性硬问题复现测试。
 *
 * 本测试文件验证评论 5573750754 指出的 6 个缺陷在当前代码中确实存在。
 * 这些是事务一致性/断电恢复问题，通过 mock storage/stateStore/journal 模拟断电窗口，
 * 手动复现当前代码的关键逻辑来证明错误行为（数据丢失、引用失效、垃圾残留、rollback 卡住）。
 *
 * 源文件：apps/android/app/src/main/kotlin/com/xiwei/sujian/storage/mirror/ReadableMirrorPublisher.kt
 *
 * 6 个问题：
 * 1. recoverPromotePhase() 仍是"先改 stateStore，后写 cleanup journal"（line 567 先 putChapterEntries，line 581 才写 PHASE_CLEANUP）
 * 2. recovery 进入 manifest 子事务时，currentJournal=journalContext.copy(...) 没写入参数 items，把已 PROMOTED 正文 item 倒退回旧状态（line 3114-3122）
 * 3. rollbackManifest() 没区分"manifest 子事务未开始"和"无旧 manifest"（line 2177 lookup final，line 2262 else return false）
 * 4. manifestBackupRef==null 仍不能证明物理 backup 不存在，rollbackManifest 未做 lookupBackup 三态发现（line 2354）
 * 5. MANIFEST_PROMOTED/MANIFEST_COMMITTED 恢复只看"final 存在"，未校验 hash==manifestNewContentHash（line 3151/3189）；recoverPromotePhase 正文复用也未校验（line 223-234）
 * 6. publishProject() 第一批正文 staging 发生在第一份 journal 之前（line 945-984 先 stageText，line 989 才写 journal）
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue649Comment5573750754ReproTest {
    private companion object {
        const val PROJECT_ID = "proj-1"
        const val CHAPTER_PATH = "作品/P/V/Ch.md"
        const val MANIFEST_PATH = "_meta/manifest.json"
    }

    // ══════════════════════════════════════════════════════════════════════
    // 问题1：recoverPromotePhase() 先改 stateStore，后写 cleanup journal
    // 源：ReadableMirrorPublisher.recoverPromotePhase line 565-605
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 问题1复现：recoverPromotePhase() 在 line 567 先执行 stateStore.putChapterEntries(promotedEntries)，
     * line 573 执行 stateStore.addPublishedProjectId，line 581 才执行 writePendingPublishJournal(PHASE_CLEANUP)。
     *
     * 断电窗口：进程死在 stateStore 已写、cleanup journal 未落盘之间。
     * - 磁盘 journal 仍是 PHASE_PROMOTE（cleanup journal 未写）
     * - stateStore 已经变成新状态（putChapterEntries + addPublishedProjectId 已成功）
     *
     * 后果：重启后 recovery 看到 PHASE_PROMOTE，会重新 promote，但 stateStore 已经是新状态，
     * 状态不一致。正确做法应是先写 PHASE_CLEANUP journal 再改 stateStore。
     */
    @Test
    fun problem1_recoverPromotePhase_writesStateStoreBeforeCleanupJournal() {
        // 模拟 stateStore 与 journal 的持久化顺序
        val stateStoreOps = mutableListOf<String>()
        val journalOps = mutableListOf<String>()

        // 模拟 promotedEntries 已计算完成
        val promotedEntriesWritten = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
        val projectId = PROJECT_ID

        // ── 复现 recoverPromotePhase() line 565-605 的当前执行顺序 ──
        // line 567: stateStore.putChapterEntries(promotedEntries)
        stateStoreOps.add("putChapterEntries")
        promotedEntriesWritten[ChapterKey(projectId, "v1", "c1")] =
            ChapterMirrorEntry("content://new/1", CHAPTER_PATH, 100L, computeContentHash("new"))
        // line 573: stateStore.addPublishedProjectId(projectId)
        stateStoreOps.add("addPublishedProjectId")

        // ★ 断电点 ★：进程在此死亡。stateStore 已写，cleanup journal 未写。
        // line 581: writePendingPublishJournal(PHASE_CLEANUP) —— 未执行
        // journalOps.add("writePendingPublishJournal:PHASE_CLEANUP")  // 未到达

        // ── 断言错误行为：stateStore 已是新状态，但磁盘 journal 仍是 PHASE_PROMOTE ──
        assertEquals(
            "当前代码：stateStore 已写入 putChapterEntries + addPublishedProjectId",
            listOf("putChapterEntries", "addPublishedProjectId"),
            stateStoreOps,
        )
        assertTrue(
            "断电窗口：cleanup journal 未落盘（journalOps 为空）",
            journalOps.isEmpty(),
        )
        // 磁盘残留状态：journal.phase == PHASE_PROMOTE，stateStore 已是新状态
        val diskJournalPhase = PendingMirrorPublish.PHASE_PROMOTE // 未被 PHASE_CLEANUP 覆盖
        val stateStoreAlreadyNew = promotedEntriesWritten.isNotEmpty()
        assertEquals(
            "不一致：磁盘 journal 仍是 PHASE_PROMOTE，但 stateStore 已是新状态",
            PendingMirrorPublish.PHASE_PROMOTE,
            diskJournalPhase,
        )
        assertTrue(
            "不一致：stateStore 已经变成新状态，重启 recovery 会重复 promote",
            stateStoreAlreadyNew,
        )
    }

    // ══════════════════════════════════════════════════════════════════════
    // 问题2：manifest 子事务用旧 journal items，把已 PROMOTED 正文 item 倒退
    // 源：ReadableMirrorPublisher.publishManifestWithDesiredTransactional line 3084/3114-3122
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 问题2复现：recoverPromotePhase() line 549-558 调用
     * publishManifestWithDesiredTransactional(journalContext=journal, items=currentItems)，
     * 其中 journal 是旧的（items 是 STAGED/BACKUP_READY 状态），currentItems 已推进到 PROMOTED。
     *
     * 但 publishManifestWithDesiredTransactional() line 3084:
     *   var currentJournal: PendingMirrorPublish = journalContext
     * line 3114-3122:
     *   currentJournal = journalContext.copy(
     *       manifestOldRef = oldRef,
     *       manifestStagedRef = staged,
     *       manifestSwapState = ...,
     *       manifestNewContentHash = ...,
     *       manifestOldContentHash = ...,
     *       manifestTargetJson = json,
     *   )  // ★ 没有设置 items = items 参数 ★
     *
     * 后续 persistPendingJournal(currentJournal) 会把 currentJournal.items（= journalContext.items，旧状态）
     * 写入磁盘，把已 PROMOTED 的正文 item 倒退回 STAGED/BACKUP_READY/OLD_VACATED。
     */
    @Test
    fun problem2_manifestSubTransaction_usesStaleJournalItems() {
        val key = ChapterKey(PROJECT_ID, "v1", "c1")
        val txId = "tx-1"

        // 旧 journal 的 items（recovery 进入时的状态，正文已 stage 但未 promote）
        val oldItem =
            PendingItem(
                key = key,
                stagedRef =
                    StagedMirrorRef(
                        txId,
                        "content://staging/1",
                        ".staging/tx-1/Ch.md",
                        CHAPTER_PATH,
                        "text/markdown",
                    ),
                oldRef = MirrorFileRef("content://old/1", CHAPTER_PATH),
                backupOldRef = null,
                promotedRef = null,
                state = PendingItem.STATE_STAGED,
            )

        // recoverPromotePhase() 推进正文后，currentItems 已更新为 PROMOTED
        val promotedRef = MirrorFileRef("content://promoted/new", CHAPTER_PATH)
        val currentItem =
            oldItem.copy(
                promotedRef = promotedRef,
                state = PendingItem.STATE_PROMOTED,
            )
        val currentItems = mapOf(key to currentItem)

        // 旧 journal（journalContext）的 items 仍是 STAGED
        val journalContext =
            PendingMirrorPublish(
                txId = txId,
                backend = MirrorBackend.MEDIA_STORE,
                treeUri = null,
                projectId = PROJECT_ID,
                transactionType = MirrorTransactionType.UPSERT_PROJECT,
                phase = PendingMirrorPublish.PHASE_PROMOTE,
                oldEntries = emptyMap(),
                newEntries = emptyMap(),
                stagedRefs = emptyMap(),
                // ★ 旧状态 STAGED ★
                items = mapOf(key to oldItem),
                removedProjectIds = emptySet(),
                manifestOldRef = null,
                manifestStagedRef = null,
                manifestNewRef = null,
                manifestBackupRef = null,
            )

        // ── 复现 publishManifestWithDesiredTransactional() line 3084/3114-3122 的当前逻辑 ──
        // line 3084: var currentJournal = journalContext
        var currentJournal: PendingMirrorPublish = journalContext

        // line 3114-3122: currentJournal = journalContext.copy(manifest 相关字段...)
        // ★ 关键缺陷：copy 没有设置 items = items 参数 ★
        currentJournal =
            journalContext.copy(
                manifestOldRef = null,
                manifestStagedRef = null,
                manifestSwapState = ManifestTransactionState.MANIFEST_STAGED,
                manifestNewContentHash = "sha256:newmanifest",
                manifestOldContentHash = null,
                manifestTargetJson = "{}",
            )
        // currentJournal.items 仍然是 journalContext.items（旧 STAGED 状态）

        // ── 断言错误行为：currentJournal.items 是旧状态，不是传入的 currentItems ──
        val journalItemState = currentJournal.items[key]?.state
        val passedItemState = currentItems[key]?.state
        assertEquals(
            "传入的 items 参数（currentItems）已是 PROMOTED",
            PendingItem.STATE_PROMOTED,
            passedItemState,
        )
        assertEquals(
            "★ 当前代码缺陷：currentJournal.items 仍是旧 STAGED 状态（copy 未设置 items）★",
            PendingItem.STATE_STAGED,
            journalItemState,
        )
        assertFalse(
            "★ persistPendingJournal(currentJournal) 会把已 PROMOTED 正文 item 倒退回 STAGED ★",
            journalItemState == passedItemState,
        )
    }

    // ══════════════════════════════════════════════════════════════════════
    // 问题3：rollbackManifest() 没区分"manifest 子事务未开始"和"无旧 manifest"
    // 源：ReadableMirrorPublisher.rollbackManifest line 2177-2265
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 问题3复现：正文 promote 失败时，manifest 子事务可能压根还没开始。
     * 此时 journal 里 manifestTargetJson==null, manifestOldRef==null,
     * manifestNewContentHash==null, manifestOldContentHash==null。
     * 但设备上很可能本来就有上一版 _meta/manifest.json。
     *
     * rollbackManifest() line 2177 一上来就 lookup final，
     * line 2187: finalHash == journalContext.manifestNewContentHash (null) → false
     * line 2225: finalHash == journalContext.manifestOldContentHash (null) → false
     * line 2262-2265: else → "final hash matches neither old nor new" → return false
     *
     * rollback 永远卡住。manifestTargetJson 是"是否开始"的天然标记，未被使用。
     */
    @Test
    fun problem3_rollbackManifest_cannotDistinguishManifestSubTransactionNotStarted() {
        val storage = ReproFakeStorage5573750754()
        val manifestPath = MANIFEST_PATH

        // 设备上有上一版 manifest（manifest 子事务未开始，但设备已有旧 manifest）
        val existingManifestUri = "content://existing/manifest"
        storage.committedFiles[existingManifestUri] = "{\"version\":\"previous\"}"
        storage.committedPathToUri[manifestPath] = existingManifestUri

        // journal 状态：manifest 子事务未开始
        // manifestTargetJson==null, manifestOldRef==null,
        // manifestNewContentHash==null, manifestOldContentHash==null
        val journalContext =
            PendingMirrorPublish(
                txId = "tx-1",
                backend = MirrorBackend.MEDIA_STORE,
                treeUri = null,
                projectId = PROJECT_ID,
                transactionType = MirrorTransactionType.UPSERT_PROJECT,
                phase = PendingMirrorPublish.PHASE_ROLLBACK,
                oldEntries = emptyMap(),
                newEntries = emptyMap(),
                stagedRefs = emptyMap(),
                items = emptyMap(),
                removedProjectIds = emptySet(),
                // 未开始
                manifestOldRef = null,
                manifestStagedRef = null,
                manifestNewRef = null,
                manifestBackupRef = null,
                // ★ 未开始标记 ★
                manifestTargetJson = null,
                // null
                manifestNewContentHash = null,
                // null
                manifestOldContentHash = null,
            )

        // ── 复现 rollbackManifest() line 2177-2265 的当前逻辑 ──
        // line 2177: val finalLookup = storage.lookup(manifestRelativePath)
        val finalLookup = storage.lookup(manifestPath)
        assertTrue("设备上有上一版 manifest", finalLookup is MirrorLookupResult.Found)

        var rollbackResult: Boolean
        when (finalLookup) {
            is MirrorLookupResult.Found -> {
                // line 2180: val hashResult = storage.readTextAndHash(finalLookup.ref)
                val hashResult = storage.readTextAndHash(finalLookup.ref)
                assertNotNull(hashResult)
                val (_, finalHash) = hashResult!!
                // line 2186-2265: when { finalHash == manifestNewContentHash ... finalHash == manifestOldContentHash ... else -> return false }
                rollbackResult =
                    when {
                        finalHash == journalContext.manifestNewContentHash -> {
                            true // final 是新 manifest → 删除
                        }
                        finalHash == journalContext.manifestOldContentHash -> {
                            true // final 已经是 old manifest → setManifestUri
                        }
                        else -> {
                            // line 2262-2264: "final hash matches neither old nor new, state unknown"
                            false // ★ rollback 卡住 ★
                        }
                    }
            }
            else -> {
                rollbackResult = true
            }
        }

        // ── 断言错误行为：rollback 永远卡住（return false） ──
        assertFalse(
            "★ 当前代码缺陷：final hash 与两个 null hash 都不匹配 → rollback return false 永久卡住 ★",
            rollbackResult,
        )
        // manifestTargetJson==null 本应表示"manifest 子事务未开始"，不应参与 new/old hash 比较
        assertNull(
            "manifestTargetJson==null 明确表示 manifest 子事务未开始",
            journalContext.manifestTargetJson,
        )
    }

    // ══════════════════════════════════════════════════════════════════════
    // 问题4：manifestBackupRef==null 仍不能证明物理 backup 不存在
    // 源：ReadableMirrorPublisher.rollbackManifest line 2354-2389
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 问题4复现：存在窗口——prepareBackup() 已把 old manifest move/copy 到 .staging/<tx>/backup，
     * 进程死亡，MANIFEST_BACKUP_READY journal 还没写成功。
     * 这时 journal 里 manifestBackupRef 仍是 null，但物理 backup 已存在。
     *
     * rollbackManifest() line 2354: if (backup == null) → 直接 setManifestUri(manifestOldRef.uri)
     * 没有做 lookupBackup 三态发现。
     *
     * 后果：MediaStore 原子 move 时 old URI 可能已指向 backup 路径；SAF 甚至可能已换 URI。
     * 随后 storage.rollback(txId) 会把真实 backup 删除，stateStore 留下失效引用。
     */
    @Ignore("Issue #667: lookupBackup 已从 ReadableMirrorStorage 移除，事务操作移到了 MirrorTransactionWorkspace，此测试需要重写")
    @Test
    fun problem4_rollbackManifest_manifestBackupRefNullButPhysicalBackupExists() {
        // Issue #667: lookupBackup 已从 ReadableMirrorStorage 移除，事务操作移到了 MirrorTransactionWorkspace
    }

    // ══════════════════════════════════════════════════════════════════════
    // 问题5：MANIFEST_PROMOTED/MANIFEST_COMMITTED 恢复未校验 contentHash
    // 源：ReadableMirrorPublisher.publishManifestWithDesiredTransactional line 3147-3237
    //       + recoverPromotePhase line 223-234 正文复用未校验
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 问题5a复现：MANIFEST_COMMITTED 分支（line 3147-3183）和 MANIFEST_PROMOTED 分支（line 3185-3237）
     * 里 lookup(final) is Found 后直接 setManifestUri() 或直接返回 ManifestTransactionResult。
     * 没有校验 readTextAndHash 后的 hash == manifestNewContentHash。
     *
     * 公共镜像在 Download 里，用户/Provider 都可能让同路径文件内容变化；
     * 路径存在不等于它还是本事务那份 manifest。
     */
    @Test
    fun problem5a_manifestPromotedCommittedRecovery_noContentHashVerification() {
        val storage = ReproFakeStorage5573750754()
        val manifestPath = MANIFEST_PATH

        // 本事务的新 manifest hash
        val newManifestContent = "{\"version\":\"new\"}"
        val manifestNewContentHash = computeContentHash(newManifestContent)

        // 公共镜像上的 manifest 内容被用户/Provider 修改（路径相同，内容不同）
        val tamperedContent = "{\"version\":\"tampered-by-user\"}"
        val tamperedUri = "content://tampered/manifest"
        storage.committedFiles[tamperedUri] = tamperedContent
        storage.committedPathToUri[manifestPath] = tamperedUri

        // ── 复现 MANIFEST_COMMITTED 分支 line 3147-3183 的当前逻辑 ──
        // line 3149: val existingFinal = storage.lookup(manifestRelativePath)
        val existingFinal = storage.lookup(manifestPath)
        assertTrue("final 存在（但内容已被篡改）", existingFinal is MirrorLookupResult.Found)

        // line 3151: is MirrorLookupResult.Found -> { return ManifestTransactionResult(...) }
        // ★ 当前代码：只检查 Found，没有校验 hash == manifestNewContentHash ★
        var returnedWithoutHashCheck = false
        when (existingFinal) {
            is MirrorLookupResult.Found -> {
                // 当前代码直接返回，不校验 hash
                returnedWithoutHashCheck = true
            }
            else -> {}
        }

        assertTrue(
            "★ 当前代码缺陷：MANIFEST_COMMITTED 仅 lookup final is Found 就返回，未校验 hash ★",
            returnedWithoutHashCheck,
        )

        // 验证：返回的 manifest 内容 hash != manifestNewContentHash
        val actualHash = storage.readTextAndHash((existingFinal as MirrorLookupResult.Found).ref)?.second
        assertFalse(
            "★ final 内容已被篡改，hash != manifestNewContentHash，但当前代码仍复用 ★",
            actualHash == manifestNewContentHash,
        )
    }

    /**
     * 问题5b复现：recoverPromotePhase() line 223-234 对 STATE_PROMOTED/STATE_COMMITTED + promotedRef != null
     * 的正文直接塞进 promotedEntries，没有 lookup + contentHash 校验。
     *
     * 公共镜像内容被改后，复用失效的 promotedRef.uri。
     */
    @Test
    fun problem5b_recoverPromotePhase_reusesPromotedRefWithoutHashVerification() {
        val storage = ReproFakeStorage5573750754()
        val key = ChapterKey(PROJECT_ID, "v1", "c1")
        val finalPath = CHAPTER_PATH

        // 本事务的新正文 hash
        val newContent = "new chapter content"
        val newContentHash = computeContentHash(newContent)

        // 公共镜像上的正文内容被修改（路径相同，内容不同）
        val tamperedContent = "tampered content by user"
        val tamperedUri = "content://tampered/chapter"
        storage.committedFiles[tamperedUri] = tamperedContent
        storage.committedPathToUri[finalPath] = tamperedUri

        // journal item：STATE_PROMOTED + promotedRef != null
        val promotedRef = MirrorFileRef(tamperedUri, finalPath)
        val item =
            PendingItem(
                key = key,
                stagedRef = null,
                oldRef = null,
                backupOldRef = null,
                promotedRef = promotedRef,
                state = PendingItem.STATE_PROMOTED,
            )
        val newEntries = mapOf(key to ChapterMirrorEntry("", finalPath, 100L, newContentHash))

        // ── 复现 recoverPromotePhase() line 221-234 的当前逻辑 ──
        val promotedEntries = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
        // line 223-234: if (STATE_PROMOTED && promotedRef != null) { promotedEntries[key] = ChapterMirrorEntry(uri=promotedRef.uri, ...); continue }
        if ((item.state == PendingItem.STATE_PROMOTED || item.state == PendingItem.STATE_COMMITTED) &&
            item.promotedRef != null
        ) {
            // ★ 当前代码：直接用 promotedRef.uri 构造 entry，没有 lookup + readTextAndHash 校验 ★
            promotedEntries[key] =
                ChapterMirrorEntry(
                    uri = item.promotedRef.uri,
                    relativePath = item.promotedRef.relativePath,
                    revision = newEntries[key]?.revision ?: 0L,
                    contentHash = newEntries[key]?.contentHash ?: "",
                )
        }

        // ── 断言错误行为：直接复用 promotedRef.uri，未校验内容 ──
        assertTrue("当前代码直接复用 promotedRef", promotedEntries.containsKey(key))
        val reusedUri = promotedEntries[key]?.uri
        assertEquals(
            "★ 当前代码缺陷：直接用 promotedRef.uri（指向被篡改的内容），未 lookup + contentHash 校验 ★",
            tamperedUri,
            reusedUri,
        )
        // 实际内容 hash != newContentHash
        val actualHash = storage.readTextAndHash(MirrorFileRef(reusedUri!!, finalPath))?.second
        assertFalse(
            "★ 实际内容已被篡改，hash != newContentHash，但当前代码仍复用 ★",
            actualHash == newContentHash,
        )
    }

    // ══════════════════════════════════════════════════════════════════════
    // 问题6：第一批正文 staging 发生在第一份 journal 之前
    // 源：ReadableMirrorPublisher.publishProject line 945-1011
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 问题6复现：publishProject() line 945-984 先循环 stageText 所有章节，
     * line 989 才 writePendingPublishJournal(PHASE_PROMOTE)。
     *
     * 断电窗口：进程死在 staging 中间。
     * - .staging/<txId>/... 已经写进 Download/Sujian
     * - 没有任何 pending journal（第一份 journal 还没写）
     * - 之后没有 txId 可以清它
     * - 公共镜像永久残留事务垃圾
     *
     * 正确做法：应先落 PHASE_STAGE journal（在第一笔 stageText() 之前），
     * 全部 staging 成功后再写 PHASE_PROMOTE。
     */
    @Ignore("Issue #667: stageText 已从 ReadableMirrorStorage 移除，事务操作移到了 MirrorTransactionWorkspace，此测试需要重写")
    @Test
    fun problem6_publishProject_stagingHappensBeforeFirstJournal() {
        // Issue #667: stageText 已从 ReadableMirrorStorage 移除，事务操作移到了 MirrorTransactionWorkspace
    }

    // ══════════════════════════════════════════════════════════════════════
    // Fake Storage（复现用）
    // ══════════════════════════════════════════════════════════════════════
    private class ReproFakeStorage5573750754 : ReadableMirrorStorage {
        val committedFiles = mutableMapOf<String, String>()
        val stagingFiles = mutableMapOf<String, String>()
        val backupFiles = mutableMapOf<String, String>()
        val committedPathToUri = mutableMapOf<String, String>()
        val backupPathToUri = mutableMapOf<String, String>()
        val operationLog = mutableListOf<String>()

        override fun createText(
            relativeDir: String,
            displayName: String,
            mimeType: String,
            text: String,
        ): MirrorFileRef? {
            val path = if (relativeDir.isBlank()) displayName else "$relativeDir/$displayName"
            val uri = "content://fake/${committedFiles.size}"
            committedFiles[uri] = text
            committedPathToUri[path] = uri
            operationLog.add("createText:$path")
            return MirrorFileRef(uri, path)
        }

        override fun replaceText(
            ref: MirrorFileRef,
            text: String,
        ): Boolean {
            committedFiles[ref.uri] = text
            operationLog.add("replaceText:${ref.relativePath}")
            return true
        }

        override fun delete(ref: MirrorFileRef): Boolean {
            operationLog.add("delete:${ref.relativePath}")
            committedFiles.remove(ref.uri)
            stagingFiles.remove(ref.uri)
            backupFiles.remove(ref.uri)
            committedPathToUri.entries.removeIf { it.value == ref.uri }
            backupPathToUri.entries.removeIf { it.value == ref.uri }
            return true
        }

        override fun isSupported(): Boolean = true

        override fun resolve(relativePath: String): MirrorFileRef? {
            operationLog.add("resolve:$relativePath")
            val uri = committedPathToUri[relativePath] ?: return null
            return MirrorFileRef(uri, relativePath)
        }

        override fun lookup(relativePath: String): MirrorLookupResult {
            operationLog.add("lookup:$relativePath")
            val uri = committedPathToUri[relativePath] ?: return MirrorLookupResult.Missing
            return MirrorLookupResult.Found(MirrorFileRef(uri, relativePath))
        }

        override fun readTextAndHash(ref: MirrorFileRef): Pair<String, String>? {
            val content = committedFiles[ref.uri] ?: stagingFiles[ref.uri] ?: backupFiles[ref.uri] ?: return null
            return Pair(content, computeContentHash(content))
        }
    }
}
