package com.xiwei.sujian.storage.mirror

import android.content.ContentResolver
import android.content.ContentValues
import android.net.Uri
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

    override fun delete(ref: MirrorFileRef): Boolean {
        val uri = tryParseUri(ref.uri) ?: return false
        return mediaStore.delete(uri)
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
    ): MirrorFileRef? {
        // #649 评论 5563333323 缺口 1：把 old 从最终路径**移动**到 tx backup 区（不是复制），
        // 最终路径真正腾空。promoteStaged 之后最终路径才被 staged 占据，不会冲突。
        // 优先用 update RELATIVE_PATH 移动；失败回退到 read+createText 到 backup + delete old。
        val oldUri = tryParseUri(old.uri) ?: return null
        val backupBase = "$STAGING_DIR/$txId/$BACKUP_DIR"
        val backupRelativePath = "$backupBase/${old.relativePath}"
        // 1. 优先尝试 update RELATIVE_PATH 移动 old 到 backup
        val movedRef = tryMoveByRelativePath(oldUri, backupRelativePath, MIME_MARKDOWN)
        if (movedRef != null) return movedRef
        // 2. 回退：read old → createText 到 backup → delete old（真正删 old 腾空最终路径）
        val content = mediaStore.readText(oldUri) ?: return null
        val parent = old.relativePath.substringBeforeLast('/', "")
        val relativeDir = if (parent.isBlank()) backupBase else "$backupBase/$parent"
        val displayName = old.relativePath.substringAfterLast('/')
        val backupUri = mediaStore.createText(relativeDir, displayName, MIME_MARKDOWN, content)
            ?: return null
        // 关键：删 old 腾空最终路径（不是保留 old）
        if (!mediaStore.delete(oldUri)) {
            // 删 old 失败：删 backup 回滚，old 仍在原位
            mediaStore.delete(backupUri)
            return null
        }
        return MirrorFileRef(uri = backupUri.toString(), relativePath = backupRelativePath)
    }

    override fun resolve(relativePath: String): MirrorFileRef? {
        // #649 评论 5563333323 缺口 1：只查不创建，用 MediaStore query RELATIVE_PATH。
        if (!mediaStore.isSupported()) return null
        val fullRelativePath = buildMediaStoreRelativePath(relativePath)
        return try {
            contentResolver
                .query(
                    MediaStore.Downloads.EXTERNAL_CONTENT_URI,
                    arrayOf(MediaStore.Downloads._ID),
                    "${MediaStore.Downloads.RELATIVE_PATH} = ? AND ${MediaStore.Downloads.IS_PENDING} = 0",
                    arrayOf(fullRelativePath),
                    null,
                )
                ?.use { cursor ->
                    if (cursor.moveToFirst()) {
                        val id = cursor.getLong(0)
                        val uri = Uri.withAppendedPath(MediaStore.Downloads.EXTERNAL_CONTENT_URI, id.toString())
                        MirrorFileRef(uri = uri.toString(), relativePath = relativePath)
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
    ): MirrorFileRef? {
        // #649 评论 5562715833 问题 2：把 backup 恢复到 final 位置（回滚用）。
        val backupUri = tryParseUri(backup.uri) ?: return null
        val content = mediaStore.readText(backupUri) ?: return null
        val relativeDir = finalRelativePath.substringBeforeLast('/', "")
        val displayName = finalRelativePath.substringAfterLast('/')
        val newUri = mediaStore.createText(relativeDir, displayName, MIME_MARKDOWN, content)
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
        val fullRelativePath = buildMediaStoreRelativePath(targetRelativePath)
        val parent = targetRelativePath.substringBeforeLast('/', "")
        val displayName = targetRelativePath.substringAfterLast('/')
        val values = ContentValues().apply {
            put(MediaStore.Downloads.RELATIVE_PATH, fullRelativePath)
            put(MediaStore.Downloads.DISPLAY_NAME, displayName)
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

    /** 构造 MediaStore 完整 RELATIVE_PATH（`Download/Sujian/<relativePath>`）。 */
    private fun buildMediaStoreRelativePath(relativePath: String): String =
        "${android.os.Environment.DIRECTORY_DOWNLOADS}/$MIRROR_ROOT_NAME/" + relativePath

    companion object {
        private const val STAGING_DIR = ".staging"
        private const val BACKUP_DIR = "backup"
        private const val MIME_MARKDOWN = "text/markdown"
        private const val MIRROR_ROOT_NAME = "Sujian"
    }
}
