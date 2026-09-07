package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import org.json.JSONArray
import org.json.JSONObject

private const val TAG = "PendingMirrorPublish"

/**
 * manifest 事务的显式 swap 状态（#649 评论 5565067997 修复 2）。
 *
 * 旧实现用 `isManifestCommitted: Boolean` 单布尔值，在 `existingBackup != null` 且
 * `existingFinal != null` 时直接把 final 当成"已经 promote 的新 manifest"提交。
 * 但拆成 prepareBackup() + vacateCommitted() 后，"backup + final 同时存在"是
 * fallback copy 正常的中间状态（backup 已复制好，old manifest 还在 final，还没 vacate）。
 *
 * 新状态机：
 * - [MANIFEST_STAGED]：新 manifest 已 stage，old 还在 final。
 * - [MANIFEST_BACKUP_READY]：old manifest 已备份（backup 就绪），old 可能还没 vacate。
 * - [MANIFEST_OLD_VACATED]：old manifest 已从 final 腾空，可以 promote 新 manifest。
 * - [MANIFEST_PROMOTED]：新 manifest 已 promote 到 final，setManifestUri 已调用。
 * - [MANIFEST_COMMITTED]：manifest 事务完全提交（isManifestCommitted=true 的等价）。
 *
 * 旧 journal 没有 manifestSwapState 字段，反序列化时根据 isManifestCommitted 推导：
 * - isManifestCommitted=true → MANIFEST_COMMITTED
 * - isManifestCommitted=false + manifestNewRef != null → MANIFEST_PROMOTED
 * - isManifestCommitted=false + manifestBackupRef != null → MANIFEST_BACKUP_READY
 * - 其他 → MANIFEST_STAGED
 */
enum class ManifestTransactionState(val journalValue: String) {
    MANIFEST_STAGED("STAGED"),
    MANIFEST_BACKUP_READY("BACKUP_READY"),
    MANIFEST_OLD_VACATED("OLD_VACATED"),
    MANIFEST_PROMOTED("PROMOTED"),
    MANIFEST_COMMITTED("COMMITTED"),
    ;

    companion object {
        /**
         * 从 JSON 值反序列化 [ManifestTransactionState]（#649 评论 5565862745 问题 5）。
         *
         * - 已知值返回对应枚举
         * - 未知值返回 null（不能回退到 MANIFEST_STAGED，否则会掩盖数据损坏）
         */
        fun fromJournalValue(value: String): ManifestTransactionState? = entries.find { it.journalValue == value }
    }
}

/**
 * 镜像事务类型。
 *
 * #649 评论 5562462046 问题 4：deleteProject() 的 pending 恢复逻辑错误。
 * cleanup 阶段恢复时需要区分本次事务是 UPSERT 还是 DELETE，才能按正确顺序
 * 提交 manifest / 删旧正文。
 */
enum class MirrorTransactionType {
    UPSERT_PROJECT,
    DELETE_PROJECT,
}

/**
 * 单个章节在事务中的逐项进度。
 *
 * #649 评论 5562462046 问题 3：journal 没有逐项记录 promote 进度，
 * 进程死在第 N 章时恢复不了。每个章节独立记录已推进到哪一步，
 * 恢复时只继续未完成的 item。
 *
 * #649 评论 5565067997 修复 1：状态机增加 [STATE_BACKUP_READY] / [STATE_OLD_VACATED]
 * 两个独立中间状态，把"backup 已准备"和"old 已腾空"分开。
 *
 * @property key 章节定位。
 * @property stagedRef 已 stage 的暂存引用（STAGED 及之后非空）。
 * @property oldRef 旧引用（可为 null，表示新建）。
 * @property backupOldRef 旧正文备份引用（BACKUP_READY 及之后非空，用于回滚）。
 *   当前实现中 promote 走真正 swap（不先删 old），backupOldRef 主要用于
 *   journal 记录"old 已被替换"这一事实，恢复时据此跳过重复 promote。
 * @property promotedRef promote 后的新引用（PROMOTED 及之后非空）。
 * @property state 推进状态：[STATE_STAGED] / [STATE_BACKUP_READY] /
 *   [STATE_OLD_VACATED] / [STATE_PROMOTED] / [STATE_COMMITTED]。
 *   旧 journal 的 [STATE_OLD_BACKED_UP] 反序列化时映射到 [STATE_BACKUP_READY]。
 * @property oldContentHash 旧正文的 SHA-256 hash（#649 评论 5566303837 问题 2）。
 *   OLD_VACATED 恢复时用于校验 final 是旧正文还是新文件残留，不能只看"final 在不在"。
 */
