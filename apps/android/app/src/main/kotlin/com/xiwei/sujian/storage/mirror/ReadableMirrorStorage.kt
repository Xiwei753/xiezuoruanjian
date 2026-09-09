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
 * promote 流程拆分后的结果，记录新引用、旧正文备份引用与被替换的旧引用。
 *
 * #649 评论 5562715833 问题 2：promote 拆成 [ReadableMirrorStorage.backupCommitted] +
 * [ReadableMirrorStorage.promoteStaged]，旧正文先备份再提升，
 * manifest 提交成功后才删旧正文和 backup。
 *
 * @property newRef 新创建/移动后的文件引用。
 * @property backupOldRef 旧正文备份引用（old != null 时非空，事务提交后由调用方删）；
 *   `null` 表示本次是新建（无旧文件被备份）。
 * @property displacedOldRef 被替换掉的旧引用（promote 前 `old` 参数原样回传）；
 *   调用方据此在 journal/stateStore 提交后再决定何时删旧。
 *   `null` 表示本次是新建（无旧文件被替换）。
 */
data class PromoteResult(
    val newRef: MirrorFileRef,
    val backupOldRef: MirrorFileRef?,
    val displacedOldRef: MirrorFileRef?,
)

/**
 * 两步 backup 的第一步结果（#649 评论 5564820566 问题 3）。
 *
 * 非原子 provider（copy → delete）在 process crash 时可能出现
 * "backup 已创建但 old 还没删" 的歧义窗口。用两步 journalable 状态消除歧义：
 * 1. [prepareBackup]：只复制/准备 backup，不删 old → [BackupReadyRef]
 * 2. [vacateCommitted]：删 old → 最终路径腾空
 *
 * @property backupRef backup 文件引用（已存在于 backup 目录）
 * @property vacated true 表示 old 已被移走/删除（原子 move 的 provider 在第一步就完成）；
 *   false 表示 old 仍在原位，调用方需要后续调用 [vacateCommitted]
 */
data class BackupReadyRef(
    val backupRef: MirrorFileRef,
    val vacated: Boolean,
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
 * restoreBackup 带身份校验的结果（#649 评论 5566303837 问题 4）。
 *
 * 旧 `MirrorFileRef?` 无法区分"final 有文件但不是旧正文"和"确实是旧正文"。
 * promote 崩溃窗口会让 final 上出现新文件，直接 return Found 会误判。
 */
sealed interface RestoreBackupResult {
    /** backup 成功恢复到 final 位置。 */
    data class Restored(val ref: MirrorFileRef) : RestoreBackupResult

    /** final 已存在且 hash 与旧正文匹配（真正已恢复）。 */
    data class AlreadyRestored(val ref: MirrorFileRef) : RestoreBackupResult

    /** final 已存在但 hash 不匹配（新文件残留），冲突。 */
    data object Conflict : RestoreBackupResult

    /** 读取/创建失败，无法确认状态。 */
    data class Failed(val cause: Throwable? = null) : RestoreBackupResult
}

/**
 * SAF 目录遍历三态（#649 评论 5566303837 问题 5）。
 *
 * 旧 `findDirectory()` 返回 null 无法区分"目录不存在"和"查询异常"。
 * lookup/lookupBackup 需要明确区分这两种情况。
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
     * 只查不创建：返回已存在于备份路径 [relativePath] 的文件 ref。
     *
     * #649 评论 5563798095：恢复时判断 backup 是否已被移动到备份目录。
     * 崩溃窗口：`backupCommitted()` 已把 old 移到 `.staging/<txId>/backup/`，
     * 但 `backupOldRef` 还没写入 journal 时进程退出。重启后 journal 仍是 STAGED，
     * 恢复会拿已失效的 old URI 再跑一次 `backupCommitted()`，
     * 失败后又 `rollback(txId)` 会把唯一 backup 删掉。
     * 用 `resolveBackup()` 检测 backup 已存在则跳过重复 backup。
     *
     * @param txId 事务 ID
     * @param relativePath 相对 `Download/Sujian/` 的路径（与 backup 中的相对路径一致）
     * @return 已存在文件的 ref；不存在或查询失败返回 null
     */
    fun resolveBackup(
        txId: String,
        relativePath: String,
    ): MirrorFileRef?

    /**
     * 三态查询：返回备份路径 [relativePath] 的 [MirrorLookupResult]。
     *
     * 与 [resolveBackup] 区别：[resolveBackup] 在"不存在"和"查询失败"时都返回 null，
     * 无法区分；[lookupBackup] 明确区分 [MirrorLookupResult.Missing] 和 [MirrorLookupResult.Failed]。
     *
     * 用于 [MirrorStorageTransaction.restoreBackup] 的 crash-idempotent 检查：
     * - [MirrorLookupResult.Found] → backup 已存在，可直接返回这个 ref（已恢复）
     * - [MirrorLookupResult.Missing] → backup 不存在，继续 restore
     * - [MirrorLookupResult.Failed] → 查询失败，返回 null
     *
     * @param txId 事务 ID
     * @param relativePath 相对 `Download/Sujian/` 的路径（与 backup 中的相对路径一致）
     * @return [MirrorLookupResult.Found] / [MirrorLookupResult.Missing] / [MirrorLookupResult.Failed]
     */
    fun lookupBackup(
        txId: String,
        relativePath: String,
    ): MirrorLookupResult

    /**
     * 读取文件内容并计算 hash（#649 评论 5566303837 问题 2/4）。
     *
     * 用于 [MirrorStorageTransaction.restoreBackup] 校验 final 是否真的是旧正文：
     * - promote 崩溃后 final 可能是新文件，不能只看"文件在不在"
     * - 用 oldEntries[key].contentHash 校验 final 内容
     *
     * @return Pair(content, contentHash)；读取失败返回 null
     */
    fun readTextAndHash(ref: MirrorFileRef): Pair<String, String>?
}

