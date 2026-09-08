package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger

/**
 * ReadableMirrorStateMigration — 旧 mirror runtime state 的一次性版本迁移。
 *
 * #649 评论 5576949398 问题 4：旧 state.json 有 manifestUri/publishedProjectIds/projects
 * 但没有 committedManifestJson/hash（committed baseline）。旧 [ReadableMirrorPublisher]
 * 把这种情况当首次发布，继续写新 manifest，会把多作品 manifest 退化成单作品。
 *
 * 本类负责把旧 state 一次性迁移成有 committed baseline 的合法状态：
 * 1. 从 [ReadableMirrorStateStore.getManifestUri] 读取已记录的 manifest URI 对应路径
 * 2. 用 [ReadableMirrorStorage.lookup] + [ReadableMirrorStorage.readTextAndHash] 读取 manifest 文本
 * 3. 用 [mirrorManifestFromJsonStrict] 严格校验 manifest schema
 * 4. 校验 manifest 中每个 chapter 的 relativePath/contentHash 与 [ReadableMirrorStateStore.getProjectEntries]
 *    记录的 private state entries 能对上（防止 manifest 与 state 不一致）
 * 5. 用 [ReadableMirrorStateStore.setCommittedManifest] 一次性写入 committed baseline
 *
 * ## 边界
 * - 只负责 mirror runtime state 的版本迁移，不进入普通 publish 逻辑
 * - 不把 Download 正文反向导入 Core
 * - 任何校验失败都停止迁移并保留旧 state（返回 [Result.FAILURE]），
 *   不允许普通 Publisher 把它当首次发布继续写
 * - 迁移成功后旧 state 变成有 committed baseline 的合法状态，
 *   下次 [ReadableMirrorStateStore.getCommittedManifestStrict] 返回 [CommittedManifestReadResult.Found]
 *
 * ## 架构约束
 * - 位于 `:app` 的 `storage/mirror` 包，只依赖 [ReadableMirrorStateStore] /
 *   [ReadableMirrorStorage] / [mirrorManifestFromJsonStrict]，不依赖 Compose/UniFFI
 * - 不持有 AppServiceBridge，不触发镜像发布
 */
