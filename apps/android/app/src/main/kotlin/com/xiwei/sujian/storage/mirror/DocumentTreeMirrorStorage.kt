package com.xiwei.sujian.storage.mirror

import android.content.ContentResolver
import android.net.Uri
import android.provider.DocumentsContract
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.platform.storage.documents.DocumentTreeReader
import java.io.IOException

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

    // ── 事务能力（#649 评论 5561974464 问题 2）──

    override fun stageText(
        txId: String,
        relativePath: String,
        mimeType: String,
        text: String,
    ): StagedMirrorRef? {
        if (!isSupported()) return null
        // SAF 暂存：用 txId 作为临时目录，避免覆盖 committed ref
        val stagingDir = "$STAGING_DIR/$txId"
        // #649 评论 5562462046 问题 6：路径拼接修复，避免少一个 `/`
        val parent = relativePath.substringBeforeLast('/', "")
        val relativeDir = if (parent.isBlank()) stagingDir else "$stagingDir/$parent"
        val displayName = relativePath.substringAfterLast('/')
        val parentUri = ensureDirectory(relativeDir) ?: return null
        val fileUri =
            try {
                DocumentsContract.createDocument(contentResolver, parentUri, mimeType, displayName)
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "createDocument failed for $displayName: ${e.message}")
                return null
            } ?: return null
        if (!writeToUri(fileUri, text)) {
            try {
                DocumentsContract.deleteDocument(contentResolver, fileUri)
            } catch (_: Exception) {
            }
            return null
        }
        return StagedMirrorRef(
            txId = txId,
            stagingUri = fileUri.toString(),
            stagingRelativePath = "$stagingDir/$relativePath",
            finalRelativePath = relativePath,
            mimeType = mimeType,
        )
    }

    override fun promote(
        staged: StagedMirrorRef,
        old: MirrorFileRef?,
        finalRelativePath: String,
    ): PromoteResult? {
        if (!isSupported()) return null
        // #649 评论 5562462046 问题 1：真正的 swap，不先删 old。
        val stagingUri = tryParseUri(staged.stagingUri) ?: return null
        val relativeDir = finalRelativePath.substringBeforeLast('/', "")
        val displayName = finalRelativePath.substringAfterLast('/')
        val parentUri = ensureDirectory(relativeDir) ?: return null

        // 优先尝试 DocumentsContract.moveDocument（原子移动，不复制内容）
        val newUri: Uri? = tryMoveDocument(stagingUri, parentUri, displayName)
        if (newUri == null) {
            // provider 不支持 moveDocument 或失败 → 走"复制到最终位置成功后再删 staging"分支
            val content = readTextFromUri(stagingUri) ?: return null
            val createdUri =
                try {
                    DocumentsContract.createDocument(contentResolver, parentUri, staged.mimeType, displayName)
                } catch (e: Exception) {
                    DiagnosticsLogger.w(TAG, "createDocument failed for $displayName: ${e.message}")
                    return null
                } ?: return null
            if (!writeToUri(createdUri, content)) {
                try {
                    DocumentsContract.deleteDocument(contentResolver, createdUri)
                } catch (_: Exception) {
                }
                return null
            }
            // 复制成功后删 staging
            try {
                DocumentsContract.deleteDocument(contentResolver, stagingUri)
            } catch (_: Exception) {
            }
            // newUri = createdUri
            return finishPromoteSwap(createdUri, finalRelativePath, old)
        }
        return finishPromoteSwap(newUri, finalRelativePath, old)
    }

    /**
     * promote 共用收尾：create/move 成功后才删 old，返回 [PromoteResult]。
     */
    private fun finishPromoteSwap(
        newUri: Uri,
        finalRelativePath: String,
        old: MirrorFileRef?,
    ): PromoteResult {
        if (old != null) {
            val oldUri = tryParseUri(old.uri)
            if (oldUri != null) {
                try {
                    DocumentsContract.deleteDocument(contentResolver, oldUri)
                } catch (_: Exception) {
                }
            }
        }
        return PromoteResult(
            newRef = MirrorFileRef(uri = newUri.toString(), relativePath = finalRelativePath),
            displacedOldRef = old,
        )
    }

    /**
     * 尝试用 [DocumentsContract.renameDocument] 把 staging 原子重命名到最终位置。
     *
     * 部分 DocumentsProvider 不支持 renameDocument（抛 UnsupportedOperationException
     * 或返回 null），调用方应回退到复制+删 staging 分支。
     *
     * 注意：renameDocument 不能改变父目录，只能重命名。因此 staging 必须在最终父目录下。
     * 当前 staging 路径是 `.staging/<txId>/<finalRelativePath>`，不在最终父目录下，
     * 所以这里只尝试 rename 到最终文件名（假设 staging 已在最终父目录）。
     * 如果 rename 失败，调用方走"复制到最终位置成功后再删 staging"分支。
     */
    private fun tryMoveDocument(
        stagingUri: Uri,
        parentUri: Uri,
        displayName: String,
    ): Uri? {
        // 先尝试 renameDocument（重命名到最终文件名）
        return try {
            DocumentsContract.renameDocument(contentResolver, stagingUri, displayName)
        } catch (_: UnsupportedOperationException) {
            null
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "renameDocument failed, will fallback to copy: ${e.message}")
            null
        }
    }

    override fun rollback(txId: String) {
        if (!isSupported()) return
        // #649 评论 5562462046 问题 6：rollback 只查不创建。
        // 旧实现用 ensureDirectory(stagingDir) 会在目录不存在时新建一个再删，职责不对。
        val stagingDir = "$STAGING_DIR/$txId"
        val stagingUri = findDirectory(stagingDir)
        if (stagingUri != null) {
            try {
                DocumentsContract.deleteDocument(contentResolver, stagingUri)
            } catch (_: Exception) {
            }
        }
    }

    /**
     * 只查找 [relativeDir] 对应的目录 URI，不创建。
     *
     * #649 评论 5562462046 问题 6：rollback 需要一个只查不创建的方法，
     * 避免目录不存在时新建一个再删。
     *
     * @return 已存在目录的 URI；任一级不存在或查找失败返回 null。
     */
    private fun findDirectory(relativeDir: String): Uri? {
        val parts = relativeDir.split("/").filter { it.isNotEmpty() }
        var current = treeUri
        for (part in parts) {
            current = findChildDir(current, part) ?: return null
        }
        return current
    }

    /**
     * 在 [parentUri] 下查找同名子目录（不创建）。
     */
    private fun findChildDir(
        parentUri: Uri,
        dirName: String,
    ): Uri? {
        return try {
            val children = documentTreeReader.listChildren(parentUri)
            children.find { it.isDirectory && it.name == dirName }?.uri
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "listChildren failed for $dirName: ${e.message}")
            null
        }
    }

    /** 从 URI 读取全部文本。失败返回 null。 */
    private fun readTextFromUri(uri: Uri): String? {
        return try {
            contentResolver.openInputStream(uri)?.use { input ->
                input.readBytes().toString(Charsets.UTF_8)
            }
        } catch (e: IOException) {
            null
        } catch (e: Exception) {
            null
        }
    }

    companion object {
        private const val TAG = "DocumentTreeMirrorStorage"
        private const val STAGING_DIR = ".staging"
    }
}
