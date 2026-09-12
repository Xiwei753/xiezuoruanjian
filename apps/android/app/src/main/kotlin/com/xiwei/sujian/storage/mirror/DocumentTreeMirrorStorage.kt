package com.xiwei.sujian.storage.mirror

import android.content.ContentResolver
import android.net.Uri
import android.provider.DocumentsContract
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.platform.storage.documents.DocumentTreeReader
import java.io.FileNotFoundException
import java.io.IOException

/**
 * SAF DocumentsProvider 后端的 [ReadableMirrorStorage] 实现。
 *
 * #649 评论 5561465552 第 3 点：SAF/MediaStore URI 体系混用问题。
 *
 * ref 保存 SAF tree/document URI，用 [DocumentsContract] + [ContentResolver] stream，
 * 不碰 [android.provider.MediaStore.Downloads.IS_PENDING]。
 *
 * Issue #667：事务能力（stage/backup/promote/rollback）已移至
 * [MirrorTransactionWorkspace]（私有目录）。本类只负责最终用户可见文件的读写和查询。
 *
 * ## 关键差异（vs [MediaStoreMirrorStorage]）
 * - [replaceText]：直接 `openOutputStream(uri)` 覆盖写（SAF 有写权限即可），
 *   不用 `IS_PENDING` 流程。SAF URI 传给 MediaStore 的 `IS_PENDING` 会返回 0
 *   （见 [com.xiwei.sujian.core.platform.storage.downloads.MediaStoreDownloads.replaceText]
 *   的返回值检查），所以必须由本类接管 SAF URI 的覆盖写。
 * - [createText]：用 [DocumentsContract.createDocument] 在 tree 下逐级建目录/文件。
 *   SAF 不支持 `RELATIVE_PATH`，必须逐级 `listChildren` 查找或 `createDocument` 建目录。
 * - [delete]：用 [DocumentsContract.deleteDocument]。
 *
 * ## 架构约束
 * - 位于 `:app` 的 `storage/mirror` 包，依赖 `:core:platform` 的 [DocumentTreeReader]
 *   （用于 listChildren 查找已有目录，避免重复创建）和 [ContentResolver]。
 * - 不把 `content://` URI 传给 Rust。
 *
 * #651 评论 5592465805：纯文档操作 helper 拆到 [DocumentTreeMirrorOps]，
 * 解决 LargeClass / TooManyFunctions。
 *
 * @param treeUri 用户通过 `OpenDocumentTree()` 选中的根 tree URI（`Download/Sujian`）。
 *   必须有持久化的读+写权限。
 * @param contentResolver 应用 [ContentResolver]。
 * @param documentTreeReader 复用 [DocumentTreeReader] 的 listChildren 能力查找已有目录。
 */