data class PendingItem(
    val key: ChapterKey,
    val stagedRef: StagedMirrorRef?,
    val oldRef: MirrorFileRef?,
    val backupOldRef: MirrorFileRef?,
    val promotedRef: MirrorFileRef?,
    val state: String,
    val oldContentHash: String? = null,
) {
    companion object {
        const val STATE_STAGED = "STAGED"

        /**
         * backup 已就绪，old 可能还没 vacate（#649 评论 5565067997 修复 1）。
         *
         * prepareBackup() 只复制/移动 old 到 backup，old 可能仍在 final（fallback copy 路径）。
         * 恢复时看到此状态需检查 old 是否已 vacate，未 vacate 则继续 vacateCommitted()。
         */
        const val STATE_BACKUP_READY = "BACKUP_READY"

        /**
         * old 已从 final 腾空，可以 promote（#649 评论 5565067997 修复 1）。
         *
         * vacateCommitted() 成功后写入此状态。只有此状态才允许 promoteStaged()。
         */
        const val STATE_OLD_VACATED = "OLD_VACATED"
        const val STATE_PROMOTED = "PROMOTED"
        const val STATE_COMMITTED = "COMMITTED"

        /**
         * 旧状态：backup 已准备 + old 已腾空揉成一个（向后兼容）。
         *
         * #649 评论 5565067997 修复 1：保留此常量仅用于反序列化旧 journal，
         * 新代码不再写入此状态。旧 journal 反序列化时映射到 [STATE_BACKUP_READY]
         * （因为旧状态语义上等价于"backup 已就绪"，old 是否已 vacate 需恢复时检查）。
         */
        const val STATE_OLD_BACKED_UP = "OLD_BACKED_UP"

        // #649 评论 5564820566 问题 2：rollback 和 recovery 共用同一套显式状态机，
        // 不再根据 "final/backup 是否存在" 猜测文件是新版还是旧版。
        const val STATE_ROLLBACK_NEW_REMOVED = "ROLLBACK_NEW_REMOVED"
        const val STATE_ROLLBACK_OLD_RESTORED = "ROLLBACK_OLD_RESTORED"

        /**
         * 把旧 journal 的 [STATE_OLD_BACKED_UP] 映射到 [STATE_BACKUP_READY]（#649 评论 5565067997 修复 1）。
         *
         * 新代码不再写入 [STATE_OLD_BACKED_UP]，但反序列化旧 journal 时需映射到新状态机。
         */
        fun normalizeState(state: String): String = if (state == STATE_OLD_BACKED_UP) STATE_BACKUP_READY else state
    }
}

