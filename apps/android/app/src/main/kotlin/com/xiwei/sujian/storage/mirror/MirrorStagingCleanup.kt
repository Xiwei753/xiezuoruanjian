package com.xiwei.sujian.storage.mirror

import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import android.os.Build
import android.provider.DocumentsContract
import android.provider.MediaStore
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.platform.storage.AndroidPrivateDataRoot
import com.xiwei.sujian.core.platform.storage.documents.DocumentTreeReader
import java.io.File

/**
 * MirrorStagingCleanup — 升级后清理旧版事务产物。
 *
 * Issue #667：旧版镜像发布系统把事务中间文件（staging、backup、manifest）
 * 写到公开的 `Download/Sujian/` 目录下的隐藏子目录：
 * - `Download/Sujian/.staging/<txId>/...` — 暂存正文
 * - `Download/Sujian/.backup/<txId>/...` — 旧正文备份
 * - `Download/Sujian/_meta/manifest.json` — manifest 文件
 *
 * 新版已将这些中间文件移到应用私有目录 `filesDir/sujian/mirror/`（通过
 * [MirrorTransactionWorkspace]），`Download/Sujian/` 只保留最终用户可见的
 * `.md` 文件。但旧版升级后，公开目录中可能仍残留这些隐藏子目录和文件，
 * 用户会在文件管理器中看到它们。
 *
 * 本类在升级后首次启动镜像模块时，通过 MediaStore 查询并删除这些旧版
 * 事务产物，确保 `Download/Sujian/` 目录干净。
 *
 * ## 幂等性
 * 使用标志文件 `filesDir/sujian/mirror/.staging-cleanup-done` 确保只执行
 * 一次。即使标志文件不存在，重复执行也是安全的（删除已不存在的文件是幂等的）。
 *
 * ## SAF 后端
 * 如果 stateStore 中保存了 SAF tree URI，也尝试通过 DocumentsContract
 * 删除 SAF 后端下的旧版事务目录。SAF 清理失败不影响 MediaStore 清理。
 *
 * ## 架构约束
 * - 位于 `:app` 的 `storage/mirror` 包，依赖 `:core:platform`（合法）。
 * - 不依赖 Compose、UniFFI、业务 Repository。
 * - 只在镜像模块初始化时调用一次，不在 UI 线程执行。
 *
 * @param context 应用 [Context]
 * @param contentResolver 应用 [ContentResolver]（用于 MediaStore 查询和删除）
 */
