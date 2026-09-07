package com.xiwei.sujian.storage.mirror

import android.content.ContentResolver
import android.content.ContentValues
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
 * ## 架构约束
 * - 位于 `:app` 的 `storage/mirror` 包，依赖 `:core:platform` 的 [MediaStoreDownloads]（合法）。
 * - 不放 Compose、UniFFI 业务调用。
 * - 不把 `content://` URI 传给 Rust。
 *
 * @param mediaStore 被包装的 [MediaStoreDownloads]（由调用方注入 [ContentResolver]）。
 * @param contentResolver 应用 [ContentResolver]（用于 update RELATIVE_PATH 移动现有 row）。
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
     * 查询三态：FOUND → 尝试删除；MISSING → 返回 true（目标已达到）；FAILED → 返回 false。
     */
    override fun delete(ref: MirrorFileRef): Boolean {
        val uri = tryParseUri(ref.uri) ?: return false // URI 无效 → 无法确认状态，返回 false
        // #649 评论 5564820566 问题 4：不再把 "后端不可用" 当删除成功。
        // 旧代码 `if (!mediaStore.isSupported()) return true` 会让 cleanup 误认为文件已删。
        val directory = mediaStoreDirectory(ref.relativePath)
        val displayName = ref.relativePath.substringAfterLast('/')
        // 三态查询：FOUND / MISSING / FAILED
        val queryResult = try {
            val exists = contentResolver.query(
                MediaStore.Downloads.EXTERNAL_CONTENT_URI,
                arrayOf(MediaStore.Downloads._ID),
                "${MediaStore.Downloads.RELATIVE_PATH} = ? AND " +
                    "${MediaStore.Downloads.DISPLAY_NAME} = ? AND " +
                    "${MediaStore.Downloads.IS_PENDING} = 0",
                arrayOf(directory, displayName),
                null,
            )?.use { it.moveToFirst() } ?: false
            if (exists) QueryResult.FOUND else QueryResult.MISSING
        } catch (_: SecurityException) {
            QueryResult.FAILED
        } catch (_: Exception) {
            QueryResult.FAILED
        }
        return when (queryResult) {
            QueryResult.MISSING -> true // 文件不存在 → 目标已达到
            QueryResult.FAILED -> false // 查询失败 → 不确定文件是否存在，返回 false
            QueryResult.FOUND -> try {
                mediaStore.delete(uri)
            } catch (_: SecurityException) {
                false // 权限异常 → 删除失败
            } catch (_: Exception) {
                false // I/O 异常 → 删除失败
            }
        }
    }

    override fun isSupported(): Boolean = mediaStore.isSupported()

    // ── 事务能力（#649 评论 5561974464 问题 2）──

    override fun stageText(
        txId: String,
        relativePath: String,
        mimeType: String,
        text: String,
    ): StagedMirrorRef? {
        // MediaStore 暂存：用 txId 作为临时目录，避免覆盖 committed ref
        val stagingDir = "$STAGING_DIR/$txId"
        // #649 评论 5562462046 问题 6：路径拼接修复，避免少一个 `/`
        val parent = relativePath.substringBeforeLast('/', "")
        val relativeDir = if (parent.isBlank()) stagingDir else "$stagingDir/$parent"
        val displayName = relativePath.substringAfterLast('/')
        val uri = mediaStore.createText(relativeDir, displayName, mimeType, text) ?: return null
        return StagedMirrorRef(
            txId = txId,
            stagingUri = uri.toString(),
            stagingRelativePath = "$stagingDir/$relativePath",
            finalRelativePath = relativePath,
            mimeType = mimeType,
        )
    }

    override fun promoteStaged(
        staged: StagedMirrorRef,
        finalRelativePath: String,
    ): MirrorFileRef? {
        // #649 评论 5563333323 缺口 1：promoteStaged 把 staging 移到最终位置。
        // 最终路径已由 backupCommitted 腾空（old 已移走），不会冲突。
        // 优先用 ContentResolver.update(RELATIVE_PATH) 移动现有 row（不复制内容）；
        // update 失败再回退到 read → createText 到 final → delete staging。
        val stagingUri = tryParseUri(staged.stagingUri) ?: return null
        // 1. 优先尝试 update RELATIVE_PATH 移动 staging row 到最终位置
        val movedRef = tryMoveByRelativePath(stagingUri, finalRelativePath, staged.mimeType)
        if (movedRef != null) return movedRef
        // 2. 回退：读取暂存内容 → 在最终位置创建新文件 → 删 staging
        val content = mediaStore.readText(stagingUri) ?: return null
        val relativeDir = finalRelativePath.substringBeforeLast('/', "")
        val displayName = finalRelativePath.substringAfterLast('/')
        val newUri = mediaStore.createText(relativeDir, displayName, staged.mimeType, content)
            ?: return null
        mediaStore.delete(stagingUri)
        return MirrorFileRef(uri = newUri.toString(), relativePath = finalRelativePath)
    }

    override fun backupCommitted(
        txId: String,
        old: MirrorFileRef,
        mimeType: String,
    ): MirrorFileRef? {
        // #649 评论 5563333323 缺口 1：把 old 从最终路径**移动**到 tx backup 区（不是复制），
        // 最终路径真正腾空。promoteStaged 之后最终路径才被 staged 占据，不会冲突。
        // 优先用 update RELATIVE_PATH 移动；失败回退到 read+createText 到 backup + delete old。
        val oldUri = tryParseUri(old.uri) ?: return null
        val backupBase = "$STAGING_DIR/$txId/$BACKUP_DIR"
        val backupRelativePath = "$backupBase/${old.relativePath}"
        // 1. 优先尝试 update RELATIVE_PATH 移动 old 到 backup
        val movedRef = tryMoveByRelativePath(oldUri, backupRelativePath, mimeType)
        if (movedRef != null) return movedRef
        // 2. 回退：read old → createText 到 backup → delete old（真正删 old 腾空最终路径）
        val content = mediaStore.readText(oldUri) ?: return null
        val parent = old.relativePath.substringBeforeLast('/', "")
        val relativeDir = if (parent.isBlank()) backupBase else "$backupBase/$parent"
        val displayName = old.relativePath.substringAfterLast('/')
        val backupUri = mediaStore.createText(relativeDir, displayName, mimeType, content)
            ?: return null
        // 关键：删 old 腾空最终路径（不是保留 old）
        if (!mediaStore.delete(oldUri)) {
            // 删 old 失败：删 backup 回滚，old 仍在原位
            mediaStore.delete(backupUri)
            return null
        }
        return MirrorFileRef(uri = backupUri.toString(), relativePath = backupRelativePath)
    }

    // #649 评论 5564820566 问题 3：两步 journalable backup — MediaStore fallback 路径

    override fun prepareBackup(
        txId: String,
        old: MirrorFileRef,
        mimeType: String,
    ): BackupReadyRef? {
        val oldUri = tryParseUri(old.uri) ?: return null
        val backupBase = "$STAGING_DIR/$txId/$BACKUP_DIR"
        val backupRelativePath = "$backupBase/${old.relativePath}"
        // 1. 优先尝试 update RELATIVE_PATH 移动 old 到 backup（原子 move）
        val movedRef = tryMoveByRelativePath(oldUri, backupRelativePath, mimeType)
        if (movedRef != null) return BackupReadyRef(backupRef = movedRef, vacated = true)
        // 2. 回退：只复制 old → backup，不删 old
        val content = mediaStore.readText(oldUri) ?: return null
        val parent = old.relativePath.substringBeforeLast('/', "")
        val relativeDir = if (parent.isBlank()) backupBase else "$backupBase/$parent"
        val displayName = old.relativePath.substringAfterLast('/')
        val backupUri = mediaStore.createText(relativeDir, displayName, mimeType, content)
            ?: return null
        return BackupReadyRef(
            backupRef = MirrorFileRef(uri = backupUri.toString(), relativePath = backupRelativePath),
            vacated = false,
        )
    }

    override fun vacateCommitted(old: MirrorFileRef): Boolean {
        val oldUri = tryParseUri(old.uri) ?: return true // URI 无效 → 无法确认 old 是否存在，视为已腾空
        return try {
            mediaStore.delete(oldUri)
        } catch (_: Exception) {
            false
        }
    }

    override fun resolve(relativePath: String): MirrorFileRef? {
        // #649 评论 5563333323 缺口 1：只查不创建，用 MediaStore query RELATIVE_PATH + DISPLAY_NAME。
        // #649 评论 5563798095：异常情况下命中多条时不随便拿第一条绑定章节，直接返回 null。
        if (!mediaStore.isSupported()) return null
        val directory = mediaStoreDirectory(relativePath)
        val displayName = relativePath.substringAfterLast('/')
        return queryByPathAndName(directory, displayName, relativePath)
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
                    "${MediaStore.Downloads.RELATIVE_PATH} = ? AND " +
                        "${MediaStore.Downloads.DISPLAY_NAME} = ? AND " +
                        "${MediaStore.Downloads.IS_PENDING} = 0",
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

    override fun resolveBackup(txId: String, relativePath: String): MirrorFileRef? {
        // #649 评论 5563798095：检查 backup 路径是否已有文件，避免崩溃窗口后重复 backup。
        if (!mediaStore.isSupported()) return null
        val backupBase = "$STAGING_DIR/$txId/$BACKUP_DIR"
        val backupRelativePath = "$backupBase/$relativePath"
        val directory = mediaStoreDirectory(backupRelativePath)
        val displayName = backupRelativePath.substringAfterLast('/')
        return queryByPathAndName(directory, displayName, backupRelativePath)
    }

    /**
     * MediaStore 公共查询：按 RELATIVE_PATH + DISPLAY_NAME 查找文件。
     *
     * #649 评论 5563798095：命中多条时返回 null 并记日志，不随便拿第一条绑定章节。
     */
    private fun queryByPathAndName(
        directory: String,
        displayName: String,
        resultRelativePath: String,
    ): MirrorFileRef? {
        return try {
            contentResolver
                .query(
                    MediaStore.Downloads.EXTERNAL_CONTENT_URI,
                    arrayOf(MediaStore.Downloads._ID),
                    "${MediaStore.Downloads.RELATIVE_PATH} = ? AND " +
                        "${MediaStore.Downloads.DISPLAY_NAME} = ? AND " +
                        "${MediaStore.Downloads.IS_PENDING} = 0",
                    arrayOf(directory, displayName),
                    null,
                )
                ?.use { cursor ->
                    if (cursor.moveToFirst()) {
                        if (cursor.count > 1) {
                            android.util.Log.w(
                                TAG,
                                "queryByPathAndName: multiple matches for $resultRelativePath, " +
                                    "count=${cursor.count}, returning null to avoid binding wrong file"
                            )
                            return null
                        }
                        val id = cursor.getLong(0)
                        val uri = Uri.withAppendedPath(MediaStore.Downloads.EXTERNAL_CONTENT_URI, id.toString())
                        MirrorFileRef(uri = uri.toString(), relativePath = resultRelativePath)
                    } else {
                        null
                    }
                }
        } catch (_: Exception) {
            null
        }
    }

    override fun restoreBackup(
        backup: MirrorFileRef,
        finalRelativePath: String,
        mimeType: String,
    ): MirrorFileRef? {
        // #649 评论 5562715833 问题 2：把 backup 恢复到 final 位置（回滚用）。
        val backupUri = tryParseUri(backup.uri) ?: return null
        val content = mediaStore.readText(backupUri) ?: return null
        val relativeDir = finalRelativePath.substringBeforeLast('/', "")
        val displayName = finalRelativePath.substringAfterLast('/')
        val newUri = mediaStore.createText(relativeDir, displayName, mimeType, content)
            ?: return null
        return MirrorFileRef(uri = newUri.toString(), relativePath = finalRelativePath)
    }

    override fun rollback(txId: String) {
        // 删除 txId 对应的整个暂存目录（含 backup 子目录）
        val stagingDir = "$STAGING_DIR/$txId"
        mediaStore.deleteByPrefix(stagingDir)
    }

    private fun tryParseUri(uriString: String): Uri? =
        try {
            Uri.parse(uriString)
        } catch (_: Exception) {
            null
        }

    /**
     * 用 ContentResolver.update(RELATIVE_PATH) 把现有 row 移到 [targetRelativePath]。
     *
     * #649 评论 5563333323 缺口 1：官方说明更新 RELATIVE_PATH 会移动底层文件。
     * 参考：https://developer.android.com/reference/android/provider/MediaStore.MediaColumns#RELATIVE_PATH
     *
     * @param sourceUri 现有 row 的 URI
     * @param targetRelativePath 相对 `Download/Sujian/` 的目标路径
     * @param mimeType MIME 类型（用于构造返回 ref，不参与 update）
     * @return 移动后的 ref；update 返回 0 或失败返回 null
     */
    private fun tryMoveByRelativePath(
        sourceUri: Uri,
        targetRelativePath: String,
        mimeType: String,
    ): MirrorFileRef? {
        if (!mediaStore.isSupported()) return null
        val values = ContentValues().apply {
            put(MediaStore.Downloads.RELATIVE_PATH, mediaStoreDirectory(targetRelativePath))
            put(MediaStore.Downloads.DISPLAY_NAME, targetRelativePath.substringAfterLast('/'))
        }
        val updated =
            try {
                contentResolver.update(sourceUri, values, null, null)
            } catch (_: Exception) {
                return null
            }
        if (updated != 1) return null
        return MirrorFileRef(uri = sourceUri.toString(), relativePath = targetRelativePath)
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
        private const val STAGING_DIR = ".staging"
        private const val BACKUP_DIR = "backup"
        private const val MIRROR_ROOT_NAME = "Sujian"

        // #649 评论 5564624383 问题 5：查询三态结果
        private enum class QueryResult { FOUND, MISSING, FAILED }
    }
}