/**
 * PendingMirrorPublish — pendingPublish journal 的完整数据模型。
 *
 * #649 评论 5561974464 问题 3：pendingPublish 没有恢复逻辑，内容不足以恢复事务。
 *
 * ## 旧 journal 的问题
 * 旧 journal 只保存 projectId/phase/writtenKeys/deletedKeys，没保存 staging ref、
 * 旧 ref、desired entry、目标路径，进程死掉后不知道该提交、回滚还是删哪个文件。
 *
 * ## 新 journal 字段
 * - [txId]：事务 ID，用于 [ReadableMirrorStorage.rollback]。
 * - [backend] / [treeUri]：事务开始时的后端（恢复时用同一后端）。
 * - [projectId]：本次事务的目标项目。
 * - [transactionType]：UPSERT 还是 DELETE（#649 评论 5562462046 问题 4）。
 * - [phase]：`stage` | `promote` | `cleanup`。
 * - [oldEntries]：本次变更前的旧条目（按 [ChapterKey] 索引）。
 * - [newEntries]：desired entries（promote 后填 URI）。
 * - [stagedRefs]：已 stage 的章节 staging 引用（向后兼容，与 [items] 中的 stagedRef 同步）。
 * - [items]：逐项进度（#649 评论 5562462046 问题 3），按 [ChapterKey] 索引。
 * - [removedProjectIds]：要从 manifest 移除的项目（deleteProject 时非空）。
 * - [manifestOldRef]：旧 manifest 引用。
 * - [manifestStagedRef]：manifest staging 引用。
 * - [manifestNewRef]：promote 后的新 manifest 引用（phase=cleanup 时非空）。
 * - [manifestBackupRef]：manifest 事务中旧 manifest 的备份引用（#649 评论 5562462046 问题 2），
 *   新 manifest 提交成功后才删。
 * - [isManifestCommitted]：DELETE_PROJECT 事务中 manifest 事务是否已提交成功。
 *   #649 评论 5562462046 问题 4：manifest 提交成功后设为 true，恢复 cleanup 阶段
 *   可区分：若 manifest 已提交，直接 publishManifest()（依赖 Core 当前快照，
 *   已不含被删项目）；若 manifest 未提交，需构造 desiredEntries 手动排除被删项目后
 *   走 publishManifestWithDesired() 重写。
 *
 * ## 恢复策略（[ReadableMirrorPublisher.recoverPendingPublishIfNeeded]）
 * - `stage`：staging 未完成，旧镜像完整 → rollback(txId) + clearPendingPublish。
 * - `promote`：staging 已写完，promote 部分完成 → 继续 promote 剩余，然后 cleanup。
 * - `cleanup`：已 promote 完，stateStore 未更新/清理未完成 → 继续 cleanup。
 *
 * ## 架构约束
 * - 位于 `:app` 的 `storage/mirror` 包，只依赖 `org.json`，不依赖 Compose/UniFFI。
 */
