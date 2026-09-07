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

    override fun promoteStaged(
        staged: StagedMirrorRef,
        finalRelativePath: String,
    ): MirrorFileRef? {
        if (!isSupported()) return null
        // #649 评论 5563333323 缺口 1：promoteStaged 把 staging 移到最终位置。
        // 最终路径已由 backupCommitted 腾空（old 已移走），不会冲突。
        // 优先用 moveDocument 跨目录移动；失败回退到复制+删 staging。
        val stagingUri = tryParseUri(staged.stagingUri) ?: return null
        val relativeDir = finalRelativePath.substringBeforeLast('/', "")
        val displayName = finalRelativePath.substringAfterLast('/')
        val targetParentUri = ensureDirectory(relativeDir) ?: return null

        val stagingParentPath = staged.stagingRelativePath.substringBeforeLast('/', "")
        val stagingParentUri = findDirectory(stagingParentPath)

        // 优先尝试 moveDocument 跨目录原子移动
        val newUri: Uri? =
            if (stagingParentUri != null) {
                tryMoveDocument(stagingUri, stagingParentUri, targetParentUri, displayName)
            } else {
                null
            }
        if (newUri == null) {
            // provider 不支持 moveDocument 或失败 → 走"复制到最终位置成功后再删 staging"分支
            val content = readTextFromUri(stagingUri) ?: return null
            val createdUri =
                try {
                    DocumentsContract.createDocument(contentResolver, targetParentUri, staged.mimeType, displayName)
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
            try {
                DocumentsContract.deleteDocument(contentResolver, stagingUri)
            } catch (_: Exception) {
            }
            return MirrorFileRef(uri = createdUri.toString(), relativePath = finalRelativePath)
        }
        return MirrorFileRef(uri = newUri.toString(), relativePath = finalRelativePath)
    }

    override fun backupCommitted(
        txId: String,
        old: MirrorFileRef,
    ): MirrorFileRef? {
        if (!isSupported()) return null
        // #649 评论 5563333323 缺口 1：把 old 从最终路径**移动**到 tx backup 区（不是复制），
        // 最终路径真正腾空。优先用 moveDocument 跨目录移动；
        // 失败回退到 read+createText 到 backup + delete old（真正删 old 腾空最终路径）。
        val oldUri = tryParseUri(old.uri) ?: return null
        val backupBase = "$STAGING_DIR/$txId/$BACKUP_DIR"
        val backupRelativePath = "$backupBase/${old.relativePath}"
        val parent = old.relativePath.substringBeforeLast('/', "")
        val relativeDir = if (parent.isBlank()) backupBase else "$backupBase/$parent"
        val displayName = old.relativePath.substringAfterLast('/')
        val backupParentUri = ensureDirectory(relativeDir) ?: return null
        // old 的父目录 URI（用于 moveDocument）
        val oldParentPath = old.relativePath.substringBeforeLast('/', "")
        val oldParentUri = if (oldParentPath.isBlank()) treeUri else findDirectory(oldParentPath)
        // 1. 优先尝试 moveDocument 把 old 移到 backup
        if (oldParentUri != null) {
            val movedUri = tryMoveDocument(oldUri, oldParentUri, backupParentUri, displayName)
            if (movedUri != null) {
                return MirrorFileRef(uri = movedUri.toString(), relativePath = backupRelativePath)
            }
        }
        // 2. 回退：read old → createText 到 backup → delete old（真正删 old 腾空最终路径）
        val content = readTextFromUri(oldUri) ?: return null
        val fileUri =
            try {
                DocumentsContract.createDocument(contentResolver, backupParentUri, MIME_MARKDOWN, displayName)
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "createDocument failed for backup $displayName: ${e.message}")
                return null
            } ?: return null
        if (!writeToUri(fileUri, content)) {
            try {
                DocumentsContract.deleteDocument(contentResolver, fileUri)
            } catch (_: Exception) {
            }
            return null
        }
        // 关键：删 old 腾空最终路径（不是保留 old）
        if (!try {
                DocumentsContract.deleteDocument(contentResolver, oldUri)
            } catch (_: Exception) {
                false
            }
        ) {
            // 删 old 失败：删 backup 回滚，old 仍在原位
            try {
                DocumentsContract.deleteDocument(contentResolver, fileUri)
            } catch (_: Exception) {
            }
            return null
        }
        return MirrorFileRef(uri = fileUri.toString(), relativePath = backupRelativePath)
    }

    override fun resolve(relativePath: String): MirrorFileRef? {
        if (!isSupported()) return null
        // #649 评论 5563333323 缺口 1：只查不创建，用 findDirectory + findChildFile。
        val parent = relativePath.substringBeforeLast('/', "")
        val displayName = relativePath.substringAfterLast('/')
        val parentUri = if (parent.isBlank()) treeUri else findDirectory(parent) ?: return null
        return try {
            val children = documentTreeReader.listChildren(parentUri)
            val match = children.find { !it.isDirectory && it.name == displayName }
            match?.let { MirrorFileRef(uri = it.uri.toString(), relativePath = relativePath) }
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "resolve listChildren failed for $displayName: ${e.message}")
            null
        }
    }

    override fun restoreBackup(
        backup: MirrorFileRef,
        finalRelativePath: String,
    ): MirrorFileRef? {
        if (!isSupported()) return null
        // #649 评论 5562715833 问题 2：把 backup 恢复到 final 位置（回滚用）。
        val backupUri = tryParseUri(backup.uri) ?: return null
        val content = readTextFromUri(backupUri) ?: return null
        val relativeDir = finalRelativePath.substringBeforeLast('/', "")
        val displayName = finalRelativePath.substringAfterLast('/')
        val parentUri = ensureDirectory(relativeDir) ?: return null
        val fileUri =
            try {
                DocumentsContract.createDocument(contentResolver, parentUri, MIME_MARKDOWN, displayName)
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "createDocument failed for restore $displayName: ${e.message}")
                return null
            } ?: return null
        if (!writeToUri(fileUri, content)) {
            try {
                DocumentsContract.deleteDocument(contentResolver, fileUri)
            } catch (_: Exception) {
            }
            return null
        }
        return MirrorFileRef(uri = fileUri.toString(), relativePath = finalRelativePath)
    }

    /**
     * 尝试用 [DocumentsContract.moveDocument] 把 staging 跨父目录原子移动到最终位置。
     *
     * #649 评论 5562715833 问题 3：旧实现只用 renameDocument，无法跨父目录移动。
     * 新实现先用 moveDocument 跨父目录移动，再视需要 renameDocument 调整文件名。
     *
     * 部分 DocumentsProvider 不支持 moveDocument（抛 UnsupportedOperationException
     * 或返回 null），调用方应回退到复制+删 staging 分支。
     *
     * @param stagingUri 暂存文件 URI
     * @param stagingParentUri 暂存文件的父目录 URI
     * @param targetParentUri 目标父目录 URI
     * @param displayName 最终文件名
     * @return 移动后的文件 URI；失败返回 null
     */
    private fun tryMoveDocument(
        stagingUri: Uri,
        stagingParentUri: Uri,
        targetParentUri: Uri,
        displayName: String,
    ): Uri? {
        // 1. moveDocument 跨父目录移动
        val movedUri =
            try {
                DocumentsContract.moveDocument(contentResolver, stagingUri, stagingParentUri, targetParentUri)
            } catch (_: UnsupportedOperationException) {
                null
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "moveDocument failed: ${e.message}")
                null
            } ?: return null
        // 2. 如目标文件名还需变化，再 renameDocument
        val currentName = getDisplayName(movedUri)
        return if (currentName == displayName) {
            movedUri
        } else {
            try {
                DocumentsContract.renameDocument(contentResolver, movedUri, displayName)
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "renameDocument failed after move: ${e.message}")
                null
            }
        }
    }

    /** 查询 URI 的 display name。 */
    private fun getDisplayName(uri: Uri): String? {
        return try {
            contentResolver
                .query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)
                ?.use { cursor ->
                    if (cursor.moveToFirst()) cursor.getString(0) else null
                }
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "getDisplayName failed: ${e.message}")
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
        private const val BACKUP_DIR = "backup"
        private const val MIME_MARKDOWN = "text/markdown"
    }
}
