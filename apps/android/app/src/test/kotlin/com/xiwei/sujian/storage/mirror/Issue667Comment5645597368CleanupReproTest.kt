package com.xiwei.sujian.storage.mirror

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.provider.MediaStore
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.core.platform.storage.AndroidPrivateDataRoot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowContentResolver
import java.io.File

/**
 * Issue #667 评论 5645597368 问题 2 回归测试。
 *
 * 目标：验证 [MirrorStagingCleanup.cleanupIfNeeded] 只在清理成功时写 `.staging-cleanup-done` 标志。
 *
 * ## 回归策略
 * 1. 反射验证 `cleanupViaMediaStore()` 和 `cleanupViaSaf()` 返回 `Boolean`，
 *    外层可据此判断清理是否成功。
 * 2. 行为测试：调用 `cleanupIfNeeded()`，在 Robolectric 环境中清理无异常时应写 done 标志。
 *
 * 修复后这些测试断言期望行为（返回 Boolean、成功时写 done 标志、done 标志存在时跳过），证明 bug 已修复。
 *
 * 相关源文件：MirrorStagingCleanup.kt 第 57-82 行。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue667Comment5645597368CleanupReproTest {
    private lateinit var context: Context
    private lateinit var cleanup: MirrorStagingCleanup
    private lateinit var cleanupFlagFile: File

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        cleanup = MirrorStagingCleanup(context, context.contentResolver)
        cleanupFlagFile = File(AndroidPrivateDataRoot.mirror(context), ".staging-cleanup-done")
        // 确保测试前 done 标志不存在
        cleanupFlagFile.delete()
    }

    /**
     * 问题 2.A 回归：cleanupViaMediaStore 和 cleanupViaSaf 返回 Boolean。
     *
     * 修复后行为：清理函数返回 Boolean，外层（cleanupIfNeeded）可据此判断是否应该写 done 标志。
     */
    @Test
    fun problem2A_cleanupFunctions_returnBoolean() {
        val cleanupViaMediaStoreMethod = MirrorStagingCleanup::class.java.getDeclaredMethod("cleanupViaMediaStore")
        val cleanupViaSafMethod = MirrorStagingCleanup::class.java.getDeclaredMethod("cleanupViaSaf")

        // 断言返回类型是 Boolean（修复后）
        assertEquals(
            "问题2.A 回归：cleanupViaMediaStore 返回 Boolean，外层可判断 MediaStore 清理是否成功。",
            java.lang.Boolean.TYPE,
            cleanupViaMediaStoreMethod.returnType,
        )
        assertEquals(
            "问题2.A 回归：cleanupViaSaf 返回 Boolean，外层可判断 SAF 清理是否成功。",
            java.lang.Boolean.TYPE,
            cleanupViaSafMethod.returnType,
        )
    }

    /**
     * 问题 2.B 回归：cleanupIfNeeded 在清理成功时写 done 标志。
     *
     * 修复后行为：cleanupIfNeeded() 调用 cleanupViaMediaStore()、cleanupEmptyLegacyDirs()
     * 和 cleanupViaSaf()，它们都返回 Boolean。在 Robolectric 测试环境中，注册空的 MediaStore
     * provider（query 返回空 cursor，无数据），SAF 无 tree URI（直接返回 true），
     * 空目录不存在（直接返回 true），所以三者都返回 true → 写 done 标志。
     *
     * Issue #667 评论 5649934255：query 返回 null 视为失败返回 false。因此本测试需要
     * 注册空的 MediaStore provider（query 返回空 cursor 而非 null），才能让 cleanupViaMediaStore()
     * 返回 true。
     *
     * 这是正确行为：清理成功时写 done 标志，下次跳过。
     */
    @Test
    fun problem2B_cleanupIfNeeded_writesDoneFlag_whenCleanupSucceeds() {
        // 调用前 done 标志不存在
        assertTrue("测试前 done 标志应不存在", !cleanupFlagFile.exists())

        // 注册空的 MediaStore provider（query 返回空 cursor，无数据）
        registerEmptyMediaProvider()

        // 调用 cleanupIfNeeded()
        cleanup.cleanupIfNeeded()

        // 断言：在 Robolectric 环境中清理成功（无异常）时写 done 标志
        assertTrue(
            "问题2.B 回归：cleanupIfNeeded() 在清理成功时写 done 标志文件。",
            cleanupFlagFile.exists(),
        )
    }

    /**
     * 问题 2.C 回归：cleanupIfNeeded 在 done 标志存在时跳过（正确行为）。
     *
     * 修复后行为：done 标志存在意味着清理已成功完成，第二次调用直接跳过。
     * 这是正确行为：避免重复清理。
     *
     * Issue #667 评论 5649934255：query 返回 null 视为失败返回 false。因此本测试需要
     * 注册空的 MediaStore provider（query 返回空 cursor 而非 null），才能让 cleanupViaMediaStore()
     * 返回 true，第一次调用才能写 done 标志。
     */
    @Test
    fun problem2C_cleanupIfNeeded_skipsAfterDoneFlag() {
        // 注册空的 MediaStore provider（query 返回空 cursor，无数据）
        registerEmptyMediaProvider()

        // 第一次调用：清理成功，写 done 标志
        cleanup.cleanupIfNeeded()
        assertTrue("第一次调用后 done 标志应存在", cleanupFlagFile.exists())

        // 记录第一次调用后的状态
        val firstFlagContent = cleanupFlagFile.readText()

        // 第二次调用：done 标志已存在，直接跳过
        cleanup.cleanupIfNeeded()

        // done 标志仍存在，内容不变（跳过清理）
        assertTrue(
            "问题2.C 回归：done 标志写入后，第二次调用 cleanupIfNeeded() 直接跳过。",
            cleanupFlagFile.exists(),
        )
        assertEquals(
            "done 标志内容不变（第二次调用跳过，未重新写入）",
            firstFlagContent,
            cleanupFlagFile.readText(),
        )
    }

    // ── 辅助方法 ──

    /**
     * 注册空的 MediaStore provider（query 返回空 cursor，无数据）。
     *
     * Issue #667 评论 5649934255：query 返回 null 视为失败返回 false。为了让
     * cleanupViaMediaStore() 返回 true，需要注册 provider 使 query 返回空 cursor 而非 null。
     */
    private fun registerEmptyMediaProvider() {
        ShadowContentResolver.registerProviderInternal(
            "media",
            EmptyMediaProvider(),
        )
    }

    /**
     * 空 MediaStore provider：query 返回空 cursor（无数据），delete 返回 0。
     */
    private class EmptyMediaProvider : ContentProvider() {
        override fun onCreate(): Boolean = true

        override fun query(
            uri: Uri,
            projection: Array<String?>?,
            selection: String?,
            selectionArgs: Array<String?>?,
            sortOrder: String?,
        ): Cursor? = MatrixCursor(arrayOf(MediaStore.Downloads._ID))

        override fun delete(
            uri: Uri,
            selection: String?,
            selectionArgs: Array<String?>?,
        ): Int = 0

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
