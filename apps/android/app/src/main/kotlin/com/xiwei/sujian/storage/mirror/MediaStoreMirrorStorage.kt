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
        val relativeDir = stagingDir + relativePath.substringBeforeLast('/', "")
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

    override fun promote(
        staged: StagedMirrorRef,
        old: MirrorFileRef?,
        finalRelativePath: String,
    ): MirrorFileRef? {
        // 1. 先删旧文件（如果有）
        if (old != null) {
            val oldUri = tryParseUri(old.uri)
            if (oldUri != null) {
                mediaStore.delete(oldUri)
            }
        }
        // 2. 把暂存文件移动到最终位置（MediaStore 用 rename 或 createText + delete）
        val stagingUri = tryParseUri(staged.stagingUri) ?: return null
        // 读取暂存内容
        val content = mediaStore.readText(stagingUri) ?: return null
        // 在最终位置创建
        val relativeDir = finalRelativePath.substringBeforeLast('/', "")
        val displayName = finalRelativePath.substringAfterLast('/')
        val newUri = mediaStore.createText(relativeDir, displayName, staged.mimeType, content) ?: return null
        // 3. 删除暂存文件
        mediaStore.delete(stagingUri)
        return MirrorFileRef(uri = newUri.toString(), relativePath = finalRelativePath)
    }

    override fun rollback(txId: String) {
        // 删除 txId 对应的整个暂存目录
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
    }
}
