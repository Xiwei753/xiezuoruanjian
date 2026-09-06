package com.xiwei.sujian.storage.mirror

import android.content.ContentResolver
import android.net.Uri
import com.xiwei.sujian.core.platform.storage.documents.DocumentTreeReader
import com.xiwei.sujian.core.platform.storage.downloads.MediaStoreDownloads

/**
 * 根据 [ReadableMirrorStateStore] 的 backend 字段选择对应的 [ReadableMirrorStorage]。
 *
 * #649 评论 5561465552 第 3 点：Publisher 构造时不再硬编码 [MediaStoreDownloads]，
 * 而是根据 stateStore.backend 选择后端：
 * - [MirrorBackend.MEDIA_STORE] → [MediaStoreMirrorStorage]
 * - [MirrorBackend.DOCUMENT_TREE] → [DocumentTreeMirrorStorage]（需要 treeUri）
 *
 * 恢复成功后 backend=document_tree 时，后续编辑直接更新用户刚选中的那棵 Download/Sujian，
 * 不再创建第二份。
 *
 * ## 架构约束
 * - 位于 `:app` 的 `storage/mirror` 包，依赖 `:core:platform` 的 [MediaStoreDownloads]
 *   和 [DocumentTreeReader]（合法）。
 * - 不把 `content://` URI 传给 Rust。
 *
 * @param stateStore 提供 backend 和 treeUri。
 * @param contentResolver 用于构造两个后端。
 * @param documentTreeReader 用于构造 [DocumentTreeMirrorStorage]。
 * @return 对应的 [ReadableMirrorStorage]；如果 backend=document_tree 但 treeUri 缺失，
 *   回退到 [MediaStoreMirrorStorage]（保守：至少能写一份）。
 */
fun selectMirrorStorage(
    stateStore: ReadableMirrorStateStore,
    contentResolver: ContentResolver,
    documentTreeReader: DocumentTreeReader,
): ReadableMirrorStorage {
    val mediaStore = MediaStoreDownloads(contentResolver)
    val backend = stateStore.getBackend()
    return when (backend) {
        MirrorBackend.DOCUMENT_TREE -> {
            val treeUriString = stateStore.getTreeUri()
            if (treeUriString != null) {
                val treeUri = Uri.parse(treeUriString)
                DocumentTreeMirrorStorage(treeUri, contentResolver, documentTreeReader)
            } else {
                // treeUri 缺失，回退到 MediaStore（保守：至少能写一份）
                MediaStoreMirrorStorage(mediaStore)
            }
        }
        MirrorBackend.MEDIA_STORE -> MediaStoreMirrorStorage(mediaStore)
    }
}
