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
 * Issue #667 评论 5650324333 Type 4 对抗性测试（Fix-Induced Vulnerability）。
 *
 * 目标：验证补丁引入的 `cleanupEmptyLegacyDirs()` 和 `deleteEmptyDirRecursively()` 新逻辑
 * 不会产生 fix-induced vulnerability。覆盖三个攻击向量：
 *
 * 1. **数学边界（递归深度）**：`deleteEmptyDirRecursively` 是递归实现无深度限制。
 *    测试深层嵌套空目录能正确自底向上递归删除。
 * 2. **上下文盲目性（残留文件保护）**：深层嵌套目录中含残留文件时，不删除文件，
 *    整个目录树保留，done 不写入。确保补丁不会为清理空目录而递归删除文件。
 * 3. **不短路语义（部分清理）**：部分目录清理失败时，其他目录仍被清理，
 *    但 done 不写入。确保补丁的不短路语义在空目录清理中正确实现。
 *
 * 相关源文件：MirrorStagingCleanup.kt（cleanupEmptyLegacyDirs, deleteEmptyDirRecursively）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue667Comment5650324333Type4AdversarialTest {
    private companion object {
        const val SUJIAN_DIR_NAME = "Sujian"
        const val STAGING_DIR = ".staging"
        const val BACKUP_DIR = ".backup"
        const val META_DIR = "_meta"
        const val STAGING_TX1 = ".staging/tx-1"
        const val BACKUP_TX1 = ".backup/tx-1"
        const val LEFTOVER_FILE = "leftover.tmp"
        const val RESIDUAL_CONTENT = "residual"
    }

    private lateinit var context: Context
    private lateinit var cleanupFlagFile: File

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        cleanupFlagFile = File(AndroidPrivateDataRoot.mirror(context), ".staging-cleanup-done")
        // 确保测试前 done 标志不存在
        cleanupFlagFile.delete()
    }

    /**
     * 对抗性测试 1（数学边界-递归深度）：
     * 验证 deleteEmptyDirRecursively 对深层嵌套空目录（5 层）能正确自底向上递归删除。
     *
     * 补丁的递归实现无深度限制。创建 .staging/tx-1/a/b/c/d/e/ 6 层嵌套空目录，
     * 验证全部被删除，.staging 不存在，done 写入。
     */
    @Test
    fun deepNestedEmptyDirsCleaned() {
        val downloadsDir =
            android.os.Environment.getExternalStoragePublicDirectory(
                android.os.Environment.DIRECTORY_DOWNLOADS,
            )
        val sujianDir = File(downloadsDir, SUJIAN_DIR_NAME)
        // 创建 6 层嵌套空目录
        val deepDir = File(sujianDir, "$STAGING_TX1/a/b/c/d/e").apply { mkdirs() }
        val backupDir = File(sujianDir, BACKUP_TX1).apply { mkdirs() }
        val metaDir = File(sujianDir, META_DIR).apply { mkdirs() }

        assertTrue("测试前置：深层嵌套目录应已创建", deepDir.exists())
        assertTrue("测试前置：.backup/tx-1 应已创建", backupDir.exists())
        assertTrue("测试前置：_meta 应已创建", metaDir.exists())

        registerEmptyMediaProvider()
        val cleanup = MirrorStagingCleanup(context, context.contentResolver)

        cleanup.cleanupIfNeeded()

        // 所有嵌套空目录都应被自底向上递归删除
        assertFalse(
            ".staging 目录应已被递归删除",
            File(sujianDir, STAGING_DIR).exists(),
        )
        assertFalse(
            ".backup 目录应已被删除",
            File(sujianDir, BACKUP_DIR).exists(),
        )
        assertFalse(
            "_meta 目录应已被删除",
            File(sujianDir, META_DIR).exists(),
        )
        assertTrue(
            "深层嵌套空目录清理成功后应写 done 标志",
            cleanupFlagFile.exists(),
        )
    }

    /**
     * 对抗性测试 2（上下文盲目性-残留文件保护）：
     * 在深层嵌套目录 .staging/tx-1/a/b/ 下放残留文件，验证 deleteEmptyDirRecursively
     * 不删除文件，整个 .staging 树保留，done 不写入。
     *
     * 确保补丁不会为清理空目录而递归删除文件（不用 deleteRecursively()）。
     */
    @Test
    fun fileInNestedDirPreventsAllDelete() {
        val downloadsDir =
            android.os.Environment.getExternalStoragePublicDirectory(
                android.os.Environment.DIRECTORY_DOWNLOADS,
            )
        val sujianDir = File(downloadsDir, SUJIAN_DIR_NAME)
        // 创建深层嵌套目录并放入残留文件
        val nestedDir = File(sujianDir, "$STAGING_TX1/a/b").apply { mkdirs() }
        val leftoverFile = File(nestedDir, LEFTOVER_FILE).apply { writeText(RESIDUAL_CONTENT) }

        assertTrue("测试前置：残留文件应已创建", leftoverFile.exists())

        registerEmptyMediaProvider()
        val cleanup = MirrorStagingCleanup(context, context.contentResolver)

        cleanup.cleanupIfNeeded()

        // 拋留文件不应被递归删除
        assertTrue(
            "深层嵌套目录中的残留文件不应被递归删除",
            leftoverFile.exists(),
        )
        // .staging/tx-1/a/b/ 因含文件不能删空，仍存在
        assertTrue(
            ".staging/tx-1/a/b 目录因含残留文件应仍存在",
            nestedDir.exists(),
        )
        // .staging 因子目录未删空，仍存在
        assertTrue(
            ".staging 目录因子目录未删空应仍存在",
            File(sujianDir, STAGING_DIR).exists(),
        )
        // .staging/tx-1/a/b/ 因含文件不能删空，仍存在
        assertTrue(
            ".staging/tx-1/a/b 目录因含残留文件应仍存在",
            nestedDir.exists(),
        )
        // .staging 因子目录未删空，仍存在
        assertTrue(
            ".staging 目录因子目录未删空应仍存在",
            File(sujianDir, ".staging").exists(),
        )
        // done 标志不应写入（emptyDirsOk = false）
        assertFalse(
            "含残留文件时不应写 done 标志",
            cleanupFlagFile.exists(),
        )
    }

    /**
     * 对抗性测试 3（不短路语义-部分清理）：
     * .staging 含残留文件（清理失败），.backup 和 _meta 为空目录（清理成功）。
     * 验证 cleanupEmptyLegacyDirs 不短路：.backup 和 _meta 被清理，.staging 保留，
     * done 不写入（因 .staging 失败）。
     *
     * 确保补丁的不短路语义在空目录清理中正确实现：一个目录失败不阻止其他目录清理。
     */
    @Test
    fun partialDirsCleanedOthersFail() {
        val downloadsDir =
            android.os.Environment.getExternalStoragePublicDirectory(
                android.os.Environment.DIRECTORY_DOWNLOADS,
            )
        val sujianDir = File(downloadsDir, "Sujian")
        // .staging 含残留文件（清理失败）
        val stagingDir = File(sujianDir, ".staging/tx-1").apply { mkdirs() }
        val leftoverFile = File(stagingDir, "leftover.tmp").apply { writeText("residual") }
        // .backup 和 _meta 为空目录（清理成功）
        val backupDir = File(sujianDir, ".backup/tx-1").apply { mkdirs() }
        val metaDir = File(sujianDir, "_meta").apply { mkdirs() }

        assertTrue("测试前置：残留文件应已创建", leftoverFile.exists())
        assertTrue("测试前置：.backup/tx-1 应已创建", backupDir.exists())
        assertTrue("测试前置：_meta 应已创建", metaDir.exists())

        registerEmptyMediaProvider()
        val cleanup = MirrorStagingCleanup(context, context.contentResolver)

        cleanup.cleanupIfNeeded()

        // .staging 因含残留文件保留
        assertTrue(
            ".staging 目录因含残留文件应仍存在",
            File(sujianDir, ".staging").exists(),
        )
        assertTrue(
            "残留文件不应被删除",
            leftoverFile.exists(),
        )
        // .backup 和 _meta 被清理（不短路）
        assertFalse(
            ".backup 目录应已被清理（不短路，即使 .staging 失败）",
            File(sujianDir, ".backup").exists(),
        )
        assertFalse(
            "_meta 目录应已被清理（不短路，即使 .staging 失败）",
            File(sujianDir, "_meta").exists(),
        )
        // done 不写入（因 .staging 失败，emptyDirsOk = false）
        assertFalse(
            "部分目录清理失败时不应写 done 标志",
            cleanupFlagFile.exists(),
        )
    }

    // ── 辅助方法 ──

    /**
     * 注册空的 MediaStore provider（query 返回空 cursor，无数据，delete 返回 1）。
     *
     * 让 cleanupViaMediaStore() 返回 true（query 返回空 cursor 而非 null）。
     */
    private fun registerEmptyMediaProvider() {
        ShadowContentResolver.registerProviderInternal(
            "media",
            EmptyMediaProvider(),
        )
    }

    /**
     * 空 MediaStore provider：query 返回空 cursor（无数据），delete 返回 1。
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
        ): Int = 1

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