/**
 * 镜像存储事务能力（#651 评论 5592465805：按职责拆 [ReadableMirrorStorage]）。
 */
interface MirrorStorageTransaction {
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
     * 把旧正文从最终路径**移动**到事务 backup 目录，最终路径真正腾空。
     *
     * #649 评论 5563333323 缺口 1：真正占位切换 swap。
     * 旧实现只复制 old 到 backup，old 仍占着最终路径，promoteStaged 在 old 仍占着的
     * 位置创建/移动同名新文件，provider 可能拒绝、改名或返回另一条记录，
     * manifest 可能记录错误路径。
     *
     * 新语义：**移动**（不是复制）old 到 tx backup 区，最终文件名真正腾空。
     * promoteStaged 之后最终路径才被 staged 占据，不会冲突。
     * 事务回滚时用 [restoreBackup] 把 backup 积回最终路径。
     *
     * @param txId 事务 ID
     * @param old 旧引用（非空）
     * @param mimeType MIME 类型（正文使用 `text/markdown`，manifest 使用 `application/json`）
     * @return backup 引用（old 已被移走，最终路径腾空）；失败返回 null（old 仍在原位）
     */
    fun backupCommitted(
        txId: String,
        old: MirrorFileRef,
        mimeType: String,
    ): MirrorFileRef?

    // #649 评论 5564820566 问题 3：两步 journalable backup，消除 "backup 已创建、old 还没删" 的歧义窗口。

    /**
     * 第一步：只复制/准备 backup，不删 old。
     *
     * 非原子 provider（MediaStore fallback）：copy old → backup，返回 [BackupReadyRef]（vacated=false），
     * 调用方需后续调用 [vacateCommitted] 删 old。
     * 原子 provider（SAF moveDocument）：move old → backup，返回 [BackupReadyRef]（vacated=true），
     * 调用方跳过 [vacateCommitted]。
     *
     * @param txId 事务 ID
     * @param old 旧引用（非空）
     * @param mimeType MIME 类型
     * @return [BackupReadyRef]；失败返回 null
     */
    fun prepareBackup(
        txId: String,
        old: MirrorFileRef,
        mimeType: String,
    ): BackupReadyRef?

    /**
     * 第二步：删除 old，腾空最终路径。
     *
     * 幂等：如果 old 已经不存在（被移动或已删除），返回 true。
     * 如果 backup 已存在但 old 还在（崩溃窗口），也返回 true。
     *
     * @param old 旧引用
     * @return true 表示最终路径已腾空；false 表示删除失败（无法确认状态）
     */
    fun vacateCommitted(old: MirrorFileRef): Boolean

    /**
     * 提升暂存文件到最终位置（不删 old，old 由调用方在事务提交后删）。
     *
     * #649 评论 5562715833 问题 2：promoteStaged 不再删 old。
     * - MediaStore：读 staging 内容 → createText 到 final → 删 staging。
     * - SAF：用 moveDocument 跨目录移动 staging 到 final（#649 评论 5562715833 问题 3）。
     *
     * @param staged 暂存引用
     * @param finalRelativePath 最终目标路径
     * @return 新文件引用；失败返回 null（staging 保留，调用方可 rollback）
     */
    fun promoteStaged(
        staged: StagedMirrorRef,
        finalRelativePath: String,
    ): MirrorFileRef?

    /**
     * 把 backup 恢复到 final 位置（回滚用）。
     *
     * #649 评论 5566303837 问题 4：返回 [RestoreBackupResult]，带旧内容身份校验。
     * 不能用 final 是否存在判断"已恢复"——promote 崩溃窗口可能在 final 上留下新文件。
     *
     * @param backup backup 引用
     * @param finalRelativePath 最终目标路径
     * @param mimeType MIME 类型（正文使用 `text/markdown`，manifest 使用 `application/json`）
     * @param expectedOldContentHash 旧正文的期望 hash（用于校验 final 上是否真的是旧正文）
     *   null 表示不校验（如新建章节，没有旧正文）
     * @return [RestoreBackupResult]
     */
    fun restoreBackup(
        backup: MirrorFileRef,
        finalRelativePath: String,
        mimeType: String,
        expectedOldContentHash: String? = null,
    ): RestoreBackupResult

    /**
     * 回滚事务：删除该 txId 对应的所有暂存文件。
     *
     * #649 评论 5566303837 问题 6：返回 Boolean，
     * 让 cleanupCommittedTransaction 区分"残留已清理"和"清理失败"。
     *
     * @param txId 事务 ID
     * @return true 表示清理成功（或目录本就不存在）；false 表示清理失败
     */
    fun rollback(txId: String): Boolean
}

/**
 * 统一镜像存储接口，隔离 MediaStore 与 SAF DocumentsProvider 两套 URI 体系。
 *
 * #649 评论 5561465552 第 3 点。#651 评论 5592465805：按职责拆成
 * [MirrorStorageCore] + [MirrorStorageLookup] + [MirrorStorageTransaction]，
 * 本接口仅组合三者，调用方仍用 [ReadableMirrorStorage] 类型访问全部能力。
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
    MirrorStorageLookup,
    MirrorStorageTransaction
