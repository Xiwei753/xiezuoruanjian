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

    private fun tryParseUri(uriString: String): Uri? =
        try {
            Uri.parse(uriString)
        } catch (_: Exception) {
            null
        }
}
