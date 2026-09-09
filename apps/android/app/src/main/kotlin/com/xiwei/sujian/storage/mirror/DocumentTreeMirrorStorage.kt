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
        val parentUri = ops.ensureDirectory(relativeDir) ?: return null
        val fileUri =
            try {
                DocumentsContract.createDocument(contentResolver, parentUri, mimeType, displayName)
            } catch (e: Exception) {
                logCreateDocumentFailed(displayName, e)
                return null
            } ?: return null
        if (!ops.writeToUri(fileUri, text)) {
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
        val stagingUri = ops.tryParseUri(staged.stagingUri) ?: return null
        val relativeDir = finalRelativePath.substringBeforeLast('/', "")
        val displayName = finalRelativePath.substringAfterLast('/')
        val targetParentUri = ops.ensureDirectory(relativeDir) ?: return null

        val stagingParentPath = staged.stagingRelativePath.substringBeforeLast('/', "")
        val stagingParentUriResult = ops.findDirectory(stagingParentPath)

        // 优先尝试 moveDocument 跨目录原子移动
        val newUri: Uri? =
            if (stagingParentUriResult is DirectoryLookupResult.Found) {
                ops.tryMoveDocument(stagingUri, stagingParentUriResult.uri, targetParentUri, displayName)
            } else {
                null
            }
        if (newUri == null) {
            // provider 不支持 moveDocument 或失败 → 走"复制到最终位置成功后再删 staging"分支
            val content = ops.readTextFromUri(stagingUri) ?: return null
            val createdUri =
                try {
                    DocumentsContract.createDocument(contentResolver, targetParentUri, staged.mimeType, displayName)
                } catch (e: Exception) {
                    logCreateDocumentFailed(displayName, e)
                    return null
                } ?: return null
            if (!ops.writeToUri(createdUri, content)) {
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
        val oldUri = ops.tryParseUri(old.uri) ?: return null
        val backupBase = ops.backupBasePath(txId)
        val backupRelativePath = "$backupBase/${old.relativePath}"
        val parent = old.relativePath.substringBeforeLast('/', "")
        val relativeDir = if (parent.isBlank()) backupBase else "$backupBase/$parent"
        val displayName = old.relativePath.substringAfterLast('/')
        val backupParentUri = ops.ensureDirectory(relativeDir) ?: return null
        val oldParentPath = old.relativePath.substringBeforeLast('/', "")
        val oldParentUriResult = ops.resolveOldParent(oldParentPath)
        // 1. 优先尝试 moveDocument 把 old 移到 backup
        if (oldParentUriResult is DirectoryLookupResult.Found) {
            val movedUri = ops.tryMoveDocument(oldUri, oldParentUriResult.uri, backupParentUri, displayName)
            if (movedUri != null) {
                return MirrorFileRef(uri = movedUri.toString(), relativePath = backupRelativePath)
            }
        }
        // 2. 回退：read old → createText 到 backup → delete old（真正删 old 腾空最终路径）
        return ops.backupCommittedViaCopy(oldUri, backupParentUri, backupRelativePath, mimeType, displayName)
    }

    // #649 评论 5564820566 问题 3：两步 journalable backup — SAF 路径

    override fun prepareBackup(
        txId: String,
        old: MirrorFileRef,
        mimeType: String,
    ): BackupReadyRef? {
        val oldUri = ops.tryParseUri(old.uri) ?: return null
        val backupBase = ops.backupBasePath(txId)
        val backupRelativePath = "$backupBase/${old.relativePath}"
        val parent = old.relativePath.substringBeforeLast('/', "")
        val relativeDir = if (parent.isBlank()) backupBase else "$backupBase/$parent"
        val displayName = old.relativePath.substringAfterLast('/')
        val backupParentUri = ops.ensureDirectory(relativeDir) ?: return null
        val oldParentPath = old.relativePath.substringBeforeLast('/', "")
        val oldParentUriResult = ops.resolveOldParent(oldParentPath)
        // 1. 优先尝试 moveDocument（原子 move）
        if (oldParentUriResult is DirectoryLookupResult.Found) {
            val movedUri = ops.tryMoveDocument(oldUri, oldParentUriResult.uri, backupParentUri, displayName)
            if (movedUri != null) {
                return BackupReadyRef(
                    backupRef = MirrorFileRef(uri = movedUri.toString(), relativePath = backupRelativePath),
                    vacated = true,
                )
            }
        }
        // 2. 回退：只复制 old → backup，不删 old
        val content = ops.readTextFromUri(oldUri) ?: return null
        val fileUri =
            try {
                DocumentsContract.createDocument(contentResolver, backupParentUri, mimeType, displayName)
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "createDocument failed for prepareBackup $displayName: ${e.message}")
                return null
            } ?: return null
        if (!ops.writeToUri(fileUri, content)) {
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
        val uri = ops.tryParseUri(old.uri) ?: return false // URI 无效 → 无法确认 old 是否已腾空，返回 false
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

    override fun resolveBackup(
        txId: String,
        relativePath: String,
    ): MirrorFileRef? {
        if (!isSupported()) return null
        // #649 评论 5563798095：检查 backup 路径是否已有文件，避免崩溃窗口后重复 backup。
        val backupBase = ops.backupBasePath(txId)
        val backupRelativePath = "$backupBase/$relativePath"
        // backup 位于 staging 目录内，需要逐级 findDirectory
        val backupParentPath = backupRelativePath.substringBeforeLast('/', "")
        val displayName = backupRelativePath.substringAfterLast('/')
        val parentUriResult = ops.findDirectory(backupParentPath)
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
        val backupBase = ops.backupBasePath(txId)
        val backupRelativePath = "$backupBase/$relativePath"
        // backup 位于 staging 目录内，需要逐级 findDirectory
        val backupParentPath = backupRelativePath.substringBeforeLast('/', "")
        val displayName = backupRelativePath.substringAfterLast('/')
        val parentUriResult = ops.findDirectory(backupParentPath)
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

    override fun restoreBackup(
        backup: MirrorFileRef,
        finalRelativePath: String,
        mimeType: String,
        expectedOldContentHash: String?,
    ): RestoreBackupResult {
        if (!isSupported()) return RestoreBackupResult.Failed(IllegalStateException("backend not supported"))
        // #649 评论 5566303837 问题 4：带身份校验的 restoreBackup
        val existingResult = verifyExistingFinal(finalRelativePath, expectedOldContentHash)
        if (existingResult != null) return existingResult
        // final 不存在（Missing），继续 restore：read backup → create final → write
        val backupUri = ops.tryParseUri(backup.uri) ?: return RestoreBackupResult.Failed(null)
        val content = ops.readTextFromUri(backupUri) ?: return RestoreBackupResult.Failed(null)
        val relativeDir = finalRelativePath.substringBeforeLast('/', "")
        val displayName = finalRelativePath.substringAfterLast('/')
        val parentUri = ops.ensureDirectory(relativeDir) ?: return RestoreBackupResult.Failed(null)
        val fileUri =
            try {
                DocumentsContract.createDocument(contentResolver, parentUri, mimeType, displayName)
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "createDocument failed for restore $displayName: ${e.message}")
                return RestoreBackupResult.Failed(e)
            } ?: return RestoreBackupResult.Failed(null)
        if (!ops.writeToUri(fileUri, content)) {
            try {
                DocumentsContract.deleteDocument(contentResolver, fileUri)
            } catch (_: Exception) {
            }
            return RestoreBackupResult.Failed(null)
        }
        return RestoreBackupResult.Restored(MirrorFileRef(uri = fileUri.toString(), relativePath = finalRelativePath))
    }

    /**
     * 校验 final 位置是否已有文件。
     *
     * @return null 表示 final 明确不存在（[MirrorLookupResult.Missing]），应继续 restore；
     *   非 null 表示应直接返回该 [RestoreBackupResult]。
     */
    private fun verifyExistingFinal(
        finalRelativePath: String,
        expectedOldContentHash: String?,
    ): RestoreBackupResult? {
        when (val existing = lookup(finalRelativePath)) {
            is MirrorLookupResult.Found -> return verifyFoundFinal(existing.ref, expectedOldContentHash)
            is MirrorLookupResult.Failed -> return RestoreBackupResult.Failed(existing.cause)
            is MirrorLookupResult.Missing -> return null // 继续 restore
        }
        return null
    }

    /** final 已存在时校验内容身份（#651 评论 5592465805：拆分降低嵌套深度）。 */
    private fun verifyFoundFinal(
        ref: MirrorFileRef,
        expectedOldContentHash: String?,
    ): RestoreBackupResult {
        if (expectedOldContentHash == null) {
            return RestoreBackupResult.AlreadyRestored(ref)
        }
        val finalHashResult = readTextAndHash(ref)
        if (finalHashResult == null) {
            return RestoreBackupResult.Failed(null)
        }
        val (_, hash) = finalHashResult
        return if (hash == expectedOldContentHash) {
            RestoreBackupResult.AlreadyRestored(ref)
        } else {
            RestoreBackupResult.Conflict
        }
    }

    /** 记录 createDocument 失败日志（#651 评论 5592465805：消除 StringLiteralDuplication）。 */
    private fun logCreateDocumentFailed(displayName: String, e: Exception) {
        DiagnosticsLogger.w(TAG, "createDocument failed for $displayName: ${e.message}")
    }

    override fun readTextAndHash(ref: MirrorFileRef): Pair<String, String>? {
        val uri = ops.tryParseUri(ref.uri) ?: return null
        val content = ops.readTextFromUri(uri) ?: return null
        return Pair(content, computeContentHash(content))
    }

    override fun rollback(txId: String): Boolean {
        if (!isSupported()) return false
        // #649 评论 5562462046 问题 6：rollback 只查不创建。
        // #649 评论 5574521549 问题 2：返回 deleteDocument() 自己返回的 Boolean，
        // 不再丢掉 provider 的删除结果。旧实现 `deleteDocument(...); true` 把 provider
        // 返回 false（删除失败）当成功，cleanup 会误删 journal 留下事务垃圾。
        // 规则：明确不存在 = 成功；明确删除成功 = 成功；状态不明/删除失败 = false。
        val stagingDir = "$STAGING_DIR/$txId"
        val stagingUriResult = ops.findDirectory(stagingDir)
        if (stagingUriResult is DirectoryLookupResult.Found) {
            return try {
                DocumentsContract.deleteDocument(contentResolver, stagingUriResult.uri)
            } catch (_: FileNotFoundException) {
                true // 明确不存在 = 成功
            } catch (_: Exception) {
                false // 状态不明/删除失败
            }
        }
        // 目录不存在或查询失败
        return stagingUriResult is DirectoryLookupResult.Missing
    }

    companion object {
        private const val TAG = "DocumentTreeMirrorStorage"
        private const val STAGING_DIR = ".staging"
        private const val BACKUP_DIR = "backup"
    }
}