class MirrorStagingCleanup(
    private val context: Context,
    private val contentResolver: ContentResolver,
) {
    /**
     * 如果尚未执行过清理，则执行一次。
     *
     * Issue #667 评论 5645597368 问题 2：清理函数返回 Boolean，只有需要清理的后端
     * 都成功处理后才能写 done 标志。失败不写标志，下次初始化继续重试，避免第一次
     * 启动时查询/删除临时失败后旧 .staging/.backup/_meta 永远残留 Download 目录。
     */
    fun cleanupIfNeeded() {
        if (cleanupFlagFile.exists()) return

        DiagnosticsLogger.i(TAG, "Starting legacy staging cleanup (Issue #667)")

        val mediaStoreOk = cleanupViaMediaStore()
        val safOk = cleanupViaSaf()

        // 只有需要清理的后端都成功处理后才能写 done 标志
        // 失败就不写标志，下次初始化继续重试
        if (mediaStoreOk && safOk) {
            try {
                cleanupFlagFile.parentFile?.mkdirs()
                cleanupFlagFile.writeText("done", Charsets.UTF_8)
                DiagnosticsLogger.i(TAG, "Legacy staging cleanup completed successfully")
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "Failed to write cleanup flag file", e)
            }
        } else {
            DiagnosticsLogger.w(
                TAG,
                "Legacy staging cleanup failed (mediaStoreOk=$mediaStoreOk, safOk=$safOk), will retry next time",
            )
        }
    }

    /**
     * 通过 MediaStore 查询并删除旧版事务产物。
     *
     * 查询 RELATIVE_PATH 以 `Download/Sujian/.staging/`、`Download/Sujian/.backup/`、
     * `Download/Sujian/_meta/` 开头的文件，逐个删除。
     *
     * MediaStore.Downloads 需要 API 29+，低版本直接返回 true（旧版也不会用 MediaStore，视为成功）。
     *
     * @return true 表示所有前缀清理都成功；false 表示任一前缀查询或删除失败
     */
    private fun cleanupViaMediaStore(): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return true

        // 全部前缀清理的合取：任一失败即整体失败
        return LEGACY_DIR_PREFIXES.all { prefix ->
            val relativePathPrefix = "${DOWNLOADS_BASE}/$prefix"
            deleteMediaStoreFilesByPathPrefix(relativePathPrefix)
        }
    }

    /**
     * 查询 MediaStore 中 RELATIVE_PATH 以指定前缀开头的文件，逐个删除。
     *
     * 使用 `LIKE '<prefix>%'` 查询，匹配所有子路径下的文件。
     *
     * Issue #667 评论 5645967475：任一 delete 抛异常或返回 0 都视为本前缀清理失败，
     * 让外层不写 done 标志、下次重试。空结果（无数据）视为成功。
     *
     * @return true 表示查询和所有删除都成功（无数据时也返回 true）；
     *   false 表示查询抛异常，或任一 delete 抛异常，或任一 delete 返回 0（需重试）
     */
    private fun deleteMediaStoreFilesByPathPrefix(pathPrefix: String): Boolean {
        val cursor =
            try {
                contentResolver.query(
                    MediaStore.Downloads.EXTERNAL_CONTENT_URI,
                    arrayOf(MediaStore.Downloads._ID),
                    "${MediaStore.Downloads.RELATIVE_PATH} LIKE ?",
                    arrayOf("$pathPrefix%"),
                    null,
                )
            } catch (e: SecurityException) {
                DiagnosticsLogger.w(TAG, "MediaStore query failed (SecurityException) for prefix $pathPrefix", e)
                return false
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "MediaStore query failed for prefix $pathPrefix", e)
                return false
            }

        var allDeleted = true
        cursor?.use { c ->
            val urisToDelete = mutableListOf<Uri>()
            while (c.moveToNext()) {
                val id = c.getLong(0)
                urisToDelete.add(Uri.withAppendedPath(MediaStore.Downloads.EXTERNAL_CONTENT_URI, id.toString()))
            }
            for (uri in urisToDelete) {
                if (!deleteSingleMediaStoreUri(uri)) {
                    allDeleted = false
                }
            }
            if (urisToDelete.isNotEmpty() && allDeleted) {
                DiagnosticsLogger.i(TAG, "Deleted ${urisToDelete.size} legacy files under $pathPrefix")
            }
        }
        // 查询成功且所有 delete 都成功（返回非 0 且未抛异常）才视为本前缀清理成功；
        // 任一 delete 失败（抛异常或返回 0）都返回 false，让外层不写 done 标志、下次重试。
        return allDeleted
    }

    /**
     * 删除单个 MediaStore URI，返回是否成功（返回非 0 且未抛异常）。
     *
     * Issue #667 评论 5645967475：把单条删除逻辑提取到辅助函数，降低 [deleteMediaStoreFilesByPathPrefix]
     * 的认知复杂度与嵌套深度。
     */
    private fun deleteSingleMediaStoreUri(uri: Uri): Boolean =
        try {
            val rows = contentResolver.delete(uri, null, null)
            if (rows == 0) {
                DiagnosticsLogger.w(TAG, "MediaStore delete returned 0 rows: $uri")
                false
            } else {
                true
            }
        } catch (e: SecurityException) {
            DiagnosticsLogger.w(TAG, "MediaStore delete failed (SecurityException): $uri", e)
            false
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "MediaStore delete failed: $uri", e)
            false
        }

    /**
     * 通过 SAF DocumentsContract 删除旧版事务目录。
     *
     * 从 [ReadableMirrorStateStore] 读取 tree URI，如果存在则尝试在 tree 下
     * 查找并删除 `.staging/`、`.backup/`、`_meta/` 子目录。
     *
     * Issue #667 评论 5645967475：任一 deleteDocument 抛异常或返回 false 都视为 SAF 清理失败，
     * 让外层不写 done 标志、下次重试。无 tree URI 或无旧版事务目录视为成功。
     *
     * @return true 表示无需清理（无 tree URI、无旧版事务目录）或所有删除都成功；
     *   false 表示 listChildren 抛异常，或任一 deleteDocument 抛异常或返回 false（需重试）
     */
    private fun cleanupViaSaf(): Boolean {
        val stateStore = ReadableMirrorStateStore(context)
        val treeUriStr = stateStore.getTreeUri() ?: return true
        val treeUri = tryParseUri(treeUriStr) ?: return true

        val documentTreeReader = DocumentTreeReader(contentResolver)

        // 只查询一次 tree 的直接子项，然后逐个检查是否为旧版事务目录
        val children =
            try {
                documentTreeReader.listChildren(treeUri)
            } catch (e: SecurityException) {
                DiagnosticsLogger.w(TAG, "SAF listChildren failed (SecurityException)", e)
                return false
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "SAF listChildren failed", e)
                return false
            }

        var allDeleted = true
        for (dirName in LEGACY_DIR_NAMES) {
            val targetDir = children.find { it.isDirectory && it.name == dirName }
            if (targetDir != null && !deleteSingleSafDir(dirName, targetDir.uri)) {
                allDeleted = false
            }
        }
        // listChildren 成功且所有 deleteDocument 都成功（返回 true 且未抛异常）才视为 SAF 清理成功；
        // 任一 deleteDocument 失败（抛异常或返回 false）都返回 false，让外层不写 done 标志、下次重试。
        return allDeleted
    }

    /**
     * 删除单个 SAF 旧版事务目录，返回是否成功（deleteDocument 返回 true 且未抛异常）。
     *
     * Issue #667 评论 5645967475：把单条删除逻辑提取到辅助函数，降低 [cleanupViaSaf]
     * 的认知复杂度与嵌套深度。
     */
    private fun deleteSingleSafDir(
        dirName: String,
        uri: Uri,
    ): Boolean =
        try {
            val deleted = DocumentsContract.deleteDocument(contentResolver, uri)
            if (deleted) {
                DiagnosticsLogger.i(TAG, "Deleted SAF legacy directory: $dirName")
                true
            } else {
                DiagnosticsLogger.w(TAG, "SAF delete returned false for $dirName")
                false
            }
        } catch (e: SecurityException) {
            DiagnosticsLogger.w(TAG, "SAF delete failed (SecurityException) for $dirName", e)
            false
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "SAF delete failed for $dirName", e)
            false
        }

    private fun tryParseUri(uriString: String): Uri? =
        try {
            Uri.parse(uriString)
        } catch (_: Exception) {
            null
        }

    private val cleanupFlagFile: File by lazy {
        File(AndroidPrivateDataRoot.mirror(context), CLEANUP_FLAG_FILE_NAME)
    }

    companion object {
        private const val TAG = "MirrorStagingCleanup"
        private const val CLEANUP_FLAG_FILE_NAME = ".staging-cleanup-done"

        /** MediaStore 查询的基础路径前缀：`Download/Sujian`。 */
        private const val DOWNLOADS_BASE = "Download/Sujian"

        /** 旧版事务目录的路径前缀（相对 `Download/Sujian/`）。 */
        private val LEGACY_DIR_PREFIXES = listOf(".staging/", ".backup/", "_meta/")

        /** 旧版事务目录名（用于 SAF 遍历查找）。 */
        private val LEGACY_DIR_NAMES = listOf(".staging", ".backup", "_meta")
    }
}
