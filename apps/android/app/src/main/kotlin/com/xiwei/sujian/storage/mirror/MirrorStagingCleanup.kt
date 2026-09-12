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
     * 清理完成后在私有目录写入标志文件，后续调用直接跳过。
     * 清理过程中任何步骤失败都不会阻止后续步骤（best-effort 清理）。
     */
    fun cleanupIfNeeded() {
        if (cleanupFlagFile.exists()) return

        DiagnosticsLogger.i(TAG, "Starting legacy staging cleanup (Issue #667)")

        cleanupViaMediaStore()
        cleanupViaSaf()

        // 写入标志文件，标记清理已完成
        try {
            cleanupFlagFile.parentFile?.mkdirs()
            cleanupFlagFile.writeText("done", Charsets.UTF_8)
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "Failed to write cleanup flag file", e)
        }

        DiagnosticsLogger.i(TAG, "Legacy staging cleanup completed")
    }

    /**
     * 通过 MediaStore 查询并删除旧版事务产物。
     *
     * 查询 RELATIVE_PATH 以 `Download/Sujian/.staging/`、`Download/Sujian/.backup/`、
     * `Download/Sujian/_meta/` 开头的文件，逐个删除。
     *
     * MediaStore.Downloads 需要 API 29+，低版本跳过（旧版也不会用 MediaStore）。
     */
    private fun cleanupViaMediaStore() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return

        for (prefix in LEGACY_DIR_PREFIXES) {
            val relativePathPrefix = "${DOWNLOADS_BASE}/$prefix"
            deleteMediaStoreFilesByPathPrefix(relativePathPrefix)
        }
    }

    /**
     * 查询 MediaStore 中 RELATIVE_PATH 以指定前缀开头的文件，逐个删除。
     *
     * 使用 `LIKE '<prefix>%'` 查询，匹配所有子路径下的文件。
     */
    private fun deleteMediaStoreFilesByPathPrefix(pathPrefix: String) {
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
                return
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "MediaStore query failed for prefix $pathPrefix", e)
                return
            }

        cursor?.use { c ->
            val urisToDelete = mutableListOf<Uri>()
            while (c.moveToNext()) {
                val id = c.getLong(0)
                urisToDelete.add(Uri.withAppendedPath(MediaStore.Downloads.EXTERNAL_CONTENT_URI, id.toString()))
            }
            for (uri in urisToDelete) {
                try {
                    contentResolver.delete(uri, null, null)
                } catch (e: SecurityException) {
                    DiagnosticsLogger.w(TAG, "MediaStore delete failed (SecurityException): $uri", e)
                } catch (e: Exception) {
                    DiagnosticsLogger.w(TAG, "MediaStore delete failed: $uri", e)
                }
            }
            if (urisToDelete.isNotEmpty()) {
                DiagnosticsLogger.i(TAG, "Deleted ${urisToDelete.size} legacy files under $pathPrefix")
            }
        }
    }

    /**
     * 通过 SAF DocumentsContract 删除旧版事务目录。
     *
     * 从 [ReadableMirrorStateStore] 读取 tree URI，如果存在则尝试在 tree 下
     * 查找并删除 `.staging/`、`.backup/`、`_meta/` 子目录。
     *
     * SAF 清理失败不影响整体清理结果（best-effort）。
     */
    private fun cleanupViaSaf() {
        val stateStore = ReadableMirrorStateStore(context)
        val treeUriStr = stateStore.getTreeUri() ?: return
        val treeUri = tryParseUri(treeUriStr) ?: return

        val documentTreeReader = DocumentTreeReader(contentResolver)

        // 只查询一次 tree 的直接子项，然后逐个检查是否为旧版事务目录
        val children =
            try {
                documentTreeReader.listChildren(treeUri)
            } catch (e: SecurityException) {
                DiagnosticsLogger.w(TAG, "SAF listChildren failed (SecurityException)", e)
                return
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "SAF listChildren failed", e)
                return
            }

        for (dirName in LEGACY_DIR_NAMES) {
            val targetDir = children.find { it.isDirectory && it.name == dirName }
            if (targetDir != null) {
                try {
                    DocumentsContract.deleteDocument(contentResolver, targetDir.uri)
                    DiagnosticsLogger.i(TAG, "Deleted SAF legacy directory: $dirName")
                } catch (e: SecurityException) {
                    DiagnosticsLogger.w(TAG, "SAF delete failed (SecurityException) for $dirName", e)
                } catch (e: Exception) {
                    DiagnosticsLogger.w(TAG, "SAF delete failed for $dirName", e)
                }
            }
        }
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
