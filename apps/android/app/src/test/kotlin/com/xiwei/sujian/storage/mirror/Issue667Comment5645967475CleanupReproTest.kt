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
            queryHandler = { newMediaStoreCursorWithRow() },
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
            queryHandler = { newMediaStoreCursorWithRow() },
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
            queryHandler = { newEmptyMediaStoreCursor() },
            deleteHandler = { 1 },
        )
        registerSafProvider(
            queryHandler = { newSafChildrenCursor() },
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
            queryHandler = { newEmptyMediaStoreCursor() },
            deleteHandler = { 1 },
        )
        registerSafProvider(
            queryHandler = { newSafChildrenCursor() },
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
            queryHandler = { newMediaStoreCursorWithRow() },
            deleteHandler = { 1 },
        )
        val cleanup = MirrorStagingCleanup(context, context.contentResolver)

        cleanup.cleanupIfNeeded()
        assertTrue(
            "全部成功时应写 done 标志",
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
        queryHandler: (Uri) -> Cursor?,
        deleteHandler: (Uri) -> Int,
    ) {
        ShadowContentResolver.registerProviderInternal(
            MEDIA_HOST,
            FakeProvider(queryHandler, deleteHandler),
        )
    }

    /** 注册 SAF authority 的 FakeProvider。 */
    private fun registerSafProvider(
        queryHandler: (Uri) -> Cursor?,
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
     */
    private class FakeProvider(
        private val queryHandler: (Uri) -> Cursor?,
        private val deleteHandler: (Uri) -> Int,
    ) : ContentProvider() {
        override fun onCreate(): Boolean = true

        override fun query(
            uri: Uri,
            projection: Array<String?>?,
            selection: String?,
            selectionArgs: Array<String?>?,
            sortOrder: String?,
        ): Cursor? = queryHandler(uri)

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
}
