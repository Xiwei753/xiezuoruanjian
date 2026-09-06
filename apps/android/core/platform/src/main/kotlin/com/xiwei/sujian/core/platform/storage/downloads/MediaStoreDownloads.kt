package com.xiwei.sujian.core.platform.storage.downloads

import android.content.ContentResolver
import android.content.ContentUris
import android.content.ContentValues
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.MediaStore
import java.io.File
import java.io.IOException
import java.io.OutputStream

/**
 * MediaStoreDownloads — 通过 MediaStore API 写入 Download/Sujian 镜像。
 *
 * #649 评论 5559759935 / 5560685734：Download 目录对用户可见且卸载后保留，
 * 用于把 Core 私有数据单向异步发布成用户可读的 `.md` 正文。
 *
 * 入口：`Download/Sujian/`（公共 Download 目录下的 `Sujian` 子目录，
 * 与私有 `filesDir/sujian` 解耦）。MediaStore 只负责这套文件；
 * 私有真相源仍是 `filesDir/sujian`。
 *
 * ## 写入流程
 * 1. [ensureMirrorRoot] 创建 `Download/Sujian/_meta/` 与 `Download/Sujian/projects/`（幂等）。
 * 2. [writeText] 写入任意相对路径下的文本文件（自动创建父目录）。
 * 3. [delete] 删除文件。
 *
 * 所有 I/O 都通过 [ContentResolver] 走 MediaStore，不直接访问文件系统路径。
 * 不需要 `MANAGE_EXTERNAL_STORAGE`；对 Android 10+ 使用 `RELATIVE_PATH` 指定
 * `Environment.DIRECTORY_DOWNLOADS + "/Sujian"`。
 *
 * ## 架构约束
 * - 位于 `:core:platform`，只封装 Android 系统能力，不放 Compose、UniFFI、业务 Repository。
 * - 不把 `content://` URI 传给 Rust——只把文本写入 MediaStore，由 Publisher 调用。
 */
