package com.xiwei.sujian.storage.mirror

import android.content.ContentResolver
import android.net.Uri
import android.os.Environment
import android.provider.MediaStore
import com.xiwei.sujian.core.platform.storage.downloads.MediaStoreDownloads

/**
 * MediaStore 后端的 [ReadableMirrorStorage] 实现。
 *
 * #649 评论 5561465552 第 3 点。
 *
 * 包装 [MediaStoreDownloads]，把 [Uri] 转成 [MirrorFileRef]。
 * - [createText] 调 [MediaStoreDownloads.createText]，返回 [MirrorFileRef]。
 * - [replaceText] 调 [MediaStoreDownloads.replaceText]（已修复 set pending 返回值检查）。
 * - [delete] 调 [MediaStoreDownloads.delete]。
 *
 * Issue #667：事务能力（stage/backup/promote/rollback）已移至
 * [MirrorTransactionWorkspace]（私有目录）。本类只负责最终用户可见文件的读写和查询。
 *
 * ## 架构约束
 * - 位于 `:app` 的 `storage/mirror` 包，依赖 `:core:platform` 的 [MediaStoreDownloads]（合法）。
 * - 不放 Compose、UniFFI 业务调用。
 * - 不把 `content://` URI 传给 Rust。
 *
 * @param mediaStore 被包装的 [MediaStoreDownloads]（由调用方注入 [ContentResolver]）。
 * @param contentResolver 应用 [ContentResolver]（用于 query 查找已有文件）。
 */
