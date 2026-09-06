package com.xiwei.sujian.core.platform.storage.documents

import android.content.ContentResolver
import android.net.Uri
import android.provider.DocumentsContract
import java.io.File
import java.io.IOException

/**
 * DocumentTreeReader — SAF 文档树读取封装。
 *
 * 只封装 [ContentResolver] / [DocumentsContract] / [DocumentFile]，不依赖 Compose、
 * UniFFI、:app。用于旧版共享存储迁移与 Download 镜像恢复：通过 SAF 读取用户选中的
 * 文档树，再由上层通过 Core API 重建数据。
 *
 * 架构约束（apps/android/AGENTS.md）：`:core:platform` 只封装 Android 系统能力，
 * 不放 Compose、UniFFI 业务调用、业务 Repository。
 *
 * 安全约束：不把 `content://` URI 传给 Rust，不尝试把 URI 转成真实
 * `/storage/emulated/0/...` 路径——本类只把 SAF 文档读成内存文本或复制到应用私有 [File]。
 */
data class DocumentEntry(
    val name: String,
    val uri: Uri,
    val isDirectory: Boolean,
    val mimeType: String?,
)

class DocumentTreeReader(private val contentResolver: ContentResolver) {
    /**
     * 列出 [tree] 目录的直接子项。
     *
     * [tree] 可以是 `ActivityResultContracts.OpenDocumentTree()` 返回的根 tree URI，
     * 也可以是 [listChildren] 返回的子目录 URI（由
     * [DocumentsContract.buildDocumentUriUsingTree] 构造）。
     *
     * @throws IOException 查询失败时抛出（不吞异常）
     */
    fun listChildren(tree: Uri): List<DocumentEntry> {
        val parentDocId =
            if (DocumentsContract.isTreeUri(tree)) {
                DocumentsContract.getTreeDocumentId(tree)
            } else {
                DocumentsContract.getDocumentId(tree)
            }
        val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(tree, parentDocId)
        val projection = arrayOf(
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_MIME_TYPE,
        )
        val cursor =
            contentResolver.query(childrenUri, projection, null, null, null)
                ?: throw IOException("Failed to query children of $tree")
        val entries = mutableListOf<DocumentEntry>()
        cursor.use { c ->
            while (c.moveToNext()) {
                val docId = c.getString(
                    c.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_DOCUMENT_ID),
                )
                val name = c.getString(
                    c.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_DISPLAY_NAME),
                )
                val mime = c.getString(
                    c.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_MIME_TYPE),
                )
                val isDir = mime == DocumentsContract.Document.MIME_TYPE_DIR
                val childUri = DocumentsContract.buildDocumentUriUsingTree(tree, docId)
                entries.add(
                    DocumentEntry(name = name, uri = childUri, isDirectory = isDir, mimeType = mime),
                )
            }
        }
        return entries
    }

    /**
     * 以 UTF-8 读取 [uri] 的文本内容。
     *
     * @throws IOException 打开流失败时抛出
     */
    fun readText(uri: Uri): String {
        val stream = contentResolver.openInputStream(uri)
            ?: throw IOException("Failed to open input stream for $uri")
        return stream.use { it.bufferedReader(Charsets.UTF_8).readText() }
    }

    /**
     * 递归把 [sourceTree] 文档树复制到普通 [targetDir]。
     *
     * 目录创建同名子 [File]，文件用 [ContentResolver.openInputStream] 复制到目标 [File]。
     * 用于把 SAF 可读的旧数据完整落到应用私有 [File]，再由 Core 接管。
     *
     * @throws IOException 复制过程中任意 I/O 失败时抛出（调用方负责清理已落地的部分）
     */
    fun copyTree(sourceTree: Uri, targetDir: File) {
        if (!targetDir.exists() && !targetDir.mkdirs()) {
            throw IOException("Failed to create target directory: $targetDir")
        }
        for (child in listChildren(sourceTree)) {
            val targetChild = File(targetDir, child.name)
            if (child.isDirectory) {
                copyTree(child.uri, targetChild)
            } else {
                copyFile(child.uri, targetChild)
            }
        }
    }

    /** 把单个 SAF 文档 [sourceUri] 复制到普通 [targetFile]。 */
    private fun copyFile(sourceUri: Uri, targetFile: File) {
        val input = contentResolver.openInputStream(sourceUri)
            ?: throw IOException("Failed to open input stream for $sourceUri")
        input.use { src ->
            targetFile.outputStream().use { dst ->
                src.copyTo(dst)
            }
        }
    }
}