class MediaStoreDownloads(
    private val contentResolver: ContentResolver,
) {
    /**
     * 镜像根目录：`Download/Sujian/`。
     *
     * 对 Android 10+，通过 [ContentValues] 的 [MediaStore.Downloads.RELATIVE_PATH] 指定
     * `Environment.DIRECTORY_DOWNLOADS + "/Sujian"`。对 Android 9 及以下，
     * 本类不直接支持（API 28 之前 MediaStore.Downloads 不可用；调用方应跳过发布）。
     */
    fun ensureMirrorRoot(): MirrorRootResult {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
            return MirrorRootResult.NotSupported("MediaStore.Downloads requires API 29+")
        }
        // 创建 _meta 子目录（幂等）
        val metaDirCreated = ensureDirectory(META_DIR_NAME)
        if (!metaDirCreated) {
            return MirrorRootResult.Failed("Failed to create _meta directory")
        }
        // 创建 projects 子目录（幂等）
        val projectsDirCreated = ensureDirectory(PROJECTS_DIR_NAME)
        if (!projectsDirCreated) {
            return MirrorRootResult.Failed("Failed to create projects directory")
        }
        return MirrorRootResult.Success
    }

    /**
     * 写入文本到 [relativePath] 下的文件（相对镜像根）。
     *
     * 例：`writeText("projects/<id>/volumes/<vid>/chapters/<cid>.md", "# 章节内容")`
     *
     * 自动创建父目录（幂等）；文件已存在则覆盖。返回写入的 URI，失败返回 null。
     */
    fun writeText(relativePath: String, content: String): Uri? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return null
        if (relativePath.isEmpty()) return null

        val fullPath = "$MIRROR_ROOT_NAME/$relativePath"
        val parentDir = File(relativePath).parent ?: ""
        // 确保父目录存在
        if (parentDir.isNotEmpty()) {
            val dirCreated = ensureDirectoryRecursive(parentDir)
            if (!dirCreated) return null
        }

        val values = ContentValues().apply {
            put(MediaStore.Downloads.DISPLAY_NAME, File(relativePath).name)
            put(MediaStore.Downloads.MIME_TYPE, "text/markdown")
            put(MediaStore.Downloads.RELATIVE_PATH, "${Environment.DIRECTORY_DOWNLOADS}/$fullPath".substringBeforeLast("/"))
            put(MediaStore.Downloads.IS_PENDING, 0)
        }

        // 先查询是否已存在同名文件
        val existingUri = findExistingFile(relativePath)
        return if (existingUri != null) {
            // 覆盖现有文件
            writeContentToUri(existingUri, content)?.let { existingUri }
        } else {
            // 插入新文件
            val insertUri = contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
            if (insertUri == null) {
                return null
            }
            writeContentToUri(insertUri, content)?.let { insertUri }
        }
    }

    /**
     * 删除 [relativePath] 下的文件。
     *
     * 返回 true 表示成功或文件已不存在（幂等）。
     */
    fun delete(relativePath: String): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return false
        val existingUri = findExistingFile(relativePath) ?: return true
        return try {
            contentResolver.delete(existingUri, null, null) > 0
        } catch (e: Exception) {
            false
        }
    }

    /**
     * 确保目录存在（仅限直接子目录，递归由调用方处理）。
     */
    private fun ensureDirectory(dirName: String): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return false
        val existing = findDirectory(dirName)
        if (existing != null) return true

        val values = ContentValues().apply {
            put(MediaStore.Downloads.DISPLAY_NAME, dirName)
            put(MediaStore.Downloads.MIME_TYPE, DocumentsContract.Document.MIME_TYPE_DIR)
            put(MediaStore.Downloads.RELATIVE_PATH, "${Environment.DIRECTORY_DOWNLOADS}/$MIRROR_ROOT_NAME")
            put(MediaStore.Downloads.IS_PENDING, 0)
        }
        return try {
            val uri = contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
            uri != null
        } catch (e: Exception) {
            false
        }
    }

    /**
     * 递归确保目录存在。
     */
    private fun ensureDirectoryRecursive(relativePath: String): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return false
        val parts = relativePath.split("/").filter { it.isNotEmpty() }
        var currentPath = ""
        for (part in parts) {
            val dirPath = if (currentPath.isEmpty()) part else "$currentPath/$part"
            val created = ensureDirectory(dirPath)
            if (!created) return false
            currentPath = dirPath
        }
        return true
    }

    /**
     * 查询已存在的文件 URI。
     */
    private fun findExistingFile(relativePath: String): Uri? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return null
        val fullPath = "$MIRROR_ROOT_NAME/$relativePath"
        val collection = MediaStore.Downloads.EXTERNAL_CONTENT_URI
        val projection = arrayOf(MediaStore.Downloads._ID)
        val selection = "${MediaStore.Downloads.RELATIVE_PATH} = ?"
        val selectionArgs = arrayOf("${Environment.DIRECTORY_DOWNLOADS}/${fullPath.substringBeforeLast("/")}")

        return try {
            val cursor = contentResolver.query(collection, projection, selection, selectionArgs, null)
            cursor?.use { c ->
                if (c.moveToFirst()) {
                    val id = c.getLong(c.getColumnIndexOrThrow(MediaStore.Downloads._ID))
                    ContentUris.withAppendedId(collection, id)
                } else {
                    null
                }
            }
        } catch (e: Exception) {
            null
        }
    }

    /**
     * 查询已存在的目录 URI。
     */
    private fun findDirectory(dirName: String): Uri? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return null
        val collection = MediaStore.Downloads.EXTERNAL_CONTENT_URI
        val projection = arrayOf(MediaStore.Downloads._ID)
        val selection = "${MediaStore.Downloads.DISPLAY_NAME} = ? AND ${MediaStore.Downloads.MIME_TYPE} = ?"
        val selectionArgs = arrayOf(dirName, DocumentsContract.Document.MIME_TYPE_DIR)

        return try {
            val cursor = contentResolver.query(collection, projection, selection, selectionArgs, null)
            cursor?.use { c ->
                if (c.moveToFirst()) {
                    val id = c.getLong(c.getColumnIndexOrThrow(MediaStore.Downloads._ID))
                    ContentUris.withAppendedId(collection, id)
                } else {
                    null
                }
            }
        } catch (e: Exception) {
            null
        }
    }

    /**
     * 把内容写入 URI。
     */
    private fun writeContentToUri(uri: Uri, content: String): Uri? {
        return try {
            val outputStream = contentResolver.openOutputStream(uri)
            if (outputStream == null) {
                return null
            }
            outputStream.use { os ->
                os.write(content.toByteArray(Charsets.UTF_8))
            }
            uri
        } catch (e: IOException) {
            null
        }
    }

    /**
     * 镜像根目录初始化结果。
     */
    sealed class MirrorRootResult {
        object Success : MirrorRootResult()
        data class Failed(val reason: String) : MirrorRootResult()
        data class NotSupported(val reason: String) : MirrorRootResult()
    }

    companion object {
        private const val MIRROR_ROOT_NAME = "Sujian"
        private const val META_DIR_NAME = "_meta"
        private const val PROJECTS_DIR_NAME = "projects"
    }
}