class MediaStoreMirrorStorage(
    private val mediaStore: MediaStoreDownloads,
    private val contentResolver: ContentResolver,
) : ReadableMirrorStorage {
    override fun createText(
        relativeDir: String,
        displayName: String,
        mimeType: String,
        text: String,
    ): MirrorFileRef? {
        val uri = mediaStore.createText(relativeDir, displayName, mimeType, text) ?: return null
        val relativePath = if (relativeDir.isBlank()) displayName else "$relativeDir/$displayName"
        return MirrorFileRef(uri = uri.toString(), relativePath = relativePath)
    }

    override fun replaceText(
        ref: MirrorFileRef,
        text: String,
    ): Boolean {
        val uri = tryParseUri(ref.uri) ?: return false
        return mediaStore.replaceText(uri, text)
    }

    /**
     * 删除引用指向的文件（幂等）。
     *
     * #649 评论 5564624383 问题 5：明确区分"不存在 → true"和"异常 → false"。
     * 幂等只应该是"明确不存在"返回 true，不是"任何异常都算成功"。
     * 例如 SAF 权限丢失、provider I/O 错误时，如果返回 true，
     * cleanupCommittedTransaction() 会认为清理完成并删除 journal，实际旧文件仍在。
     *
     * 改法：按 ref.uri 本身判断并删除，不再用 relativePath 查询。
     * 同一路径可能已是恢复后的旧 manifest：路径查询 FOUND，但 ref.uri 已不存在。
     *
     * 新逻辑：
     * 1. 解析 ref.uri 得到 Uri
     * 2. 直接用 contentResolver.delete(uri, null, null) 尝试删除
     * 3. 删除成功（返回 1）→ true
     * 4. URI 不存在（删除返回 0）→ true（幂等：目标已达到）
     * 5. SecurityException / 其他异常 → false
     */
    override fun delete(ref: MirrorFileRef): Boolean {
        val uri = tryParseUri(ref.uri) ?: return false // URI 无效 → 无法确认状态，返回 false
        // #649 评论 5564820566 问题 4：不再把 "后端不可用" 当删除成功。
        // 旧代码 `if (!mediaStore.isSupported()) return true` 会让 cleanup 误认为文件已删。
        return try {
            val deleted = contentResolver.delete(uri, null, null)
            // deleted > 0: 成功删除一行 → true
            // deleted == 0: URI 不存在（已删除或从未存在）→ 幂等，目标已达到 → true
            deleted >= 0
        } catch (_: SecurityException) {
            false // 权限异常 → 删除失败
        } catch (_: Exception) {
            false // I/O 异常 → 删除失败
        }
    }

    override fun isSupported(): Boolean = mediaStore.isSupported()

    override fun resolve(relativePath: String): MirrorFileRef? {
        // #649 评论 5563333323 缺口 1：只查不创建，用 MediaStore query RELATIVE_PATH + DISPLAY_NAME。
        // #649 评论 5563798095：异常情况下命中多条时不随便拿第一条绑定章节，直接返回 null。
        if (!mediaStore.isSupported()) return null
        val directory = mediaStoreDirectory(relativePath)
        val displayName = relativePath.substringAfterLast('/')
        // 内联 queryByPathAndName（#651 评论 5592465805：减少函数数量）
        val cursor =
            try {
                contentResolver
                    .query(
                        MediaStore.Downloads.EXTERNAL_CONTENT_URI,
                        arrayOf(MediaStore.Downloads._ID),
                        SELECTION_BY_PATH_AND_NAME,
                        arrayOf(directory, displayName),
                        null,
                    )
            } catch (_: Exception) {
                return null
            }
        return cursor?.use { c ->
            if (!c.moveToFirst()) return null
            if (c.count > 1) {
                android.util.Log.w(
                    TAG,
                    "resolve: multiple matches for $relativePath, " +
                        "count=${c.count}, returning null to avoid binding wrong file",
                )
                return null
            }
            val id = c.getLong(0)
            val uri = Uri.withAppendedPath(MediaStore.Downloads.EXTERNAL_CONTENT_URI, id.toString())
            MirrorFileRef(uri = uri.toString(), relativePath = relativePath)
        }
    }

    /**
     * 三态查询实现（#649 评论 5565067997 修复 5）。
     *
     * - 查询成功且有唯一匹配 → [MirrorLookupResult.Found]
     * - 查询成功但无匹配 → [MirrorLookupResult.Missing]
     * - 查询抛异常（SecurityException / provider I/O）→ [MirrorLookupResult.Failed]
     * - 命中多条（数据异常）→ [MirrorLookupResult.Failed]（不绑定错误文件）
     * - 后端不可用 → [MirrorLookupResult.Failed]（不静默当 Missing）
     */
    override fun lookup(relativePath: String): MirrorLookupResult {
        if (!mediaStore.isSupported()) {
            // #649 评论 5565067997 修复 5：后端不可用是查询失败，不是 Missing。
            return MirrorLookupResult.Failed(IllegalStateException("MediaStore backend not supported"))
        }
        val directory = mediaStoreDirectory(relativePath)
        val displayName = relativePath.substringAfterLast('/')
        return try {
            contentResolver
                .query(
                    MediaStore.Downloads.EXTERNAL_CONTENT_URI,
                    arrayOf(MediaStore.Downloads._ID),
                    SELECTION_BY_PATH_AND_NAME,
                    arrayOf(directory, displayName),
                    null,
                )
                ?.use { cursor ->
                    if (!cursor.moveToFirst()) {
                        MirrorLookupResult.Missing
                    } else if (cursor.count > 1) {
                        android.util.Log.w(
                            TAG,
                            "lookup: multiple matches for $relativePath, " +
                                "count=${cursor.count}, returning Failed to avoid binding wrong file",
                        )
                        MirrorLookupResult.Failed(IllegalStateException("multiple matches for $relativePath"))
                    } else {
                        val id = cursor.getLong(0)
                        val uri = Uri.withAppendedPath(MediaStore.Downloads.EXTERNAL_CONTENT_URI, id.toString())
                        MirrorLookupResult.Found(MirrorFileRef(uri = uri.toString(), relativePath = relativePath))
                    }
                } ?: MirrorLookupResult.Failed(IllegalStateException("contentResolver.query returned null"))
        } catch (e: SecurityException) {
            MirrorLookupResult.Failed(e)
        } catch (e: Exception) {
            MirrorLookupResult.Failed(e)
        }
    }

    override fun readTextAndHash(ref: MirrorFileRef): Pair<String, String>? {
        val uri = tryParseUri(ref.uri) ?: return null
        val content = mediaStore.readText(uri) ?: return null
        return Pair(content, computeContentHash(content))
    }

    private fun tryParseUri(uriString: String): Uri? =
        try {
            Uri.parse(uriString)
        } catch (_: Exception) {
            null
        }

    /**
     * 构造 MediaStore 目录路径（`Download/Sujian/<parent>/`）。
     * 返回纯目录路径，不含文件名。
     */
    private fun mediaStoreDirectory(relativePath: String): String {
        val parent = relativePath.substringBeforeLast('/', "")
        val base = "${Environment.DIRECTORY_DOWNLOADS}/$MIRROR_ROOT_NAME"
        return if (parent.isBlank()) "$base/" else "$base/$parent/"
    }

    companion object {
        private const val TAG = "MediaStoreMirrorStorage"
        private const val MIRROR_ROOT_NAME = "Sujian"
        private const val SELECTION_BY_PATH_AND_NAME =
            "${MediaStore.Downloads.RELATIVE_PATH} = ? AND " +
                "${MediaStore.Downloads.DISPLAY_NAME} = ? AND " +
                "${MediaStore.Downloads.IS_PENDING} = 0"
    }
}
