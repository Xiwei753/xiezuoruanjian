package com.xiwei.sujian.storage.mirror

import android.content.ContentResolver
import android.net.Uri
import android.provider.DocumentsContract
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.platform.storage.documents.DocumentTreeReader

/**
 * SAF DocumentsProvider 后端的 [ReadableMirrorStorage] 实现。
 *
 * #649 评论 5561465552 第 3 点：SAF/MediaStore URI 体系混用问题。
 *
 * ref 保存 SAF tree/document URI，用 [DocumentsContract] + [ContentResolver] stream，
 * 不碰 [android.provider.MediaStore.Downloads.IS_PENDING]。
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
                ensureDirectory(relativeDir) ?: return null
            }
        // 在父目录下创建文件。SAF 不支持同名覆盖，createDocument 会自动加 (1) 后缀。
        // 调用方应先尝试 replaceText 旧 URI，失败再 createText，避免重复文件。
        val fileUri =
            try {
                DocumentsContract.createDocument(contentResolver, parentUri, mimeType, displayName)
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "createDocument failed for $displayName: ${e.message}")
                return null
            } ?: return null
        // 写内容
        if (!writeToUri(fileUri, text)) {
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
        val uri = tryParseUri(ref.uri) ?: return false
        return writeToUri(uri, text)
    }

    override fun delete(ref: MirrorFileRef): Boolean {
        if (!isSupported()) return false
        val uri = tryParseUri(ref.uri) ?: return false
        return try {
            DocumentsContract.deleteDocument(contentResolver, uri)
        } catch (_: Exception) {
            false
        }
    }

    /**
     * treeUri 非空且可查询时返回 true。
     *
     * 实际权限检查在第一次 I/O 时由 ContentResolver 抛 SecurityException 体现；
     * 此处只做基本可用性判断。
     */
    override fun isSupported(): Boolean {
        return try {
            // 触发一次轻量查询验证 tree URI 仍可访问
            documentTreeReader.listChildren(treeUri)
            true
        } catch (_: Exception) {
            false
        }
    }

    // ── 内部 ──

    /**
     * 逐级在 [treeUri] 下查找或创建 [relativeDir] 指定的目录路径。
     *
     * SAF 不支持 `RELATIVE_PATH`，必须逐级 `listChildren` 查找已有目录，
     * 找不到则 `DocumentsContract.createDocument` 建 `MIME_TYPE_DIR`。
     *
     * @return 最深层目录的 URI；任一级失败返回 null。
     */
    private fun ensureDirectory(relativeDir: String): Uri? {
        val parts = relativeDir.split("/").filter { it.isNotEmpty() }
        var current = treeUri
        for (part in parts) {
            current = findOrCreateChildDir(current, part) ?: return null
        }
        return current
    }

    private fun findOrCreateChildDir(
        parentUri: Uri,
        dirName: String,
    ): Uri? {
        // 先查找已有同名目录
        try {
            val children = documentTreeReader.listChildren(parentUri)
            val existing = children.find { it.isDirectory && it.name == dirName }
            if (existing != null) return existing.uri
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "listChildren failed for $dirName: ${e.message}")
            return null
        }
        // 不存在则创建
        return try {
            DocumentsContract.createDocument(
                contentResolver,
                parentUri,
                DocumentsContract.Document.MIME_TYPE_DIR,
                dirName,
            )
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "createDocument dir failed for $dirName: ${e.message}")
            null
        }
    }

    private fun writeToUri(
        uri: Uri,
        text: String,
    ): Boolean {
        return try {
            contentResolver.openOutputStream(uri)?.use { os ->
                os.write(text.toByteArray(Charsets.UTF_8))
                true
            } ?: false
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "writeToUri failed: ${e.message}")
            false
        }
    }

    private fun tryParseUri(uriString: String): Uri? =
        try {
            Uri.parse(uriString)
        } catch (_: Exception) {
            null
        }

    companion object {
        private const val TAG = "DocumentTreeMirrorStorage"
    }
}
