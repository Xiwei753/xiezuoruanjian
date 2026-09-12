package com.xiwei.sujian.storage.mirror

import android.net.Uri

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
 * @property txId 事务 ID，用于 [MirrorTransactionWorkspace.rollback]。
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
 * 镜像查询三态结果（#649 评论 5565067997 修复 5）。
 *
 * 旧 [ReadableMirrorStorage.resolve] 返回 `MirrorFileRef?`，把"文件不存在"和"查询失败"
 * 都返回 null，cleanup 会把查询失败当"文件不存在"然后清 journal，丢失未完成的事务。
 *
 * 新 [ReadableMirrorStorage.lookup] 返回三态：
 * - [Found]：文件存在，附带 [ref]。
 * - [Missing]：明确不存在（查询成功但无结果）。
 * - [Failed]：查询失败（SecurityException / provider I/O / query 异常），附带 [cause]。
 *   调用方必须停止操作，不能当 Missing 处理。
 */
sealed interface MirrorLookupResult {
    /** 文件存在。 */
    data class Found(val ref: MirrorFileRef) : MirrorLookupResult

    /** 文件明确不存在（查询成功，无匹配记录）。 */
    data object Missing : MirrorLookupResult

    /** 查询失败（权限/IO/异常），无法确认文件是否存在。 */
    data class Failed(val cause: Throwable? = null) : MirrorLookupResult
}

/**
 * 磁盘 journal 读取严格结果（#649 评论 5566303837 问题 1）。
 *
 * rollback 前必须从磁盘读最新 journal，不能用调用方传入的旧对象。
 * 此三态区分"找到同一事务"、"无文件"、"损坏/txId 不匹配"。
 */
sealed interface LatestPending {
    /** 找到同一事务的 journal。 */
    data class Found(val journal: PendingMirrorPublish) : LatestPending

    /** 磁盘上不存在 pending publish journal。 */
    data object NotExists : LatestPending

    /** journal 文件损坏或 txId 不匹配。 */
    data object CorruptedOrMismatch : LatestPending
}

/**
 * SAF 目录遍历三态（#649 评论 5566303837 问题 5）。
 *
 * 旧 `findDirectory()` 返回 null 无法区分"目录不存在"和"查询异常"。
 * lookup 需要明确区分这两种情况。
 */
sealed interface DirectoryLookupResult {
    /** 找到目录。 */
    data class Found(val uri: Uri) : DirectoryLookupResult

    /** 目录明确不存在。 */
    data object Missing : DirectoryLookupResult

    /** 遍历过程中查询失败。 */
    data class Failed(val cause: Throwable? = null) : DirectoryLookupResult
}

/**
 * 镜像存储基础读写能力（#651 评论 5592465805：按职责拆 [ReadableMirrorStorage]）。
 */
interface MirrorStorageCore {
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
     * #649 评论 5564379115 问题 3：改成幂等语义 — 文件不存在也返回 true（目标状态已达到）。
     * cleanup 重跑不能因为第二次 delete 返回 false 永远卡住 journal。
     *
     * @return true 表示文件已删除或本就不存在；false 表示删除失败（权限、IO 错误等）。
     */
    fun delete(ref: MirrorFileRef): Boolean

    /** 当前后端是否可用。 */
    fun isSupported(): Boolean
}

/**
 * 镜像存储查询能力（#651 评论 5592465805：按职责拆 [ReadableMirrorStorage]）。
 */
interface MirrorStorageLookup {
    /**
     * 只查不创建：返回已存在于 [relativePath] 的文件 ref。
     *
     * #649 评论 5563333323 缺口 1：恢复时判断 staged/final/backup 的真实位置。
     * 移动是幂等的：如果文件已在目标位置，resolve() 发现后直接返回，
     * 恢复时才能从任意一步继续。
     *
     * @param relativePath 相对 `Download/Sujian/` 的路径
     * @return 已存在文件的 ref；不存在或查询失败返回 null
     */
    fun resolve(relativePath: String): MirrorFileRef?

    /**
     * 三态查询：返回 [relativePath] 的 [MirrorLookupResult]（#649 评论 5565067997 修复 5）。
     *
     * 与 [resolve] 区别：[resolve] 在"不存在"和"查询失败"时都返回 null，无法区分；
     * [lookup] 明确区分 [MirrorLookupResult.Missing] 和 [MirrorLookupResult.Failed]。
     *
     * 新代码（cleanup / recover / vacate）应使用 [lookup] 而非 [resolve]：
     * - [MirrorLookupResult.Found] → 文件存在，可继续删除/处理。
     * - [MirrorLookupResult.Missing] → 文件明确不存在，目标已达到（幂等成功）。
     * - [MirrorLookupResult.Failed] → 查询失败，必须停止，不能清 journal。
     *
     * @param relativePath 相对 `Download/Sujian/` 的路径
     * @return [MirrorLookupResult.Found] / [MirrorLookupResult.Missing] / [MirrorLookupResult.Failed]
     */
    fun lookup(relativePath: String): MirrorLookupResult

    /**
     * 读取文件内容并计算 hash（#649 评论 5566303837 问题 2/4）。
     *
     * 用于校验 final 位置文件的内容身份：
     * - promote 崩溃后 final 可能是新文件，不能只看"文件在不在"
     * - 用 oldEntries[key].contentHash 校验 final 内容
     *
     * @return Pair(content, contentHash)；读取失败返回 null
     */
    fun readTextAndHash(ref: MirrorFileRef): Pair<String, String>?
}

/**
 * 统一镜像存储接口，隔离 MediaStore 与 SAF DocumentsProvider 两套 URI 体系。
 *
 * #649 评论 5561465552 第 3 点。#651 评论 5592465805：按职责拆成
 * [MirrorStorageCore] + [MirrorStorageLookup]，
 * 本接口仅组合两者，调用方用 [ReadableMirrorStorage] 类型访问全部能力。
 *
 * Issue #667：事务能力（stage/backup/promote/rollback）已移至
 * [MirrorTransactionWorkspace]（私有目录），[ReadableMirrorStorage] 只保留
 * 最终用户可见文件的读写和查询能力。
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
interface ReadableMirrorStorage :
    MirrorStorageCore,
    MirrorStorageLookup