class DocumentTreeMirrorStorage(
    private val treeUri: Uri,
    private val contentResolver: ContentResolver,
    private val documentTreeReader: DocumentTreeReader,
) : ReadableMirrorStorage {
    private val ops = DocumentTreeMirrorOps(treeUri, contentResolver, documentTreeReader)

    override fun createText(
        relativeDir: String,
        displayName: String,
        mimeType: String,
        text: String,
    ): MirrorFileRef? {
        if (!isSupported()) return null
        // 逐级进入或创建目录
        val parentUri =
            if (relativeDir.isBlank()) {
                treeUri
            } else {
                ops.ensureDirectory(relativeDir) ?: return null
            }
        // 在父目录下创建文件。SAF 不支持同名覆盖，createDocument 会自动加 (1) 后缀。
        // 调用方应先尝试 replaceText 旧 URI，失败再 createText，避免重复文件。
        val fileUri =
            try {
                DocumentsContract.createDocument(contentResolver, parentUri, mimeType, displayName)
            } catch (e: Exception) {
                logCreateDocumentFailed(displayName, e)
                return null
            } ?: return null
        // 写内容
        if (!ops.writeToUri(fileUri, text)) {
            try {
                DocumentsContract.deleteDocument(contentResolver, fileUri)
            } catch (_: Exception) {
            }
            return null
        }
        val relativePath = if (relativeDir.isBlank()) displayName else "$relativeDir/$displayName"
        return MirrorFileRef(uri = fileUri.toString(), relativePath = relativePath)
    }

    /**
     * 直接 `openOutputStream(uri)` 覆盖写。
     *
     * SAF 有写权限即可覆盖，不需要 `IS_PENDING` 流程。
     */
    override fun replaceText(
        ref: MirrorFileRef,
        text: String,
    ): Boolean {
        if (!isSupported()) return false
        val uri = ops.tryParseUri(ref.uri) ?: return false
        return ops.writeToUri(uri, text)
    }

    /**
     * 删除引用指向的文件（幂等）。
     *
     * #649 评论 5564624383 问题 5：明确区分"不存在 → true"和"异常 → false"。
     * 幂等只应该是"明确不存在"返回 true，不是"任何异常都算成功"。
     * 例如 SAF 权限丢失、provider I/O 错误时，如果返回 true，
     * cleanupCommittedTransaction() 会认为清理完成并删除 journal，实际旧文件仍在。
     *
     * - FileNotFoundException → true（明确不存在）
     * - 删除成功 → true
     * - SecurityException / IOException / provider 异常 → false（无法确认是否存在）
     */
    override fun delete(ref: MirrorFileRef): Boolean {
        // #649 评论 5564820566 问题 4：不再把 "后端不可用" 当删除成功。
        // 旧代码 `if (!isSupported()) return true` 会让 cleanup 误认为文件已删。
        // SAF 权限丢失、provider I/O 异常时，不支持的 I/O 会由下面的 try/catch 捕获。
        val uri = ops.tryParseUri(ref.uri) ?: return false // URI 无效 → 无法确认状态，返回 false
        return try {
            DocumentsContract.deleteDocument(contentResolver, uri)
        } catch (_: FileNotFoundException) {
            true // 文件不存在 → 目标已达到
        } catch (_: SecurityException) {
            false // 权限异常 → 无法确认文件状态
        } catch (_: IOException) {
            false // I/O 异常 → 无法确认文件状态
        } catch (_: Exception) {
            false // 其他异常 → 无法确认文件状态
        }
    }

    /**
     * treeUri 非空且可查询时返回 true。
     *
     * 实际权限检查在第一次 I/O 时由 ContentResolver 抛 SecurityException 体现；
     * 此处只做基本可用性判断。
     */
    override fun isSupported(): Boolean =
        try {
            // 触发一次轻量查询验证 tree URI 仍可访问
            documentTreeReader.listChildren(treeUri)
            true
        } catch (_: Exception) {
            false
        }

    override fun resolve(relativePath: String): MirrorFileRef? {
        if (!isSupported()) return null
        // #649 评论 5563333323 缺口 1：只查不创建，用 findDirectory + findChildFile。
        return ops.resolveInTree(relativePath, treeUri)
    }

    /**
     * 三态查询实现（#649 评论 5565067997 修复 5）。
     *
     * - 后端不可用 → [MirrorLookupResult.Failed]（不静默当 Missing）
     * - 查询成功且找到文件 → [MirrorLookupResult.Found]
     * - 查询成功但路径不存在/无匹配 → [MirrorLookupResult.Missing]
     * - 查询抛异常（SecurityException / IOException / provider 异常）→ [MirrorLookupResult.Failed]
     */
    override fun lookup(relativePath: String): MirrorLookupResult {
        if (!isSupported()) {
            return MirrorLookupResult.Failed(IllegalStateException("DocumentTree backend not supported"))
        }
        val parent = relativePath.substringBeforeLast('/', "")
        val displayName = relativePath.substringAfterLast('/')
        // 先定位父目录
        val dirUriResult = if (parent.isBlank()) DirectoryLookupResult.Found(treeUri) else ops.findDirectory(parent)
        when (dirUriResult) {
            is DirectoryLookupResult.Failed -> {
                // #649 评论 5566303837 问题 5：目录遍历失败明确传播，不静默当 Missing
                return MirrorLookupResult.Failed(dirUriResult.cause)
            }
            is DirectoryLookupResult.Missing -> {
                return MirrorLookupResult.Missing
            }
            is DirectoryLookupResult.Found -> { /* 继续查询文件 */ }
        }
        return try {
            val children = documentTreeReader.listChildren(dirUriResult.uri)
            val match = children.find { !it.isDirectory && it.name == displayName }
            if (match != null) {
                MirrorLookupResult.Found(MirrorFileRef(uri = match.uri.toString(), relativePath = relativePath))
            } else {
                MirrorLookupResult.Missing
            }
        } catch (e: SecurityException) {
            MirrorLookupResult.Failed(e)
        } catch (e: Exception) {
            MirrorLookupResult.Failed(e)
        }
    }

    override fun readTextAndHash(ref: MirrorFileRef): Pair<String, String>? {
        val uri = ops.tryParseUri(ref.uri) ?: return null
        val content = ops.readTextFromUri(uri) ?: return null
        return Pair(content, computeContentHash(content))
    }

    /** 记录 createDocument 失败日志（#651 评论 5592465805：消除 StringLiteralDuplication）。 */
    private fun logCreateDocumentFailed(
        displayName: String,
        e: Exception,
    ) {
        DiagnosticsLogger.w(TAG, "createDocument failed for $displayName: ${e.message}")
    }

    companion object {
        private const val TAG = "DocumentTreeMirrorStorage"
    }
}
