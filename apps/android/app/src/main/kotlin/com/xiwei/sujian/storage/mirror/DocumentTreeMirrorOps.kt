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
 * 文件读写等底层文档操作。不含事务语义，不实现 [ReadableMirrorStorage]。
 *
 * Issue #667：事务专用方法（tryMoveDocument、backupBasePath、resolveOldParent、
 * backupCommittedViaCopy）已随事务能力移至 [MirrorTransactionWorkspace] 一并移除。
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
    ): Boolean =
        try {
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

    companion object {
        private const val TAG = "DocumentTreeMirrorOps"
    }
}
