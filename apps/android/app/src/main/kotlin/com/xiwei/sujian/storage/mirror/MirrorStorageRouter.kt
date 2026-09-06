package com.xiwei.sujian.storage.mirror

import android.net.Uri

/**
 * MirrorStorageRouter — 运行时根据 [ReadableMirrorStateStore] 的 backend 字段路由到对应 [ReadableMirrorStorage]。
 *
 * #649 评论 5561974464 问题 1：SAF 恢复后，Publisher 仍然不会立即切到 DocumentTree 后端。
 *
 * ## 旧实现的问题
 * [selectMirrorStorage] 在应用启动时一次性选择后端，构造固定 [ReadableMirrorStorage]
 * 传给 [ReadableMirrorPublisher]。SAF 恢复在应用启动后才把 `backend=document_tree` 写进
 * stateStore，已创建的 Publisher 仍握着启动时的 [MediaStoreMirrorStorage]，恢复成功后
 * 第一次编辑仍写到另一套 MediaStore 镜像，只有重启才重新选择后端。
 *
 * ## 新实现
 * 不在启动时固化后端选择。每次发布事务开始时调 [current] 重新读取 backend，
 * 返回对应的 storage：
 * - [MirrorBackend.MEDIA_STORE] → [mediaStoreStorage]
 * - [MirrorBackend.DOCUMENT_TREE] → 用 [documentTreeFactory] 构造（require treeUri）
 *
 * 同一次事务里固定一份 storage（事务内多次调 [current] 也返回同一实例由调用方保证，
 * 事务入口只调一次 [current]），下一次事务重新读 backend。
 *
 * ## 架构约束
 * - 位于 `:app` 的 `storage/mirror` 包，依赖 [ReadableMirrorStateStore] 与两个 storage 实现。
 * - 不把 `content://` URI 传给 Rust。
 *
 * @param stateStore 提供 backend 和 treeUri。
 * @param mediaStoreStorage MediaStore 后端实例（无状态，可复用）。
 * @param documentTreeFactory 根据 tree URI 构造 [DocumentTreeMirrorStorage] 的工厂。
 *   用工厂而非缓存实例：tree URI 可能在运行时变化（用户重新选树），每次按当前 treeUri 构造。
 */
class MirrorStorageRouter(
    private val stateStore: ReadableMirrorStateStore,
    private val mediaStoreStorage: MediaStoreMirrorStorage,
    private val documentTreeFactory: (Uri) -> DocumentTreeMirrorStorage,
) {
    /**
     * 返回当前 backend 对应的 [ReadableMirrorStorage]。
     *
     * - [MirrorBackend.DOCUMENT_TREE] 但 treeUri 缺失时回退到 [mediaStoreStorage]
     *   （保守：至少能写一份，与旧 [selectMirrorStorage] 行为一致）。
     *
     * 调用方应在每次事务开始时调一次本方法，在同一次事务里复用返回的 storage。
     */
    fun current(): ReadableMirrorStorage {
        return when (stateStore.getBackend()) {
            MirrorBackend.DOCUMENT_TREE -> {
                val treeUriString = stateStore.getTreeUri()
                if (treeUriString != null) {
                    val treeUri = Uri.parse(treeUriString)
                    documentTreeFactory(treeUri)
                } else {
                    // treeUri 缺失，回退到 MediaStore（保守：至少能写一份）
                    mediaStoreStorage
                }
            }
            MirrorBackend.MEDIA_STORE -> mediaStoreStorage
        }
    }

    /**
     * 按指定的 [backend] / [treeUri] 构造 [ReadableMirrorStorage]，不从 stateStore 读。
     *
     * #649 评论 5562462046 问题 3：恢复 pending publish 时必须用 journal 记录的
     * backend/treeUri 构造当时那套 storage，不能用 [current] 猜当前 stateStore
     * （stateStore 可能已被后续操作改写，或 journal 的事务后端与当前不同）。
     *
     * [MirrorBackend.DOCUMENT_TREE] 但 [treeUri] 为 null 时回退到 [mediaStoreStorage]
     * （与 [current] 行为一致）。
     */
    fun forBackend(
        backend: MirrorBackend,
        treeUri: String?,
    ): ReadableMirrorStorage {
        return when (backend) {
            MirrorBackend.DOCUMENT_TREE -> {
                if (treeUri != null) {
                    documentTreeFactory(Uri.parse(treeUri))
                } else {
                    mediaStoreStorage
                }
            }
            MirrorBackend.MEDIA_STORE -> mediaStoreStorage
        }
    }
}
