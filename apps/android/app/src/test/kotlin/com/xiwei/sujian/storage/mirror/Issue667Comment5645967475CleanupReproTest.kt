package com.xiwei.sujian.storage.mirror

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.MediaStore
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.core.platform.storage.AndroidPrivateDataRoot
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowContentResolver
import java.io.File

/**
 * Issue #667 评论 5645967475 回归测试。
 *
 * 目标：验证 [MirrorStagingCleanup] 内部两个清理函数在单条删除失败（抛异常或返回 0/false）
 * 时整体返回 false，从而外层 [MirrorStagingCleanup.cleanupIfNeeded] 不写
 * `.staging-cleanup-done` 标志，下次启动会再次清理，避免旧事务文件永久残留。
 *
 * ## 测试策略
 * 用自定义 [FakeProvider]（子类化 [ContentProvider]）控制 MediaStore 和 SAF 的查询/删除行为，
 * 通过 [ShadowContentResolver.registerProviderInternal] 注册到 Robolectric 的 ContentResolver，
 * 对四个失败分支做实际行为断言：
 * 1. MediaStore delete 抛 SecurityException → done 不写 + 下次仍清理。
 * 2. MediaStore delete 返回 0 → done 不写。
 * 3. SAF deleteDocument 返回 false（delete 返回 0）→ done 不写。
 * 4. SAF deleteDocument 抛 SecurityException → done 不写。
 * 另加对照测试确认全部成功时仍写 done 标志（避免过度修复破坏正常路径）。
 *
 * ## FakeProvider 拦截原理
 * - API 34 中 `ContentResolver.query`/`delete` 是 final，不能子类化重写。
 * - 但 Robolectric 的 [ShadowContentResolver] 的 query/delete 会根据 URI authority 查找注册的
 *   [ContentProvider] 并委托（见 ShadowContentResolver 字节码：getProvider → provider.query/delete）。
 * - `ContentProvider` 的 query/delete 不是 final，可自由实现。
 * - `DocumentsContract.deleteDocument(cr, uri)` 内部调用 `cr.delete(uri, null, null)`，
 *   经 shadow 委托给注册的 SAF provider：delete 返回 0 → deleteDocument 返回 false；
 *   delete 抛异常 → deleteDocument 抛异常。
 * - [ShadowContentResolver.reset] 在每个测试前清理 providers map，无跨测试污染。
 *
 * 相关源文件：MirrorStagingCleanup.kt。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue667Comment5645967475CleanupReproTest {
    private lateinit var context: Context
    private lateinit var cleanupFlagFile: File
    private lateinit var stateFile: File

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        cleanupFlagFile = File(AndroidPrivateDataRoot.mirror(context), CLEANUP_FLAG_NAME)
        stateFile = File(AndroidPrivateDataRoot.mirror(context), STATE_FILE_NAME)
        // 确保测试前 done 标志和 state.json 不存在（避免上个测试残留 tree URI）
        cleanupFlagFile.delete()
        stateFile.delete()
    }

    /**
     * 失败分支 1：MediaStore delete 抛 SecurityException 时不写 done 标志，下次调用仍会清理。
     *
     * 注册 media provider：query 返回 1 行 cursor（id=1），delete 抛 SecurityException。
     * 不设 tree URI（SAF 直接返回 true）。
     * 期望：cleanupViaMediaStore 返回 false → done 不写；再次调用 done 仍不写（未被跳过）。
     */
    @Test
    fun mediaStoreDeleteThrows_noDoneFlag_retriesNextTime() {
        registerMediaProvider(
            queryHandler = { _, _ -> newMediaStoreCursorWithRow() },
            deleteHandler = { throw SecurityException("FakeProvider: MediaStore delete denied") },
        )
        val cleanup = MirrorStagingCleanup(context, context.contentResolver)

        // 第一次调用：MediaStore delete 抛异常 → done 不写
        cleanup.cleanupIfNeeded()
        assertFalse(
            "MediaStore delete 抛异常时不应写 done 标志",
            cleanupFlagFile.exists(),
        )

        // 第二次调用：done 标志不存在，应再次清理（不跳过），仍失败 → done 仍不写
        cleanup.cleanupIfNeeded()
        assertFalse(
            "第二次调用后 done 标志仍不应存在（重试仍失败，未被跳过）",
            cleanupFlagFile.exists(),
        )
    }

    /**
     * 失败分支 2：MediaStore delete 返回 0 时不写 done 标志。
     *
     * 注册 media provider：query 返回 1 行 cursor（id=1），delete 返回 0。
     * 不设 tree URI。
     * 期望：cleanupViaMediaStore 返回 false → done 不写。
     */
    @Test
    fun mediaStoreDeleteReturnsZero_noDoneFlag() {
        registerMediaProvider(
            queryHandler = { _, _ -> newMediaStoreCursorWithRow() },
            deleteHandler = { 0 },
        )
        val cleanup = MirrorStagingCleanup(context, context.contentResolver)

        cleanup.cleanupIfNeeded()
        assertFalse(
            "MediaStore delete 返回 0 时不应写 done 标志",
            cleanupFlagFile.exists(),
        )
    }

    /**
     * 失败分支 3：SAF deleteDocument 返回 false（delete 返回 0）时不写 done 标志。
     *
     * 设 tree URI；注册 media provider（query 返回空 cursor，MediaStore 成功）和 SAF provider
     * （query 对 children 返回含 `.staging` 目录的 cursor，delete 返回 0）。
     * 期望：cleanupViaSaf 返回 false → done 不写。
     */
    @Test
    fun safDeleteReturnsFalse_noDoneFlag() {
        setTreeUri()
        registerMediaProvider(
            queryHandler = { _, _ -> newEmptyMediaStoreCursor() },
            deleteHandler = { 1 },
        )
        registerSafProvider(
            queryHandler = { _, _ -> newSafChildrenCursor() },
            deleteHandler = { 0 },
        )
        val cleanup = MirrorStagingCleanup(context, context.contentResolver)

        cleanup.cleanupIfNeeded()
        assertFalse(
            "SAF deleteDocument 返回 false 时不应写 done 标志",
            cleanupFlagFile.exists(),
        )
    }

    /**
     * 失败分支 4：SAF deleteDocument 抛 SecurityException 时不写 done 标志。
     *
     * 设 tree URI；注册 media provider（query 返回空 cursor）和 SAF provider
     * （query 同上，delete 抛 SecurityException）。
     * 期望：cleanupViaSaf 返回 false → done 不写。
     */
    @Test
    fun safDeleteThrows_noDoneFlag() {
        setTreeUri()
        registerMediaProvider(
            queryHandler = { _, _ -> newEmptyMediaStoreCursor() },
            deleteHandler = { 1 },
        )
        registerSafProvider(
            queryHandler = { _, _ -> newSafChildrenCursor() },
            deleteHandler = { throw SecurityException("FakeProvider: SAF delete denied") },
        )
        val cleanup = MirrorStagingCleanup(context, context.contentResolver)

        cleanup.cleanupIfNeeded()
        assertFalse(
            "SAF deleteDocument 抛异常时不应写 done 标志",
            cleanupFlagFile.exists(),
        )
    }

    /**
     * 对照测试：全部成功时仍写 done 标志（避免过度修复破坏正常路径）。
     *
     * 注册 media provider：query 返回 1 行 cursor（id=1），delete 返回 1（成功）。
     * 不设 tree URI（SAF 直接返回 true）。
     * 期望：done 标志写入。
     */
    @Test
    fun allSucceed_writesDoneFlag() {
        registerMediaProvider(
            queryHandler = { _, _ -> newMediaStoreCursorWithRow() },
            deleteHandler = { 1 },
        )
        val cleanup = MirrorStagingCleanup(context, context.contentResolver)

        cleanup.cleanupIfNeeded()
        assertTrue(
            "全部成功时应写 done 标志",
            cleanupFlagFile.exists(),
        )
    }

    /**
     * 边界 1：MediaStore query 返回 null 时不写 done 标志，且第二次调用真的再次查询（重试）。
     *
     * Issue #667 评论 5649934255：`ContentResolver.query()` 契约允许返回 nullable Cursor，
     * null 代表本次根本没有拿到可确认的查询结果。修复前 `cursor?.use { ... }` 在 null 时跳过
     * use 块，allDeleted 保持 true，函数返回 true，外层会写 done 标志。修复后 null 应返回 false。
     *
     * 注册 media provider：query 返回 null，delete 不会被调用。
     * 不设 tree URI（SAF 直接返回 true）。
     * 期望：cleanupViaMediaStore 返回 false → done 不写；第二次调用 done 仍不写，
     * 且 provider 的 query 调用次数继续增加（证明真的重试，不只是检查 flag）。
     */
    @Test
    fun mediaStoreQueryReturnsNull_noDoneFlag_retriesNextTime() {
        val provider =
            CountingFakeProvider(
                queryHandler = { _, _ -> null },
                deleteHandler = { 1 },
            )
        ShadowContentResolver.registerProviderInternal(MEDIA_HOST, provider)
        val cleanup = MirrorStagingCleanup(context, context.contentResolver)

        val queryCountBefore = provider.queryCount
        cleanup.cleanupIfNeeded()
        assertFalse("query 返回 null 时不应写 done 标志", cleanupFlagFile.exists())
        val queryCountAfterFirst = provider.queryCount
        assertTrue("第一次调用应触发 query", queryCountAfterFirst > queryCountBefore)

        cleanup.cleanupIfNeeded()
        assertFalse("第二次调用后 done 标志仍不应存在", cleanupFlagFile.exists())
        val queryCountAfterSecond = provider.queryCount
        assertTrue("第二次调用应再次触发 query（真重试，非仅检查 flag）", queryCountAfterSecond > queryCountAfterFirst)
    }

    /**
     * 边界 2：.staging/ 删除失败但 .backup/、_meta/ 成功时，三组 prefix 都实际执行，最终不写 done。
     *
     * Issue #667 评论 5649934255：修复前 `LEGACY_DIR_PREFIXES.all { ... }` 在第一个失败前缀
     * （.staging/）处短路，.backup/ 和 _meta/ 本轮根本不会被调用。修复后三前缀各自尝试，
     * 不短路。本测试验证三个前缀都被查询了，且最终不写 done（因 .staging/ 失败）。
     *
     * 注册 media provider：对 .staging/ 前缀的 query 返回 1 行 cursor 且 delete 返回 0（失败），
     * 对 .backup/ 和 _meta/ 前缀的 query 返回空 cursor（成功，无数据）。
     * 不设 tree URI（SAF 直接返回 true）。
     * 期望：三组 prefix 都实际执行了 query（不短路）；cleanupViaMediaStore 返回 false → done 不写。
     */
    @Test
    fun stagingFailsButBackupAndMetaAttempted_noShortCircuit() {
        val queriedPrefixes = mutableListOf<String>()
        val provider =
            CountingFakeProvider(
                queryHandler = { _, selectionArgs ->
                    val prefix = selectionArgs?.firstOrNull() ?: ""
                    queriedPrefixes.add(prefix)
                    if (prefix.contains(".staging/")) {
                        newMediaStoreCursorWithRow()
                    } else {
                        newEmptyMediaStoreCursor()
                    }
                },
                deleteHandler = { 0 },
            )
        ShadowContentResolver.registerProviderInternal(MEDIA_HOST, provider)
        val cleanup = MirrorStagingCleanup(context, context.contentResolver)

        cleanup.cleanupIfNeeded()
        assertFalse(".staging/ 删除失败时不应写 done 标志", cleanupFlagFile.exists())
        assertTrue("三个前缀都应被查询（不短路）", queriedPrefixes.any { it.contains(".staging/") })
        assertTrue(".backup/ 前缀应被查询", queriedPrefixes.any { it.contains(".backup/") })
        assertTrue("_meta/ 前缀应被查询", queriedPrefixes.any { it.contains("_meta/") })
    }

    /**
     * 边界 3：_meta/ 前缀的 LIKE 查询转义下划线，不会误匹配 ameta/ 等非事务目录。
     *
     * Issue #667 评论 5650127639：SQLite LIKE 中 `_` 是"任意单个字符"通配符，
     * `Download/Sujian/_meta/%` 会误匹配 `ameta/`。修复后用 `ESCAPE '!'` 转义，
     * `_` 按字面量匹配。本测试用 RecordingFakeProvider 模拟 MediaStore 记录，
     * 内含真正的 `_meta/manifest.json`（id=1）和非目标 `ameta/notes.md`（id=2），
     * 验证：
     * 1. _meta/ 那一组 query 的 selection 包含 `ESCAPE '!'`。
     * 2. _meta/ 那一组 query 的 selectionArgs 是 `Download/Sujian/!_meta/%`。
     * 3. ameta/ 记录（id=2）不会进入删除路径——delete 从未被以 id=2 的 URI 调用。
     * 4. _meta/ 记录（id=1）会被删除——delete 以 id=1 的 URI 调用。
     *
     * 这不是只断言 helper 返回字符串，而是锁住"不会误删非事务目录"这个实际行为：
     * RecordingFakeProvider 真正按 LIKE + ESCAPE 语义过滤记录，ameta/ 不匹配
     * 转义后的 `!_meta/` 模式，所以不会出现在 cursor 中，不会进入删除路径。
     */
    @Test
    fun metaPrefixEscaped_doesNotMatchAmetaDir() {
        val provider = RecordingFakeProvider()
        ShadowContentResolver.registerProviderInternal(MEDIA_HOST, provider)
        val cleanup = MirrorStagingCleanup(context, context.contentResolver)

        cleanup.cleanupIfNeeded()

        // 断言 1：_meta/ 查询使用了 ESCAPE '!'
        val metaQuery =
            provider.queryCalls.find { (_, args) ->
                args?.firstOrNull()?.contains("_meta/") == true
            }
        assertTrue(
            "_meta/ 前缀的查询应使用 ESCAPE '!'",
            metaQuery != null && metaQuery.first.contains("ESCAPE '!'"),
        )

        // 断言 2：_meta/ 查询的参数是转义后的 Download/Sujian/!_meta/%
        val metaArgs = metaQuery!!.second?.firstOrNull()
        assertTrue(
            "_meta/ 前缀的查询参数应是 Download/Sujian/!_meta/%，实际: $metaArgs",
            metaArgs == "Download/Sujian/!_meta/%",
        )

        // 断言 3：ameta/ 记录（id=2）不会被删除
        val ametaUri = Uri.withAppendedPath(MediaStore.Downloads.EXTERNAL_CONTENT_URI, "2")
        assertFalse(
            "ameta/ 记录（id=2）不应进入删除路径",
            provider.deletedUris.any { it == ametaUri },
        )

        // 断言 4：_meta/ 记录（id=1）会被删除
        val metaUri = Uri.withAppendedPath(MediaStore.Downloads.EXTERNAL_CONTENT_URI, "1")
        assertTrue(
            "_meta/ 记录（id=1）应被删除",
            provider.deletedUris.any { it == metaUri },
        )
    }

    /**
     * Issue #667 评论 5650324333：MediaStore 文件清理后，残留的空事务目录应被清理，done 标志写入。
     *
     * 在 Download/Sujian/ 下创建 .staging/tx-1/、.backup/tx-1/、_meta/ 空目录（模拟 MediaProvider
     * 删除文件后残留的空父目录）。注册 MediaStore provider（query 返回空 cursor，delete 返回 1），
     * 不设 tree URI（SAF 直接返回 true）。期望：cleanupIfNeeded() 执行后三个目录都不存在，done 写入。
     */
    @Test
    fun emptyDirsCleaned_upAfterMediaStoreCleanup() {
        val downloadsDir = android.os.Environment.getExternalStoragePublicDirectory(android.os.Environment.DIRECTORY_DOWNLOADS)
        val sujianDir = File(downloadsDir, "Sujian")
        // 创建空事务目录结构（含子目录 tx-1）
        val stagingTxDir = File(sujianDir, ".staging/tx-1").apply { mkdirs() }
        val backupTxDir = File(sujianDir, ".backup/tx-1").apply { mkdirs() }
        val metaDir = File(sujianDir, "_meta").apply { mkdirs() }

        assertTrue("测试前置：.staging/tx-1 应已创建", stagingTxDir.exists())
        assertTrue("测试前置：.backup/tx-1 应已创建", backupTxDir.exists())
        assertTrue("测试前置：_meta 应已创建", metaDir.exists())

        registerMediaProvider(
            queryHandler = { _, _ -> newEmptyMediaStoreCursor() },
            deleteHandler = { 1 },
        )
        val cleanup = MirrorStagingCleanup(context, context.contentResolver)

        cleanup.cleanupIfNeeded()

        // 三个旧事务目录都应被清理（不存在）
        assertFalse(
            ".staging 目录应已被清理",
            File(sujianDir, ".staging").exists(),
        )
        assertFalse(
            ".backup 目录应已被清理",
            File(sujianDir, ".backup").exists(),
        )
        assertFalse(
            "_meta 目录应已被清理",
            File(sujianDir, "_meta").exists(),
        )
        // done 标志应写入（三步骤都成功）
        assertTrue(
            "空目录清理成功后应写 done 标志",
            cleanupFlagFile.exists(),
        )
    }

    /**
     * Issue #667 评论 5650324333：残留普通文件阻止空目录删除，done 标志不写入。
     *
     * 在 .staging/tx-1/ 下放一个残留普通文件 leftover.tmp。注册 MediaStore provider（query 返回空
     * cursor，delete 返回 1），不设 tree URI。期望：cleanupIfNeeded() 执行后残留文件不被递归删除，
     * .staging/tx-1/ 和 .staging/ 仍存在，done 不写入（因 emptyDirsOk = false）。
     */
    @Test
    fun residualFilePreventsDirDelete_noDoneFlag() {
        val downloadsDir = android.os.Environment.getExternalStoragePublicDirectory(android.os.Environment.DIRECTORY_DOWNLOADS)
        val sujianDir = File(downloadsDir, "Sujian")
        // 创建 .staging/tx-1/ 目录并放入残留文件
        val stagingTxDir = File(sujianDir, ".staging/tx-1").apply { mkdirs() }
        val leftoverFile = File(stagingTxDir, "leftover.tmp").apply { writeText("residual") }

        assertTrue("测试前置：残留文件应已创建", leftoverFile.exists())

        registerMediaProvider(
            queryHandler = { _, _ -> newEmptyMediaStoreCursor() },
            deleteHandler = { 1 },
        )
        val cleanup = MirrorStagingCleanup(context, context.contentResolver)

        cleanup.cleanupIfNeeded()

        // 残留文件不应被递归删除
        assertTrue(
            "残留普通文件不应被递归删除",
            leftoverFile.exists(),
        )
        // .staging/tx-1/ 因含文件不能删空，仍存在
        assertTrue(
            ".staging/tx-1 目录因含残留文件应仍存在",
            stagingTxDir.exists(),
        )
        // .staging/ 因子目录未删空，仍存在
        assertTrue(
            ".staging 目录因子目录未删空应仍存在",
            File(sujianDir, ".staging").exists(),
        )
        // done 标志不应写入（emptyDirsOk = false）
        assertFalse(
            "空目录清理失败时不应写 done 标志",
            cleanupFlagFile.exists(),
        )
    }

    // ── 辅助方法 ──

    /** 持久化 SAF tree URI，让 cleanupViaSaf 能读到。 */
    private fun setTreeUri() {
        ReadableMirrorStateStore(context).setTreeUri(SAF_TREE_URI_STRING)
    }

    /** 注册 media authority 的 FakeProvider。 */
    private fun registerMediaProvider(
        queryHandler: (Uri, Array<String?>?) -> Cursor?,
        deleteHandler: (Uri) -> Int,
    ) {
        ShadowContentResolver.registerProviderInternal(
            MEDIA_HOST,
            FakeProvider(queryHandler, deleteHandler),
        )
    }

    /** 注册 SAF authority 的 FakeProvider。 */
    private fun registerSafProvider(
        queryHandler: (Uri, Array<String?>?) -> Cursor?,
        deleteHandler: (Uri) -> Int,
    ) {
        ShadowContentResolver.registerProviderInternal(
            SAF_HOST,
            FakeProvider(queryHandler, deleteHandler),
        )
    }

    /** MediaStore query cursor：1 行，_id=1。每次返回新实例（避免 cursor 关闭后重用）。 */
    private fun newMediaStoreCursorWithRow(): MatrixCursor =
        MatrixCursor(arrayOf(MediaStore.Downloads._ID)).apply {
            addRow(arrayOf<Any?>(1L))
        }

    /** MediaStore query 空 cursor（无数据）。每次返回新实例。 */
    private fun newEmptyMediaStoreCursor(): MatrixCursor = MatrixCursor(arrayOf(MediaStore.Downloads._ID))

    /** SAF children query cursor：1 行 `.staging` 目录。每次返回新实例。 */
    private fun newSafChildrenCursor(): MatrixCursor {
        val cursor =
            MatrixCursor(
                arrayOf(
                    DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                    DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                    DocumentsContract.Document.COLUMN_MIME_TYPE,
                ),
            )
        cursor.addRow(
            arrayOf<Any?>(
                "primary:Download/Sujian/.staging",
                ".staging",
                DocumentsContract.Document.MIME_TYPE_DIR,
            ),
        )
        return cursor
    }

    companion object {
        private const val CLEANUP_FLAG_NAME = ".staging-cleanup-done"
        private const val STATE_FILE_NAME = "state.json"
        private const val MEDIA_HOST = "media"
        private const val SAF_HOST = "com.android.externalstorage.documents"
        private const val SAF_TREE_URI_STRING =
            "content://com.android.externalstorage.documents/tree/primary%3ADownload%2FSujian"
    }

    /**
     * FakeProvider — 子类化 [ContentProvider] 控制 query/delete 行为。
     *
     * API 34 中 [android.content.ContentResolver] 的 query/delete 是 final 无法子类化，
     * 但 Robolectric 的 [ShadowContentResolver] 会把 query/delete 委托给按 authority 注册的
     * [ContentProvider]，而 [ContentProvider] 的 query/delete 不是 final 可自由实现。
     *
     * [queryHandler] 和 [deleteHandler] 是 lambda，内部可抛异常模拟失败分支。
     *
     * Issue #667 评论 5649934255：[queryHandler] 签名改为 `(Uri, Array<String?>?) -> Cursor?`，
     * 第二个参数是 query 的 `selectionArgs`，用于按前缀区分行为（三个前缀的 query URI 都是同一个
     * `MediaStore.Downloads.EXTERNAL_CONTENT_URI`，无法按 URI 区分）。
     */
    private class FakeProvider(
        private val queryHandler: (Uri, Array<String?>?) -> Cursor?,
        private val deleteHandler: (Uri) -> Int,
    ) : ContentProvider() {
        override fun onCreate(): Boolean = true

        override fun query(
            uri: Uri,
            projection: Array<String?>?,
            selection: String?,
            selectionArgs: Array<String?>?,
            sortOrder: String?,
        ): Cursor? = queryHandler(uri, selectionArgs)

        override fun delete(
            uri: Uri,
            selection: String?,
            selectionArgs: Array<String?>?,
        ): Int = deleteHandler(uri)

        override fun insert(
            uri: Uri,
            values: ContentValues?,
        ): Uri? = null

        override fun update(
            uri: Uri,
            values: ContentValues?,
            selection: String?,
            selectionArgs: Array<String?>?,
        ): Int = 0

        override fun getType(uri: Uri): String? = null
    }

    /**
     * 带计数器的 FakeProvider，用于验证 query 是否真的被调用（重试验证）。
     *
     * Issue #667 评论 5649934255：用于边界 1（query 返回 null 重试）和边界 2（不短路）测试，
     * 通过 [queryCount] 计数器确认 query 真的被调用，而非仅检查 done flag。
     */
    private class CountingFakeProvider(
        private val queryHandler: (Uri, Array<String?>?) -> Cursor?,
        private val deleteHandler: (Uri) -> Int,
    ) : ContentProvider() {
        var queryCount: Int = 0
            private set

        override fun onCreate(): Boolean = true

        override fun query(
            uri: Uri,
            projection: Array<String?>?,
            selection: String?,
            selectionArgs: Array<String?>?,
            sortOrder: String?,
        ): Cursor? {
            queryCount++
            return queryHandler(uri, selectionArgs)
        }

        override fun delete(
            uri: Uri,
            selection: String?,
            selectionArgs: Array<String?>?,
        ): Int = deleteHandler(uri)

        override fun insert(
            uri: Uri,
            values: ContentValues?,
        ): Uri? = null

        override fun update(
            uri: Uri,
            values: ContentValues?,
            selection: String?,
            selectionArgs: Array<String?>?,
        ): Int = 0

        override fun getType(uri: Uri): String? = null
    }

    /**
     * RecordingFakeProvider — 记录 query/delete 调用并模拟 LIKE + ESCAPE 过滤。
     *
     * Issue #667 评论 5650127639：用于验证 _meta/ 前缀的 LIKE 查询转义后不会误匹配
     * ameta/ 等非事务目录。内部维护记录列表，query 时按 LIKE + ESCAPE 语义过滤，
     * 真正模拟 SQLite 的匹配行为。
     */
    private class RecordingFakeProvider : ContentProvider() {
        /** 内部 MediaStore 记录：_ID -> RELATIVE_PATH。 */
        private val records =
            listOf(
                1L to "Download/Sujian/_meta/manifest.json",
                2L to "Download/Sujian/ameta/notes.md",
            )

        /** 记录每次 query 调用的 (selection, selectionArgs)。 */
        val queryCalls = mutableListOf<Pair<String, Array<String?>?>>()

        /** 记录每次 delete 调用的 URI。 */
        val deletedUris = mutableListOf<Uri>()

        override fun onCreate(): Boolean = true

        override fun query(
            uri: Uri,
            projection: Array<String?>?,
            selection: String?,
            selectionArgs: Array<String?>?,
            sortOrder: String?,
        ): Cursor? {
            queryCalls.add((selection ?: "") to selectionArgs)

            // 从 selection 判断是否有 ESCAPE 子句，解析 ESCAPE 字符
            val escapeChar = parseEscapeChar(selection)
            // selectionArgs[0] 是 LIKE 模式（如 "Download/Sujian/!_meta/%"）
            val pattern = selectionArgs?.firstOrNull() ?: return newEmptyCursor()

            val cursor = MatrixCursor(arrayOf(MediaStore.Downloads._ID))
            for ((id, relativePath) in records) {
                if (likeMatches(pattern, relativePath, escapeChar)) {
                    cursor.addRow(arrayOf<Any?>(id))
                }
            }
            return cursor
        }

        override fun delete(
            uri: Uri,
            selection: String?,
            selectionArgs: Array<String?>?,
        ): Int {
            deletedUris.add(uri)
            return 1
        }

        override fun insert(
            uri: Uri,
            values: ContentValues?,
        ): Uri? = null

        override fun update(
            uri: Uri,
            values: ContentValues?,
            selection: String?,
            selectionArgs: Array<String?>?,
        ): Int = 0

        override fun getType(uri: Uri): String? = null

        /** 从 selection 中解析 ESCAPE 字符，如 "RELATIVE_PATH LIKE ? ESCAPE '!'" 返回 '!'。 */
        private fun parseEscapeChar(selection: String?): Char? {
            if (selection == null) return null
            val regex = Regex("ESCAPE\\s+'(.)'")
            val match = regex.find(selection)
            return match?.groupValues?.get(1)?.firstOrNull()
        }

        /**
         * 模拟 SQL LIKE + ESCAPE 匹配。
         *
         * 将 LIKE 模式转为正则：`%` -> `.*`，`_` -> `.`（任意单字符），
         * 被 ESCAPE 字符前缀的字符按字面量匹配。无 ESCAPE 字符时 `%` 和 `_` 都是通配符。
         */
        private fun likeMatches(
            pattern: String,
            value: String,
            escape: Char?,
        ): Boolean {
            val regex = StringBuilder()
            var i = 0
            while (i < pattern.length) {
                val c = pattern[i]
                if (escape != null && c == escape && i + 1 < pattern.length) {
                    // 转义序列：下一个字符按字面量匹配
                    regex.append(Regex.escape(pattern[i + 1].toString()))
                    i += 2
                } else if (c == '%') {
                    regex.append(".*")
                    i++
                } else if (c == '_') {
                    regex.append(".")
                    i++
                } else {
                    regex.append(Regex.escape(c.toString()))
                    i++
                }
            }
            return Regex("^$regex$", RegexOption.DOT_MATCHES_ALL).matches(value)
        }

        private fun newEmptyCursor(): MatrixCursor = MatrixCursor(arrayOf(MediaStore.Downloads._ID))
    }
}
