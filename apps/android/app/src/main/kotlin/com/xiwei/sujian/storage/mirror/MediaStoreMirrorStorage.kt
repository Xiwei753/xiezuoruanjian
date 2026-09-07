package com.xiwei.sujian.storage.mirror

import android.net.Uri
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
 */
class MediaStoreMirrorStorage(
    private val mediaStore: MediaStoreDownloads,
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
        // #649 评论 5562715833 问题 2：promoteStaged 不删 old，只提升 staging 到 final。
        // 1. 读取暂存内容
        val stagingUri = tryParseUri(staged.stagingUri) ?: return null
        val content = mediaStore.readText(stagingUri) ?: return null
        // 2. 在最终位置创建新文件（old 不动，由调用方在事务提交后删）
        val relativeDir = finalRelativePath.substringBeforeLast('/', "")
        val displayName = finalRelativePath.substringAfterLast('/')
        val newUri = mediaStore.createText(relativeDir, displayName, staged.mimeType, content)
            ?: return null
        // 3. 删 staging
        mediaStore.delete(stagingUri)
        return MirrorFileRef(uri = newUri.toString(), relativePath = finalRelativePath)
    }

    override fun backupCommitted(
        txId: String,
        old: MirrorFileRef,
    ): MirrorFileRef? {
        // #649 评论 5562715833 问题 2：把 old 复制到 tx backup 目录，old 不动。
        val oldUri = tryParseUri(old.uri) ?: return null
        val content = mediaStore.readText(oldUri) ?: return null
        val backupBase = "$STAGING_DIR/$txId/$BACKUP_DIR"
        val parent = old.relativePath.substringBeforeLast('/', "")
        val relativeDir = if (parent.isBlank()) backupBase else "$backupBase/$parent"
        val displayName = old.relativePath.substringAfterLast('/')
        val backupUri = mediaStore.createText(relativeDir, displayName, MIME_MARKDOWN, content)
            ?: return null
        return MirrorFileRef(uri = backupUri.toString(), relativePath = "$backupBase/${old.relativePath}")
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

    companion object {
        private const val STAGING_DIR = ".staging"
        private const val BACKUP_DIR = "backup"
        private const val MIME_MARKDOWN = "text/markdown"
    }
}