data class PendingMirrorPublish(
    val txId: String,
    val backend: MirrorBackend,
    val treeUri: String?,
    val projectId: String,
    val transactionType: MirrorTransactionType,
    val phase: String,
    val oldEntries: Map<ChapterKey, ChapterMirrorEntry>,
    val newEntries: Map<ChapterKey, ChapterMirrorEntry>,
    val stagedRefs: Map<ChapterKey, StagedMirrorRef>,
    val items: Map<ChapterKey, PendingItem>,
    val removedProjectIds: Set<String>,
    val manifestOldRef: MirrorFileRef?,
    val manifestStagedRef: StagedMirrorRef?,
    val manifestNewRef: MirrorFileRef?,
    val manifestBackupRef: MirrorFileRef?,
    val isManifestCommitted: Boolean = false,
    // #649 评论 5565067997 修复 2：manifest 事务显式 swap 状态。
    // 替代 isManifestCommitted: Boolean 单布尔值，区分 backup_ready / old_vacated / promoted / committed。
    // 旧 journal 没有此字段，反序列化时根据 isManifestCommitted + manifestNewRef/manifestBackupRef 推导。
    val manifestSwapState: ManifestTransactionState = ManifestTransactionState.MANIFEST_STAGED,
    // #649 评论 5564820566 问题 5：零章节作品也需要独立 project 状态。
    // 正常 UPSERT 事务中记录本次要 publish 的 projectId 集合（含子项目），
    // DELETE 事务中记录被删的 projectId。恢复时据此维护 publishedProjectIds。
    val affectedProjectIds: Set<String> = emptySet(),
    // #649 评论 5566303837 问题 3：manifest promote 崩溃窗口。
    // MANIFEST_OLD_VACATED 恢复时 final 上可能是新文件残留，
    // 不能只看"final 在不在"，需要用 hash 校验身份。
    // manifestNewContentHash: 新 manifest 的期望 hash（生成 JSON 时计算）
    // manifestOldContentHash: 旧 manifest 的 hash（备份前读取记录）
    val manifestNewContentHash: String? = null,
    val manifestOldContentHash: String? = null,
) {
    /** 序列化为 JSON 字符串，供 [ReadableMirrorStateStore.writePendingPublish] 持久化。 */
    fun toJson(): String {
        val root = JSONObject()
        root.put(KEY_TX_ID, txId)
        root.put(KEY_BACKEND, backend.toJsonValue())
        if (treeUri != null) root.put(KEY_TREE_URI, treeUri)
        root.put(KEY_PROJECT_ID, projectId)
        root.put(KEY_TRANSACTION_TYPE, transactionType.toJsonValue())
        root.put(KEY_PHASE, phase)
        root.put(KEY_OLD_ENTRIES, encodeEntries(oldEntries))
        root.put(KEY_NEW_ENTRIES, encodeEntries(newEntries))
        root.put(KEY_STAGED_REFS, encodeStagedRefs(stagedRefs))
        root.put(KEY_ITEMS, encodeItems(items))
        root.put(KEY_REMOVED_PROJECT_IDS, JSONArray(removedProjectIds.toList()))
        if (manifestOldRef != null) root.put(KEY_MANIFEST_OLD_REF, encodeFileRef(manifestOldRef))
        if (manifestStagedRef != null) root.put(KEY_MANIFEST_STAGED_REF, encodeStagedRef(manifestStagedRef))
        if (manifestNewRef != null) root.put(KEY_MANIFEST_NEW_REF, encodeFileRef(manifestNewRef))
        if (manifestBackupRef != null) root.put(KEY_MANIFEST_BACKUP_REF, encodeFileRef(manifestBackupRef))
        root.put(KEY_IS_MANIFEST_COMMITTED, isManifestCommitted)
        // #649 评论 5565067997 修复 2：持久化 manifestSwapState
        root.put(KEY_MANIFEST_SWAP_STATE, manifestSwapState.journalValue)
        if (affectedProjectIds.isNotEmpty()) {
            root.put(KEY_AFFECTED_PROJECT_IDS, JSONArray(affectedProjectIds.toList()))
        }
        if (manifestNewContentHash != null) root.put(KEY_MANIFEST_NEW_CONTENT_HASH, manifestNewContentHash)
        if (manifestOldContentHash != null) root.put(KEY_MANIFEST_OLD_CONTENT_HASH, manifestOldContentHash)
        return root.toString()
    }

    companion object {
        const val TAG = "PendingMirrorPublish"
        const val PHASE_STAGE = "stage"
        const val PHASE_PROMOTE = "promote"
        const val PHASE_CLEANUP = "cleanup"

        // #649 评论 5564624383 问题 2：rollback 本身做成 journal 状态。
        // 进程死在回滚中间，下次是继续回滚，不会又转回 forward promote。
        const val PHASE_ROLLBACK = "rollback"

        private const val KEY_TX_ID = "txId"
        private const val KEY_BACKEND = "backend"
        private const val KEY_TREE_URI = "treeUri"
        private const val KEY_PROJECT_ID = "projectId"
        private const val KEY_TRANSACTION_TYPE = "transactionType"
        private const val KEY_PHASE = "phase"
        private const val KEY_OLD_ENTRIES = "oldEntries"
        private const val KEY_NEW_ENTRIES = "newEntries"
        private const val KEY_STAGED_REFS = "stagedRefs"
        private const val KEY_ITEMS = "items"
        private const val KEY_REMOVED_PROJECT_IDS = "removedProjectIds"
        private const val KEY_MANIFEST_OLD_REF = "manifestOldRef"
        private const val KEY_MANIFEST_STAGED_REF = "manifestStagedRef"
        private const val KEY_MANIFEST_NEW_REF = "manifestNewRef"
        private const val KEY_MANIFEST_BACKUP_REF = "manifestBackupRef"
        private const val KEY_URI = "uri"
        private const val KEY_RELATIVE_PATH = "relativePath"
        private const val KEY_REVISION = "revision"
        private const val KEY_CONTENT_HASH = "contentHash"
        private const val KEY_STAGING_URI = "stagingUri"
        private const val KEY_STAGING_RELATIVE_PATH = "stagingRelativePath"
        private const val KEY_FINAL_RELATIVE_PATH = "finalRelativePath"
        private const val KEY_MIME_TYPE = "mimeType"
        private const val KEY_OLD_REF = "oldRef"
        private const val KEY_BACKUP_OLD_REF = "backupOldRef"
        private const val KEY_PROMOTED_REF = "promotedRef"
        private const val KEY_STATE = "state"
        private const val KEY_IS_MANIFEST_COMMITTED = "isManifestCommitted"
        private const val KEY_MANIFEST_SWAP_STATE = "manifestSwapState"
        private const val KEY_AFFECTED_PROJECT_IDS = "affectedProjectIds"
        private const val KEY_MANIFEST_NEW_CONTENT_HASH = "manifestNewContentHash"
        private const val KEY_MANIFEST_OLD_CONTENT_HASH = "manifestOldContentHash"
        private const val KEY_OLD_CONTENT_HASH = "oldContentHash"

        /**
         * 校验 phase 值是否合法（#649 评论 5565862745 问题 5）。
         *
         * 只接受枚举中的明确值，未知值返回 null。
         */
        private fun validatePhase(value: String): String? =
            when (value) {
                PHASE_STAGE, PHASE_PROMOTE, PHASE_CLEANUP, PHASE_ROLLBACK -> value
                else -> null
            }

        /** 从 [ReadableMirrorStateStore.readPendingPublish] 的 JSON 字符串反序列化。 */
        fun fromJson(json: String): PendingMirrorPublish? {
            return try {
                val root = JSONObject(json)
                // #649 评论 5565862745 问题 5：未知值返回 null，不能回退到默认值
                val backend = mirrorBackendFromJsonValue(root.optString(KEY_BACKEND))
                val treeUri = root.optString(KEY_TREE_URI).takeIf { it.isNotEmpty() }
                val projectId = root.getString(KEY_PROJECT_ID)
                val transactionType =
                    mirrorTransactionTypeFromJsonValue(root.optString(KEY_TRANSACTION_TYPE))
                val phase = validatePhase(root.getString(KEY_PHASE))
                // 校验必填字段解析成功（#649 评论 5565862745 问题 5）
                if (backend == null || transactionType == null || phase == null) {
                    DiagnosticsLogger.e(TAG, "PendingMirrorPublish.fromJson: invalid backend/transactionType/phase")
                    return null
                }
                val oldEntries = decodeEntries(root.optJSONObject(KEY_OLD_ENTRIES))
                val newEntries = decodeEntries(root.optJSONObject(KEY_NEW_ENTRIES))
                val stagedRefs = decodeStagedRefs(root.optJSONObject(KEY_STAGED_REFS))
                val items = decodeItems(root.optJSONObject(KEY_ITEMS))
                val removedProjectIds = decodeStringSet(root.optJSONArray(KEY_REMOVED_PROJECT_IDS))
                val manifestOldRef = decodeFileRef(root.optJSONObject(KEY_MANIFEST_OLD_REF))
                val manifestStagedRef = decodeStagedRef(root.optJSONObject(KEY_MANIFEST_STAGED_REF))
                val manifestNewRef = decodeFileRef(root.optJSONObject(KEY_MANIFEST_NEW_REF))
                val manifestBackupRef = decodeFileRef(root.optJSONObject(KEY_MANIFEST_BACKUP_REF))
                val isManifestCommitted = root.optBoolean(KEY_IS_MANIFEST_COMMITTED, false)
                val affectedProjectIds = decodeStringSet(root.optJSONArray(KEY_AFFECTED_PROJECT_IDS))
                // #649 评论 5566303837 问题 3：反序列化 manifest 内容 hash
                val manifestNewContentHash = root.optString(KEY_MANIFEST_NEW_CONTENT_HASH).takeIf { it.isNotEmpty() }
                val manifestOldContentHash = root.optString(KEY_MANIFEST_OLD_CONTENT_HASH).takeIf { it.isNotEmpty() }
                // #649 评论 5565067997 修复 2：反序列化 manifestSwapState。
                // 旧 journal 没有此字段，根据 isManifestCommitted + manifestNewRef/manifestBackupRef 推导。
                val manifestSwapState =
                    if (root.has(KEY_MANIFEST_SWAP_STATE)) {
                        // #649 评论 5565862745 问题 5：未知值返回 null
                        ManifestTransactionState.fromJournalValue(root.optString(KEY_MANIFEST_SWAP_STATE))
                            ?: return null
                    } else {
                        deriveManifestSwapState(isManifestCommitted, manifestNewRef, manifestBackupRef)
                    }
                PendingMirrorPublish(
                    txId = root.getString(KEY_TX_ID),
                    backend = backend,
                    treeUri = treeUri,
                    projectId = projectId,
                    transactionType = transactionType,
                    phase = phase,
                    oldEntries = oldEntries,
                    newEntries = newEntries,
                    stagedRefs = stagedRefs,
                    items = items,
                    removedProjectIds = removedProjectIds,
                    manifestOldRef = manifestOldRef,
                    manifestStagedRef = manifestStagedRef,
                    manifestNewRef = manifestNewRef,
                    manifestBackupRef = manifestBackupRef,
                    isManifestCommitted = isManifestCommitted,
                    manifestSwapState = manifestSwapState,
                    affectedProjectIds = affectedProjectIds,
                    manifestNewContentHash = manifestNewContentHash,
                    manifestOldContentHash = manifestOldContentHash,
                )
            } catch (_: Exception) {
                null
            }
        }

        private fun encodeEntries(entries: Map<ChapterKey, ChapterMirrorEntry>): JSONObject {
            val obj = JSONObject()
            for ((key, entry) in entries) {
                val keyStr = "${key.projectId}/${key.volumeId}/${key.chapterId}"
                obj.put(
                    keyStr,
                    JSONObject().apply {
                        put(KEY_URI, entry.uri)
                        put(KEY_RELATIVE_PATH, entry.relativePath)
                        put(KEY_REVISION, entry.revision)
                        put(KEY_CONTENT_HASH, entry.contentHash)
                    },
                )
            }
            return obj
        }

        private fun decodeEntries(obj: JSONObject?): Map<ChapterKey, ChapterMirrorEntry> {
            if (obj == null) return emptyMap()
            val result = mutableMapOf<ChapterKey, ChapterMirrorEntry>()
            val keys = obj.keys()
            while (keys.hasNext()) {
                val keyStr = keys.next()
                val parts = keyStr.split("/", limit = 3)
                if (parts.size != 3) continue
                val entryObj = obj.optJSONObject(keyStr) ?: continue
                result[ChapterKey(parts[0], parts[1], parts[2])] =
                    ChapterMirrorEntry(
                        uri = entryObj.optString(KEY_URI),
                        relativePath = entryObj.optString(KEY_RELATIVE_PATH),
                        revision = entryObj.optLong(KEY_REVISION),
                        contentHash = entryObj.optString(KEY_CONTENT_HASH),
                    )
            }
            return result
        }

        private fun encodeStagedRefs(refs: Map<ChapterKey, StagedMirrorRef>): JSONObject {
            val obj = JSONObject()
            for ((key, ref) in refs) {
                val keyStr = "${key.projectId}/${key.volumeId}/${key.chapterId}"
                obj.put(keyStr, encodeStagedRef(ref))
            }
            return obj
        }

        private fun encodeStagedRef(ref: StagedMirrorRef): JSONObject =
            JSONObject().apply {
                put(KEY_TX_ID, ref.txId)
                put(KEY_STAGING_URI, ref.stagingUri)
                put(KEY_STAGING_RELATIVE_PATH, ref.stagingRelativePath)
                put(KEY_FINAL_RELATIVE_PATH, ref.finalRelativePath)
                put(KEY_MIME_TYPE, ref.mimeType)
            }

        private fun decodeStagedRefs(obj: JSONObject?): Map<ChapterKey, StagedMirrorRef> {
            if (obj == null) return emptyMap()
            val result = mutableMapOf<ChapterKey, StagedMirrorRef>()
            val keys = obj.keys()
            while (keys.hasNext()) {
                val keyStr = keys.next()
                val parts = keyStr.split("/", limit = 3)
                if (parts.size != 3) continue
                val refObj = obj.optJSONObject(keyStr) ?: continue
                result[ChapterKey(parts[0], parts[1], parts[2])] = decodeStagedRef(refObj) ?: continue
            }
            return result
        }

        private fun decodeStagedRef(obj: JSONObject?): StagedMirrorRef? {
            if (obj == null) return null
            return StagedMirrorRef(
                txId = obj.optString(KEY_TX_ID),
                stagingUri = obj.optString(KEY_STAGING_URI),
                stagingRelativePath = obj.optString(KEY_STAGING_RELATIVE_PATH),
                finalRelativePath = obj.optString(KEY_FINAL_RELATIVE_PATH),
                mimeType = obj.optString(KEY_MIME_TYPE),
            )
        }

        private fun encodeFileRef(ref: MirrorFileRef): JSONObject =
            JSONObject().apply {
                put(KEY_URI, ref.uri)
                put(KEY_RELATIVE_PATH, ref.relativePath)
            }

        private fun decodeFileRef(obj: JSONObject?): MirrorFileRef? {
            if (obj == null) return null
            val uri = obj.optString(KEY_URI)
            if (uri.isEmpty()) return null
            return MirrorFileRef(uri = uri, relativePath = obj.optString(KEY_RELATIVE_PATH))
        }

        private fun decodeStringSet(arr: org.json.JSONArray?): Set<String> {
            if (arr == null) return emptySet()
            val result = mutableSetOf<String>()
            for (i in 0 until arr.length()) {
                result.add(arr.getString(i))
            }
            return result
        }

        private fun encodeItems(items: Map<ChapterKey, PendingItem>): JSONObject {
            val obj = JSONObject()
            for ((key, item) in items) {
                val keyStr = "${key.projectId}/${key.volumeId}/${key.chapterId}"
                obj.put(keyStr, encodeItem(item))
            }
            return obj
        }

        private fun encodeItem(item: PendingItem): JSONObject =
            JSONObject().apply {
                put(KEY_STATE, item.state)
                if (item.stagedRef != null) put(KEY_STAGED_REFS, encodeStagedRef(item.stagedRef))
                if (item.oldRef != null) put(KEY_OLD_REF, encodeFileRef(item.oldRef))
                if (item.backupOldRef != null) put(KEY_BACKUP_OLD_REF, encodeFileRef(item.backupOldRef))
                if (item.promotedRef != null) put(KEY_PROMOTED_REF, encodeFileRef(item.promotedRef))
                if (item.oldContentHash != null) put(KEY_OLD_CONTENT_HASH, item.oldContentHash)
            }

        private fun decodeItems(obj: JSONObject?): Map<ChapterKey, PendingItem> {
            if (obj == null) return emptyMap()
            val result = mutableMapOf<ChapterKey, PendingItem>()
            val keys = obj.keys()
            while (keys.hasNext()) {
                val keyStr = keys.next()
                val parts = keyStr.split("/", limit = 3)
                if (parts.size != 3) continue
                val itemObj = obj.optJSONObject(keyStr) ?: continue
                val key = ChapterKey(parts[0], parts[1], parts[2])
                val item = decodeItem(key, itemObj) ?: continue
                result[key] = item
            }
            return result
        }

        private fun decodeItem(
            key: ChapterKey,
            obj: JSONObject,
        ): PendingItem? {
            return PendingItem(
                key = key,
                stagedRef = decodeStagedRef(obj.optJSONObject(KEY_STAGED_REFS)),
                oldRef = decodeFileRef(obj.optJSONObject(KEY_OLD_REF)),
                backupOldRef = decodeFileRef(obj.optJSONObject(KEY_BACKUP_OLD_REF)),
                promotedRef = decodeFileRef(obj.optJSONObject(KEY_PROMOTED_REF)),
                // #649 评论 5565067997 修复 1：旧 journal 的 STATE_OLD_BACKED_UP 映射到 STATE_BACKUP_READY
                state = PendingItem.normalizeState(obj.optString(KEY_STATE).ifEmpty { PendingItem.STATE_STAGED }),
                // #649 评论 5566303837 问题 2：反序列化 oldContentHash
                oldContentHash = obj.optString(KEY_OLD_CONTENT_HASH).takeIf { it.isNotEmpty() },
            )
        }
    }
}

