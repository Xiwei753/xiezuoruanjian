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
     * 严格读取当前 backend 和 treeUri。
     *
     * #649 评论 5563798095：与 [ReadableMirrorStateStore.readSnapshotStrict] 对齐，
     * 区分"损坏/不存在"与"正常但字段为空"，让调用方决定是停止操作还是报告错误。
     *
     * @return [Result.success] 包含 (backend, treeUri)；[Result.failure] 包含读取失败的异常
     */
    fun readSnapshotStrict(): Result<Pair<MirrorBackend, String?>> {
        return stateStore.readSnapshotStrict().map { snapshot ->
            Pair(snapshot.backend, snapshot.treeUri)
        }
    }

    /**
     * 返回当前 backend 对应的 [ReadableMirrorStorage]。
     *
     * #649 评论 5563798095：严格模式 — 损坏或缺失必需字段时返回错误，**不回退到 MediaStore**。
     * 调用方应在事务入口调 [currentResult]，失败时停止本轮操作、不镜像。
     *
     * - [MirrorBackend.DOCUMENT_TREE] 但 treeUri 缺失时返回 [Result.failure]
     *   （违背 #649 "一个镜像后端，不猜、不反向覆盖"原则，不能回退 MediaStore）。
     * - state.json 损坏时返回 [Result.failure]。
     *
     * 调用方应在每次事务开始时调一次本方法，在同一次事务里复用返回的 storage。
     *
     * @return [Result.success] 包含对应的 storage；[Result.failure] 包含 [IllegalStateException]
     *   或读取失败的异常
     */
    fun currentResult(): Result<ReadableMirrorStorage> {
        return readSnapshotStrict().mapCatching { (backend, treeUri) ->
            when (backend) {
                MirrorBackend.DOCUMENT_TREE -> {
                    // #649 评论 5563798095：DOCUMENT_TREE 缺 treeUri 时返回错误，不回退 MediaStore
                    // 违背"一个镜像后端，不猜、不反向覆盖"原则
                    requireNotNull(treeUri) {
                        "Mirror state claims DOCUMENT_TREE backend but treeUri is missing. " +
                                "Refusing to fall back to MEDIA_STORE to avoid writing to wrong backend."
                    }
                    val treeUriParsed = Uri.parse(treeUri)
                    documentTreeFactory(treeUriParsed)
                }
                MirrorBackend.MEDIA_STORE -> mediaStoreStorage
            }
        }
    }

    /**
     * 返回当前 backend 对应的 [ReadableMirrorStorage]（兼容旧版，不推荐新代码使用）。
     *
     * #649 评论 5563798095：旧版行为 — 损坏时回退到 MEDIA_STORE，DOCUMENT_TREE 缺 treeUri 时也回退。
     * 仅保留给未适配 [currentResult] 的旧调用方，新代码请用 [currentResult]。
     *
     * @see currentResult
     */
    @Deprecated(
        message = "Use currentResult() for strict error handling. " +
                "This fallback behavior violates #649 single-backend principle.",
        ReplaceWith("currentResult().getOrThrow()"),
    )
    fun current(): ReadableMirrorStorage {
        return when (stateStore.getBackend()) {
            MirrorBackend.DOCUMENT_TREE -> {
                val treeUriString = stateStore.getTreeUri()
                if (treeUriString != null) {
                    val treeUri = Uri.parse(treeUriString)
                    documentTreeFactory(treeUri)
                } else {
                    // treeUri 缺失，回退到 MediaStore（旧版行为，违背 #649 原则）
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
     * #649 评论 5563798095：严格模式 — [MirrorBackend.DOCUMENT_TREE] 但 [treeUri] 为 null 时返回错误，
     * **不回退到 MediaStore**（违背 #649 "一个镜像后端，不猜、不反向覆盖"原则）。
     *
     * @return [Result.success] 包含对应的 storage；[Result.failure] 包含 [IllegalStateException]
     */
    fun forBackendResult(
        backend: MirrorBackend,
        treeUri: String?,
    ): Result<ReadableMirrorStorage> {
        return runCatching {
            when (backend) {
                MirrorBackend.DOCUMENT_TREE -> {
                    // #649 评论 5563798095：DOCUMENT_TREE 缺 treeUri 时返回错误，不回退 MediaStore
                    requireNotNull(treeUri) {
                        "forBackend(DOCUMENT_TREE, treeUri=null) is invalid. " +
                                "Refusing to fall back to MEDIA_STORE to avoid writing to wrong backend."
                    }
                    documentTreeFactory(Uri.parse(treeUri))
                }
                MirrorBackend.MEDIA_STORE -> mediaStoreStorage
            }
        }
    }

    /**
     * 按指定的 [backend] / [treeUri] 构造 [ReadableMirrorStorage]（兼容旧版，不推荐新代码使用）。
     *
     * #649 评论 5563798095：旧版行为 — DOCUMENT_TREE 缺 treeUri 时回退到 MEDIA_STORE。
     * 仅保留给未适配 [forBackendResult] 的旧调用方，新代码请用 [forBackendResult]。
     *
     * @see forBackendResult
     */
    @Deprecated(
        message = "Use forBackendResult() for strict error handling. " +
                "This fallback behavior violates #649 single-backend principle.",
        ReplaceWith("forBackendResult(backend, treeUri).getOrThrow()"),
    )
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