class ReadableMirrorStateMigration(
    private val stateStore: ReadableMirrorStateStore,
    private val storage: ReadableMirrorStorage,
) {
    /** 迁移结果。 */
    enum class Result {
        /** 迁移成功，或 state 本就不需要迁移（已有 committed baseline 或真正全空）。 */
        SUCCESS,

        /** 迁移失败（manifest 读取/校验/写入失败），保留旧 state，调用方应停止本轮发布。 */
        FAILURE,
    }

    /**
     * 执行一次迁移。
     *
     * 幂等：如果 state 已经有 committed baseline（[CommittedManifestReadResult.Found]），
     * 或真正全空（[CommittedManifestReadResult.NotExists]），直接返回 [Result.SUCCESS]。
     * 只有 [CommittedManifestReadResult.NeedsMigration] 时才真正执行迁移。
     *
     * @return 迁移结果
     */
    fun migrate(): Result {
        return when (val readResult = stateStore.getCommittedManifestStrict()) {
            is CommittedManifestReadResult.Found -> Result.SUCCESS
            is CommittedManifestReadResult.NotExists -> Result.SUCCESS
            is CommittedManifestReadResult.Corrupted -> {
                DiagnosticsLogger.w(
                    TAG,
                    "State migration: state corrupted, cannot migrate: ${readResult.cause.message}",
                )
                Result.FAILURE
            }
            is CommittedManifestReadResult.NeedsMigration -> migrateFromOldState()
        }
    }

    /**
     * 从旧 state 迁移 committed baseline。
     *
     * 步骤：
     * 1. 读取 stateStore.getManifestUri()，null → FAILURE
     * 2. 用 storage.lookup(manifestRelativePath) 找到 manifest 文件
     * 3. 用 storage.readTextAndHash(ref) 读取内容
     * 4. 用 mirrorManifestFromJsonStrict 严格解析
     * 5. 校验 manifest chapters 与 stateStore entries 对上
     * 6. 用 stateStore.setCommittedManifest 写入 committed baseline
     */
    private fun migrateFromOldState(): Result {
        val manifestUri = stateStore.getManifestUri()
        if (manifestUri == null) {
            DiagnosticsLogger.w(TAG, "State migration: NeedsMigration but manifestUri is null")
            return Result.FAILURE
        }
        // #649 评论 5577831998 问题 2：迁移器现在用 stateStore.getManifestUri() 返回的精确旧 URI，
        // 不再按路径猜当前文件。SAF 允许出现同名文档，这里尤其不能猜。
        // 旧 URI 失效就停止迁移，不要按同路径找到另一份文件后自动认成旧 committed manifest。
        val oldManifestRef =
            MirrorFileRef(
                uri = manifestUri,
                relativePath = "_meta/manifest.json",
            )
        val (manifestJson, computedHash) =
            storage.readTextAndHash(oldManifestRef) ?: run {
                DiagnosticsLogger.w(TAG, "State migration: readTextAndHash failed for manifest at uri=$manifestUri")
                return Result.FAILURE
            }
        // 严格解析 manifest
        val manifest =
            try {
                mirrorManifestFromJsonStrict(manifestJson)
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "State migration: manifest strict parse failed: ${e.message}")
                return Result.FAILURE
            }
        // 校验 manifest chapters 与 stateStore private entries 对上
        if (!verifyManifestAgainstState(manifest)) {
            DiagnosticsLogger.w(TAG, "State migration: manifest vs state entries mismatch")
            return Result.FAILURE
        }
        // 一次性写入 committed baseline
        if (!stateStore.setCommittedManifest(manifestJson, computedHash)) {
            DiagnosticsLogger.w(TAG, "State migration: setCommittedManifest failed")
            return Result.FAILURE
        }
        DiagnosticsLogger.i(TAG, "State migration: successfully migrated committed baseline")
        return Result.SUCCESS
    }

    /**
     * 校验 manifest 与 stateStore 的 private entries 完全相等。
     *
     * #649 评论 5577831998 问题 2：旧实现只检查 manifest ⊆ state（允许 state 有额外 entries），
     * 但迁移是在 ensurePendingRecovered 之后触发的，没有合法的"事务中间状态"需要容忍。
     * 旧 private state 如果记录 A、B 两个已发布作品，而公共 manifest 只剩 A，
     * 现在迁移会把 A-only manifest 接纳为 committed baseline，后续编辑 B 时不会回到 manifest。
     * 零章节作品没有 chapter entry，旧校验对它完全是空的，根本没有检查 publishedProjectIds 是否对齐。
     *
     * 新实现改成完全相等校验：
     * - manifestProjectIds == stateProjectIds
     * - manifestEntries.keys == stateEntries.keys
     * - 每个 entry 的 relativePath / contentHash / revision 都相等
     *
     * @return true 表示 manifest 与 state 完全一致，可以作为 committed baseline
     */
    private fun verifyManifestAgainstState(manifest: MirrorManifest): Boolean {
        // 1. 检查 project IDs 完全相等
        val manifestProjectIds = manifest.projects.map { it.id }.toSet()
        val stateProjectIdsFromStore = stateStore.getAllProjectIds()
        if (manifestProjectIds != stateProjectIdsFromStore) {
            DiagnosticsLogger.w(
                TAG,
                "State migration: project IDs mismatch: manifest=${manifestProjectIds}, state=${stateProjectIdsFromStore}",
            )
            return false
        }
        // 2. 检查 chapter entries 完全相等（包括 revision）
        val manifestEntries = flattenManifestEntries(manifest)
        val stateResult = stateStore.getAllChapterEntriesStrict()
        if (stateResult.isFailure) {
            DiagnosticsLogger.w(
                TAG,
                "State migration: failed to read state entries: ${stateResult.exceptionOrNull()?.message}",
            )
            return false
        }
        val (stateProjectIds, stateEntries) = stateResult.getOrThrow()
        // 把 stateProjectIds 也纳入校验（覆盖零章节作品）
        val manifestProjectIdsFromEntries = manifestEntries.keys.map { it.projectId }.toSet()
        if (manifestProjectIdsFromEntries != stateProjectIds) {
            DiagnosticsLogger.w(
                TAG,
                "State migration: project IDs from entries mismatch: " +
                    "manifest=$manifestProjectIdsFromEntries, state=$stateProjectIds",
            )
            return false
        }
        if (manifestEntries.keys != stateEntries.keys) {
            val manifestKeys = manifestEntries.keys
            val stateKeys = stateEntries.keys
            val onlyInManifest = manifestKeys - stateKeys
            val onlyInState = stateKeys - manifestKeys
            DiagnosticsLogger.w(
                TAG,
                "State migration: chapter keys mismatch: onlyInManifest=$onlyInManifest, onlyInState=$onlyInState",
            )
            return false
        }
        for (key in manifestEntries.keys) {
            val manifestEntry = manifestEntries.getValue(key)
            val stateEntry = stateEntries.getValue(key)
            if (manifestEntry.relativePath != stateEntry.relativePath) {
                DiagnosticsLogger.w(
                    TAG,
                    "State migration: relativePath mismatch for $key: " +
                        "manifest=${manifestEntry.relativePath}, state=${stateEntry.relativePath}",
                )
                return false
            }
            if (manifestEntry.contentHash != stateEntry.contentHash) {
                DiagnosticsLogger.w(
                    TAG,
                    "State migration: contentHash mismatch for $key",
                )
                return false
            }
            if (manifestEntry.revision != stateEntry.revision) {
                DiagnosticsLogger.w(
                    TAG,
                    "State migration: revision mismatch for $key: " +
                        "manifest=${manifestEntry.revision}, state=${stateEntry.revision}",
                )
                return false
            }
        }
        return true
    }

    /**
     * 把 manifest 的所有 chapter entries 拍平成 key -> entry 映射。
     *
     * 与 stateStore 的 [ReadableMirrorStateStore.getAllChapterEntriesStrict] 输出格式对齐，
     * 方便做完全相等校验。
     */
    private fun flattenManifestEntries(manifest: MirrorManifest): Map<ChapterKey, ManifestChapterEntry> {
        val result = mutableMapOf<ChapterKey, ManifestChapterEntry>()
        for (project in manifest.projects) {
            for (volume in project.volumes) {
                for (chapter in volume.chapters) {
                    val key = ChapterKey(project.id, volume.id, chapter.id)
                    result[key] =
                        ManifestChapterEntry(
                            relativePath = chapter.contentFile,
                            contentHash = chapter.contentHash,
                            revision = chapter.revision,
                        )
                }
            }
        }
        return result
    }

    /**
     * Manifest 中的 chapter 条目信息（用于与 state entries 比较）。
     *
     * 与 [ChapterMirrorEntry] 不同，这里不需要 uri（manifest 不记录 URI），
     * 只比较 relativePath / contentHash / revision。
     */
    private data class ManifestChapterEntry(
        val relativePath: String,
        val contentHash: String,
        val revision: Long,
    )

    companion object {
        private const val TAG = "ReadableMirrorStateMigration"
        private const val META_DIR = "_meta"
        private const val MANIFEST_FILE_NAME = "manifest.json"
    }
}
