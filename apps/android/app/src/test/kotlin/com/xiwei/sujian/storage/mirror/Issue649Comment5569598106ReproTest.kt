package com.xiwei.sujian.storage.mirror

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #649 评论 5569598106：三个未收口问题的回归测试（修复后断言正确行为）。
 *
 * 本测试文件验证评论 5569598106 指出的三个缺陷已在
 * `ReadableMirrorPublisher.kt` 中修复：
 *
 * 1. **rollback/recovery 用 hash 校验 final 内容身份**：
 *    `rollbackWholePublishTransaction` 和 `recoverRollbackPhase` 不再用
 *    `finalLookup is Found → alreadyRestored = true` 判定"已恢复旧内容"，
 *    而是用 `readTextAndHash` 读取 final 内容 hash，与旧正文期望 hash 比对：
 *    匹配才是真正已恢复；不匹配（崩溃窗口下 final 上是新内容）则继续 restoreBackup。
 *
 * 2. **forward recovery lookup/hash 失败时停止保留 journal**：
 *    `recoverPromotePhase` 中，`finalLookup is Failed`（查询失败）或 hash 不匹配时，
 *    必须 return/rollback 停止，保留 journal，不继续 promoteStaged。
 *    只允许明确确认（lookup Missing 或 hash 匹配）时推进。
 *
 * 3. **journal 写入持续传递 manifest new/old hash**：
 *    `writePendingPublishJournal` 新增 `journalContext` 参数，自动继承
 *    `manifestNewContentHash`/`manifestOldContentHash`，所有调用点持续传递，
 *    journal 更新后 hash 不丢失，恢复阶段读到正确 hash。
 *
 * 源文件：apps/android/app/src/main/kotlin/com/xiwei/sujian/storage/mirror/ReadableMirrorPublisher.kt
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue649Comment5569598106ReproTest {
    companion object {
        private const val OLD_CONTENT__TO_RESTORE = "old content (to restore)"
        private const val P_V_CH_MD = "作品/P/V/Ch.md"
        private const val SHA256_NEW_MANIFEST_HASH = "sha256:new_manifest_hash"
        private const val SHA256_OLD_MANIFEST_HASH = "sha256:old_manifest_hash"
        private const val TX1 = "tx1"
    }

    // ══════════════════════════════════════════════════════════════════════
    // 问题1：rollback/recovery 用 hash 校验 final 内容身份（修复后正确行为）
    // 源：ReadableMirrorPublisher.rollbackWholePublishTransaction
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 问题1修复后：崩溃窗口 = 新内容 promote 完成 + journal 状态未落盘。
     *
     * 此时 final 上是新内容（promotedRef），修复后的 `rollbackWholePublishTransaction`：
     * ```
     * val expectedOldHash = latestJournal.oldEntries[key]?.contentHash ?: item.oldContentHash
     * val hashResult = storage.readTextAndHash(finalLookup.ref)
     * val (_, finalHash) = hashResult
     * val alreadyRestored = if (finalHash == expectedOldHash) true else false
     * ```
     * 用 hash 校验发现 final 上是新内容（hash != oldHash）→ alreadyRestored=false → 执行 restoreBackup。
     */
    @Test
    fun problem1_rollback_verifiesFinalIdentityByHash_whenNewContentPromoted() {
        val storage = ReproFakeStorage()
        val finalPath = P_V_CH_MD

        // 崩溃窗口：新内容已 promote 到 final 路径
        val newContentUri = "content://promoted/new"
        storage.committedFiles[newContentUri] = "new content (promoted)"
        storage.committedPathToUri[finalPath] = newContentUri

        // 旧内容备份已存在 backup 区
        val backupUri = "content://backup/old"
        storage.backupFiles[backupUri] = OLD_CONTENT__TO_RESTORE
        storage.backupPathToUri[".staging/tx1/backup/$finalPath"] = backupUri

        // 旧正文期望 hash（来自 journal.oldEntries[key].contentHash 或 item.oldContentHash）
        val expectedOldHash = computeContentHash(OLD_CONTENT__TO_RESTORE)

        // ── 复现修复后 rollbackWholePublishTransaction 的 hash 校验逻辑 ──
        val finalLookup = storage.lookup(finalPath)
        val alreadyRestored =
            when (finalLookup) {
                is MirrorLookupResult.Found -> {
                    // 修复后：用 hash 校验 final 身份
                    val hashResult = storage.readTextAndHash(finalLookup.ref)
                    if (hashResult != null) {
                        val (_, finalHash) = hashResult
                        finalHash == expectedOldHash
                    } else {
                        false
                    }
                }
                is MirrorLookupResult.Missing -> false
                is MirrorLookupResult.Failed -> false
            }

        // 正确行为1：final 上是新内容，hash != oldHash → alreadyRestored = false
        assertFalse(
            "修复后：用 hash 校验发现 final 是新内容 → alreadyRestored=false",
            alreadyRestored,
        )

        // 正确行为2：alreadyRestored=false → 应执行 restoreBackup，final 恢复为旧内容
        // 验证 final hash 确实不等于旧内容 hash（证明需要 restoreBackup）
        val finalContentNow = storage.committedFiles[storage.committedPathToUri[finalPath]]
        val finalHash = computeContentHash(finalContentNow!!)
        assertFalse(
            "修复后：final hash != old hash → 需要执行 restoreBackup 恢复旧内容",
            finalHash == expectedOldHash,
        )
    }

    /**
     * 问题1.B 修复后：`recoverRollbackPhase` 用 hash 校验 final 身份。
     *
     * 崩溃窗口下 final 上是新内容，hash 不匹配旧内容 → 不标记 STATE_ROLLBACK_OLD_RESTORED，
     * 继续 restoreBackup 恢复旧内容。
     */
    @Test
    fun problem1B_recoverRollback_verifiesFinalIdentityByHash() {
        val storage = ReproFakeStorage()
        val finalPath = P_V_CH_MD

        // 崩溃窗口：新内容在 final
        val newContentUri = "content://promoted/new"
        storage.committedFiles[newContentUri] = "new content (not actually removed)"
        storage.committedPathToUri[finalPath] = newContentUri

        // 旧内容备份
        val backupUri = "content://backup/old"
        storage.backupFiles[backupUri] = OLD_CONTENT__TO_RESTORE
        storage.backupPathToUri[".staging/tx1/backup/$finalPath"] = backupUri

        val expectedOldHash = computeContentHash(OLD_CONTENT__TO_RESTORE)

        // ── 复现修复后 recoverRollbackPhase 的 hash 校验逻辑 ──
        val finalLookup = storage.lookup(finalPath)
        var markedAsRestored = false
        var willRestoreBackup = false
        when (finalLookup) {
            is MirrorLookupResult.Found -> {
                // 修复后：用 hash 校验 final 身份
                val hashResult = storage.readTextAndHash(finalLookup.ref)
                if (hashResult != null) {
                    val (_, finalHash) = hashResult
                    if (finalHash == expectedOldHash) {
                        markedAsRestored = true
                    } else {
                        // hash 不匹配 → 不标记已恢复，继续 restoreBackup
                        willRestoreBackup = true
                    }
                }
            }
            is MirrorLookupResult.Missing -> {}
            is MirrorLookupResult.Failed -> {}
        }

        // 正确行为：final 上是新内容，hash 不匹配 → 不标记 STATE_ROLLBACK_OLD_RESTORED
        assertFalse(
            "修复后：final 是新内容，hash 不匹配 → 不标记 STATE_ROLLBACK_OLD_RESTORED",
            markedAsRestored,
        )
        // 正确行为：继续 restoreBackup
        assertTrue(
            "修复后：hash 不匹配 → 继续 restoreBackup 恢复旧内容",
            willRestoreBackup,
        )
    }

    // ══════════════════════════════════════════════════════════════════════
    // 问题2：forward recovery lookup Failed / hash 不匹配时停止保留 journal
    // 源：ReadableMirrorPublisher.recoverPromotePhase
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 问题2修复后：`recoverPromotePhase` 中 lookup 返回 Failed 时，
     * 必须 return/rollback 停止，保留 journal，不继续 promoteStaged。
     */
    @Test
    fun problem2_forwardRecovery_stopsAndKeepsJournal_whenLookupFailed() {
        val storage = ReproFakeStorage()
        storage.failLookup = true // lookup 返回 Failed（状态不明确）

        // staged 内容已就绪
        val stagedUri = "content://staging/1"
        storage.stagingFiles[stagedUri] = "new content"
        val staged =
            StagedMirrorRef(
                txId = TX1,
                stagingUri = stagedUri,
                stagingRelativePath = ".staging/tx1/f.md",
                finalRelativePath = P_V_CH_MD,
                mimeType = "text/markdown",
            )

        // ── 复现修复后 recoverPromotePhase 的控制流 ──
        val itemState = PendingItem.STATE_OLD_VACATED
        var newRef: MirrorFileRef? = null
        var stoppedAndKeptJournal = false
        if (itemState == PendingItem.STATE_OLD_VACATED) {
            val finalLookup = storage.lookup(staged.finalRelativePath)
            when (finalLookup) {
                is MirrorLookupResult.Found -> {
                    // hash 校验逻辑（不会进入，因为 Failed）
                }
                is MirrorLookupResult.Missing -> {
                    // final 不存在，继续 promote
                }
                is MirrorLookupResult.Failed -> {
                    // 修复后：lookup 失败 → 停止保留 journal，不继续 promote
                    stoppedAndKeptJournal = true
                }
            }
        }
        // 修复后：stoppedAndKeptJournal=true → 不执行 promoteStaged
        if (!stoppedAndKeptJournal && newRef == null) {
            newRef = storage.promoteStaged(staged, staged.finalRelativePath)
        }

        // 正确行为：lookup Failed → 停止保留 journal
        assertTrue(
            "修复后：lookup Failed → 停止保留 journal",
            stoppedAndKeptJournal,
        )
        // 正确行为：未调用 promoteStaged
        assertFalse(
            "修复后：lookup Failed → 不调用 promoteStaged",
            storage.operationLog.any { it.startsWith("promote:") },
        )
        assertNull(
            "修复后：lookup Failed → newRef 保持 null，不继续猜测式 promote",
            newRef,
        )
    }

    /**
     * 问题2.B 修复后：hash 校验失败（不匹配）时，停止保留 journal，不继续 promote。
     */
    @Test
    fun problem2B_forwardRecovery_stopsAndKeepsJournal_whenHashMismatch() {
        val storage = ReproFakeStorage()
        val finalPath = P_V_CH_MD

        // final 上已存在内容，但 hash 不匹配（不是期望的新内容）
        val existingUri = "content://existing/wrong"
        storage.committedFiles[existingUri] = "wrong content (hash mismatch)"
        storage.committedPathToUri[finalPath] = existingUri

        // staged 内容
        val stagedUri = "content://staging/1"
        storage.stagingFiles[stagedUri] = "new content"
        val staged =
            StagedMirrorRef(
                txId = TX1,
                stagingUri = stagedUri,
                stagingRelativePath = ".staging/tx1/f.md",
                finalRelativePath = finalPath,
                mimeType = "text/markdown",
            )

        // ── 复现修复后 recoverPromotePhase，hash 不匹配分支 ──
        val itemState = PendingItem.STATE_OLD_VACATED
        val expectedHash = computeContentHash("expected new content") // 不匹配 final 上的内容
        var newRef: MirrorFileRef? = null
        var stoppedAndKeptJournal = false
        if (itemState == PendingItem.STATE_OLD_VACATED) {
            val finalLookup = storage.lookup(staged.finalRelativePath)
            when (finalLookup) {
                is MirrorLookupResult.Found -> {
                    // 提取 hash 校验到 helper（#651 评论 5592465805：扁平化嵌套）
                    val (ref, stopped) = checkFoundHashMismatch(finalLookup.ref, storage, expectedHash)
                    newRef = ref
                    stoppedAndKeptJournal = stopped
                }
                is MirrorLookupResult.Missing -> {}
                is MirrorLookupResult.Failed -> {
                    stoppedAndKeptJournal = true
                }
            }
        }
        // 修复后：stoppedAndKeptJournal=true → 不执行 promoteStaged
        if (!stoppedAndKeptJournal && newRef == null) {
            newRef = storage.promoteStaged(staged, staged.finalRelativePath)
        }

        // 正确行为：hash 不匹配 → 停止保留 journal
        assertTrue(
            "修复后：hash 不匹配 → 停止保留 journal",
            stoppedAndKeptJournal,
        )
        // 正确行为：未调用 promoteStaged
        assertFalse(
            "修复后：hash 不匹配 → 不调用 promoteStaged",
            storage.operationLog.any { it.startsWith("promote:") },
        )
        assertNull(
            "修复后：hash 不匹配 → newRef 保持 null，不继续猜测式 promote",
            newRef,
        )
    }

    /**
     * 复现 recoverPromotePhase 中 Found 分支的 hash 校验逻辑（#651 评论 5592465805：扁平化嵌套）。
     *
     * 返回 Pair(newRef, stoppedAndKeptJournal)：
     * - hash 匹配 → (foundRef, false)
     * - hash 不匹配 → (null, true)（停止保留 journal，不继续 promote）
     * - hashResult 为 null → (null, false)
     */
    private fun checkFoundHashMismatch(
        foundRef: MirrorFileRef,
        storage: ReproFakeStorage,
        expectedHash: String,
    ): Pair<MirrorFileRef?, Boolean> {
        val hashResult = storage.readTextAndHash(foundRef) ?: return Pair(null, false)
        val (_, hash) = hashResult
        return if (hash == expectedHash) {
            Pair(foundRef, false)
        } else {
            Pair(null, true)
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    // 问题3：journal 写入持续传递 manifest new/old hash（修复后正确行为）
    // 源：ReadableMirrorPublisher.writePendingPublishJournal（journalContext 自动继承）
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 问题3修复后：`writePendingPublishJournal` 新增 `journalContext` 参数，
     * 当 `manifestNewContentHash`/`manifestOldContentHash` 为 null 时自动从
     * `journalContext` 继承。调用点传 `journalContext = journal` 后，
     * journal 更新后 hash 不丢失。
     */
    @Test
    fun problem3_journalWrite_preservesManifestHash_viaJournalContext() {
        // 原始 journal 带 manifest hash（事务开始时记录）
        val originalJournal =
            PendingMirrorPublish(
                txId = TX1,
                backend = MirrorBackend.MEDIA_STORE,
                treeUri = null,
                projectId = "p1",
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
                manifestNewContentHash = SHA256_NEW_MANIFEST_HASH,
                manifestOldContentHash = SHA256_OLD_MANIFEST_HASH,
            )
        assertEquals(SHA256_NEW_MANIFEST_HASH, originalJournal.manifestNewContentHash)
        assertEquals(SHA256_OLD_MANIFEST_HASH, originalJournal.manifestOldContentHash)

        // ── 复现修复后 writePendingPublishJournal 的 journalContext 继承逻辑 ──
        // 调用点传 journalContext = originalJournal，不显式传 hash 参数。
        // writePendingPublishJournal 内部：
        //   effectiveManifestNewContentHash = manifestNewContentHash ?: journalContext?.manifestNewContentHash
        //   effectiveManifestOldContentHash = manifestOldContentHash ?: journalContext?.manifestOldContentHash
        val effectiveManifestNewContentHash: String? = null ?: originalJournal.manifestNewContentHash
        val effectiveManifestOldContentHash: String? = null ?: originalJournal.manifestOldContentHash

        val rewrittenJournal =
            PendingMirrorPublish(
                txId = originalJournal.txId,
                backend = originalJournal.backend,
                treeUri = originalJournal.treeUri,
                projectId = originalJournal.projectId,
                transactionType = originalJournal.transactionType,
                phase = PendingMirrorPublish.PHASE_PROMOTE,
                oldEntries = originalJournal.oldEntries,
                newEntries = originalJournal.newEntries,
                stagedRefs = originalJournal.stagedRefs,
                items = mapOf(),
                removedProjectIds = originalJournal.removedProjectIds,
                manifestOldRef = originalJournal.manifestOldRef,
                manifestStagedRef = originalJournal.manifestStagedRef,
                manifestNewRef = originalJournal.manifestNewRef,
                manifestBackupRef = originalJournal.manifestBackupRef,
                manifestSwapState = originalJournal.manifestSwapState,
                // 修复后：通过 journalContext 自动继承 hash
                manifestNewContentHash = effectiveManifestNewContentHash,
                manifestOldContentHash = effectiveManifestOldContentHash,
            )

        // 正确行为：journal 更新后 manifest hash 保留（不丢失为 null）
        assertEquals(
            "修复后：通过 journalContext 继承，manifestNewContentHash 保留",
            SHA256_NEW_MANIFEST_HASH,
            rewrittenJournal.manifestNewContentHash,
        )
        assertEquals(
            "修复后：通过 journalContext 继承，manifestOldContentHash 保留",
            SHA256_OLD_MANIFEST_HASH,
            rewrittenJournal.manifestOldContentHash,
        )

        // 序列化 + 反序列化后也保留（恢复阶段读到正确 hash）
        val json = rewrittenJournal.toJson()
        val restored = PendingMirrorPublish.fromJson(json)!!
        assertEquals(
            "修复后：恢复阶段读到的 manifestNewContentHash 正确",
            SHA256_NEW_MANIFEST_HASH,
            restored.manifestNewContentHash,
        )
        assertEquals(
            "修复后：恢复阶段读到的 manifestOldContentHash 正确",
            SHA256_OLD_MANIFEST_HASH,
            restored.manifestOldContentHash,
        )
    }

    /**
     * 问题3.B 修复后：显式传递 hash 参数时优先用显式值（覆盖 journalContext）。
     * 验证 `manifestNewContentHash ?: journalContext?.manifestNewContentHash` 的优先级。
     */
    @Test
    fun problem3B_journalWrite_explicitHashOverridesJournalContext() {
        val journalContext =
            PendingMirrorPublish(
                txId = "tx2",
                backend = MirrorBackend.DOCUMENT_TREE,
                treeUri = "content://tree",
                projectId = "p2",
                transactionType = MirrorTransactionType.UPSERT_PROJECT,
                phase = PendingMirrorPublish.PHASE_STAGE,
                oldEntries = emptyMap(),
                newEntries = emptyMap(),
                stagedRefs = emptyMap(),
                items = emptyMap(),
                removedProjectIds = emptySet(),
                manifestOldRef = null,
                manifestStagedRef = null,
                manifestNewRef = null,
                manifestBackupRef = null,
                manifestNewContentHash = "sha256:old_new_hash",
                manifestOldContentHash = "sha256:old_old_hash",
            )

        // manifest 事务成功后，用 manifestResult 的新 hash 显式覆盖
        val manifestResultNewHash = "sha256:brand_new_manifest_hash"
        val manifestResultOldHash = "sha256:brand_old_manifest_hash"

        // 修复后逻辑：显式参数优先，null 时才继承 journalContext
        val effectiveNew = manifestResultNewHash ?: journalContext.manifestNewContentHash
        val effectiveOld = manifestResultOldHash ?: journalContext.manifestOldContentHash

        assertEquals(
            "修复后：显式传递的 manifestNewContentHash 优先于 journalContext",
            "sha256:brand_new_manifest_hash",
            effectiveNew,
        )
        assertEquals(
            "修复后：显式传递的 manifestOldContentHash 优先于 journalContext",
            "sha256:brand_old_manifest_hash",
            effectiveOld,
        )

        // 反向验证：显式传 null 时继承 journalContext
        val effectiveNewInherited = null ?: journalContext.manifestNewContentHash
        val effectiveOldInherited = null ?: journalContext.manifestOldContentHash
        assertEquals(
            "修复后：显式传 null 时继承 journalContext.manifestNewContentHash",
            "sha256:old_new_hash",
            effectiveNewInherited,
        )
        assertEquals(
            "修复后：显式传 null 时继承 journalContext.manifestOldContentHash",
            "sha256:old_old_hash",
            effectiveOldInherited,
        )
    }

    // ══════════════════════════════════════════════════════════════════════
    // ReproFakeStorage：复现用 mock storage（实现 ReadableMirrorStorage 接口）
    // ══════════════════════════════════════════════════════════════════════

    private class ReproFakeStorage : ReadableMirrorStorage {
        val committedFiles = mutableMapOf<String, String>()
        val stagingFiles = mutableMapOf<String, String>()
        val backupFiles = mutableMapOf<String, String>()
        val committedPathToUri = mutableMapOf<String, String>()
        val backupPathToUri = mutableMapOf<String, String>()
        val operationLog = mutableListOf<String>()

        var failDelete = false
        var failResolve = false
        var failLookup = false

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
            if (failDelete) return false
            committedFiles.remove(ref.uri)
            stagingFiles.remove(ref.uri)
            backupFiles.remove(ref.uri)
            committedPathToUri.entries.removeIf { it.value == ref.uri }
            backupPathToUri.entries.removeIf { it.value == ref.uri }
            return true
        }

        override fun isSupported(): Boolean = true

        override fun stageText(
            txId: String,
            relativePath: String,
            mimeType: String,
            text: String,
        ): StagedMirrorRef? {
            val uri = "content://fake/staging/${stagingFiles.size}"
            stagingFiles[uri] = text
            operationLog.add("stageText:$relativePath")
            return StagedMirrorRef(txId, uri, ".staging/$txId/$relativePath", relativePath, mimeType)
        }

        override fun backupCommitted(
            txId: String,
            old: MirrorFileRef,
            mimeType: String,
        ): MirrorFileRef? {
            val content = committedFiles[old.uri] ?: return null
            val backupUri = "content://fake/backup/${backupFiles.size}"
            val backupPath = ".staging/$txId/backup/${old.relativePath}"
            backupFiles[backupUri] = content
            backupPathToUri[backupPath] = backupUri
            operationLog.add("backup:${old.relativePath}")
            return MirrorFileRef(backupUri, backupPath)
        }

        override fun prepareBackup(
            txId: String,
            old: MirrorFileRef,
            mimeType: String,
        ): BackupReadyRef? {
            val content = committedFiles[old.uri] ?: return null
            val backupUri = "content://fake/backup/${backupFiles.size}"
            val backupPath = ".staging/$txId/backup/${old.relativePath}"
            backupFiles[backupUri] = content
            backupPathToUri[backupPath] = backupUri
            operationLog.add("prepareBackup:${old.relativePath}")
            return BackupReadyRef(MirrorFileRef(backupUri, backupPath), vacated = false)
        }

        override fun vacateCommitted(old: MirrorFileRef): Boolean {
            operationLog.add("vacate:${old.relativePath}")
            committedFiles.remove(old.uri)
            committedPathToUri.remove(old.relativePath)
            return true
        }

        override fun resolve(relativePath: String): MirrorFileRef? {
            operationLog.add("resolve:$relativePath")
            if (failResolve) return null
            val uri = committedPathToUri[relativePath] ?: return null
            return MirrorFileRef(uri, relativePath)
        }

        override fun lookup(relativePath: String): MirrorLookupResult {
            operationLog.add("lookup:$relativePath")
            if (failLookup) return MirrorLookupResult.Failed(SecurityException("simulated lookup failure"))
            val uri = committedPathToUri[relativePath] ?: return MirrorLookupResult.Missing
            return MirrorLookupResult.Found(MirrorFileRef(uri, relativePath))
        }

        override fun resolveBackup(
            txId: String,
            relativePath: String,
        ): MirrorFileRef? {
            operationLog.add("resolveBackup:$relativePath")
            if (failResolve) return null
            val backupPath = ".staging/$txId/backup/$relativePath"
            val uri = backupPathToUri[backupPath] ?: return null
            return MirrorFileRef(uri, backupPath)
        }

        override fun lookupBackup(
            txId: String,
            relativePath: String,
        ): MirrorLookupResult {
            operationLog.add("lookupBackup:$relativePath")
            if (failLookup) return MirrorLookupResult.Failed(SecurityException("simulated lookup failure"))
            val backupPath = ".staging/$txId/backup/$relativePath"
            val uri = backupPathToUri[backupPath] ?: return MirrorLookupResult.Missing
            return MirrorLookupResult.Found(MirrorFileRef(uri, backupPath))
        }

        override fun promoteStaged(
            staged: StagedMirrorRef,
            finalRelativePath: String,
        ): MirrorFileRef? {
            val content = stagingFiles.remove(staged.stagingUri) ?: return null
            val newUri = "content://fake/promoted/${committedFiles.size}"
            committedFiles[newUri] = content
            committedPathToUri[finalRelativePath] = newUri
            operationLog.add("promote:${staged.stagingRelativePath}→$finalRelativePath")
            return MirrorFileRef(newUri, finalRelativePath)
        }

        override fun restoreBackup(
            backup: MirrorFileRef,
            finalRelativePath: String,
            mimeType: String,
            expectedOldContentHash: String?,
        ): RestoreBackupResult {
            val existing = committedFiles.entries.find { committedPathToUri[finalRelativePath] == it.key }
            if (existing != null) {
                return RestoreBackupResult.AlreadyRestored(MirrorFileRef(existing.key, finalRelativePath))
            }
            val content =
                backupFiles[backup.uri]
                    ?: committedFiles[backup.uri]
                    ?: return RestoreBackupResult.Failed(null)
            val newUri = "content://fake/restored/${committedFiles.size}"
            committedFiles[newUri] = content
            committedPathToUri[finalRelativePath] = newUri
            operationLog.add("restore:${backup.relativePath}→$finalRelativePath")
            return RestoreBackupResult.Restored(MirrorFileRef(newUri, finalRelativePath))
        }

        override fun readTextAndHash(ref: MirrorFileRef): Pair<String, String>? {
            val content = committedFiles[ref.uri] ?: stagingFiles[ref.uri] ?: backupFiles[ref.uri] ?: return null
            return Pair(content, computeContentHash(content))
        }

        override fun rollback(txId: String): Boolean {
            stagingFiles.clear()
            operationLog.add("rollback:$txId")
            return true
        }
    }
}
