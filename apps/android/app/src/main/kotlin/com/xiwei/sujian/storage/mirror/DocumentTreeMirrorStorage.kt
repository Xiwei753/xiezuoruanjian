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
        val uri = tryParseUri(ref.uri) ?: return false // URI 无效 → 无法确认状态，返回 false
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
        val stagingParentUriResult = findDirectory(stagingParentPath)

        // 优先尝试 moveDocument 跨目录原子移动
        val newUri: Uri? =
            if (stagingParentUriResult is DirectoryLookupResult.Found) {
                tryMoveDocument(stagingUri, stagingParentUriResult.uri, targetParentUri, displayName)
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
        mimeType: String,
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
        val oldParentUriResult =
            if (oldParentPath.isBlank()) {
                DirectoryLookupResult.Found(
                    treeUri,
                )
            } else {
                findDirectory(oldParentPath)
            }
        // 1. 优先尝试 moveDocument 把 old 移到 backup
        if (oldParentUriResult is DirectoryLookupResult.Found) {
            val movedUri = tryMoveDocument(oldUri, oldParentUriResult.uri, backupParentUri, displayName)
            if (movedUri != null) {
                return MirrorFileRef(uri = movedUri.toString(), relativePath = backupRelativePath)
            }
        }
        // 2. 回退：read old → createText 到 backup → delete old（真正删 old 腾空最终路径）
        val content = readTextFromUri(oldUri) ?: return null
        val fileUri =
            try {
                DocumentsContract.createDocument(contentResolver, backupParentUri, mimeType, displayName)
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

    // #649 评论 5564820566 问题 3：两步 journalable backup — SAF 路径

    override fun prepareBackup(
        txId: String,
        old: MirrorFileRef,
        mimeType: String,
    ): BackupReadyRef? {
        val oldUri = tryParseUri(old.uri) ?: return null
        val backupBase = "$STAGING_DIR/$txId/$BACKUP_DIR"
        val backupRelativePath = "$backupBase/${old.relativePath}"
        val parent = old.relativePath.substringBeforeLast('/', "")
        val relativeDir = if (parent.isBlank()) backupBase else "$backupBase/$parent"
        val displayName = old.relativePath.substringAfterLast('/')
        val backupParentUri = ensureDirectory(relativeDir) ?: return null
        val oldParentPath = old.relativePath.substringBeforeLast('/', "")
        val oldParentUriResult =
            if (oldParentPath.isBlank()) {
                DirectoryLookupResult.Found(
                    treeUri,
                )
            } else {
                findDirectory(oldParentPath)
            }
        // 1. 优先尝试 moveDocument（原子 move）
        if (oldParentUriResult is DirectoryLookupResult.Found) {
            val movedUri = tryMoveDocument(oldUri, oldParentUriResult.uri, backupParentUri, displayName)
            if (movedUri != null) {
                return BackupReadyRef(
                    backupRef = MirrorFileRef(uri = movedUri.toString(), relativePath = backupRelativePath),
                    vacated = true,
                )
            }
        }
        // 2. 回退：只复制 old → backup，不删 old
        val content = readTextFromUri(oldUri) ?: return null
        val fileUri =
            try {
                DocumentsContract.createDocument(contentResolver, backupParentUri, mimeType, displayName)
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "createDocument failed for prepareBackup $displayName: ${e.message}")
                return null
            } ?: return null
        if (!writeToUri(fileUri, content)) {
            try {
                DocumentsContract.deleteDocument(contentResolver, fileUri)
            } catch (_: Exception) {
            }
            return null
        }
        return BackupReadyRef(
            backupRef = MirrorFileRef(uri = fileUri.toString(), relativePath = backupRelativePath),
            vacated = false,
        )
    }

    override fun vacateCommitted(old: MirrorFileRef): Boolean {
        val uri = tryParseUri(old.uri) ?: return false // URI 无效 → 无法确认 old 是否已腾空，返回 false
        return try {
            DocumentsContract.deleteDocument(contentResolver, uri)
        } catch (_: FileNotFoundException) {
            true // 文件已不存在 → 目标已达到
        } catch (_: Exception) {
            false
        }
    }

    override fun resolve(relativePath: String): MirrorFileRef? {
        if (!isSupported()) return null
        // #649 评论 5563333323 缺口 1：只查不创建，用 findDirectory + findChildFile。
        return resolveInTree(relativePath, treeUri)
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
        val dirUriResult = if (parent.isBlank()) DirectoryLookupResult.Found(treeUri) else findDirectory(parent)
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

    override fun resolveBackup(
        txId: String,
        relativePath: String,
    ): MirrorFileRef? {
        if (!isSupported()) return null
        // #649 评论 5563798095：检查 backup 路径是否已有文件，避免崩溃窗口后重复 backup。
        val backupBase = "$STAGING_DIR/$txId/$BACKUP_DIR"
        val backupRelativePath = "$backupBase/$relativePath"
        // backup 位于 staging 目录内，需要逐级 findDirectory
        val backupParentPath = backupRelativePath.substringBeforeLast('/', "")
        val displayName = backupRelativePath.substringAfterLast('/')
        val parentUriResult = findDirectory(backupParentPath)
        val parentUri =
            when (parentUriResult) {
                is DirectoryLookupResult.Found -> parentUriResult.uri
                else -> return null
            }
        return try {
            val children = documentTreeReader.listChildren(parentUri)
            val match = children.find { !it.isDirectory && it.name == displayName }
            match?.let { MirrorFileRef(uri = it.uri.toString(), relativePath = backupRelativePath) }
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "resolveBackup listChildren failed for $displayName: ${e.message}")
            null
        }
    }

    override fun lookupBackup(
        txId: String,
        relativePath: String,
    ): MirrorLookupResult {
        if (!isSupported()) {
            return MirrorLookupResult.Failed(IllegalStateException("DocumentTree backend not supported"))
        }
        val backupBase = "$STAGING_DIR/$txId/$BACKUP_DIR"
        val backupRelativePath = "$backupBase/$relativePath"
        // backup 位于 staging 目录内，需要逐级 findDirectory
        val backupParentPath = backupRelativePath.substringBeforeLast('/', "")
        val displayName = backupRelativePath.substringAfterLast('/')
        val parentUriResult = findDirectory(backupParentPath)
        when (parentUriResult) {
            is DirectoryLookupResult.Failed -> {
                // #649 评论 5566303837 问题 5：目录遍历失败明确传播
                return MirrorLookupResult.Failed(parentUriResult.cause)
            }
            is DirectoryLookupResult.Missing -> {
                return MirrorLookupResult.Missing
            }
            is DirectoryLookupResult.Found -> { /* 继续查询文件 */ }
        }
        return try {
            val children = documentTreeReader.listChildren(parentUriResult.uri)
            val match = children.find { !it.isDirectory && it.name == displayName }
            if (match != null) {
                MirrorLookupResult.Found(MirrorFileRef(uri = match.uri.toString(), relativePath = backupRelativePath))
            } else {
                MirrorLookupResult.Missing
            }
        } catch (e: SecurityException) {
            MirrorLookupResult.Failed(e)
        } catch (e: Exception) {
            MirrorLookupResult.Failed(e)
        }
    }

    /**
     * 在给定的 parentUri 下查找文件（只查不创建）。
     * [resolve] 和 [resolveBackup] 共用此实现。
     */
    private fun resolveInTree(
        relativePath: String,
        parentUri: Uri,
    ): MirrorFileRef? {
        val parent = relativePath.substringBeforeLast('/', "")
        val displayName = relativePath.substringAfterLast('/')
        val dirUriResult = if (parent.isBlank()) DirectoryLookupResult.Found(parentUri) else findDirectory(parent)
        val dirUri =
            when (dirUriResult) {
                is DirectoryLookupResult.Found -> dirUriResult.uri
                else -> return null
            }
        return try {
            val children = documentTreeReader.listChildren(dirUri)
            val match = children.find { !it.isDirectory && it.name == displayName }
            match?.let { MirrorFileRef(uri = it.uri.toString(), relativePath = relativePath) }
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "resolveInTree listChildren failed for $displayName: ${e.message}")
            null
        }
    }

    override fun restoreBackup(
        backup: MirrorFileRef,
        finalRelativePath: String,
        mimeType: String,
        expectedOldContentHash: String?,
    ): RestoreBackupResult {
        if (!isSupported()) return RestoreBackupResult.Failed(IllegalStateException("backend not supported"))
        // #649 评论 5566303837 问题 4：带身份校验的 restoreBackup
        when (val existing = lookup(finalRelativePath)) {
            is MirrorLookupResult.Found -> {
                // final 已存在，需要校验内容身份
                if (expectedOldContentHash != null) {
                    val finalHashResult = readTextAndHash(existing.ref)
                    if (finalHashResult != null) {
                        val (_, hash) = finalHashResult
                        return if (hash == expectedOldContentHash) {
                            RestoreBackupResult.AlreadyRestored(existing.ref)
                        } else {
                            RestoreBackupResult.Conflict
                        }
                    }
                    return RestoreBackupResult.Failed(null)
                }
                return RestoreBackupResult.AlreadyRestored(existing.ref)
            }
            is MirrorLookupResult.Failed -> return RestoreBackupResult.Failed(existing.cause)
            is MirrorLookupResult.Missing -> { /* 继续 restore */ }
        }
        val backupUri = tryParseUri(backup.uri) ?: return RestoreBackupResult.Failed(null)
        val content = readTextFromUri(backupUri) ?: return RestoreBackupResult.Failed(null)
        val relativeDir = finalRelativePath.substringBeforeLast('/', "")
        val displayName = finalRelativePath.substringAfterLast('/')
        val parentUri = ensureDirectory(relativeDir) ?: return RestoreBackupResult.Failed(null)
        val fileUri =
            try {
                DocumentsContract.createDocument(contentResolver, parentUri, mimeType, displayName)
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "createDocument failed for restore $displayName: ${e.message}")
                return RestoreBackupResult.Failed(e)
            } ?: return RestoreBackupResult.Failed(null)
        if (!writeToUri(fileUri, content)) {
            try {
                DocumentsContract.deleteDocument(contentResolver, fileUri)
            } catch (_: Exception) {
            }
            return RestoreBackupResult.Failed(null)
        }
        return RestoreBackupResult.Restored(MirrorFileRef(uri = fileUri.toString(), relativePath = finalRelativePath))
    }

    override fun readTextAndHash(ref: MirrorFileRef): Pair<String, String>? {
        val uri = tryParseUri(ref.uri) ?: return null
        val content = readTextFromUri(uri) ?: return null
        return Pair(content, computeContentHash(content))
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

    override fun rollback(txId: String): Boolean {
        if (!isSupported()) return false
        // #649 评论 5562462046 问题 6：rollback 只查不创建。
        val stagingDir = "$STAGING_DIR/$txId"
        val stagingUriResult = findDirectory(stagingDir)
        if (stagingUriResult is DirectoryLookupResult.Found) {
            return try {
                DocumentsContract.deleteDocument(contentResolver, stagingUriResult.uri)
                true
            } catch (_: FileNotFoundException) {
                true // 已不存在 → 目标已达到
            } catch (_: Exception) {
                false // 删除失败
            }
        }
        // 目录不存在或查询失败
        return stagingUriResult is DirectoryLookupResult.Missing
    }

    /**
     * 只查找 [relativeDir] 对应的目录 URI，不创建。
     *
     * #649 评论 5566303837 问题 5：返回 [DirectoryLookupResult]，
     * 区分"目录不存在"和"查询异常"，不再把两者都压成 null。
     *
     * @return [DirectoryLookupResult]
     */
    private fun findDirectory(relativeDir: String): DirectoryLookupResult {
        val parts = relativeDir.split("/").filter { it.isNotEmpty() }
        var current: Uri = treeUri
        for (part in parts) {
            when (val result = findChildDir(current, part)) {
                is DirectoryLookupResult.Found -> current = result.uri
                is DirectoryLookupResult.Missing -> return DirectoryLookupResult.Missing
                is DirectoryLookupResult.Failed -> return DirectoryLookupResult.Failed(result.cause)
            }
        }
        return DirectoryLookupResult.Found(current)
    }

    /**
     * 在 [parentUri] 下查找同名子目录（不创建），返回三态。
     */
    private fun findChildDir(
        parentUri: Uri,
        dirName: String,
    ): DirectoryLookupResult {
        return try {
            val children = documentTreeReader.listChildren(parentUri)
            val match = children.find { it.isDirectory && it.name == dirName }
            if (match != null) {
                DirectoryLookupResult.Found(match.uri)
            } else {
                DirectoryLookupResult.Missing
            }
        } catch (e: SecurityException) {
            DirectoryLookupResult.Failed(e)
        } catch (e: Exception) {
            DirectoryLookupResult.Failed(e)
        }
    }

    /**
     * 从 [DirectoryLookupResult] 提取 URI；Failed 时抛异常（用于调用方快速失败）。
     */
    private fun DirectoryLookupResult.getUriOrThrow(): Uri =
        when (this) {
            is DirectoryLookupResult.Found -> uri
            is DirectoryLookupResult.Missing -> throw FileNotFoundException("directory not found")
            is DirectoryLookupResult.Failed -> throw cause ?: IOException("directory lookup failed")
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
    }
}
