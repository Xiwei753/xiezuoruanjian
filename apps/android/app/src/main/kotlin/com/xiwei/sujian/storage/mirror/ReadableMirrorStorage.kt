package com.xiwei.sujian.storage.mirror

/**
 * 镜像文件引用：统一封装 MediaStore URI 或 SAF document URI。
 *
 * #649 评论 5561465552 第 3 点：SAF/MediaStore URI 体系混用问题。
 *
 * 旧实现把 `content://media/external/downloads/<id>`（MediaStore）和
 * `content://com.android.providers.../document/...`（SAF）混在一起，
 * Publisher 把 SAF URI 传给 [com.xiwei.sujian.core.platform.storage.downloads.MediaStoreDownloads.replaceText]，
 * 后者用 `IS_PENDING` 流程对 SAF URI 不适用（update 返回 0），但
 * `openOutputStream` 因 SAF 写权限反而成功，把旧备份正文改了。
 *
 * [MirrorFileRef] 把 URI 体系细节藏在实现里，调用方只看到统一的引用。
 *
 * @property uri 实际 URI 字符串（MediaStore 或 SAF document URI）。
 * @property relativePath 相对 `Download/Sujian/` 的路径，如 `作品/作品名/卷名/章节名.md`。
 *   保留 relativePath 是因为 manifest 需要记录用户可读路径，与 URI 体系无关。
 */
data class MirrorFileRef(
    val uri: String,
    val relativePath: String,
)

/**
 * 事务中暂存的镜像文件引用。
 *
 * #649 评论 5561974464 问题 2：事务性发布需要 stage → promote 两阶段。
 * 正文先写到 staging 暂存（不覆盖 committed ref），promote 成功后才提交。
 *
 * @property txId 事务 ID，用于 [ReadableMirrorStorage.rollback]。
 * @property stagingUri 暂存文件的 URI。
 * @property stagingRelativePath 暂存文件的相对路径。
 * @property finalRelativePath 最终目标路径（promote 后重命名/移动到这个位置）。
 * @property mimeType MIME 类型。
 */
data class StagedMirrorRef(
    val txId: String,
    val stagingUri: String,
    val stagingRelativePath: String,
    val finalRelativePath: String,
    val mimeType: String,
)

/**
 * 统一镜像存储接口，隔离 MediaStore 与 SAF DocumentsProvider 两套 URI 体系。
 *
 * #649 评论 5561465552 第 3 点。
 *
 * ## 两套实现
 * - [MediaStoreMirrorStorage]：包装 [com.xiwei.sujian.core.platform.storage.downloads.MediaStoreDownloads]，
 *   ref 保存 MediaStore URI，使用 `IS_PENDING`/`RELATIVE_PATH`。
 * - [DocumentTreeMirrorStorage]：ref 保存 SAF tree/document URI，用
 *   `DocumentsContract` + `ContentResolver` stream，不碰 `MediaStore.Downloads.IS_PENDING`。
 *
 * ## 架构约束
 * - 接口和 [MirrorFileRef] 放 `:app` 的 `storage/mirror` 包（因为 [MirrorFileRef]
 *   是业务模型，Publisher/Restorer 都要消费）。
 * - [MediaStoreMirrorStorage] 可依赖 `:core:platform` 的 [com.xiwei.sujian.core.platform.storage.downloads.MediaStoreDownloads]。
 * - [DocumentTreeMirrorStorage] 用 `DocumentsContract` + `ContentResolver`，
 *   可依赖 `:core:platform` 的 [com.xiwei.sujian.core.platform.storage.documents.DocumentTreeReader]。
 * - 不把 `content://` URI 传给 Rust。
 */
interface ReadableMirrorStorage {
    /**
     * 创建新文本文件，返回引用；失败返回 null。
     *
     * @param relativeDir 相对 `Download/Sujian/` 的目录（如 `作品/作品名/卷名`），
     *   空字符串表示直接放 `Download/Sujian/` 下。
     * @param displayName 文件名（如 `章节名.md`）。
     * @param mimeType MIME 类型（如 `text/markdown`）。
     * @param text 文本内容。
     * @return 新创建文件的引用；任何步骤失败返回 null（不留下半写记录）。
     */
    fun createText(
        relativeDir: String,
        displayName: String,
        mimeType: String,
        text: String,
    ): MirrorFileRef?

    /**
     * 覆盖现有引用的内容。返回 false 表示失败（调用方应回退到 [createText]）。
     *
     * 对 MediaStore 后端：走 `IS_PENDING=1 → 写 → IS_PENDING=0` 流程。
     * 对 SAF 后端：直接 `openOutputStream(uri)` 覆盖写（SAF 有写权限即可），
     *   不用 `IS_PENDING`。
     */
    fun replaceText(
        ref: MirrorFileRef,
        text: String,
    ): Boolean

    /**
     * 删除引用指向的文件（幂等）。
     *
     * URI 不存在或已删除返回 false，不抛异常。
     */
    fun delete(ref: MirrorFileRef): Boolean

    /** 当前后端是否可用。 */
    fun isSupported(): Boolean

    // ── 事务能力（#649 评论 5561974464 问题 2）──

    /**
     * 暂存正文到事务 staging（不覆盖 committed ref）。
     *
     * 事务性发布的两阶段写：
     * 1. 所有新正文先写到 staging（不能覆盖 committed ref）
     * 2. promotion 成功后写正式 manifest
     * 3. manifest 成功后一次性写 desiredEntries 到 stateStore
     *
     * @param txId 事务 ID（同一事务内所有 stage 调用用相同 txId）
     * @param relativePath 相对 `Download/Sujian/` 的目标路径
     * @param mimeType MIME 类型
     * @param text 正文内容
     * @return 暂存引用；失败返回 null
     */
    fun stageText(
        txId: String,
        relativePath: String,
        mimeType: String,
        text: String,
    ): StagedMirrorRef?

    /**
     * 提升暂存文件到最终位置。
     *
     * @param staged 暂存引用
     * @param old 旧引用（promote 成功后删除；可为 null，表示新建）
     * @param finalRelativePath 最终目标路径（通常等于 staged.finalRelativePath）
     * @return 新文件的引用；失败返回 null
     */
    fun promote(
        staged: StagedMirrorRef,
        old: MirrorFileRef?,
        finalRelativePath: String,
    ): MirrorFileRef?

    /**
     * 回滚事务：删除该 txId 对应的所有暂存文件。
     *
     * @param txId 事务 ID
     */
    fun rollback(txId: String)
}
