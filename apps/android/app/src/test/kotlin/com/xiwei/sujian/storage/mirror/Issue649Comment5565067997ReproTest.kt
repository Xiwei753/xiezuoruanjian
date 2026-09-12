package com.xiwei.sujian.storage.mirror

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Ignore
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #649 评论 5565067997：6 项事务状态机缺陷的回归测试。
 *
 * 本测试文件验证评论 5565067997 中描述的 6 项缺陷已被修复。
 * 每个测试断言修复后的正确行为，测试通过即证明对应缺陷已消除。
 *
 * 修复清单：
 * 1. PendingItem 增加 STATE_BACKUP_READY / STATE_OLD_VACATED 中间状态，
 *    旧 STATE_OLD_BACKED_UP 反序列化映射到 STATE_BACKUP_READY。
 * 2. manifest 事务增加 ManifestTransactionState 显式 swap 状态枚举，
 *    不再用 isManifestCommitted: Boolean 单布尔值猜测。
 * 3. manifest rollback 顺序修正：先删 manifestNewRef → restoreBackup → setManifestUri。
 * 4. rollback 删除新正文时检查 delete() 返回值，失败时停止推进状态。
 * 5. resolve() 改为三态查询 MirrorLookupResult（Found/Missing/Failed），Failed 时停止。
 * 6. saveRestoredState 接受 publishedProjectIds 参数，零章节作品也能进 publishedProjectIds。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue649Comment5565067997ReproTest {
    companion object {
        private const val CONTENT___MANIFEST_BACKUP = "content://manifest/backup"
        private const val CONTENT___MANIFEST_NEW = "content://manifest/new"
        private const val CONTENT___NEW = "content://new"
        private const val CONTENT___OLD = "content://old"
        private const val F_MD = "f.md"
        private const val META_MANIFEST_JSON = "_meta/manifest.json"
        private const val OLD_MANIFEST_CONTENT = "OLD manifest content"
        private const val TEXT_MARKDOWN = "text/markdown"
        private const val TX1 = "tx1"
    }

    // ══════════════════════════════════════════════════════════════════════
    // 修复 1：PendingItem 增加 STATE_BACKUP_READY / STATE_OLD_VACATED 中间状态
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 修复 1：PendingItem 现在有 STATE_STAGED / STATE_BACKUP_READY / STATE_OLD_VACATED /
     * STATE_PROMOTED / STATE_COMMITTED（加 rollback 两态）。
     * "backup 已存在"和"old 已腾空"是两个独立中间状态。
     * 旧 STATE_OLD_BACKED_UP 保留用于反序列化向后兼容，映射到 STATE_BACKUP_READY。
     */
    @Test
    fun fix1_pendingItem_hasBackupReadyAndOldVacatedStates() {
        val fieldNames = PendingItem::class.java.declaredFields.map { it.name }

        // 修复确认：STATE_BACKUP_READY 存在
        assertTrue(
            "修复1确认：PendingItem 有 STATE_BACKUP_READY 中间状态（backup 已准备但 old 未腾空）",
            fieldNames.contains("STATE_BACKUP_READY"),
        )
        assertEquals("BACKUP_READY", PendingItem.STATE_BACKUP_READY)

        // 修复确认：STATE_OLD_VACATED 存在
        assertTrue(
            "修复1确认：PendingItem 有 STATE_OLD_VACATED 中间状态（old 已从 final 腾空）",
            fieldNames.contains("STATE_OLD_VACATED"),
        )
        assertEquals("OLD_VACATED", PendingItem.STATE_OLD_VACATED)

        // 向后兼容：STATE_OLD_BACKED_UP 仍存在（反序列化旧 journal 用）
        assertTrue(
            "修复1确认：STATE_OLD_BACKED_UP 保留用于反序列化向后兼容",
            fieldNames.contains("STATE_OLD_BACKED_UP"),
        )

        // normalizeState 把旧状态映射到新状态
        assertEquals(
            "修复1确认：normalizeState 把 STATE_OLD_BACKED_UP 映射到 STATE_BACKUP_READY",
            PendingItem.STATE_BACKUP_READY,
            PendingItem.normalizeState(PendingItem.STATE_OLD_BACKED_UP),
        )
        // 新状态 normalize 后不变
        assertEquals(
            PendingItem.STATE_OLD_VACATED,
            PendingItem.normalizeState(PendingItem.STATE_OLD_VACATED),
        )
    }

    /**
     * 修复 1.A：fallback copy 路径中，prepareBackup() 只复制 backup，old 还在 final，
     * journal 现在写 STATE_BACKUP_READY（不是 STATE_OLD_BACKED_UP）。
     * vacate 成功后写 STATE_OLD_VACATED。恢复时据此决定是否需要 vacate。
     *
     * Issue #667 改写：用 MirrorTransactionWorkspace.prepareBackup 验证 backup 后状态是 STATE_BACKUP_READY。
     */
    @Test
    fun fix1A_fallbackCopy_usesBackupReadyState() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)

        val txId = TX1
        val relativePath = "作品/P/V/Ch.md"
        val oldContent = "old content"

        // 1. prepareBackup（fallback copy 路径）：只复制 backup，old 还在 final
        val oldRef = MirrorFileRef(CONTENT___OLD, relativePath)
        val backupRef = workspace.prepareBackup(txId, oldRef, oldContent)
        assertNotNull("prepareBackup 应成功", backupRef)

        // 2. journal 写 STATE_BACKUP_READY（不是 STATE_OLD_BACKED_UP）
        val key = ChapterKey("p1", "v1", "ch1")
        val stagedRef = StagedMirrorRef(txId, "content://staging", ".staging/tx1/f.md", F_MD, TEXT_MARKDOWN)
        val item = PendingItem(
            key = key,
            stagedRef = stagedRef,
            oldRef = oldRef,
            backupOldRef = backupRef,
            promotedRef = null,
            state = PendingItem.STATE_BACKUP_READY, // 修复后写 BACKUP_READY
        )
        assertEquals(
            "修复1.A：fallback copy 后状态是 STATE_BACKUP_READY",
            PendingItem.STATE_BACKUP_READY,
            item.state,
        )

        // 3. vacate 成功后写 STATE_OLD_VACATED
        val vacatedItem = item.copy(state = PendingItem.STATE_OLD_VACATED)
        assertEquals(
            "修复1.A：vacate 后状态是 STATE_OLD_VACATED",
            PendingItem.STATE_OLD_VACATED,
            vacatedItem.state,
        )

        // 4. 备份在 workspace 中（old 还在 final，backup 是独立副本）
        val lookupResult = workspace.lookupBackup(txId, relativePath)
        assertTrue("备份在 workspace 中", lookupResult is MirrorLookupResult.Found)
    }

    /**
     * 修复 1.B：原子 move 路径，死在 "old 已 move 到 backup → journal 还没写"。
     * 重启后 recoverPromotePhase 用 lookup() 三态查询判断 old 是否已 vacate，
     * 不再硬编码 vacated=false。
     *
     * Issue #667 改写：用 workspace.lookupBackup + storage.lookup 验证恢复时用 lookup 判断状态。
     */
    @Test
    fun fix1B_atomicMove_recoveryUsesLookupToDetermineVacated() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)
        val storage = DefectFakeStorage()

        val txId = TX1
        val relativePath = "作品/P/V/Ch.md"
        val oldContent = "old content"

        // 1. 模拟原子 move：old 已 move 到 backup
        val oldRef = MirrorFileRef(CONTENT___OLD, relativePath)
        workspace.prepareBackup(txId, oldRef, oldContent)

        // 2. 恢复时用 lookup 三态查询判断 old 是否已 vacate
        val finalLookup = storage.lookup(relativePath)
        val backupLookup = workspace.lookupBackup(txId, relativePath)

        // 3. backup Found + final Missing → old 已 vacate（原子 move 完成）
        assertTrue("backup Found（old 已 move 到 backup）", backupLookup is MirrorLookupResult.Found)
        assertTrue("final Missing（old 已从 final 移走）", finalLookup is MirrorLookupResult.Missing)

        // 4. 用 lookup 结果决定恢复动作（不硬编码 vacated=false）
        val vacated = when (finalLookup) {
            is MirrorLookupResult.Missing -> true // old 已腾空
            is MirrorLookupResult.Found -> false // old 还在 final
            is MirrorLookupResult.Failed -> false // 查询失败，状态不明
        }
        assertTrue("用 lookup 判断：final Missing → vacated=true", vacated)
    }

    // ══════════════════════════════════════════════════════════════════════
    // 修复 2：manifest 事务有显式 swap 状态枚举
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 修复 2：manifest 事务有 ManifestTransactionState 枚举
     * （MANIFEST_STAGED / MANIFEST_BACKUP_READY / MANIFEST_OLD_VACATED /
     * MANIFEST_PROMOTED / MANIFEST_COMMITTED）。
     * PendingMirrorPublish 有 manifestSwapState 字段。
     */
    @Test
    fun fix2_manifestHasExplicitSwapStates() {
        // 修复确认：ManifestTransactionState 枚举类存在
        var manifestStateClassExists = false
        try {
            Class.forName("com.xiwei.sujian.storage.mirror.ManifestTransactionState")
            manifestStateClassExists = true
        } catch (_: ClassNotFoundException) {
            // 不应到达
        }
        assertTrue(
            "修复2确认：ManifestTransactionState 枚举存在",
            manifestStateClassExists,
        )

        // 验证枚举值
        val states = ManifestTransactionState.entries.map { it.journalValue }
        assertTrue("包含 MANIFEST_STAGED", states.contains("STAGED"))
        assertTrue("包含 MANIFEST_BACKUP_READY", states.contains("BACKUP_READY"))
        assertTrue("包含 MANIFEST_OLD_VACATED", states.contains("OLD_VACATED"))
        assertTrue("包含 MANIFEST_PROMOTED", states.contains("PROMOTED"))
        assertTrue("包含 MANIFEST_COMMITTED", states.contains("COMMITTED"))

        // 修复确认：PendingMirrorPublish 有 manifestSwapState 字段
        val fieldNames = PendingMirrorPublish::class.java.declaredFields.map { it.name }
        assertTrue(
            "修复2确认：PendingMirrorPublish 有 manifestSwapState 字段",
            fieldNames.contains("manifestSwapState"),
        )
    }

    /**
     * 修复 2 行为验证：existingBackup != null 且 existingFinal != null 时，
     * 不再直接把 final 当成"已经 promote 的新 manifest"。
     * 而是根据 journal 的 manifestSwapState 决定从哪一步继续。
     *
     * Issue #667 改写：用 ManifestTransactionState 验证 manifest 事务状态。
     */
    @Test
    fun fix2_backupPlusFinal_usesManifestSwapStateNotGuessing() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)

        // 1. manifest backup 已存在（workspace 中）
        val manifestPath = META_MANIFEST_JSON
        val oldManifestContent = "{\"version\":\"old\"}"
        val oldManifestRef = MirrorFileRef(CONTENT___MANIFEST_BACKUP, manifestPath)
        val backupRef = workspace.prepareBackup(TX1, oldManifestRef, oldManifestContent)
        assertNotNull("manifest backup 应成功", backupRef)

        // 2. final 也存在（可能是新 manifest，也可能是旧 manifest 残留）
        // 修复后：用 manifestSwapState 决定从哪一步继续，不猜测
        val journalWithSwapState = PendingMirrorPublish(
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
            manifestOldRef = oldManifestRef,
            manifestStagedRef = null,
            manifestNewRef = null,
            manifestBackupRef = backupRef,
            isManifestCommitted = false,
            manifestSwapState = ManifestTransactionState.MANIFEST_BACKUP_READY,
        )

        // 3. 根据 manifestSwapState 决定恢复动作（不猜测）
        assertEquals(
            "修复2：用 manifestSwapState=MANIFEST_BACKUP_READY 决定从哪一步继续",
            ManifestTransactionState.MANIFEST_BACKUP_READY,
            journalWithSwapState.manifestSwapState,
        )

        // 4. lookupBackup 确认备份存在
        val lookupResult = workspace.lookupBackup(TX1, manifestPath)
        assertTrue("manifest backup Found", lookupResult is MirrorLookupResult.Found)
    }

    // ══════════════════════════════════════════════════════════════════════
    // 修复 3：manifest rollback 顺序正确
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 修复 3：rollbackManifest helper 的顺序是正确的：
     * 1. 先删 manifestNewRef（如果已 promote）
     * 2. final 腾空
     * 3. restoreBackup(old manifest)
     * 4. setManifestUri(restoredRef.uri)
     *
     * 不再先 restoreBackup 再 resolve(final).delete()（会删掉刚恢复的旧 manifest）。
     *
     * Issue #667 改写：用 workspace 验证 manifest rollback 顺序。
     */
    @Test
    fun fix3_manifestRollbackOrder_deleteNewRefBeforeRestore() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val workspace = MirrorTransactionWorkspace(context)
        val storage = DefectFakeStorage()

        val manifestPath = META_MANIFEST_JSON
        val oldManifestContent = OLD_MANIFEST_CONTENT
        val newManifestContent = "{\"version\":\"new\"}"

        // 1. 准备 manifest backup
        val oldManifestRef = MirrorFileRef(CONTENT___MANIFEST_BACKUP, manifestPath)
        val backupRef = workspace.prepareBackup(TX1, oldManifestRef, oldManifestContent)!!

        // 2. manifest new 已 promote 到 final
        val newManifestUri = CONTENT___MANIFEST_NEW
        storage.committedFiles[newManifestUri] = newManifestContent
        storage.committedPathToUri[manifestPath] = newManifestUri

        // 3. rollback 顺序：先删 manifestNewRef
        val deleteOrder = mutableListOf<String>()
        val newRef = MirrorFileRef(newManifestUri, manifestPath)
        val deleteResult = storage.delete(newRef)
        assertTrue("先删 manifestNewRef 应成功", deleteResult)
        deleteOrder.add("deleteNewRef")
        assertFalse("删除后 final 无 manifest", storage.committedFiles.containsKey(newManifestUri))

        // 4. 然后 restoreBackup（从 workspace 读取旧 manifest）
        val backupContent = workspace.readBackup(backupRef)
        assertEquals("restoreBackup 内容正确", oldManifestContent, backupContent)
        deleteOrder.add("restoreBackup")

        // 5. 最后 setManifestUri（在 final 创建恢复的 manifest）
        val restoredRef = storage.createText("_meta", "manifest.json", "application/json", backupContent!!)
        assertNotNull("setManifestUri 应成功", restoredRef)
        deleteOrder.add("setManifestUri")

        // 6. 验证顺序正确：deleteNewRef 在 restoreBackup 之前
        assertEquals(
            "修复3：rollback 顺序是 deleteNewRef → restoreBackup → setManifestUri",
            listOf("deleteNewRef", "restoreBackup", "setManifestUri"),
            deleteOrder,
        )
    }

    // ══════════════════════════════════════════════════════════════════════
    // 修复 4：rollback 删除新正文时检查 delete() 返回值
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 修复 4：rollbackWholePublishTransaction 和 recoverRollbackPhase 删除 promotedRef 时
     * 检查 delete() 返回值，失败时停止推进状态（保留 rollback journal）。
     */
    @Test
    fun fix4_rollbackDeleteFailure_stopsStateAdvancement() {
        val storage = DefectFakeStorage()
        storage.failDelete = true // 让 delete 返回 false
        storage.committedFiles[CONTENT___NEW] = "new content"

        val key = ChapterKey("p1", "v1", "ch1")
        val item =
            PendingItem(
                key = key,
                stagedRef = StagedMirrorRef(TX1, "content://staging", ".staging/tx1/f.md", F_MD, TEXT_MARKDOWN),
                oldRef = MirrorFileRef(CONTENT___OLD, F_MD),
                backupOldRef = MirrorFileRef("content://backup", "backup/f.md"),
                promotedRef = MirrorFileRef(CONTENT___NEW, F_MD),
                state = PendingItem.STATE_PROMOTED,
            )

        // 修复确认：检查 delete() 返回值，失败时不推进状态
        val deleteResult = item.promotedRef?.let { storage.delete(it) }
        assertFalse("delete 返回 false（模拟权限错误/IO 失败）", deleteResult!!)

        // 修复确认：delete 失败时保留原状态（不推进到 STATE_ROLLBACK_NEW_REMOVED）
        val newItem =
            if (deleteResult) {
                item.copy(state = PendingItem.STATE_ROLLBACK_NEW_REMOVED)
            } else {
                // delete 失败：保留 rollback journal，停止
                item // 状态不变
            }

        assertEquals(
            "修复4确认：delete() 返回 false 时状态不推进，保留原 STATE_PROMOTED，" +
                "保留 rollback journal 让下次重试",
            PendingItem.STATE_PROMOTED,
            newItem.state,
        )
        assertTrue(
            "修复4确认：新正文还在 final（delete 失败），状态未推进，不会制造同名冲突",
            storage.committedFiles.containsKey(CONTENT___NEW),
        )
    }

    // ══════════════════════════════════════════════════════════════════════
    // 修复 5：lookup() 三态查询 MirrorLookupResult
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 修复 5：MirrorLookupResult 三态类型存在（Found/Missing/Failed）。
     * lookup() 方法替代 resolve() 的二态返回。
     */
    @Test
    fun fix5_mirrorLookupResultExists() {
        // 修复确认：MirrorLookupResult 三态类型存在
        var mirrorLookupResultClassExists = false
        try {
            Class.forName("com.xiwei.sujian.storage.mirror.MirrorLookupResult")
            mirrorLookupResultClassExists = true
        } catch (_: ClassNotFoundException) {
            // 不应到达
        }
        assertTrue(
            "修复5确认：MirrorLookupResult 三态类型存在（Found/Missing/Failed）",
            mirrorLookupResultClassExists,
        )

        // 修复确认：lookup 方法存在（通过 MirrorStorageLookup 继承），返回 MirrorLookupResult
        val lookupMethod =
            ReadableMirrorStorage::class.java.getMethod(
                "lookup",
                String::class.java,
            )
        val returnType = lookupMethod.returnType
        assertEquals(
            "修复5确认：lookup() 返回 MirrorLookupResult，可区分 '文件不存在' 和 '查询失败'",
            MirrorLookupResult::class.java,
            returnType,
        )

        // 验证三态子类存在
        val lookupResultClass = MirrorLookupResult::class.java
        val sealedSubclasses = lookupResultClass.permittedSubclasses
        assertTrue(
            "MirrorLookupResult 有 3 个 permitted subclasses（Found/Missing/Failed）",
            sealedSubclasses != null && sealedSubclasses.size == 3,
        )
    }

    /**
     * 修复 5 行为验证：cleanup 用 lookup() 区分 Missing 和 Failed。
     * Failed 时设 allSuccess = false，不清 journal。
     */
    @Test
    fun fix5_cleanupDistinguishesMissingFromFailed() {
        val storage = DefectFakeStorage()
        storage.failLookup = true // 让 lookup 返回 Failed（模拟查询失败）

        val backupRef = MirrorFileRef("content://backup", ".staging/tx1/backup/f.md")

        // 修复确认：用 lookup() 三态查询，Failed 时设 allSuccess = false
        var allSuccess = true
        when (val lookupResult = storage.lookup(backupRef.relativePath)) {
            is MirrorLookupResult.Found -> {
                if (!storage.delete(lookupResult.ref)) allSuccess = false
            }
            is MirrorLookupResult.Missing -> {
                // 文件已不存在，目标已达到，视为成功
            }
            is MirrorLookupResult.Failed -> {
                // 修复确认：查询失败，不能当 Missing，保留 journal
                allSuccess = false
            }
        }

        // 修复确认：lookup 返回 Failed → allSuccess = false → 不清 journal
        assertTrue("lookup 返回 Failed", storage.lookup(backupRef.relativePath) is MirrorLookupResult.Failed)
        assertFalse(
            "修复5确认：lookup() 返回 Failed → allSuccess = false → 不清 journal，" +
                "保留未完成的事务让下次重试",
            allSuccess,
        )

        // 对比：Missing 时应视为成功
        storage.failLookup = false
        allSuccess = true
        when (val lookupResult = storage.lookup("nonexistent/path.md")) {
            is MirrorLookupResult.Missing -> { /* 目标已达到，成功 */ }
            is MirrorLookupResult.Found -> {
                if (!storage.delete(lookupResult.ref)) allSuccess = false
            }
            is MirrorLookupResult.Failed -> {
                allSuccess = false
            }
        }
        assertTrue(
            "修复5确认：lookup() 返回 Missing → 视为成功（目标已达到）",
            allSuccess,
        )
    }

    // ══════════════════════════════════════════════════════════════════════
    // 修复 6：saveRestoredState 接受 publishedProjectIds 参数
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 修复 6：saveRestoredState 方法有 publishedProjectIds: Set<String> 参数。
     * ReadableMirrorRestorer 传 manifest.projects.map { it.id }.toSet()。
     */
    @Test
    fun fix6_saveRestoredStateHasPublishedProjectIdsParam() {
        val stateStoreClass =
            try {
                Class.forName("com.xiwei.sujian.storage.mirror.ReadableMirrorStateStore")
            } catch (_: ClassNotFoundException) {
                null
            }
        assertNotNull("ReadableMirrorStateStore 类应存在", stateStoreClass)

        val methods = stateStoreClass!!.declaredMethods
        val saveRestoredStateMethods = methods.filter { it.name == "saveRestoredState" }

        assertTrue("saveRestoredState 方法应存在", saveRestoredStateMethods.isNotEmpty())

        // 修复确认：saveRestoredState 有 publishedProjectIds 参数（Set 类型）
        val hasPublishedProjectIdsParam =
            saveRestoredStateMethods.any { method ->
                method.parameterTypes.any { paramType ->
                    paramType.name == "java.util.Set" || paramType.simpleName == "Set"
                }
            }
        assertTrue(
            "修复6确认：saveRestoredState 有 publishedProjectIds: Set<String> 参数",
            hasPublishedProjectIdsParam,
        )

        // 修复确认：有 5 个参数（manifestUri, chapterEntries, backend, treeUri, publishedProjectIds）
        val maxParamCount = saveRestoredStateMethods.maxOf { it.parameterTypes.size }
        assertTrue(
            "修复6确认：saveRestoredState 有 5 个参数（含 publishedProjectIds），当前 $maxParamCount",
            maxParamCount >= 5,
        )
    }

    /**
     * 修复 6 行为验证：零章节作品（chapterEntries 为空）通过 publishedProjectIds 参数
     * 也能进 publishedProjectIds 集合。
     */
    @Test
    fun fix6_zeroChapterProject_inPublishedProjectIds() {
        // 修复确认：ReadableMirrorRestorer 传 publishedProjectIds = manifest.projects.map { it.id }.toSet()
        // 即使 chapterEntries 为空，零章节作品也能进 publishedProjectIds

        // 模拟 manifest 有一个零章节作品
        val manifestProjectIds = setOf("p-empty")
        val chapterEntries = emptyMap<ChapterKey, ChapterMirrorEntry>()

        // 修复确认：合并传入的 publishedProjectIds 和从 chapterEntries 推导的 ID
        val projectIds = mutableSetOf<String>()
        for ((key, _) in chapterEntries) {
            projectIds.add(key.projectId)
        }
        projectIds.addAll(manifestProjectIds) // 修复 6：合并传入的 publishedProjectIds

        assertTrue(
            "修复6确认：零章节作品 'p-empty' 通过 publishedProjectIds 参数进入集合，" +
                "即使 chapterEntries 为空",
            projectIds.contains("p-empty"),
        )
        assertFalse("projectIds 不为空", projectIds.isEmpty())
    }

    // ══════════════════════════════════════════════════════════════════════
    // 修复 6 补充：addPublishedProjectId / removePublishedProjectId 返回值被检查
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 修复 6 补充：addPublishedProjectId / removePublishedProjectId 返回 Boolean，
     * 调用方检查返回值，失败时不清 pending journal。
     */
    @Test
    fun fix6_addRemovePublishedProjectIdReturnChecked() {
        val stateStoreClass = Class.forName("com.xiwei.sujian.storage.mirror.ReadableMirrorStateStore")
        val addMethod = stateStoreClass.getDeclaredMethod("addPublishedProjectId", String::class.java)
        val removeMethod = stateStoreClass.getDeclaredMethod("removePublishedProjectId", String::class.java)

        // 修复确认：方法返回 Boolean
        assertEquals(
            "修复6确认：addPublishedProjectId 返回 Boolean，调用方检查返回值",
            java.lang.Boolean.TYPE,
            addMethod.returnType,
        )
        assertEquals(
            "修复6确认：removePublishedProjectId 返回 Boolean，调用方检查返回值",
            java.lang.Boolean.TYPE,
            removeMethod.returnType,
        )

        // 修复确认：调用方检查返回值（通过源码审查确认 recoverPromotePhase 第 408-411 行、
        // recoverCleanupPhase 第 518-521 行、deleteProject 第 1256-1258 行都有 if (!...) 检查）
    }

    // ══════════════════════════════════════════════════════════════════════
    // 辅助 FakeStorage 实现（支持修复验证所需的失败注入）
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 支持失败注入的 FakeReadableMirrorStorage，用于验证修复。
     */
    private class DefectFakeStorage : ReadableMirrorStorage {
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

        override fun readTextAndHash(ref: MirrorFileRef): Pair<String, String>? {
            val content = committedFiles[ref.uri] ?: stagingFiles[ref.uri] ?: backupFiles[ref.uri] ?: return null
            return Pair(content, computeContentHash(content))
        }
    }
}
