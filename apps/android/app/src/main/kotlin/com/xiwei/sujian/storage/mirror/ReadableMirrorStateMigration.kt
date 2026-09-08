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
        // manifest 在镜像中的相对路径固定为 _meta/manifest.json
        val manifestRelativePath = "$META_DIR/$MANIFEST_FILE_NAME"
        val lookupResult = storage.lookup(manifestRelativePath)
        val manifestRef =
            when (lookupResult) {
                is MirrorLookupResult.Found -> lookupResult.ref
                is MirrorLookupResult.Missing -> {
                    DiagnosticsLogger.w(
                        TAG,
                        "State migration: manifest file missing at $manifestRelativePath (uri=$manifestUri)",
                    )
                    return Result.FAILURE
                }
                is MirrorLookupResult.Failed -> {
                    DiagnosticsLogger.w(
                        TAG,
                        "State migration: manifest lookup failed: ${lookupResult.cause?.message}",
                    )
                    return Result.FAILURE
                }
            }
        val (manifestJson, computedHash) =
            storage.readTextAndHash(manifestRef) ?: run {
                DiagnosticsLogger.w(TAG, "State migration: readTextAndHash failed for manifest")
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
     * 校验 manifest 中每个 chapter 的 relativePath/contentHash 与 stateStore 的 private entries 对上。
     *
     * 防止 manifest 与 state 不一致时把错误的 baseline 写入 committed。
     * - manifest 中每个 chapter 必须在 stateStore 对应 project 的 entries 中存在
     * - manifest chapter.contentFile == entry.relativePath
     * - manifest chapter.contentHash == entry.contentHash
     *
     * 注意：允许 stateStore 有 manifest 中没有的 entries（可能是迁移中间状态），
     * 但不允许 manifest 有 stateStore 中没有的 chapter（manifest 引用了不存在的 state）。
     */
    private fun verifyManifestAgainstState(manifest: MirrorManifest): Boolean {
        for (project in manifest.projects) {
            val stateEntries = stateStore.getProjectEntries(project.id)
            for (volume in project.volumes) {
                for (chapter in volume.chapters) {
                    val key = ChapterKey(project.id, volume.id, chapter.id)
                    val entry = stateEntries[key]
                    if (entry == null) {
                        DiagnosticsLogger.w(
                            TAG,
                            "State migration: manifest chapter $key not in state entries",
                        )
                        return false
                    }
                    if (chapter.contentFile != entry.relativePath) {
                        DiagnosticsLogger.w(
                            TAG,
                            "State migration: contentFile mismatch for $key: " +
                                "manifest=${chapter.contentFile}, state=${entry.relativePath}",
                        )
                        return false
                    }
                    if (chapter.contentHash != entry.contentHash) {
                        DiagnosticsLogger.w(
                            TAG,
                            "State migration: contentHash mismatch for $key: " +
                                "manifest=${chapter.contentHash}, state=${entry.contentHash}",
                        )
                        return false
                    }
                }
            }
        }
        return true
    }

    companion object {
        private const val TAG = "ReadableMirrorStateMigration"
        private const val META_DIR = "_meta"
        private const val MANIFEST_FILE_NAME = "manifest.json"
    }
}
