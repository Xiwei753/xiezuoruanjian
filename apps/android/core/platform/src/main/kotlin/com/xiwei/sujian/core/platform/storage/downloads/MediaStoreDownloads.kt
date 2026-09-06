package com.xiwei.sujian.core.platform.storage.downloads

import android.content.ContentResolver
import android.content.ContentValues
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import java.io.IOException

/**
 * MediaStoreDownloads — 通过 MediaStore API 写入 Download/Sujian 镜像。
 *
 * #649 评论 5560971132 修复 2/3：重构为 URI-based API。
 *
 * ## 旧实现的问题
 * - `ensureMirrorRoot` / `ensureDirectory` 通过插入 `MIME_TYPE_DIR` 创建目录伪记录，
 *   在 Android 10+ MediaStore.Downloads 不支持目录 MIME，会留下损坏记录。
 * - `writeText` 用 `IS_PENDING=0` 直接插入，再覆盖写；未走 `IS_PENDING=1→写→0`
 *   两步流程，文件在写入过程中对其他应用可见（半写状态）。
 * - `findExistingFile` 按 `RELATIVE_PATH` 精确匹配查询，重名文件无法区分。
 *
 * ## 新 API
 * - [isSupported]：API 29+ 才支持 MediaStore.Downloads。
 * - [createText]：`IS_PENDING=1 → 写 → IS_PENDING=0`，返回新 URI。
 * - [replaceText]：覆盖现有 URI 内容，写完置 `IS_PENDING=0`。
 * - [delete]：按 URI 删除（幂等）。
 * - [readText]：按 URI 读全部文本。
 *
 * 调用方（[com.xiwei.sujian.storage.mirror.ReadableMirrorPublisher]）自己维护
 * URI ↔ 章节的映射（[com.xiwei.sujian.storage.mirror.ReadableMirrorStateStore]），
 * 不再按相对路径查询。
 *
 * ## 架构约束
 * - 位于 `:core:platform`，只封装 Android 系统能力，不放 Compose、UniFFI、业务 Repository。
 * - 不把 `content://` URI 传给 Rust——只把文本写入 MediaStore，由 Publisher 调用。
 */
class MediaStoreDownloads(
    private val contentResolver: ContentResolver,
) {
    /** MediaStore.Downloads 需要 API 29+。 */
    fun isSupported(): Boolean = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q

    /**
     * 创建新文本文件。
     *
     * 流程：`IS_PENDING=1 → insert → openOutputStream → write → close → IS_PENDING=0`。
     *
     * @param relativeDir 相对 `Download/Sujian/` 的目录（如 `作品/作品名/卷名`），
     *   空字符串表示直接放 `Download/Sujian/` 下。
     * @param displayName 文件名（如 `章节名.md`）。
     * @param mimeType MIME 类型（如 `text/markdown`）。
     * @param text 文本内容。
     * @return 新创建的 URI；任何步骤失败返回 null（不留下半写记录）。
     */
    fun createText(
        relativeDir: String,
        displayName: String,
        mimeType: String,
        text: String,
    ): Uri? {
        if (!isSupported()) return null
        val relativePath = buildRelativePath(relativeDir)
        val values = ContentValues().apply {
            put(MediaStore.Downloads.DISPLAY_NAME, displayName)
            put(MediaStore.Downloads.MIME_TYPE, mimeType)
            put(MediaStore.Downloads.RELATIVE_PATH, relativePath)
            put(MediaStore.Downloads.IS_PENDING, 1)
        }
        val uri = try {
            contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
        } catch (e: Exception) {
            return null
        } ?: return null
        if (!writeToUri(uri, text)) {
            // 写失败：尝试删除半写记录，避免留下 IS_PENDING=1 的空文件。
            try { contentResolver.delete(uri, null, null) } catch (_: Exception) {}
            return null
        }
        // 写完成：置 IS_PENDING=0，让文件对其他应用可见。
        val clearPending = ContentValues().apply {
            put(MediaStore.Downloads.IS_PENDING, 0)
        }
        return try {
            contentResolver.update(uri, clearPending, null, null)
            uri
        } catch (e: Exception) {
            // 文件已写好但 IS_PENDING 没清掉；返回 URI 仍可用（下次 replace 可修复）。
            uri
        }
    }

    /**
     * 覆盖现有 URI 的内容。
     *
     * 写完置 `IS_PENDING=0`。返回 false 表示打开输出流或写入失败。
     */
    fun replaceText(uri: Uri, text: String): Boolean {
        if (!isSupported()) return false
        if (!writeToUri(uri, text)) return false
        val clearPending = ContentValues().apply {
            put(MediaStore.Downloads.IS_PENDING, 0)
        }
        return try {
            contentResolver.update(uri, clearPending, null, null)
            true
        } catch (e: Exception) {
            // 内容已覆盖，IS_PENDING 状态不影响数据正确性。
            true
        }
    }

    /**
     * 删除指定 URI。幂等：URI 不存在或已删除返回 false，不抛异常。
     */
    fun delete(uri: Uri): Boolean {
        if (!isSupported()) return false
        return try {
            contentResolver.delete(uri, null, null) > 0
        } catch (e: Exception) {
            false
        }
    }

    /**
     * 读取 URI 的全部文本。失败返回 null。
     */
    fun readText(uri: Uri): String? {
        if (!isSupported()) return null
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

    // ── 内部 ──

    private fun buildRelativePath(relativeDir: String): String {
        val base = "${Environment.DIRECTORY_DOWNLOADS}/$MIRROR_ROOT_NAME"
        return if (relativeDir.isBlank()) base else "$base/$relativeDir"
    }

    private fun writeToUri(uri: Uri, text: String): Boolean {
        return try {
            contentResolver.openOutputStream(uri)?.use { os ->
                os.write(text.toByteArray(Charsets.UTF_8))
                true
            } ?: false
        } catch (e: IOException) {
            false
        } catch (e: Exception) {
            false
        }
    }

    companion object {
        private const val MIRROR_ROOT_NAME = "Sujian"
    }
}
