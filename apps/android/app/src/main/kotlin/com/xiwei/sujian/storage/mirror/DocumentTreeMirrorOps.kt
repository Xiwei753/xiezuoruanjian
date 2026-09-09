package com.xiwei.sujian.storage.mirror

import android.content.ContentResolver
import android.net.Uri
import android.provider.DocumentsContract
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.platform.storage.documents.DocumentTreeReader
import java.io.FileNotFoundException
import java.io.IOException

/**
 * SAF 文档树纯操作 helper（#651 评论 5592465805：从 [DocumentTreeMirrorStorage] 拆出，
 * 解决 LargeClass / TooManyFunctions）。
 *
 * 持有 [treeUri] / [contentResolver] / [documentTreeReader]，提供目录查找/创建、
 * 文件读写、跨目录移动等底层文档操作。不含事务语义，不实现 [ReadableMirrorStorage]。
 */
internal class DocumentTreeMirrorOps(
    internal val treeUri: Uri,
    private val contentResolver: ContentResolver,
    private val documentTreeReader: DocumentTreeReader,
) {
    /** 逐级在 [treeUri] 下查找或创建 [relativeDir] 指定的目录路径。 */
    fun ensureDirectory(relativeDir: String): Uri? {
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
        try {
            val children = documentTreeReader.listChildren(parentUri)
            val existing = children.find { it.isDirectory && it.name == dirName }
            if (existing != null) return existing.uri
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "listChildren failed for $dirName: ${e.message}")
            return null
        }
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

    fun writeToUri(
        uri: Uri,
        text: String,
    ): Boolean = try {
        contentResolver.openOutputStream(uri)?.use { os ->
            os.write(text.toByteArray(Charsets.UTF_8))
            true
        } ?: false
    } catch (e: Exception) {
        DiagnosticsLogger.w(TAG, "writeToUri failed: ${e.message}")
        false
    }

    fun tryParseUri(uriString: String): Uri? =
        try {
            Uri.parse(uriString)
        } catch (_: Exception) {
            null
        }

    fun readTextFromUri(uri: Uri): String? =
        try {
            contentResolver.openInputStream(uri)?.use { input ->
                input.readBytes().toString(Charsets.UTF_8)
            }
        } catch (_: IOException) {
            null
        } catch (_: Exception) {
            null
        }

    fun tryMoveDocument(
        stagingUri: Uri,
        stagingParentUri: Uri,
        targetParentUri: Uri,
        displayName: String,
    ): Uri? {
        val movedUri =
            try {
                DocumentsContract.moveDocument(contentResolver, stagingUri, stagingParentUri, targetParentUri)
            } catch (_: UnsupportedOperationException) {
                null
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "moveDocument failed: ${e.message}")
                null
            } ?: return null
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

    private fun getDisplayName(uri: Uri): String? =
        try {
            contentResolver
                .query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)
                ?.use { cursor ->
                    if (cursor.moveToFirst()) cursor.getString(0) else null
                }
        } catch (e: Exception) {
            DiagnosticsLogger.w(TAG, "getDisplayName failed: ${e.message}")
            null
        }

    fun findDirectory(relativeDir: String): DirectoryLookupResult {
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

    private fun findChildDir(
        parentUri: Uri,
        dirName: String,
    ): DirectoryLookupResult =
        try {
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

    fun DirectoryLookupResult.getUriOrThrow(): Uri =
        when (this) {
            is DirectoryLookupResult.Found -> uri
            is DirectoryLookupResult.Missing -> throw FileNotFoundException("directory not found")
            is DirectoryLookupResult.Failed -> throw cause ?: IOException("directory lookup failed")
        }

    fun resolveInTree(
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

    /** 事务 backup 基路径：`.staging/<txId>/backup`。 */
    fun backupBasePath(txId: String): String = "$STAGING_DIR/$txId/$BACKUP_DIR"

    /** 解析 old 的父目录 URI（空路径表示根 tree）。 */
    fun resolveOldParent(oldParentPath: String): DirectoryLookupResult =
        if (oldParentPath.isBlank()) {
            DirectoryLookupResult.Found(treeUri)
        } else {
            findDirectory(oldParentPath)
        }

    /** 回退路径：read old → create backup → write → delete old 腾空最终路径。 */
    fun backupCommittedViaCopy(
        oldUri: Uri,
        backupParentUri: Uri,
        backupRelativePath: String,
        mimeType: String,
        displayName: String,
    ): MirrorFileRef? {
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
        if (!try {
                DocumentsContract.deleteDocument(contentResolver, oldUri)
            } catch (_: Exception) {
                false
            }
        ) {
            try {
                DocumentsContract.deleteDocument(contentResolver, fileUri)
            } catch (_: Exception) {
            }
            return null
        }
        return MirrorFileRef(uri = fileUri.toString(), relativePath = backupRelativePath)
    }

    companion object {
        private const val TAG = "DocumentTreeMirrorOps"
        internal const val STAGING_DIR = ".staging"
        internal const val BACKUP_DIR = "backup"
    }
}