/** [MirrorBackend] 与 journal 字符串互转。 */
private fun MirrorBackend.toJsonValue(): String =
    when (this) {
        MirrorBackend.MEDIA_STORE -> "media_store"
        MirrorBackend.DOCUMENT_TREE -> "document_tree"
    }

/**
 * 从 JSON 值反序列化 [MirrorBackend]（#649 评论 5565862745 问题 5）。
 *
 * - 已知值返回对应枚举
 * - 未知值返回 null（不能回退到 MEDIA_STORE，否则会掩盖数据损坏）
 */
private fun mirrorBackendFromJsonValue(value: String): MirrorBackend? =
    when (value) {
        "document_tree" -> MirrorBackend.DOCUMENT_TREE
        "media_store" -> MirrorBackend.MEDIA_STORE
        else -> null
    }

/** [MirrorTransactionType] 与 journal 字符串互转。 */
private fun MirrorTransactionType.toJsonValue(): String =
    when (this) {
        MirrorTransactionType.UPSERT_PROJECT -> "upsert_project"
        MirrorTransactionType.DELETE_PROJECT -> "delete_project"
    }

/**
 * 从 JSON 值反序列化 [MirrorTransactionType]（#649 评论 5565862745 问题 5）。
 *
 * - 已知值返回对应枚举
 * - 未知值返回 null（不能回退到 UPSERT_PROJECT，否则会掩盖数据损坏）
 */
private fun mirrorTransactionTypeFromJsonValue(value: String): MirrorTransactionType? =
    when (value) {
        "delete_project" -> MirrorTransactionType.DELETE_PROJECT
        "upsert_project" -> MirrorTransactionType.UPSERT_PROJECT
        else -> null
    }

/**
 * 旧 journal 没有 manifestSwapState 字段时，根据 isManifestCommitted + manifestNewRef/manifestBackupRef 推导。
 * #649 评论 5565067997 修复 2：向后兼容。
 */
private fun deriveManifestSwapState(
    isManifestCommitted: Boolean,
    manifestNewRef: MirrorFileRef?,
    manifestBackupRef: MirrorFileRef?,
): ManifestTransactionState =
    when {
        isManifestCommitted -> ManifestTransactionState.MANIFEST_COMMITTED
        manifestNewRef != null -> ManifestTransactionState.MANIFEST_PROMOTED
        manifestBackupRef != null -> ManifestTransactionState.MANIFEST_BACKUP_READY
        else -> ManifestTransactionState.MANIFEST_STAGED
    }
